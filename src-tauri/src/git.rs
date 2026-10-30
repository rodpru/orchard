use crate::error::{Error, Result};
use crate::process::{run, run_within};
use futures_util::{StreamExt, stream};
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::time::Duration;
use ts_rs::TS;

/// Parallel `git status` calls per refresh. Enough to hide latency without
/// flooding the machine when a repo has many worktrees.
const STATUS_CONCURRENCY: usize = 8;

/// Every git call skips optional locks: Orchard polls `git status` while agents
/// run `git add`/`git commit` in the same worktree, and a refresh that takes
/// `index.lock` makes their commands fail with "File exists".
pub(crate) async fn git(cwd: &Path, args: &[&str]) -> Result<String> {
    run(cwd, "git", args, &[NO_OPTIONAL_LOCKS]).await
}

/// Checking out or deleting a large worktree (`node_modules` and the like)
/// can outlast the default timeout, and killing it halfway leaves a mess.
const WORKTREE_TIMEOUT: Duration = Duration::from_secs(10 * 60);

async fn git_worktree(cwd: &Path, args: &[&str]) -> Result<String> {
    run_within(WORKTREE_TIMEOUT, cwd, "git", args, &[NO_OPTIONAL_LOCKS]).await
}

pub(crate) const NO_OPTIONAL_LOCKS: (&str, &str) = ("GIT_OPTIONAL_LOCKS", "0");

#[derive(Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Worktree {
    path: String,
    branch: Option<String>,
    head: String,
    is_main: bool,
    locked: bool,
    /// Why another tool locked the worktree, as given to `git worktree lock --reason`.
    lock_reason: Option<String>,
    prunable: bool,
    #[ts(type = "number")]
    changes: usize,
}

fn parse_worktrees(porcelain: &str) -> Vec<Worktree> {
    porcelain
        .split("\n\n")
        .filter(|block| !block.trim().is_empty())
        .enumerate()
        .filter_map(|(index, block)| {
            let mut wt = Worktree {
                path: String::new(),
                branch: None,
                head: String::new(),
                is_main: index == 0,
                locked: false,
                lock_reason: None,
                prunable: false,
                changes: 0,
            };
            for line in block.lines() {
                let (key, value) = line.split_once(' ').unwrap_or((line, ""));
                match key {
                    "worktree" => wt.path = value.to_owned(),
                    "HEAD" => wt.head = value.to_owned(),
                    "branch" => wt.branch = Some(value.trim_start_matches("refs/heads/").to_owned()),
                    "locked" => {
                        wt.locked = true;
                        wt.lock_reason = (!value.is_empty()).then(|| unquote(value));
                    }
                    "prunable" => wt.prunable = true,
                    _ => {}
                }
            }
            (!wt.path.is_empty()).then_some(wt)
        })
        .collect()
}

/// Undoes git's C-style quoting, which `--porcelain` applies to a lock reason
/// with quotes, backslashes or control characters in it.
fn unquote(value: &str) -> String {
    let Some(inner) = value.strip_prefix('"').and_then(|v| v.strip_suffix('"')) else {
        return value.to_owned();
    };
    let mut bytes = Vec::with_capacity(inner.len());
    let mut chars = inner.bytes();
    while let Some(byte) = chars.next() {
        if byte != b'\\' {
            bytes.push(byte);
            continue;
        }
        match chars.next() {
            Some(b'n') => bytes.push(b'\n'),
            Some(b't') => bytes.push(b'\t'),
            Some(b'r') => bytes.push(b'\r'),
            Some(b'a') => bytes.push(0x07),
            Some(b'b') => bytes.push(0x08),
            Some(b'f') => bytes.push(0x0c),
            Some(b'v') => bytes.push(0x0b),
            // Bytes outside ASCII come as three octal digits each.
            Some(digit @ b'0'..=b'7') => {
                let rest: Vec<u8> = chars.by_ref().take(2).collect();
                let octal = [digit].iter().chain(&rest).fold(0u32, |n, d| n * 8 + u32::from(d - b'0'));
                bytes.push(octal as u8);
            }
            Some(other) => bytes.push(other),
            None => bytes.push(b'\\'),
        }
    }
    String::from_utf8_lossy(&bytes).into_owned()
}

/// Resolves any path inside a repository (or one of its worktrees) to the
/// main worktree, so a repo is always registered by the same path.
pub async fn resolve_repo(path: &Path) -> Result<String> {
    let porcelain = git(path, &["worktree", "list", "--porcelain"]).await?;
    parse_worktrees(&porcelain)
        .into_iter()
        .next()
        .map(|wt| wt.path)
        .ok_or_else(|| Error::Invalid("not a git repository".into()))
}

