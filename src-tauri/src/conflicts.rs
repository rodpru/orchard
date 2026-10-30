//! Warns about colliding work before it reaches a merge: each worktree of a
//! repo against the other worktrees, the default branch, its own remote
//! branch and the open PRs.
//!
//! Agents keep most of their work uncommitted, so every worktree is first
//! turned into a snapshot commit of its working tree. The snapshot uses a
//! temporary index and never touches the worktree, its index or its lock.

use crate::error::{Error, Result};
use crate::git::{self, NO_OPTIONAL_LOCKS, git};
use crate::process;
use futures_util::{StreamExt, stream};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{LazyLock, Mutex};
use std::time::Duration;
use ts_rs::TS;

/// Parallel git calls per check, as for `git status` in `git.rs`.
const CONCURRENCY: usize = 8;
/// A fetch waiting on a credential prompt or a dead network gives up here.
const FETCH_TIMEOUT: Duration = Duration::from_secs(60);
/// Where open PR heads are fetched to. Outside `refs/heads` and
/// `refs/remotes`, so they never show up as branches.
const PR_REFS: &str = "refs/orchard/pr/";
/// Fixed identity and dates make a snapshot of unchanged files the same
/// commit every time, so repeated checks write no new objects.
const SNAPSHOT_ENV: [(&str, &str); 7] = [
    NO_OPTIONAL_LOCKS,
    ("GIT_AUTHOR_NAME", "Orchard"),
    ("GIT_AUTHOR_EMAIL", "orchard@localhost"),
    ("GIT_AUTHOR_DATE", "1000000000 +0000"),
    ("GIT_COMMITTER_NAME", "Orchard"),
    ("GIT_COMMITTER_EMAIL", "orchard@localhost"),
    ("GIT_COMMITTER_DATE", "1000000000 +0000"),
];

#[derive(Serialize, TS, Clone, Copy, PartialEq, Debug)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum ClashKind {
    /// Merging the two would stop on conflicts in `files`.
    Conflict,
    /// Both sides changed `files`, but git merges them cleanly.
    Overlap,
    /// The remote branch has commits the local one lacks and the other way
    /// round, so a push is rejected. `files` are the ones both changed.
    Diverged,
}

#[derive(Serialize, TS, Clone, PartialEq, Debug)]
#[serde(rename_all = "camelCase", tag = "type")]
#[ts(export)]
pub enum ClashTarget {
    Worktree {
        path: String,
        branch: Option<String>,
    },
    /// The default branch, for example `origin/main`.
    Base {
        name: String,
    },
    /// The worktree's own branch on `origin`.
    Upstream {
        name: String,
    },
    Pr {
        number: u32,
    },
}

#[derive(Serialize, TS, Clone, PartialEq, Debug)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Clash {
    target: ClashTarget,
    kind: ClashKind,
    files: Vec<String>,
}

#[derive(Serialize, TS, Debug)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ConflictReport {
    /// Clashes per worktree path. Worktrees without any are absent.
    clashes: HashMap<String, Vec<Clash>>,
    /// Why the fetch failed; the check still ran on the refs already there.
    fetch_error: Option<String>,
}

/// An open PR as `gh pr list` reports it.
#[derive(Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
struct OpenPr {
    number: u32,
    head_ref_name: String,
    /// Opened from a fork, so its branch name says nothing about local branches.
    is_cross_repository: bool,
}

/// Open PRs of the repo; `None` when `gh` fails, for example on a remote
/// that is not on GitHub or without `gh auth login`.
async fn open_prs(repo: &Path) -> Option<Vec<OpenPr>> {
    let args = ["pr", "list", "--state", "open", "--limit", "100", "--json", "number,headRefName,isCrossRepository"];
    let out = process::run(repo, "gh", &args, &[]).await.ok()?;
    serde_json::from_str(&out).ok()
}

/// A temporary index file, removed with its lock when dropped.
struct TempIndex(PathBuf);

impl TempIndex {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        Self(std::env::temp_dir().join(format!("orchard-index-{}-{n}", std::process::id())))
    }
}

impl Drop for TempIndex {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
        let _ = std::fs::remove_file(self.0.with_extension("lock"));
    }
}

