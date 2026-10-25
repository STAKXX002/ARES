//! Per-repository report: the verified facts ARES prints about a repo.
//!
//! Everything here comes from git output, never from inference.

use crate::git::runner::{self, Query, RunError};
use crate::github::{self, RepoRef};
use crate::git::status::{self, Status};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;
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
        let changed_files = s.entries.len() + s.untracked.len();
        debug_assert_eq!(s.is_dirty(), changed_files > 0);
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
            changed_files,
        }
    }

    /// How the current branch is shown: its name, or `(detached)`.
    pub fn branch_label(&self) -> String {
        if self.detached {
            "(detached)".to_string()
        } else {
            self.branch.clone().unwrap_or_else(|| "?".to_string())
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

/// Upper bound on worker threads, whatever the caller asks for.
pub const MAX_WORKERS: usize = 16;

/// Apply `f` to every item using a bounded number of worker threads.
///
/// Results come back in the same order as `items`, so output does not depend
/// on which worker finished first. Workers pull the next index from a shared
/// counter, so one slow item does not hold up the others.
pub fn par_map<T, R, F>(items: &[T], max_parallel: usize, f: F) -> Vec<R>
where
    T: Sync,
    R: Send,
    F: Fn(&T) -> R + Sync,
{
    let workers = max_parallel.clamp(1, MAX_WORKERS).min(items.len().max(1));
    if workers == 1 {
        return items.iter().map(&f).collect();
    }

    let next = AtomicUsize::new(0);
    let mut indexed: Vec<(usize, R)> = thread::scope(|scope| {
        let handles: Vec<_> = (0..workers)
            .map(|_| {
                scope.spawn(|| {
                    let mut local = Vec::new();
                    loop {
                        let i = next.fetch_add(1, Ordering::Relaxed);
                        if i >= items.len() {
                            break;
                        }
                        local.push((i, f(&items[i])));
                    }
                    local
                })
            })
            .collect();
        handles
            .into_iter()
            // Callers report failures as values, so a panic here is a bug.
            .flat_map(|h| h.join().expect("worker panicked"))
            .collect()
    });
    indexed.sort_by_key(|(i, _)| *i);
    indexed.into_iter().map(|(_, r)| r).collect()
}

/// Inspect many repositories with a bounded number of worker threads.
/// Results are in the same order as `paths`.
pub fn inspect_all(paths: &[PathBuf], timeout: Duration, max_parallel: usize) -> Vec<RepoReport> {
    par_map(paths, max_parallel, |p| inspect(p, timeout))
}

/// Where a repository's `origin` points.
///
/// Only the GitHub owner and name are kept. Other remotes are reported as
/// `Other` without their URL, since URLs can embed credentials.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Remote {
    GitHub(RepoRef),
    /// An `origin` that is not a recognizable github.com repository.
    Other,
    /// No `origin` remote is configured.
    None,
}

/// Look up a repository's `origin`. Never contacts the network.
pub fn inspect_remote(path: &Path, timeout: Duration) -> Result<Remote, String> {
    match runner::run(path, Query::RemoteUrl, timeout) {
        Ok(out) => {
            let url = out.trim();
            Ok(match github::parse_remote(url) {
                Some(r) => Remote::GitHub(r),
                None if url.is_empty() => Remote::None,
                None => Remote::Other,
            })
        }
        // `git remote get-url` fails this way when there is no origin.
        Err(RunError::Failed { stderr, .. }) if stderr.contains("No such remote") => {
            Ok(Remote::None)
        }
        Err(e) => Err(e.to_string()),
    }
}

/// Look up the remotes of many repositories in parallel, in input order.
pub fn inspect_remotes(
    paths: &[PathBuf],
    timeout: Duration,
    max_parallel: usize,
) -> Vec<Result<Remote, String>> {
    par_map(paths, max_parallel, |p| inspect_remote(p, timeout))
}

/// A repository paired with where its `origin` points.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteReport {
    pub path: PathBuf,
    pub name: String,
    pub remote: Result<Remote, String>,
}

/// Look up the remote of every repository, in input order.
pub fn remote_reports(paths: &[PathBuf], timeout: Duration, max_parallel: usize) -> Vec<RemoteReport> {
    inspect_remotes(paths, timeout, max_parallel)
        .into_iter()
        .zip(paths)
        .map(|(remote, path)| RemoteReport {
            path: path.clone(),
            name: name_of(path),
            remote,
        })
        .collect()
}

/// Parse the output of the `OriginRefs` query into branch name -> object id.
/// Lines that do not look right, and the symbolic `HEAD`, are skipped.
pub fn parse_origin_refs(text: &str) -> HashMap<String, String> {
    let mut refs = HashMap::new();
    for line in text.lines() {
        if let Some((name, id)) = line.rsplit_once(' ') {
            if name != "HEAD" && !name.is_empty() && crate::git::is_object_id(id) {
                refs.insert(name.to_string(), id.to_string());
            }
        }
    }
    refs
}

/// Where each of origin's branches was at the last fetch. Never contacts the
/// network.
// Remove this allow once `github --online` uses it.
#[allow(dead_code)]
pub fn origin_refs(path: &Path, timeout: Duration) -> Result<HashMap<String, String>, String> {
    runner::run(path, Query::OriginRefs, timeout)
        .map(|t| parse_origin_refs(&t))
        .map_err(|e| e.to_string())
}
