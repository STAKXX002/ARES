//! Per-repository report: the verified facts ARES prints about a repo.
//!
//! Everything here comes from git output, never from inference.

use crate::git::runner::{self, Query};
use crate::git::status::{self, Status};
use std::path::{Path, PathBuf};
use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoState {
    pub branch: Option<String>,
    pub detached: bool,
    /// True on a branch with no commits yet.
    pub unborn: bool,
    pub upstream: Option<String>,
    /// Commits not yet pushed. `None` when there is no upstream to compare to.
    pub ahead: Option<u32>,
    /// Commits on the upstream not yet integrated, as of the last fetch.
    pub behind: Option<u32>,
    pub staged: usize,
    pub unstaged: usize,
    pub untracked: usize,
    pub conflicted: usize,
    /// Distinct paths with changes, counting untracked files.
    pub changed_files: usize,
}

impl RepoState {
    pub fn from_status(s: &Status) -> RepoState {
        RepoState {
            branch: s.branch.head.clone(),
            detached: s.branch.detached,
            unborn: s.branch.oid.is_none(),
            upstream: s.branch.upstream.clone(),
            ahead: s.branch.ahead_behind.map(|(a, _)| a),
            behind: s.branch.ahead_behind.map(|(_, b)| b),
            staged: s.staged(),
            unstaged: s.unstaged(),
            untracked: s.untracked_count(),
            conflicted: s.conflicted(),
            changed_files: s.entries.len() + s.untracked.len(),
        }
    }

    /// Uncommitted work of any kind.
    pub fn is_dirty(&self) -> bool {
        self.changed_files > 0
    }

    pub fn has_unpushed(&self) -> bool {
        self.ahead.unwrap_or(0) > 0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoReport {
    pub path: PathBuf,
    pub name: String,
    /// The state, or the reason it could not be read.
    pub state: Result<RepoState, String>,
}

fn name_of(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

impl RepoReport {
    pub fn new(path: &Path, state: Result<RepoState, String>) -> RepoReport {
        RepoReport {
            path: path.to_path_buf(),
            name: name_of(path),
            state,
        }
    }

    pub fn is_dirty(&self) -> bool {
        matches!(&self.state, Ok(s) if s.is_dirty())
    }

    pub fn is_error(&self) -> bool {
        self.state.is_err()
    }
}

/// Inspect one repository by running read-only git and parsing the result.
/// Failures are captured in the report rather than aborting the whole scan.
pub fn inspect(path: &Path, timeout: Duration) -> RepoReport {
    let state = runner::run(path, Query::Status, timeout)
        .map_err(|e| e.to_string())
        .and_then(|raw| status::parse(&raw).map_err(|e| format!("could not parse git output: {e}")))
        .map(|s| RepoState::from_status(&s));
    RepoReport::new(path, state)
}