/// A commit holding the worktree's files as they are on disk, untracked ones
/// included and ignored ones left out. A worktree without changes is its
/// HEAD. `None` for a branch without commits.
async fn snapshot(worktree: &Path) -> Result<Option<String>> {
    let Ok(heads) = git(worktree, &["rev-parse", "HEAD", "HEAD^{tree}"]).await else {
        return Ok(None);
    };
    let (head, head_tree) = heads.split_once('\n').unwrap_or((&heads, ""));
    let index = worktree.join(git(worktree, &["rev-parse", "--git-path", "index"]).await?);
    let tmp = TempIndex::new();
    if let Ok(meta) = tokio::fs::metadata(&index).await {
        // Starting from the real index keeps git's stat cache, so `add` only
        // rereads files that changed. The copy keeps the index's mtime: git
        // rereads files written in the same second as the index ("racy git"),
        // and a fresh mtime would hide a same-size edit made in that second.
        tokio::fs::copy(&index, &tmp.0).await?;
        std::fs::File::options().write(true).open(&tmp.0)?.set_modified(meta.modified()?)?;
    }
    let tmp_path = tmp.0.to_string_lossy();
    let env = [NO_OPTIONAL_LOCKS, ("GIT_INDEX_FILE", &tmp_path)];
    process::run(worktree, "git", &["add", "-A"], &env).await?;
    let tree = process::run(worktree, "git", &["write-tree"], &env).await?;
    if tree == head_tree {
        return Ok(Some(head.to_owned()));
    }
    let args = ["commit-tree", "--no-gpg-sign", &tree, "-p", head, "-m", "orchard snapshot"];
    process::run(worktree, "git", &args, &SNAPSHOT_ENV).await.map(Some)
}

/// Results of git commands that only read commits named by their full id.
/// Those never change, so a check every 30 s costs little after the first.
static PURE: LazyLock<Mutex<HashMap<String, (i32, String)>>> = LazyLock::new(Default::default);
/// Entries kept before the cache starts over.
const PURE_LIMIT: usize = 50_000;

/// Runs a git command whose answer depends only on the commit ids in
/// `args`, through `PURE`. Exit 1 is an answer (no merge base, a conflict).
async fn pure_git(repo: &Path, args: &[&str]) -> Result<(i32, String)> {
    let key = args.join("\0");
    if let Some(hit) = PURE.lock().unwrap_or_else(|e| e.into_inner()).get(&key) {
        return Ok(hit.clone());
    }
    let full = [&["-c", "core.quotePath=false"], args].concat();
    let output = process::output(repo, "git", &full, &[NO_OPTIONAL_LOCKS]).await?;
    let code = output.status.code().filter(|c| *c <= 1).ok_or_else(|| process::failure("git", &full, &output))?;
    let answer = (code, String::from_utf8_lossy(&output.stdout).trim_end().to_owned());
    let mut cache = PURE.lock().unwrap_or_else(|e| e.into_inner());
    if cache.len() >= PURE_LIMIT {
        cache.clear();
    }
    cache.insert(key, answer.clone());
    Ok(answer)
}

async fn merge_base(repo: &Path, a: &str, b: &str) -> Result<Option<String>> {
    let (code, out) = pure_git(repo, &["merge-base", a, b]).await?;
    Ok((code == 0).then_some(out))
}

async fn changed_files(repo: &Path, from: &str, to: &str) -> Result<BTreeSet<String>> {
    let (_, out) = pure_git(repo, &["diff", "--name-only", "--no-renames", from, to]).await?;
    Ok(out.lines().map(str::to_owned).collect())
}

/// Files a merge of the two would stop on; empty when it merges cleanly.
async fn merge_conflicts(repo: &Path, a: &str, b: &str) -> Result<BTreeSet<String>> {
    let (code, out) = pure_git(repo, &["merge-tree", "--write-tree", "--name-only", "--no-messages", a, b]).await?;
    // The first line is the merged tree, the rest the conflicted files.
    Ok(if code == 1 {
        out.lines().skip(1).filter(|l| !l.is_empty()).map(str::to_owned).collect()
    } else {
        BTreeSet::new()
    })
}

/// One side of a comparison: a commit and the work that is its own.
struct Side {
    commit: String,
    /// Where it left the default branch; `None` without a default branch
    /// or shared history, and then sides are compared pairwise.
    start: Option<String>,
    /// Files changed since `start`.
    own: BTreeSet<String>,
}

