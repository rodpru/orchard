use crate::error::Result;
use crate::process::run;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use ts_rs::TS;

/// Newest PRs per repo considered when matching worktree branches. One call
/// per repo replaces one `gh pr view` per worktree.
const PR_LIMIT: &str = "100";
const FIELDS: &str = "headRefName,number,title,state,url,isDraft,statusCheckRollup";
const PASSING: [&str; 3] = ["SUCCESS", "NEUTRAL", "SKIPPED"];

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GhPullRequest {
    head_ref_name: String,
    number: u32,
    title: String,
    state: PrState,
    url: String,
    is_draft: bool,
    #[serde(default)]
    status_check_rollup: Option<Vec<GhCheck>>,
}

/// `gh` mixes check runs (Actions) and commit statuses (external CI).
#[derive(Deserialize)]
#[serde(tag = "__typename")]
enum GhCheck {
    CheckRun {
        status: String,
        conclusion: Option<String>,
    },
    StatusContext {
        state: String,
    },
    #[serde(other)]
    Unknown,
}

#[derive(Serialize, Deserialize, TS, Clone, Copy, PartialEq, Debug)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[ts(export)]
pub enum PrState {
    Open,
    Closed,
    Merged,
}

#[derive(Serialize, TS, Clone, Copy, PartialEq, Debug)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum CheckState {
    Success,
    Failure,
    Pending,
    None,
}

#[derive(Serialize, TS, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Checks {
    state: CheckState,
    passed: u32,
    failed: u32,
    pending: u32,
}

#[derive(Serialize, TS, Debug)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PrSummary {
    number: u32,
    title: String,
    state: PrState,
    url: String,
    is_draft: bool,
    checks: Checks,
}

fn summarize_checks(rollup: &[GhCheck]) -> Checks {
    let (mut passed, mut failed, mut pending) = (0, 0, 0);
    for check in rollup {
        match check {
            GhCheck::CheckRun { status, .. } if status != "COMPLETED" => pending += 1,
            GhCheck::CheckRun { conclusion, .. } => {
                if PASSING.contains(&conclusion.as_deref().unwrap_or("")) {
                    passed += 1;
                } else {
                    failed += 1;
                }
            }
            GhCheck::StatusContext { state } => match state.as_str() {
                "SUCCESS" => passed += 1,
                "PENDING" | "EXPECTED" => pending += 1,
                _ => failed += 1,
            },
            GhCheck::Unknown => {}
        }
    }
    let state = if failed > 0 {
        CheckState::Failure
    } else if pending > 0 {
        CheckState::Pending
    } else if passed > 0 {
        CheckState::Success
    } else {
        CheckState::None
    };
    Checks { state, passed, failed, pending }
}

/// Keeps the newest PR per head branch; `gh pr list` returns newest first.
fn by_branch(prs: Vec<GhPullRequest>) -> HashMap<String, PrSummary> {
    let mut map = HashMap::new();
    for pr in prs {
        map.entry(pr.head_ref_name).or_insert_with(|| PrSummary {
            number: pr.number,
            title: pr.title,
            state: pr.state,
            url: pr.url,
            is_draft: pr.is_draft,
            checks: summarize_checks(pr.status_check_rollup.as_deref().unwrap_or_default()),
        });
    }
    map
}

/// PRs of the repo keyed by head branch. Branches without a PR are absent.
#[tauri::command]
pub async fn gh_pr_list(repo: String) -> Result<HashMap<String, PrSummary>> {
    let out =
        run(Path::new(&repo), "gh", &["pr", "list", "--state", "all", "--limit", PR_LIMIT, "--json", FIELDS], &[])
            .await?;
    Ok(by_branch(serde_json::from_str(&out)?))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(json: &str) -> HashMap<String, PrSummary> {
        by_branch(serde_json::from_str(json).unwrap())
    }

    #[test]
    fn summarizes_mixed_checks() {
        let prs = parse(
            r#"[{"headRefName":"feat","number":7,"title":"t","state":"OPEN","url":"https://x","isDraft":false,
                "statusCheckRollup":[
                  {"__typename":"CheckRun","status":"COMPLETED","conclusion":"SUCCESS"},
                  {"__typename":"CheckRun","status":"COMPLETED","conclusion":"SKIPPED"},
                  {"__typename":"CheckRun","status":"IN_PROGRESS","conclusion":null},
                  {"__typename":"StatusContext","state":"PENDING"},
                  {"__typename":"StatusContext","state":"SUCCESS"}]}]"#,
        );
        let checks = &prs["feat"].checks;
        assert_eq!(checks, &Checks { state: CheckState::Pending, passed: 3, failed: 0, pending: 2 });
    }

    #[test]
    fn failure_wins_and_empty_rollup_is_none() {
        let prs = parse(
            r#"[{"headRefName":"a","number":1,"title":"t","state":"OPEN","url":"u","isDraft":true,
                 "statusCheckRollup":[{"__typename":"CheckRun","status":"COMPLETED","conclusion":"FAILURE"},
                                      {"__typename":"StatusContext","state":"SUCCESS"}]},
                {"headRefName":"b","number":2,"title":"t","state":"MERGED","url":"u","isDraft":false,"statusCheckRollup":null}]"#,
        );
        assert_eq!(prs["a"].checks.state, CheckState::Failure);
        assert_eq!(prs["b"].checks, Checks { state: CheckState::None, passed: 0, failed: 0, pending: 0 });
        assert_eq!(prs["b"].state, PrState::Merged);
    }

    #[test]
    fn newest_pr_per_branch_wins() {
        let prs = parse(
            r#"[{"headRefName":"feat","number":9,"title":"new","state":"OPEN","url":"u","isDraft":false},
                {"headRefName":"feat","number":3,"title":"old","state":"CLOSED","url":"u","isDraft":false}]"#,
        );
        assert_eq!(prs["feat"].number, 9);
    }
}
