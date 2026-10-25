//! Read-only git command runner.
//!
//! Safety model: callers cannot pass arbitrary arguments. They pick a
//! [`Query`] variant, and each variant maps to a fixed, reviewed argument
//! list. Subcommands such as `branch` and `remote` can mutate state when
//! given the wrong flags, so an argument filter would be fragile. A closed
//! enum is safe by construction.
//!
//! Every invocation also:
//! - passes `--no-optional-locks`, so `git status` does not refresh and
//!   rewrite the index inside the repository,
//! - disables `core.fsmonitor`, which can execute a configured program,
//! - disables terminal prompts and forces the `C` locale,
//! - is killed if it exceeds a timeout or produces too much output.

use crate::proc::{self, ProcError};
use crate::redact::redact;
use std::error::Error;
use std::fmt;
use std::io;
use std::path::Path;
use std::process::Command;
use std::time::Duration;

/// Most stdout kept from one git invocation.
const MAX_OUTPUT: usize = 32 * 1024 * 1024;

/// The complete set of git operations ARES is allowed to perform.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Query {
    /// `git status --porcelain=v2 --branch -z`, parsed by `git::status`.
    Status,
    /// `git remote get-url origin`. Read-only; resolves URL rewrites but
    /// contacts nothing.
    RemoteUrl,
    /// `git for-each-ref` over `refs/remotes/origin`: the local record of
    /// where each of origin's branches was at the last fetch. Prints
    /// `<branch> <object id>` per line. Read-only; contacts nothing.
    OriginRefs,
    /// `git log -1 --branches --format=%ct`: the commit time (seconds since
    /// the epoch) of the newest commit on any local branch. Prints nothing
    /// when there are no commits.
    LastCommitTime,
}

impl Query {
    fn args(self) -> &'static [&'static str] {
        match self {
            Query::Status => &[
                "status",
                "--porcelain=v2",
                "--branch",
                "-z",
                "--untracked-files=normal",
            ],
            Query::RemoteUrl => &["remote", "get-url", "origin"],
            Query::OriginRefs => &[
                "for-each-ref",
                "--format=%(refname:strip=3) %(objectname)",
                "refs/remotes/origin",
            ],
            Query::LastCommitTime => &["log", "-1", "--branches", "--format=%ct"],
        }
    }
}

#[derive(Debug)]
pub enum RunError {
    /// git could not be started (for example, not installed).
    Spawn(io::Error),
    /// Waiting on the process failed.
    Io(io::Error),
    /// git exited with a non-zero status.
    Failed { code: Option<i32>, stderr: String },
    /// git did not finish within the timeout and was killed.
    Timeout(Duration),
    /// git produced more output than ARES will keep.
    OutputTooLarge { limit: usize },
}

impl fmt::Display for RunError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RunError::Spawn(e) => write!(f, "could not start git: {e}"),
            RunError::Io(e) => write!(f, "i/o error while running git: {e}"),
            RunError::Failed { code, stderr } => match code {
                Some(c) => write!(f, "git exited with status {c}: {stderr}"),
                None => write!(f, "git was terminated by a signal: {stderr}"),
            },
            RunError::Timeout(d) => write!(f, "git timed out after {}s", d.as_secs()),
            RunError::OutputTooLarge { limit } => {
                write!(f, "git produced more than {limit} bytes of output")
            }
        }
    }
}

impl Error for RunError {}

fn base_command(repo: &Path) -> Command {
    let mut cmd = Command::new("git");
    cmd.arg("--no-optional-locks")
        .args(["-c", "core.fsmonitor=false"])
        .arg("-C")
        .arg(repo)
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("LC_ALL", "C");
    cmd
}

/// Run a query in `repo` and return its stdout, decoded lossily as UTF-8.
pub fn run(repo: &Path, query: Query, timeout: Duration) -> Result<String, RunError> {
    run_limited(repo, query, timeout, MAX_OUTPUT)
}

fn run_limited(
    repo: &Path,
    query: Query,
    timeout: Duration,
    max_output: usize,
) -> Result<String, RunError> {
    let mut cmd = base_command(repo);
    cmd.args(query.args());
    let captured = proc::run_capture(cmd, timeout, max_output).map_err(|e| match e {
        ProcError::Spawn(e) => RunError::Spawn(e),
        ProcError::Io(e) => RunError::Io(e),
        ProcError::Timeout(d) => RunError::Timeout(d),
        ProcError::OutputTooLarge { limit } => RunError::OutputTooLarge { limit },
    })?;

    if !captured.status.success() {
        return Err(RunError::Failed {
            code: captured.status.code(),
            // Error text can echo a remote URL that contains a password.
            stderr: redact(String::from_utf8_lossy(&captured.stderr).trim()),
        });
    }
    Ok(String::from_utf8_lossy(&captured.stdout).into_owned())
}

#[cfg(test)]
pub(super) fn run_with_limit_for_test(
    repo: &Path,
    query: Query,
    timeout: Duration,
    max_output: usize,
) -> Result<String, RunError> {
    run_limited(repo, query, timeout, max_output)
}