async fn side(repo: &Path, commit: String, base: Option<&str>) -> Result<Side> {
    let start = match base {
        Some(base) => merge_base(repo, &commit, base).await?,
        None => None,
    };
    let own = match &start {
        Some(start) => changed_files(repo, start, &commit).await?,
        None => BTreeSet::new(),
    };
    Ok(Side { commit, start, own })
}

/// What `compare` found: the kind and the files behind it.
type Found = (ClashKind, Vec<String>);

/// Compares the work of two sides. Only files both changed themselves count,
/// so a PR opened on an old base does not clash with everything the default
/// branch did since. A conflict counts only if `ours` causes it: one that
/// merging the other side into where `ours` started already has is not ours.
async fn compare(repo: &Path, ours: &Side, theirs: &Side) -> Result<Option<Found>> {
    let (a, b) = match (&ours.start, &theirs.start) {
        (Some(_), Some(_)) => (ours.own.clone(), theirs.own.clone()),
        _ => {
            let Some(fork) = merge_base(repo, &ours.commit, &theirs.commit).await? else { return Ok(None) };
            futures_util::try_join!(
                changed_files(repo, &fork, &ours.commit),
                changed_files(repo, &fork, &theirs.commit)
            )?
        }
    };
    let shared: BTreeSet<String> = a.intersection(&b).cloned().collect();
    if shared.is_empty() {
        return Ok(None);
    }
    let mut files: BTreeSet<String> =
        merge_conflicts(repo, &ours.commit, &theirs.commit).await?.intersection(&shared).cloned().collect();
    if !files.is_empty()
        && let Some(start) = &ours.start
    {
        let before = merge_conflicts(repo, start, &theirs.commit).await?;
        files.retain(|f| !before.contains(f));
    }
    Ok(Some(match files.is_empty() {
        true => (ClashKind::Overlap, shared.into_iter().collect()),
        false => (ClashKind::Conflict, files.into_iter().collect()),
    }))
}

/// The commit id `name` points at, if it exists.
async fn resolve(repo: &Path, name: &str) -> Option<String> {
    git(repo, &["rev-parse", "--verify", "-q", &format!("{name}^{{commit}}")]).await.ok()
}

async fn is_ancestor(repo: &Path, older: &str, newer: &str) -> bool {
    git(repo, &["merge-base", "--is-ancestor", older, newer]).await.is_ok()
}

/// `origin/<branch>` and its commit when it exists and the two have both
/// moved on. A branch that is only behind has nothing to push and is not
/// reported.
async fn diverged_upstream(repo: &Path, branch: &str) -> Option<(String, String)> {
    let upstream = format!("origin/{branch}");
    let local = format!("refs/heads/{branch}");
    let commit = resolve(repo, &format!("refs/remotes/{upstream}")).await?;
    let diverged = !is_ancestor(repo, &commit, &local).await && !is_ancestor(repo, &local, &commit).await;
    diverged.then_some((upstream, commit))
}

struct Snapshot {
    path: String,
    branch: Option<String>,
    side: Side,
}

/// Fetches `origin` and the heads of the open PRs, and drops the refs of
/// the others. A PR from this repo whose branch is checked out here is the
/// worktree itself and is left out. Without a PR list only `origin` is
/// fetched and the PR refs stay as they are. Never prompts for credentials.
async fn fetch(repo: &Path, prs: Option<&[OpenPr]>) -> Result<()> {
    if !git(repo, &["remote"]).await?.lines().any(|r| r == "origin") {
        return Ok(());
    }
    let env = [NO_OPTIONAL_LOCKS, ("GIT_TERMINAL_PROMPT", "0")];
    let timed = |args: Vec<String>| async move {
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        process::run_within(FETCH_TIMEOUT, repo, "git", &args, &env).await
    };
    let base = ["fetch", "--quiet", "--no-tags"].map(String::from);
    timed([&base[..], &["--prune".into(), "origin".into()]].concat()).await?;
    let Some(prs) = prs else { return Ok(()) };
    let local: Vec<String> = git::checkouts(repo).await?.into_iter().filter_map(|(_, branch)| branch).collect();
    let prs: Vec<&OpenPr> =
        prs.iter().filter(|pr| pr.is_cross_repository || !local.contains(&pr.head_ref_name)).collect();
    if !prs.is_empty() {
        let specs = prs.iter().map(|pr| format!("+refs/pull/{0}/head:{PR_REFS}{0}", pr.number));
        timed(base.iter().cloned().chain(["origin".into()]).chain(specs).collect()).await?;
    }
    let open: Vec<String> = prs.iter().map(|pr| format!("{PR_REFS}{}", pr.number)).collect();
    for name in git(repo, &["for-each-ref", "--format=%(refname)", PR_REFS]).await?.lines() {
        if !open.iter().any(|o| o == name) {
            git(repo, &["update-ref", "-d", name]).await?;
        }
    }
    Ok(())
}