/// Path and branch of every worktree whose folder still exists, main first.
pub(crate) async fn checkouts(repo: &Path) -> Result<Vec<(String, Option<String>)>> {
    let porcelain = git(repo, &["worktree", "list", "--porcelain"]).await?;
    Ok(parse_worktrees(&porcelain).into_iter().filter(|wt| !wt.prunable).map(|wt| (wt.path, wt.branch)).collect())
}

pub async fn list_worktrees(repo: &Path) -> Result<Vec<Worktree>> {
    let porcelain = git(repo, &["worktree", "list", "--porcelain"]).await?;
    let worktrees = parse_worktrees(&porcelain);
    let paths: Vec<Option<PathBuf>> =
        worktrees.iter().map(|wt| (!wt.prunable).then(|| PathBuf::from(&wt.path))).collect();
    let counts: Vec<usize> = stream::iter(paths)
        .map(|path| async move {
            let Some(path) = path else { return 0 };
            git(&path, &["status", "--porcelain"]).await.map(|out| out.lines().count()).unwrap_or(0)
        })
        .buffered(STATUS_CONCURRENCY)
        .collect()
        .await;
    Ok(worktrees.into_iter().zip(counts).map(|(wt, changes)| Worktree { changes, ..wt }).collect())
}

#[derive(Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Branches {
    local: Vec<String>,
    remote: Vec<String>,
    default_branch: String,
}

pub async fn list_branches(repo: &Path) -> Result<Branches> {
    let refs = |pattern: &'static str| async move {
        Ok::<_, Error>(
            git(repo, &["for-each-ref", "--format=%(refname:short)", pattern])
                .await?
                .lines()
                .filter(|l| !l.ends_with("/HEAD") && !l.is_empty())
                .map(str::to_owned)
                .collect::<Vec<_>>(),
        )
    };
    let default_branch = default_branch(repo).await;
    Ok(Branches { local: refs("refs/heads").await?, remote: refs("refs/remotes").await?, default_branch })
}

/// `origin/HEAD` (for example `origin/main`), else the main worktree's branch.
pub(crate) async fn default_branch(repo: &Path) -> String {
    match git(repo, &["symbolic-ref", "--short", "refs/remotes/origin/HEAD"]).await {
        Ok(branch) => branch,
        Err(_) => git(repo, &["branch", "--show-current"]).await.unwrap_or_else(|_| "main".into()),
    }
}

fn sanitize(branch: &str) -> String {
    branch.chars().map(|c| if c.is_alphanumeric() || c == '-' || c == '_' || c == '.' { c } else { '-' }).collect()
}

/// Default location: `<parent>/<repo>.worktrees/<branch>`, next to the repo.
fn default_worktree_path(repo: &Path, branch: &str) -> PathBuf {
    let name = repo.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    repo.parent().unwrap_or(repo).join(format!("{name}.worktrees")).join(sanitize(branch))
}

fn shellexpand_home(path: &str) -> String {
    match (path.strip_prefix("~/"), dirs::home_dir()) {
        (Some(rest), Some(home)) => home.join(rest).to_string_lossy().into_owned(),
        _ => path.to_owned(),
    }
}

async fn local_branch_exists(repo: &Path, branch: &str) -> bool {
    git(repo, &["show-ref", "--verify", "--quiet", &format!("refs/heads/{branch}")]).await.is_ok()
}

pub async fn create_worktree(repo: &Path, branch: &str, base: Option<&str>, root: Option<&str>) -> Result<String> {
    let branch = branch.trim();
    if branch.is_empty() {
        return Err(Error::Invalid("branch name is required".into()));
    }
    git(repo, &["check-ref-format", "--branch", branch]).await?;

    let path = match root.map(str::trim).filter(|r| !r.is_empty()) {
        Some(root) => {
            let name = repo.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            PathBuf::from(shellexpand_home(root)).join(name).join(sanitize(branch))
        }
        None => default_worktree_path(repo, branch),
    };
    let path_str = path.to_string_lossy().into_owned();

    if local_branch_exists(repo, branch).await {
        git_worktree(repo, &["worktree", "add", "--", &path_str, branch]).await?;
    } else {
        // --no-track: a branch started from `origin/main` must not push to it.
        let base = base.unwrap_or("HEAD");
        git_worktree(repo, &["worktree", "add", "--no-track", "-b", branch, "--", &path_str, base]).await?;
    }
    Ok(path_str)
}

