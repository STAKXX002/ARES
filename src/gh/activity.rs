//! Turning GitHub API responses into plain facts.
//!
//! Responses come from the network, so nothing is trusted: types are
//! checked, strings are length-limited, and anything unexpected becomes an
//! error value instead of a panic. Parsing is pure (text in, data out).

use crate::timefmt::parse_rfc3339_utc;
use serde_json::Value;

/// Longest string kept from a response.
const MAX_STR: usize = 200;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoInfo {
    pub private: bool,
    pub archived: bool,
    pub fork: bool,
    pub default_branch: Option<String>,
    /// Seconds since the Unix epoch.
    pub pushed_at: Option<u64>,
    /// GitHub counts open pull requests as issues, so this includes both.
    pub open_issues_and_prs: u64,
}

/// The latest workflow run on a branch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ci {
    pub status: String,
    pub conclusion: Option<String>,
    pub branch: Option<String>,
    pub workflow: Option<String>,
    pub updated_at: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CiState {
    Passing,
    Failing,
    Running,
    /// Anything else, such as `cancelled` or `skipped`.
    Other(String),
}

impl Ci {
    pub fn state(&self) -> CiState {
        match self.status.as_str() {
            "completed" => match self.conclusion.as_deref() {
                Some("success") => CiState::Passing,
                Some("failure") | Some("timed_out") | Some("startup_failure") => CiState::Failing,
                Some(other) => CiState::Other(other.to_string()),
                None => CiState::Other("completed".to_string()),
            },
            "in_progress" | "queued" | "waiting" | "pending" | "requested" => CiState::Running,
            other => CiState::Other(other.to_string()),
        }
    }
}

/// How the last-fetched copy of a branch relates to the branch on GitHub.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SyncState {
    /// Your last fetch is exactly GitHub's current tip.
    Current,
    /// GitHub has commits your last fetch does not.
    FetchNeeded { new_commits: u64 },
    /// Each side has commits the other lacks.
    Diverged { ahead_by: u64, behind_by: u64 },
    /// Your last fetch contains commits GitHub no longer has (the branch
    /// was rewound or force-pushed).
    RemoteBehind { by: u64 },
    /// Your last-fetched commit is unknown to GitHub.
    NotOnGitHub,
    /// There is no fetched copy of the branch locally.
    NoLocalCopy,
    /// The comparison could not be made.
    Unavailable,
}

impl SyncState {
    /// True when a `git fetch` would bring in something new.
    pub fn needs_fetch(&self) -> bool {
        matches!(self, SyncState::FetchNeeded { .. } | SyncState::Diverged { .. })
    }
}

/// Everything ARES reports about one GitHub repository.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Activity {
    pub private: bool,
    pub archived: bool,
    pub fork: bool,
    pub default_branch: Option<String>,
    pub pushed_at: Option<u64>,
    pub open_prs: usize,
    /// True when there were as many open PRs as one page holds, so the real
    /// number may be higher.
    pub prs_capped: bool,
    /// Open issues excluding PRs. `None` when the PR count was capped, since
    /// the subtraction would then be unreliable.
    pub open_issues: Option<u64>,
    pub ci: Option<Ci>,
    /// Why CI information is missing, when it could not be fetched.
    pub ci_error: Option<String>,
    /// How the default branch compares with your last fetch. `None` when the
    /// comparison was not attempted.
    pub sync: Option<SyncState>,
}

/// One page of the pull request list holds at most this many.
pub const PULLS_PAGE: usize = 100;

fn parse_json(text: &str) -> Result<Value, String> {
    serde_json::from_str(text).map_err(|e| format!("unexpected response: not valid JSON ({e})"))
}

fn bounded(s: &str) -> String {
    s.chars().take(MAX_STR).collect()
}

fn opt_str(v: &Value, key: &str) -> Option<String> {
    v.get(key).and_then(Value::as_str).map(bounded)
}

