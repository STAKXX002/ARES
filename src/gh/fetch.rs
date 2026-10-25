//! Fetching a repository's GitHub activity through `gh`.

use super::activity::{self, Activity};
use super::runner::{branch_ok, run, GhError, Query};
use crate::github::RepoRef;
use crate::repo::par_map;
use std::path::Path;
use std::time::Duration;

/// GitHub throttles bursts of requests, so network work uses fewer workers
/// than local git work does.
pub const MAX_NETWORK_WORKERS: usize = 4;

fn is_missing(e: &GhError) -> bool {
    matches!(e, GhError::Failed { message, .. } if message.contains("HTTP 404"))
}

/// Fetch one repository's activity: its details, open pull requests, and
/// the latest workflow run on the default branch.
///
/// Problems with the workflow lookup alone do not discard the rest; they are
/// recorded in `ci_error`. Anything else is an error.
pub fn fetch_activity(program: &Path, repo: &RepoRef, timeout: Duration) -> Result<Activity, String> {
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

    Ok(activity::combine(&info, open_prs, ci))
}

/// Fetch many repositories in parallel (at most [`MAX_NETWORK_WORKERS`]),
/// keeping input order. One repository failing does not affect the others.
pub fn fetch_all(
    program: &Path,
    repos: &[RepoRef],
    timeout: Duration,
    max_parallel: usize,
) -> Vec<Result<Activity, String>> {
    par_map(repos, max_parallel.min(MAX_NETWORK_WORKERS), |r| {
        fetch_activity(program, r, timeout)
    })
}
