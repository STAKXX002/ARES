//! Fetching a repository's GitHub activity through `gh`.

use super::activity::{self, Activity, SyncState};
use super::runner::{branch_ok, run, GhError, Query};
use crate::github::RepoRef;
use crate::repo::par_map;
use std::collections::HashMap;
use std::path::Path;
use std::time::Duration;

/// GitHub throttles bursts of requests, so network work uses fewer workers
/// than local git work does. Each repository needs a few sequential `gh`
/// calls, so 8 at a time keeps a typical scan of a dozen or two repositories
/// to a few seconds without anything close to a burst.
pub const MAX_NETWORK_WORKERS: usize = 8;

fn is_missing(e: &GhError) -> bool {
    matches!(e, GhError::Failed { message, .. } if message.contains("HTTP 404"))
}

/// One repository to look up, with what the local clone knows about origin.
#[derive(Debug, Clone)]
pub struct Target {
    pub repo: RepoRef,
    /// Branch -> commit id as of the last fetch (see `repo::origin_refs`).
    /// `None` when that could not be read, in which case no comparison is
    /// attempted.
    pub origin_refs: Option<HashMap<String, String>>,
}

fn compare(program: &Path, repo: &RepoRef, base: &str, head: &str, timeout: Duration) -> SyncState {
    match run(program, &Query::Compare { repo, base, head }, timeout) {
        Ok(t) => activity::parse_compare(&t).unwrap_or(SyncState::Unavailable),
        // GitHub does not know the commit we last fetched.
        Err(e) if is_missing(&e) => SyncState::NotOnGitHub,
        Err(_) => SyncState::Unavailable,
    }
}

/// Fetch one repository's activity: its details, open pull requests, and
/// the latest workflow run on the default branch.
///
/// Problems with the workflow lookup alone do not discard the rest; they are
/// recorded in `ci_error`. Anything else is an error.
pub fn fetch_activity(
    program: &Path,
    repo: &RepoRef,
    origin_refs: Option<&HashMap<String, String>>,
    timeout: Duration,
) -> Result<Activity, String> {
    let info = run(program, &Query::Repo(repo), timeout)
        .map_err(|e| e.to_string())
        .and_then(|t| activity::parse_repo(&t))?;

    let open_prs = run(program, &Query::OpenPulls(repo), timeout)
        .map_err(|e| e.to_string())
        .and_then(|t| activity::parse_pulls(&t))?;

    let ci = if info.archived {
        Ok(None)
    } else {
        // Only filter by branch when the name is safe to put in the request.
        let branch = info.default_branch.as_deref().filter(|b| branch_ok(b));
        match run(program, &Query::LatestRun { repo, branch }, timeout) {
            Ok(t) => activity::parse_latest_run(&t),
            // No access to Actions, or Actions never enabled.
            Err(e) if is_missing(&e) => Ok(None),
            Err(e) => Err(e.to_string()),
        }
    };

    // Compare GitHub's default branch with the copy from your last fetch.
    let sync = match (info.default_branch.as_deref().filter(|b| branch_ok(b)), origin_refs) {
        (Some(branch), Some(refs)) => Some(match refs.get(branch) {
            None => SyncState::NoLocalCopy,
            Some(base) => compare(program, repo, base, branch, timeout),
        }),
        _ => None,
    };

    Ok(activity::combine(&info, open_prs, ci, sync))
}

/// Fetch many repositories in parallel (at most [`MAX_NETWORK_WORKERS`]),
/// keeping input order. One repository failing does not affect the others.
pub fn fetch_all(
    program: &Path,
    targets: &[Target],
    timeout: Duration,
    max_parallel: usize,
) -> Vec<Result<Activity, String>> {
    par_map(targets, max_parallel.min(MAX_NETWORK_WORKERS), |t| {
        fetch_activity(program, &t.repo, t.origin_refs.as_ref(), timeout)
    })
}