pub fn parse_repo(text: &str) -> Result<RepoInfo, String> {
    let v = parse_json(text)?;
    if !v.is_object() {
        return Err("unexpected response: repository is not an object".to_string());
    }
    let open = v
        .get("open_issues_count")
        .and_then(Value::as_u64)
        .ok_or_else(|| "unexpected response: missing open_issues_count".to_string())?;
    let flag = |k: &str| v.get(k).and_then(Value::as_bool).unwrap_or(false);
    Ok(RepoInfo {
        private: flag("private"),
        archived: flag("archived"),
        fork: flag("fork"),
        default_branch: opt_str(&v, "default_branch"),
        pushed_at: v
            .get("pushed_at")
            .and_then(Value::as_str)
            .and_then(parse_rfc3339_utc),
        open_issues_and_prs: open,
    })
}

/// Number of open pull requests on the first page.
pub fn parse_pulls(text: &str) -> Result<usize, String> {
    match parse_json(text)? {
        Value::Array(items) => Ok(items.len()),
        _ => Err("unexpected response: pull requests are not a list".to_string()),
    }
}

/// The most recent workflow run, or `None` when there are no runs.
pub fn parse_latest_run(text: &str) -> Result<Option<Ci>, String> {
    let v = parse_json(text)?;
    let runs = v
        .get("workflow_runs")
        .and_then(Value::as_array)
        .ok_or_else(|| "unexpected response: missing workflow_runs".to_string())?;
    let first = match runs.first() {
        None => return Ok(None),
        Some(r) => r,
    };
    let status = opt_str(first, "status")
        .ok_or_else(|| "unexpected response: workflow run has no status".to_string())?;
    Ok(Some(Ci {
        status,
        conclusion: opt_str(first, "conclusion"),
        branch: opt_str(first, "head_branch"),
        workflow: opt_str(first, "name"),
        updated_at: first
            .get("updated_at")
            .and_then(Value::as_str)
            .and_then(parse_rfc3339_utc),
    }))
}

/// Interpret a compare response, where `base` was the last-fetched commit
/// and `head` was GitHub's branch.
pub fn parse_compare(text: &str) -> Result<SyncState, String> {
    let v = parse_json(text)?;
    let status = v
        .get("status")
        .and_then(Value::as_str)
        .ok_or_else(|| "unexpected response: compare has no status".to_string())?;
    let count = |k: &str| {
        v.get(k)
            .and_then(Value::as_u64)
            .ok_or_else(|| format!("unexpected response: compare has no {k}"))
    };
    let (ahead_by, behind_by) = (count("ahead_by")?, count("behind_by")?);
    match status {
        "identical" => Ok(SyncState::Current),
        "ahead" => Ok(SyncState::FetchNeeded { new_commits: ahead_by }),
        "behind" => Ok(SyncState::RemoteBehind { by: behind_by }),
        "diverged" => Ok(SyncState::Diverged { ahead_by, behind_by }),
        other => Err(format!("unexpected response: compare status {:?}", bounded(other))),
    }
}

/// Combine the responses into one [`Activity`].
pub fn combine(
    info: &RepoInfo,
    open_prs: usize,
    ci: Result<Option<Ci>, String>,
    sync: Option<SyncState>,
) -> Activity {
    let prs_capped = open_prs >= PULLS_PAGE;
    let open_issues = if prs_capped {
        None
    } else {
        Some(info.open_issues_and_prs.saturating_sub(open_prs as u64))
    };
    let (ci, ci_error) = match ci {
        Ok(c) => (c, None),
        Err(e) => (None, Some(e)),
    };
    Activity {
        private: info.private,
        archived: info.archived,
        fork: info.fork,
        default_branch: info.default_branch.clone(),
        pushed_at: info.pushed_at,
        open_prs,
        prs_capped,
        open_issues,
        ci,
        ci_error,
        sync,
    }
}