/// Removes the worktree and, when asked, its **local** branch. Remote
/// branches are never touched. `unlock` first lifts a lock another tool put
/// on the worktree; if the removal then fails, the lock is put back.
pub async fn remove_worktree(
    repo: &Path,
    path: &str,
    force: bool,
    unlock: bool,
    delete_branch: Option<&str>,
) -> Result<()> {
    let lock = if unlock { lift_lock(repo, path).await? } else { None };
    let mut args = vec!["worktree", "remove"];
    if force {
        args.push("--force");
    }
    args.extend(["--", path]);
    if let Err(e) = git_worktree(repo, &args).await {
        if let Some(reason) = lock {
            let mut relock = vec!["worktree", "lock"];
            if !reason.is_empty() {
                relock.extend(["--reason", &reason]);
            }
            relock.extend(["--", path]);
            let _ = git(repo, &relock).await;
        }
        return Err(e);
    }
    if let Some(branch) = delete_branch {
        if !local_branch_exists(repo, branch).await {
            return Err(Error::Invalid(format!("worktree removed, but {branch} is not a local branch")));
        }
        git(repo, &["branch", "-D", "--", branch])
            .await
            .map_err(|e| Error::Invalid(format!("worktree removed, but deleting branch {branch} failed: {e}")))?;
    }
    Ok(())
}

/// Unlocks `path` if it is locked and returns the lock's reason, empty when
/// it had none, so a failed removal can lock it again.
async fn lift_lock(repo: &Path, path: &str) -> Result<Option<String>> {
    let porcelain = git(repo, &["worktree", "list", "--porcelain"]).await?;
    let canonical = |p: &str| std::fs::canonicalize(p).unwrap_or_else(|_| PathBuf::from(p));
    let target = canonical(path);
    let Some(wt) = parse_worktrees(&porcelain).into_iter().find(|wt| wt.locked && canonical(&wt.path) == target) else {
        return Ok(None);
    };
    git(repo, &["worktree", "unlock", "--", path]).await?;
    Ok(Some(wt.lock_reason.unwrap_or_default()))
}

#[tauri::command]
pub async fn git_resolve_repo(path: String) -> Result<String> {
    resolve_repo(Path::new(&path)).await
}

#[tauri::command]
pub async fn git_list_worktrees(repo: String) -> Result<Vec<Worktree>> {
    list_worktrees(Path::new(&repo)).await
}

#[tauri::command]
pub async fn git_list_branches(repo: String) -> Result<Branches> {
    list_branches(Path::new(&repo)).await
}

#[tauri::command]
pub async fn git_create_worktree(
    repo: String,
    branch: String,
    base: Option<String>,
    root: Option<String>,
) -> Result<String> {
    create_worktree(Path::new(&repo), &branch, base.as_deref(), root.as_deref()).await
}

