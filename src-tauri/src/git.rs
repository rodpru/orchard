use crate::error::{Error, Result};
use crate::process::run;
use futures_util::{StreamExt, stream};
use serde::Serialize;
use std::path::{Path, PathBuf};
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
                prunable: false,
                changes: 0,
            };
            for line in block.lines() {
                let (key, value) = line.split_once(' ').unwrap_or((line, ""));
                match key {
                    "worktree" => wt.path = value.to_owned(),
                    "HEAD" => wt.head = value.to_owned(),
                    "branch" => wt.branch = Some(value.trim_start_matches("refs/heads/").to_owned()),
                    "locked" => wt.locked = true,
                    "prunable" => wt.prunable = true,
                    _ => {}
                }
            }
            (!wt.path.is_empty()).then_some(wt)
        })
        .collect()
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
        git(repo, &["worktree", "add", "--", &path_str, branch]).await?;
    } else {
        // --no-track: a branch started from `origin/main` must not push to it.
        let base = base.unwrap_or("HEAD");
        git(repo, &["worktree", "add", "--no-track", "-b", branch, "--", &path_str, base]).await?;
    }
    Ok(path_str)
}

/// Removes the worktree and, when asked, its **local** branch. Remote
/// branches are never touched.
pub async fn remove_worktree(repo: &Path, path: &str, force: bool, delete_branch: Option<&str>) -> Result<()> {
    let mut args = vec!["worktree", "remove"];
    if force {
        args.push("--force");
    }
    args.extend(["--", path]);
    git(repo, &args).await?;
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
pub async fn git_remove_worktree(repo: String, path: String, force: bool, delete_branch: Option<String>) -> Result<()> {
    remove_worktree(Path::new(&repo), &path, force, delete_branch.as_deref()).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use tauri::async_runtime::block_on;

    #[test]
    fn parses_porcelain() {
        let input = "worktree /r\nHEAD abc\nbranch refs/heads/main\n\nworktree /r.wt/feat\nHEAD def\nbranch refs/heads/feat/x\nlocked\n\nworktree /r.wt/d\nHEAD 123\ndetached\nprunable gitdir file points to non-existent location\n";
        let wts = parse_worktrees(input);
        assert_eq!(wts.len(), 3);
        assert!(wts[0].is_main);
        assert_eq!(wts[1].branch.as_deref(), Some("feat/x"));
        assert!(wts[1].locked);
        assert_eq!(wts[2].branch, None);
        assert!(wts[2].prunable);
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

        let dirty = block_on(remove_worktree(&repo, &path, false, None)).unwrap_err();
        assert!(dirty.to_string().starts_with("git worktree remove -- "), "{dirty}");
        block_on(remove_worktree(&repo, &path, true, Some("feat/x"))).unwrap();
        let branches = block_on(list_branches(&repo)).unwrap();
        assert_eq!(branches.local, vec!["main".to_string()]);
        assert_eq!(branches.remote, vec!["origin/main".to_string()]);
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
        let err = block_on(remove_worktree(&repo, &path, false, Some("origin/feat"))).unwrap_err();
        assert!(err.to_string().contains("not a local branch"), "{err}");
        assert_eq!(sh(&repo, &["for-each-ref", "--format=%(refname)", "refs/remotes"]), "refs/remotes/origin/feat");
    }

    #[test]
    fn default_path_sits_next_to_repo() {
        let p = default_worktree_path(Path::new("/code/app"), "feat/login");
        assert_eq!(p, PathBuf::from("/code/app.worktrees/feat-login"));
    }
}