pub async fn check(repo: &Path, with_fetch: bool) -> Result<ConflictReport> {
    let fetch_error = match with_fetch {
        true => fetch(repo, open_prs(repo).await.as_deref()).await.err().map(|e| e.to_string()),
        false => None,
    };
    let base_name = git::default_branch(repo).await;
    let base_commit = resolve(repo, &base_name).await;
    let base = base_commit.as_deref();

    let snapshots: Vec<Snapshot> = stream::iter(git::checkouts(repo).await?)
        .map(|(path, branch)| {
            let base = base_commit.clone();
            async move {
                let Some(commit) = snapshot(Path::new(&path)).await? else { return Ok(None) };
                Ok::<_, Error>(Some(Snapshot { path, branch, side: side(repo, commit, base.as_deref()).await? }))
            }
        })
        .buffered(CONCURRENCY)
        .collect::<Vec<_>>()
        .await
        .into_iter()
        .filter_map(Result::transpose)
        .collect::<Result<_>>()?;
    // Without a default branch every worktree counts as busy.
    let busy: Vec<&Snapshot> =
        snapshots.iter().filter(|s| base.is_none() || s.side.start.is_none() || !s.side.own.is_empty()).collect();

    // The last fetch left one ref per open PR worth checking.
    let refs = git(repo, &["for-each-ref", "--format=%(refname) %(objectname)", PR_REFS]).await?;
    let heads: Vec<(u32, String)> = refs
        .lines()
        .filter_map(|line| {
            let (name, commit) = line.split_once(' ')?;
            Some((name.strip_prefix(PR_REFS)?.parse().ok()?, commit.to_owned()))
        })
        .collect();
    let prs: Vec<(u32, Side)> = stream::iter(heads)
        .map(|(number, commit)| {
            let base = base_commit.clone();
            async move { Ok::<_, Error>((number, side(repo, commit, base.as_deref()).await?)) }
        })
        .buffered(CONCURRENCY)
        .collect::<Vec<_>>()
        .await
        .into_iter()
        .collect::<Result<_>>()?;

    // Each job compares one busy worktree with one other side.
    let mut jobs: Vec<(usize, ClashTarget, Side)> = Vec::new();
    for (i, s) in busy.iter().enumerate() {
        for b in &busy[i + 1..] {
            let side = Side { commit: b.side.commit.clone(), start: b.side.start.clone(), own: b.side.own.clone() };
            jobs.push((i, ClashTarget::Worktree { path: b.path.clone(), branch: b.branch.clone() }, side));
        }
        if let (Some(base), Some(start)) = (base, &s.side.start) {
            // The default branch's own work is what it did since this worktree left it.
            let own = changed_files(repo, start, base).await?;
            let side = Side { commit: base.to_owned(), start: Some(start.clone()), own };
            jobs.push((i, ClashTarget::Base { name: base_name.clone() }, side));
        }
        for (number, pr) in &prs {
            let side = Side { commit: pr.commit.clone(), start: pr.start.clone(), own: pr.own.clone() };
            jobs.push((i, ClashTarget::Pr { number: *number }, side));
        }
        if let Some(branch) = &s.branch
            && let Some((name, commit)) = diverged_upstream(repo, branch).await
        {
            // Compared pairwise: what matters is what each pushed since they split.
            jobs.push((i, ClashTarget::Upstream { name }, Side { commit, start: None, own: BTreeSet::new() }));
        }
    }

    let results: Vec<(usize, ClashTarget, Option<Found>)> = stream::iter(jobs)
        .map(|(i, target, theirs)| {
            let ours = &busy[i].side;
            async move { compare(repo, ours, &theirs).await.map(|found| (i, target, found)) }
        })
        .buffered(CONCURRENCY)
        .collect::<Vec<_>>()
        .await
        .into_iter()
        .collect::<Result<_>>()?;

    let mut clashes: HashMap<String, Vec<Clash>> = HashMap::new();
    let mut add = |i: usize, clash: Clash| clashes.entry(busy[i].path.clone()).or_default().push(clash);
    for (i, target, found) in results {
        match (target, found) {
            // A diverged branch is worth a warning even when the files do not meet.
            (target @ ClashTarget::Upstream { .. }, found) => {
                let (kind, files) = match found {
                    Some((ClashKind::Conflict, files)) => (ClashKind::Conflict, files),
                    Some((_, files)) => (ClashKind::Diverged, files),
                    None => (ClashKind::Diverged, Vec::new()),
                };
                add(i, Clash { target, kind, files });
            }
            (_, None) => {}
            // Worktree pairs are checked once and reported on both sides.
            (target @ ClashTarget::Worktree { .. }, Some((kind, files))) => {
                let j =
                    busy.iter().position(|s| matches!(&target, ClashTarget::Worktree { path, .. } if *path == s.path));
                if let Some(j) = j {
                    let back = ClashTarget::Worktree { path: busy[i].path.clone(), branch: busy[i].branch.clone() };
                    add(j, Clash { target: back, kind, files: files.clone() });
                }
                add(i, Clash { target, kind, files });
            }
            (target, Some((kind, files))) => add(i, Clash { target, kind, files }),
        }
    }
    Ok(ConflictReport { clashes, fetch_error })
}

