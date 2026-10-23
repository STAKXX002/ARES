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
//! - is killed if it exceeds a timeout.

use std::error::Error;
use std::fmt;
use std::io::{self, Read};
use std::path::Path;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(10);

/// The complete set of git operations ARES is allowed to perform.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Query {
    /// `git status --porcelain=v2 --branch -z`, parsed by `git::status`.
    Status,
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
    let mut child = base_command(repo)
        .args(query.args())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(RunError::Spawn)?;

    // Drain both pipes on their own threads so a large output cannot
    // fill the pipe buffer and stall the child.
    let mut out_pipe = child.stdout.take().expect("stdout is piped");
    let mut err_pipe = child.stderr.take().expect("stderr is piped");
    let out_thread = thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = out_pipe.read_to_end(&mut buf);
        buf
    });
    let err_thread = thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = err_pipe.read_to_end(&mut buf);
        buf
    });

    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    let _ = out_thread.join();
                    let _ = err_thread.join();
                    return Err(RunError::Timeout(timeout));
                }
                thread::sleep(Duration::from_millis(5));
            }
            Err(e) => return Err(RunError::Io(e)),
        }
    };

    let stdout = out_thread.join().unwrap_or_default();
    let stderr = err_thread.join().unwrap_or_default();

    if !status.success() {
        return Err(RunError::Failed {
            code: status.code(),
            stderr: String::from_utf8_lossy(&stderr).trim().to_string(),
        });
    }
    Ok(String::from_utf8_lossy(&stdout).into_owned())
}
