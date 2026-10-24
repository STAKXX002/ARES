//! Comparing two recorded scans. Pure: takes reports, returns differences.
//!
//! Only facts that both scans recorded are compared. A repository whose
//! upstream was unknown in one scan is never described as "pushed" because
//! of that.

use crate::repo::RepoReport;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Change {
    BecameDirty { files: usize },
    BecameClean { was: usize },
    FilesChanged { from: usize, to: usize },
    NewlyUnpushed { commits: u32 },
    Pushed { was: u32 },
    UnpushedChanged { from: u32, to: u32 },
    BranchChanged { from: String, to: String },
    BecameUnreadable,
    ReadableAgain,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoChange {
    pub name: String,
    pub path: PathBuf,
    pub change: Change,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Changes {
    /// Repositories present now but not in the previous scan.
    pub added: Vec<String>,
    /// Repositories present before but not now.
    pub removed: Vec<String>,
    /// Per-repository differences, ordered by path.
    pub changes: Vec<RepoChange>,
}

impl Changes {
    pub fn is_empty(&self) -> bool {
        self.added.is_empty() && self.removed.is_empty() && self.changes.is_empty()
    }
}

fn by_path(reports: &[RepoReport]) -> BTreeMap<&Path, &RepoReport> {
    reports.iter().map(|r| (r.path.as_path(), r)).collect()
}

pub fn diff(previous: &[RepoReport], current: &[RepoReport]) -> Changes {
    let prev = by_path(previous);
    let cur = by_path(current);
    let mut out = Changes::default();

    for (path, r) in &cur {
        let before = match prev.get(path) {
            None => {
                out.added.push(r.name.clone());
                continue;
            }
            Some(b) => b,
        };
        let mut push = |change: Change| {
            out.changes.push(RepoChange {
                name: r.name.clone(),
                path: path.to_path_buf(),
                change,
            })
        };

        match (&before.state, &r.state) {
            (Ok(_), Err(_)) => push(Change::BecameUnreadable),
            (Err(_), Ok(_)) => push(Change::ReadableAgain),
            (Err(_), Err(_)) => {}
            (Ok(p), Ok(c)) => {
                match (p.is_dirty(), c.is_dirty()) {
                    (false, true) => push(Change::BecameDirty { files: c.changed_files }),
                    (true, false) => push(Change::BecameClean { was: p.changed_files }),
                    (true, true) if p.changed_files != c.changed_files => {
                        push(Change::FilesChanged {
                            from: p.changed_files,
                            to: c.changed_files,
                        })
                    }
                    _ => {}
                }

                let (pa, ca) = (p.ahead.unwrap_or(0), c.ahead.unwrap_or(0));
                match (pa > 0, ca > 0) {
                    (false, true) => push(Change::NewlyUnpushed { commits: ca }),
                    // Only claim "pushed" when the current scan really knows
                    // the upstream state.
                    (true, false) if c.ahead.is_some() => push(Change::Pushed { was: pa }),
                    (true, true) if pa != ca => push(Change::UnpushedChanged { from: pa, to: ca }),
                    _ => {}
                }

                if p.branch_label() != c.branch_label() {
                    push(Change::BranchChanged {
                        from: p.branch_label(),
                        to: c.branch_label(),
                    });
                }
            }
        }
    }

    for (path, r) in &prev {
        if !cur.contains_key(path) {
            out.removed.push(r.name.clone());
        }
    }
    out
}