#[tauri::command]
pub async fn git_remove_worktree(
    repo: String,
    path: String,
    force: bool,
    unlock: bool,
    delete_branch: Option<String>,
) -> Result<()> {
    remove_worktree(Path::new(&repo), &path, force, unlock, delete_branch.as_deref()).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use tauri::async_runtime::block_on;

    #[test]
    fn parses_porcelain() {
        let input = "worktree /r\nHEAD abc\nbranch refs/heads/main\n\nworktree /r.wt/feat\nHEAD def\nbranch refs/heads/feat/x\nlocked {\"owner\":\"supacode\"}\n\nworktree /r.wt/d\nHEAD 123\ndetached\nprunable gitdir file points to non-existent location\n";
        let wts = parse_worktrees(input);
        assert_eq!(wts.len(), 3);
        assert!(wts[0].is_main);
        assert_eq!(wts[1].branch.as_deref(), Some("feat/x"));
        assert!(wts[1].locked);
        assert_eq!(wts[1].lock_reason.as_deref(), Some(r#"{"owner":"supacode"}"#));
        assert!(wts[0].lock_reason.is_none());
        assert_eq!(wts[2].branch, None);
        assert!(wts[2].prunable);
    }

    #[test]
    fn unquotes_lock_reasons() {
        assert_eq!(unquote("plain reason"), "plain reason");
        assert_eq!(unquote(r#""{\"owner\":\"supacode\"}""#), r#"{"owner":"supacode"}"#);
        assert_eq!(unquote(r#""a\\b\tc""#), "a\\b\tc");
        assert_eq!(unquote(r#""caf\303\251""#), "café");
    }

    fn sh(cwd: &Path, args: &[&str]) -> String {
        block_on(git(cwd, args)).unwrap()
    }

    #[test]
    fn create_list_and_remove_worktree() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("app");
        std::fs::create_dir_all(&repo).unwrap();
        sh(&repo, &["init", "-q", "-b", "main"]);
        sh(&repo, &["-c", "user.email=t@t", "-c", "user.name=t", "commit", "-q", "--allow-empty", "-m", "init"]);
        // A fake remote-tracking ref, to check new branches do not track it.
        sh(&repo, &["update-ref", "refs/remotes/origin/main", "HEAD"]);

        let path = block_on(create_worktree(&repo, "feat/x", Some("origin/main"), None)).unwrap();
        assert!(path.ends_with("app.worktrees/feat-x"));
        assert!(block_on(git(&repo, &["rev-parse", "--abbrev-ref", "feat/x@{upstream}"])).is_err());
        std::fs::write(Path::new(&path).join("new.txt"), "hi").unwrap();

        let resolved = block_on(resolve_repo(Path::new(&path))).unwrap();
        assert_eq!(std::fs::canonicalize(resolved).unwrap(), std::fs::canonicalize(&repo).unwrap());

        let wts = block_on(list_worktrees(&repo)).unwrap();
        assert_eq!(wts.len(), 2);
        assert_eq!(wts[1].branch.as_deref(), Some("feat/x"));
        assert_eq!(wts[1].changes, 1);
        // Polling status must not leave an index lock behind for agents.
        assert!(!repo.join(".git/worktrees/feat-x/index.lock").exists());

        // The branch is already checked out, so a second worktree fails.
        assert!(block_on(create_worktree(&repo, "feat/x", None, None)).is_err());

        let dirty = block_on(remove_worktree(&repo, &path, false, false, None)).unwrap_err();
        assert!(dirty.to_string().starts_with("git worktree remove -- "), "{dirty}");
        block_on(remove_worktree(&repo, &path, true, false, Some("feat/x"))).unwrap();
        let branches = block_on(list_branches(&repo)).unwrap();
        assert_eq!(branches.local, vec!["main".to_string()]);
        assert_eq!(branches.remote, vec!["origin/main".to_string()]);
    }

    #[test]
    fn a_worktree_locked_by_another_tool_is_unlocked_to_remove() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("app");
        std::fs::create_dir_all(&repo).unwrap();
        sh(&repo, &["init", "-q", "-b", "main"]);
        sh(&repo, &["-c", "user.email=t@t", "-c", "user.name=t", "commit", "-q", "--allow-empty", "-m", "init"]);
        let path = block_on(create_worktree(&repo, "locked", None, None)).unwrap();
        let reason = r#"{"owner":"supacode","version":"0.11.6"}"#;
        sh(&repo, &["worktree", "lock", "--reason", reason, "--", &path]);

        let refused = block_on(remove_worktree(&repo, &path, true, false, None)).unwrap_err();
        assert!(refused.to_string().contains("locked"), "{refused}");

        // A failed removal (dirty, no force) puts the same lock back.
        std::fs::write(Path::new(&path).join("new.txt"), "hi").unwrap();
        assert!(block_on(remove_worktree(&repo, &path, false, true, None)).is_err());
        let wts = block_on(list_worktrees(&repo)).unwrap();
        assert_eq!(wts[1].lock_reason.as_deref(), Some(reason));

        block_on(remove_worktree(&repo, &path, true, true, None)).unwrap();
        assert_eq!(block_on(list_worktrees(&repo)).unwrap().len(), 1);
    }

    #[test]
    fn deleting_a_non_local_branch_is_refused() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("app");
        std::fs::create_dir_all(&repo).unwrap();
        sh(&repo, &["init", "-q", "-b", "main"]);
        sh(&repo, &["-c", "user.email=t@t", "-c", "user.name=t", "commit", "-q", "--allow-empty", "-m", "init"]);
        sh(&repo, &["update-ref", "refs/remotes/origin/feat", "HEAD"]);
        let path = block_on(create_worktree(&repo, "other", None, None)).unwrap();
        let err = block_on(remove_worktree(&repo, &path, false, false, Some("origin/feat"))).unwrap_err();
        assert!(err.to_string().contains("not a local branch"), "{err}");
        assert_eq!(sh(&repo, &["for-each-ref", "--format=%(refname)", "refs/remotes"]), "refs/remotes/origin/feat");
    }

    #[test]
    fn default_path_sits_next_to_repo() {
        let p = default_worktree_path(Path::new("/code/app"), "feat/login");
        assert_eq!(p, PathBuf::from("/code/app.worktrees/feat-login"));
    }
}
