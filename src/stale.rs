//! Finding repositories that have gone quiet. Pure: takes already-gathered
//! facts and returns a classification.
//!
//! "Quiet" means the newest commit on any local branch is at least `days`
//! old. That is a fact about git history, not a guess about intent: a repo
//! can be quiet because it is finished.

use crate::repo::RepoReport;
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StaleItem {
    pub name: String,
    pub path: PathBuf,
    /// Whole days since the newest commit.
    pub age_days: u64,
    /// Files with uncommitted changes. Non-zero on a quiet repo suggests
    /// unfinished work.
    pub dirty_files: usize,
    /// Commits not yet pushed to the upstream.
    pub unpushed: u32,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StaleReport {
    pub days: u64,
    pub total: usize,
    /// Quiet repositories, longest-quiet first.
    pub inactive: Vec<StaleItem>,
    /// Repositories that have no commits yet.
    pub no_commits: Vec<String>,
    /// Repositories whose state or history could not be read.
    pub unreadable: Vec<String>,
}

impl StaleReport {
    /// Repositories with recent commits.
    pub fn active(&self) -> usize {
        self.total - self.inactive.len() - self.no_commits.len() - self.unreadable.len()
    }
}

const DAY: u64 = 86_400;

/// Classify repositories by the age of their newest commit.
///
/// Each entry pairs a repository's working-tree report with the result of
/// looking up its newest commit time.
pub fn classify(
    repos: &[(RepoReport, Result<Option<u64>, String>)],
    now: u64,
    days: u64,
) -> StaleReport {
    let mut out = StaleReport {
        days,
        total: repos.len(),
        ..StaleReport::default()
    };

    for (report, last) in repos {
        let (state, time) = match (&report.state, last) {
            (Ok(s), Ok(t)) => (s, *t),
            _ => {
                out.unreadable.push(report.name.clone());
                continue;
            }
        };
        match time {
            None => out.no_commits.push(report.name.clone()),
            Some(t) => {
                // A commit dated in the future counts as brand new.
                let age_days = now.saturating_sub(t) / DAY;
                if age_days >= days {
                    out.inactive.push(StaleItem {
                        name: report.name.clone(),
                        path: report.path.clone(),
                        age_days,
                        dirty_files: state.changed_files,
                        unpushed: state.ahead.unwrap_or(0),
                    });
                }
            }
        }
    }

    out.inactive
        .sort_by(|a, b| b.age_days.cmp(&a.age_days).then_with(|| a.path.cmp(&b.path)));
    out
}