#[tauri::command]
pub async fn conflicts_check(repo: String, fetch: bool) -> Result<ConflictReport> {
    check(Path::new(&repo), fetch).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use tauri::async_runtime::block_on;

    fn sh(cwd: &Path, args: &[&str]) -> String {
        block_on(process::run(cwd, "git", args, &SNAPSHOT_ENV)).unwrap()
    }

    fn write(dir: &Path, file: &str, text: &str) {
        std::fs::write(dir.join(file), text).unwrap();
    }

    fn commit(dir: &Path, message: &str) {
        sh(dir, &["add", "-A"]);
        sh(dir, &["commit", "-q", "--no-gpg-sign", "-m", message]);
    }

    /// A repo cloned from a bare `origin` with `f.txt` and `g.txt` on `main`.
    fn setup(tmp: &Path) -> (PathBuf, PathBuf) {
        let origin = tmp.join("origin.git");
        let seed = tmp.join("seed");
        std::fs::create_dir_all(&seed).unwrap();
        sh(tmp, &["init", "-q", "--bare", "-b", "main", origin.to_str().unwrap()]);
        sh(&seed, &["init", "-q", "-b", "main"]);
        write(&seed, "f.txt", "a\nb\nc\n");
        write(&seed, "g.txt", "x\n");
        commit(&seed, "init");
        sh(&seed, &["push", "-q", origin.to_str().unwrap(), "main"]);
        let repo = tmp.join("app");
        sh(tmp, &["clone", "-q", origin.to_str().unwrap(), repo.to_str().unwrap()]);
        // git reports real paths (/private/var on macOS), the keys of the report.
        (std::fs::canonicalize(repo).unwrap(), seed)
    }

    fn add_worktree(repo: &Path, branch: &str) -> PathBuf {
        let path = repo.with_file_name(branch);
        sh(repo, &["worktree", "add", "-q", "-b", branch, path.to_str().unwrap(), "origin/main"]);
        path
    }

    fn clashes_of<'a>(report: &'a ConflictReport, path: &Path) -> &'a [Clash] {
        report.clashes.get(path.to_str().unwrap()).map_or(&[], Vec::as_slice)
    }

    #[test]
    fn uncommitted_edits_conflict_without_touching_the_worktree() {
        let tmp = tempfile::tempdir().unwrap();
        let (repo, _) = setup(tmp.path());
        let a = add_worktree(&repo, "a");
        let b = add_worktree(&repo, "b");
        let c = add_worktree(&repo, "c");
        write(&a, "f.txt", "A\nb\nc\n");
        commit(&a, "a");
        write(&b, "f.txt", "B\nb\nc\n");
        write(&b, "new.txt", "untracked\n");
        // Another line of the same file: shared, but merges cleanly.
        write(&c, "f.txt", "a\nb\nC\n");
        let status = sh(&b, &["status", "--porcelain"]);

        let report = block_on(check(&repo, false)).unwrap();
        let on_b = clashes_of(&report, &b);
        let with_a = on_b
            .iter()
            .find(|c| matches!(&c.target, ClashTarget::Worktree { branch, .. } if branch.as_deref() == Some("a")));
        assert_eq!(with_a.map(|c| (c.kind, c.files.clone())), Some((ClashKind::Conflict, vec!["f.txt".to_string()])));
        assert!(clashes_of(&report, &a).iter().any(|c| c.kind == ClashKind::Conflict));
        assert!(clashes_of(&report, &c).iter().all(|c| c.kind == ClashKind::Overlap), "{report:?}");
        assert!(!clashes_of(&report, &c).is_empty());
        // The clean main worktree has no work of its own and is never a peer.
        assert!(clashes_of(&report, &repo).is_empty());
        assert_eq!(sh(&b, &["status", "--porcelain"]), status);
        assert!(!repo.join(".git/worktrees/b/index.lock").exists());

        // Unchanged files give the same snapshot commit every time.
        let first = block_on(snapshot(&b)).unwrap();
        assert_eq!(block_on(snapshot(&b)).unwrap(), first);
    }

    #[test]
    fn base_upstream_and_prs_after_a_fetch() {
        let tmp = tempfile::tempdir().unwrap();
        let (repo, seed) = setup(tmp.path());
        let origin = tmp.path().join("origin.git");
        let wt = add_worktree(&repo, "feat");
        write(&wt, "f.txt", "mine\nb\nc\n");
        commit(&wt, "feat");
        sh(&wt, &["push", "-q", "origin", "feat"]);

        // Meanwhile on origin: main changes the same line, someone pushes to
        // `feat`, and an open PR (number 7) touches `f.txt` too.
        write(&seed, "f.txt", "theirs\nb\nc\n");
        commit(&seed, "main moves");
        sh(&seed, &["push", "-q", origin.to_str().unwrap(), "main"]);
        sh(&seed, &["fetch", "-q", origin.to_str().unwrap(), "feat"]);
        sh(&seed, &["checkout", "-q", "-b", "feat", "FETCH_HEAD"]);
        write(&seed, "g.txt", "pushed elsewhere\n");
        commit(&seed, "remote feat");
        sh(&seed, &["push", "-q", origin.to_str().unwrap(), "feat"]);
        sh(&seed, &["checkout", "-q", "-b", "other", "main~1"]);
        write(&seed, "f.txt", "other\nb\nc\n");
        commit(&seed, "pr");
        sh(&seed, &["push", "-q", origin.to_str().unwrap(), "HEAD:refs/pull/7/head"]);
        write(&wt, "g.txt", "local\n");
        commit(&wt, "local g");

        // Without a fetch only the refs already there count.
        let stale = block_on(check(&repo, false)).unwrap();
        assert!(clashes_of(&stale, &wt).is_empty(), "{stale:?}");

        // `gh` needs GitHub, so the PR list is handed to `fetch` directly.
        // PR 8 is `feat` itself, from this repo; PR 9 is a fork's `feat`.
        let pr =
            |number, branch: &str, fork| OpenPr { number, head_ref_name: branch.into(), is_cross_repository: fork };
        sh(&seed, &["push", "-q", origin.to_str().unwrap(), "HEAD:refs/pull/9/head"]);
        let prs = [pr(7, "other", false), pr(8, "feat", false), pr(9, "feat", true)];
        block_on(fetch(&repo, Some(&prs))).unwrap();
        assert!(!ref_exists_sync(&repo, "refs/orchard/pr/8"));
        assert!(ref_exists_sync(&repo, "refs/orchard/pr/9"));
        let report = block_on(check(&repo, false)).unwrap();
        let clashes = clashes_of(&report, &wt);
        let kind_of = |pred: &dyn Fn(&ClashTarget) -> bool| clashes.iter().find(|c| pred(&c.target)).map(|c| c.kind);
        assert_eq!(
            kind_of(&|t| matches!(t, ClashTarget::Base { name } if name == "origin/main")),
            Some(ClashKind::Conflict)
        );
        assert_eq!(kind_of(&|t| matches!(t, ClashTarget::Pr { number: 7, .. })), Some(ClashKind::Conflict));
        assert_eq!(kind_of(&|t| matches!(t, ClashTarget::Upstream { .. })), Some(ClashKind::Conflict), "{clashes:?}");

        // Without a PR list (`gh` fails on this local remote) the PR refs are left alone.
        assert_eq!(block_on(check(&repo, true)).unwrap().fetch_error, None);
        assert!(ref_exists_sync(&repo, "refs/orchard/pr/7"));
        // A PR that closed loses its ref on the next fetch.
        block_on(fetch(&repo, Some(&[]))).unwrap();
        assert!(!ref_exists_sync(&repo, "refs/orchard/pr/7"));
    }

    #[test]
    fn a_pr_on_an_old_base_only_clashes_with_work_it_shares() {
        let tmp = tempfile::tempdir().unwrap();
        let (repo, seed) = setup(tmp.path());
        let origin = tmp.path().join("origin.git");
        // The PR left main at `init`, rewrote the first line of `f.txt` and `g.txt`.
        sh(&seed, &["checkout", "-q", "-b", "old-pr"]);
        write(&seed, "f.txt", "pr\nb\nc\n");
        write(&seed, "g.txt", "pr\n");
        commit(&seed, "pr");
        sh(&seed, &["push", "-q", origin.to_str().unwrap(), "HEAD:refs/pull/3/head"]);
        // Since then main rewrote `g.txt` too, so the PR conflicts with main
        // there, and changed the last line of `f.txt`.
        sh(&seed, &["checkout", "-q", "main"]);
        write(&seed, "f.txt", "a\nb\nmain\n");
        write(&seed, "g.txt", "main\n");
        commit(&seed, "main moves");
        sh(&seed, &["push", "-q", origin.to_str().unwrap(), "main"]);
        sh(&repo, &["fetch", "-q", "origin"]);
        let pr = OpenPr { number: 3, head_ref_name: "old-pr".into(), is_cross_repository: false };
        block_on(fetch(&repo, Some(&[pr]))).unwrap();

        // Each worktree starts from the new main.
        let quiet = add_worktree(&repo, "quiet");
        write(&quiet, "q.txt", "new file\n");
        // `g.txt` already conflicts between main and the PR; that is the PR's to fix.
        let g = add_worktree(&repo, "g");
        write(&g, "g.txt", "mine\n");
        let head = add_worktree(&repo, "head");
        write(&head, "f.txt", "head\nb\nmain\n");
        let tail = add_worktree(&repo, "tail");
        write(&tail, "f.txt", "a\nb\ntail\n");

        let report = block_on(check(&repo, false)).unwrap();
        let with_pr = |path: &Path| {
            clashes_of(&report, path).iter().find(|c| c.target == ClashTarget::Pr { number: 3 }).map(|c| c.kind)
        };
        assert_eq!(with_pr(&quiet), None, "{report:?}");
        assert_eq!(with_pr(&g), Some(ClashKind::Overlap), "{report:?}");
        assert_eq!(with_pr(&head), Some(ClashKind::Conflict), "{report:?}");
        assert_eq!(with_pr(&tail), Some(ClashKind::Overlap), "{report:?}");
    }

    #[test]
    fn a_same_size_edit_right_after_checkout_is_seen() {
        let tmp = tempfile::tempdir().unwrap();
        let (repo, _) = setup(tmp.path());
        let wt = add_worktree(&repo, "racy");
        // Same size as "x\n", likely in the same second as the checkout.
        write(&wt, "g.txt", "y\n");
        // The snapshot's index is then a second newer than the file.
        std::thread::sleep(std::time::Duration::from_millis(1100));
        let head = sh(&wt, &["rev-parse", "HEAD"]);
        assert_ne!(block_on(snapshot(&wt)).unwrap(), Some(head));
    }

    fn ref_exists_sync(repo: &Path, name: &str) -> bool {
        block_on(resolve(repo, name)).is_some()
    }
}
