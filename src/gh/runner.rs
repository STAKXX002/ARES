//! Read-only GitHub CLI runner.
//!
//! The user's `gh` login typically carries broad scopes (`repo`,
//! `workflow`), so the token cannot be relied on to limit what ARES does.
//! Safety comes from ARES only being able to build a few fixed requests:
//!
//! - callers choose a [`Query`] variant; they cannot pass arguments;
//! - every request is `gh api --method GET --hostname github.com <path>`,
//!   where the path is assembled from validated owner, name, and branch
//!   strings and always begins with `repos/`;
//! - no request body, field flags, or other flags are ever added;
//! - output is capped, the call has a timeout, and the child runs in its
//!   own process group (see `proc`);
//! - error text is passed through redaction before it is returned.

use crate::github::{valid_name, valid_owner, RepoRef};
use crate::proc::{self, ProcError};
use crate::redact::redact;
use std::error::Error;
use std::fmt;
use std::io;
use std::path::Path;
use std::process::Command;
use std::time::Duration;

/// Most stdout kept from one `gh` call.
const MAX_OUTPUT: usize = 8 * 1024 * 1024;

/// Longest error text kept.
const MAX_ERROR_TEXT: usize = 300;

/// The complete set of GitHub requests ARES is allowed to make.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Query<'a> {
    /// `GET repos/{owner}/{name}`: visibility, archived, fork, default
    /// branch, last push, and the open issue plus PR count.
    Repo(&'a RepoRef),
    /// `GET repos/{owner}/{name}/pulls?state=open&per_page=100`
    OpenPulls(&'a RepoRef),
    /// `GET repos/{owner}/{name}/actions/runs?per_page=1&exclude_pull_requests=true`
    /// optionally limited to one branch.
    LatestRun {
        repo: &'a RepoRef,
        branch: Option<&'a str>,
    },
}

#[derive(Debug)]
pub enum GhError {
    /// `gh` could not be started, most likely because it is not installed.
    NotInstalled(io::Error),
    /// Waiting on the process failed.
    Io(io::Error),
    /// `gh` exited with an error. The text is already redacted.
    Failed { code: Option<i32>, message: String },
    /// `gh` did not answer in time.
    Timeout(Duration),
    /// The response was larger than ARES will read.
    TooLarge { limit: usize },
    /// ARES refused to build the request (an argument failed validation).
    InvalidArgument(String),
}

impl fmt::Display for GhError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            GhError::NotInstalled(e) => write!(f, "could not start gh (is it installed?): {e}"),
            GhError::Io(e) => write!(f, "i/o error while running gh: {e}"),
            GhError::Failed { message, .. } => write!(f, "{message}"),
            GhError::Timeout(d) => write!(f, "gh timed out after {}s", d.as_secs()),
            GhError::TooLarge { limit } => write!(f, "gh returned more than {limit} bytes"),
            GhError::InvalidArgument(a) => write!(f, "refusing to query: {a}"),
        }
    }
}

impl Error for GhError {}

/// True for branch names ARES is willing to place in a request path.
pub(crate) fn branch_ok(b: &str) -> bool {
    (1..=100).contains(&b.len())
        && b.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-')
        && !b.starts_with('-')
        && !b.starts_with('.')
}

fn checked_repo(r: &RepoRef) -> Result<String, GhError> {
    if !valid_owner(&r.owner) || !valid_name(&r.name) {
        return Err(GhError::InvalidArgument(format!(
            "repository name {:?} is not valid",
            r.full_name()
        )));
    }
    Ok(format!("repos/{}/{}", r.owner, r.name))
}

impl Query<'_> {
    /// The API path for this request. Always starts with `repos/`.
    pub fn endpoint(&self) -> Result<String, GhError> {
        match self {
            Query::Repo(r) => checked_repo(r),
            Query::OpenPulls(r) => Ok(format!("{}/pulls?state=open&per_page=100", checked_repo(r)?)),
            Query::LatestRun { repo, branch } => {
                let mut path = format!(
                    "{}/actions/runs?per_page=1&exclude_pull_requests=true",
                    checked_repo(repo)?
                );
                if let Some(b) = branch {
                    if !branch_ok(b) {
                        return Err(GhError::InvalidArgument(format!("branch name {b:?} is not valid")));
                    }
                    path.push_str("&branch=");
                    path.push_str(b);
                }
                Ok(path)
            }
        }
    }
}

pub(super) fn command(program: &Path, endpoint: &str) -> Command {
    let mut cmd = Command::new(program);
    cmd.args(["api", "--method", "GET", "--hostname", "github.com"])
        .args(["-H", "Accept: application/vnd.github+json"])
        .arg(endpoint)
        .env("GH_PROMPT_DISABLED", "1")
        .env("GH_NO_UPDATE_NOTIFIER", "1")
        .env("GH_SPINNER_DISABLED", "1")
        .env("NO_COLOR", "1")
        // Debug logging could write credentials to the text we capture.
        .env_remove("GH_DEBUG")
        .env_remove("DEBUG");
    cmd
}

fn trim_error(s: &str) -> String {
    let one_line = s.split_whitespace().collect::<Vec<_>>().join(" ");
    let clean = redact(&one_line);
    if clean.chars().count() > MAX_ERROR_TEXT {
        let cut: String = clean.chars().take(MAX_ERROR_TEXT).collect();
        format!("{cut}...")
    } else {
        clean
    }
}

/// Run one request through the program at `program` (normally `gh`).
pub fn run(program: &Path, query: &Query<'_>, timeout: Duration) -> Result<String, GhError> {
    run_limited(program, query, timeout, MAX_OUTPUT)
}

fn run_limited(
    program: &Path,
    query: &Query<'_>,
    timeout: Duration,
    max_output: usize,
) -> Result<String, GhError> {
    let endpoint = query.endpoint()?;
    let captured = proc::run_capture(command(program, &endpoint), timeout, max_output).map_err(|e| match e {
        ProcError::Spawn(e) => GhError::NotInstalled(e),
        ProcError::Io(e) => GhError::Io(e),
        ProcError::Timeout(d) => GhError::Timeout(d),
        ProcError::OutputTooLarge { limit } => GhError::TooLarge { limit },
    })?;
    if !captured.status.success() {
        let stderr = String::from_utf8_lossy(&captured.stderr);
        let stdout = String::from_utf8_lossy(&captured.stdout);
        // `gh api` reports HTTP errors on stderr, sometimes with a JSON body
        // on stdout. Prefer stderr, and fall back to stdout.
        let text = if stderr.trim().is_empty() { stdout } else { stderr };
        return Err(GhError::Failed {
            code: captured.status.code(),
            message: trim_error(&text),
        });
    }
    Ok(String::from_utf8_lossy(&captured.stdout).into_owned())
}

#[cfg(test)]
pub(super) fn run_with_limit_for_test(
    program: &Path,
    query: &Query<'_>,
    timeout: Duration,
    max_output: usize,
) -> Result<String, GhError> {
    run_limited(program, query, timeout, max_output)
}
