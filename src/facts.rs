//! The verified facts that an AI layer is allowed to reason over.
//!
//! Every fact is a short fixed sentence built from numbers that ARES read
//! itself (git state, commit dates, GitHub counts), attached to a repository
//! name. Nothing else goes in: no git error text, branch names, commit
//! messages, or file contents. That keeps repository-controlled text out of
//! anything a model reads, apart from the repository's directory name.
//!
//! Facts get stable ids (`F1`, `F2`, ...) so that a model's statements can
//! cite them and ARES can check the citations.

use crate::gh::activity::{Activity, CiState, SyncState};
use crate::repo::RepoReport;
use crate::stale::StaleReport;
use std::collections::{HashMap, HashSet};
use std::path::Path;

/// Most facts kept. When there are more, the lowest-priority ones are
/// dropped and counted in [`Facts::omitted`].
pub const MAX_FACTS: usize = 150;

/// Longest repository name kept.
const MAX_NAME: usize = 60;

/// What a fact is about. The order is the priority order: facts are numbered
/// in this order, so the most urgent facts get the lowest ids and are the last
/// to be dropped when there are too many.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Kind {
    Summary,
    Conflicted,
    Unreadable,
    Uncommitted,
    Unpushed,
    Behind,
    Detached,
    FetchNeeded,
    FailingCi,
    OpenPrs,
    OpenIssues,
    GitHubUnavailable,
    Quiet,
    NoCommits,
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Summary => "summary",
            Kind::Conflicted => "conflicted",
            Kind::Unreadable => "unreadable",
            Kind::Uncommitted => "uncommitted",
            Kind::Unpushed => "unpushed",
            Kind::Behind => "behind",
            Kind::Detached => "detached",
            Kind::FetchNeeded => "fetch_needed",
            Kind::FailingCi => "failing_ci",
            Kind::OpenPrs => "open_prs",
            Kind::OpenIssues => "open_issues",
            Kind::GitHubUnavailable => "github_unavailable",
            Kind::Quiet => "quiet",
            Kind::NoCommits => "no_commits",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fact {
    /// 1-based; shown as `F{id}`.
    pub id: usize,
    pub kind: Kind,
    /// Index into [`Facts::names`], or `None` for facts about the whole scan.
    pub subject: Option<usize>,
    /// The rest of the sentence after the subject, such as
    /// `has 3 uncommitted files (...).`
    pub predicate: String,
}

impl Fact {
    pub fn label(&self) -> String {
        format!("F{}", self.id)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Facts {
    /// Repository names, cleaned and made unique. Indexed by `Fact::subject`.
    pub names: Vec<String>,
    pub facts: Vec<Fact>,
    /// How many facts were dropped because there were more than `MAX_FACTS`.
    pub omitted: usize,
}

impl Facts {
    /// Look a fact up by its label, such as `F12`.
    ///
    /// Only the exact canonical form matches: `F3`, never `F03`, `F+3`,
    /// `f3`, or text with spaces. Citations are checked with this.
    pub fn find(&self, label: &str) -> Option<&Fact> {
        let id: usize = label.strip_prefix('F')?.parse().ok()?;
        if format!("F{id}") != label {
            return None;
        }
        self.facts.iter().find(|f| f.id == id)
    }

    /// The name used for a fact's subject: the real name, or `R{n}` when
    /// aliasing so that real names never leave the machine.
    pub fn subject_name(&self, fact: &Fact, alias: bool) -> Option<String> {
        fact.subject.map(|i| {
            if alias {
                format!("R{}", i + 1)
            } else {
                self.names[i].clone()
            }
        })
    }
}

fn plural(n: u64, one: &str, many: &str) -> String {
    format!("{} {}", n, if n == 1 { one } else { many })
}

/// Make a directory name safe to show to a model: control characters become
/// `?` and the length is capped.
fn clean_name(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_control() { '?' } else { c })
        .take(MAX_NAME)
        .collect()
}

/// Clean names, adding `#2`, `#3`, ... to repeats so each name is unique.
fn unique_names(reports: &[RepoReport]) -> Vec<String> {
    let mut seen: HashMap<String, usize> = HashMap::new();
    let mut taken: HashSet<String> = HashSet::new();
    reports
        .iter()
        .map(|r| {
            let base = clean_name(&r.name);
            let n = seen.entry(base.clone()).or_insert(0);
            *n += 1;
            let mut name = if *n == 1 {
                base.clone()
            } else {
                format!("{base} #{n}")
            };
            while !taken.insert(name.clone()) {
                name.push('_');
            }
            name
        })
        .collect()
}

type Raw = (Kind, Option<usize>, String);

fn local_facts(i: usize, r: &RepoReport, raw: &mut Vec<Raw>) {
    let s = match &r.state {
        Err(_) => {
            raw.push((Kind::Unreadable, Some(i), "could not be read.".to_string()));
            return;
        }
        Ok(s) => s,
    };
    if s.conflicted > 0 {
        raw.push((
            Kind::Conflicted,
            Some(i),
            format!("has {} conflicted.", plural(s.conflicted as u64, "file", "files")),
        ));
    }
    if s.changed_files > 0 {
        raw.push((
            Kind::Uncommitted,
            Some(i),
            format!(
                "has {} uncommitted ({} staged, {} unstaged, {} untracked).",
                plural(s.changed_files as u64, "file", "files"),
                s.staged,
                s.unstaged,
                s.untracked
            ),
        ));
    }
    if let Some(a) = s.ahead.filter(|a| *a > 0) {
        raw.push((
            Kind::Unpushed,
            Some(i),
            format!("has {} not yet pushed to its upstream.", plural(a as u64, "commit", "commits")),
        ));
    }
    if let Some(b) = s.behind.filter(|b| *b > 0) {
        raw.push((
            Kind::Behind,
            Some(i),
            format!(
                "is {} behind its upstream as of the last fetch.",
                plural(b as u64, "commit", "commits")
            ),
        ));
    }
    if s.detached {
        raw.push((Kind::Detached, Some(i), "is on a detached HEAD.".to_string()));
    }
    if s.unborn {
        raw.push((Kind::NoCommits, Some(i), "has no commits yet.".to_string()));
    }
}

fn online_facts(i: usize, result: &Result<Activity, String>, raw: &mut Vec<Raw>) {
    let a = match result {
        Err(_) => {
            raw.push((
                Kind::GitHubUnavailable,
                Some(i),
                "could not be checked on GitHub.".to_string(),
            ));
            return;
        }
        Ok(a) => a,
    };
    match &a.sync {
        Some(SyncState::FetchNeeded { new_commits }) => raw.push((
            Kind::FetchNeeded,
            Some(i),
            format!(
                "has {} on GitHub that are not fetched yet.",
                plural(*new_commits, "new commit", "new commits")
            ),
        )),
        Some(SyncState::Diverged { .. }) => raw.push((
            Kind::FetchNeeded,
            Some(i),
            "has diverged from GitHub: each side has commits the other lacks.".to_string(),
        )),
        _ => {}
    }
    if a.ci.as_ref().map(|c| c.state()) == Some(CiState::Failing) {
        raw.push((
            Kind::FailingCi,
            Some(i),
            "latest CI run on the default branch is failing.".to_string(),
        ));
    }
    if a.open_prs > 0 {
        let text = if a.prs_capped {
            format!("has {} or more open pull requests.", a.open_prs)
        } else {
            format!("has {} open.", plural(a.open_prs as u64, "pull request", "pull requests"))
        };
        raw.push((Kind::OpenPrs, Some(i), text));
    }
    if let Some(n) = a.open_issues.filter(|n| *n > 0) {
        raw.push((
            Kind::OpenIssues,
            Some(i),
            format!("has {} open.", plural(n, "issue", "issues")),
        ));
    }
}

/// Build the facts for one scan.
///
/// `online`, when given, must line up with `reports` (as produced for
/// `github --online`): `None` for repositories that are not on GitHub.
pub fn build(
    reports: &[RepoReport],
    stale: &StaleReport,
    online: Option<&[Option<Result<Activity, String>>]>,
) -> Facts {
    let names = unique_names(reports);
    let mut raw: Vec<Raw> = vec![
        (
            Kind::Summary,
            None,
            format!("{} scanned.", plural(reports.len() as u64, "repository was", "repositories were")),
        ),
        (
            Kind::Summary,
            None,
            format!(
                "A repository counts as quiet after {} without a commit.",
                plural(stale.days, "day", "days")
            ),
        ),
    ];

    for (i, r) in reports.iter().enumerate() {
        local_facts(i, r, &mut raw);
        if let Some(Some(result)) = online.map(|o| o.get(i)).flatten() {
            online_facts(i, result, &mut raw);
        }
    }

    let by_path: HashMap<&Path, usize> = reports
        .iter()
        .enumerate()
        .map(|(i, r)| (r.path.as_path(), i))
        .collect();
    for item in &stale.inactive {
        if let Some(&i) = by_path.get(item.path.as_path()) {
            raw.push((
                Kind::Quiet,
                Some(i),
                format!("has had no commit for {}.", plural(item.age_days, "day", "days")),
            ));
        }
    }

    // Stable sort: by kind priority, then by repository, keeping insertion
    // order for facts of the same kind and repository.
    raw.sort_by_key(|(kind, subject, _)| (*kind, subject.unwrap_or(0)));

    let omitted = raw.len().saturating_sub(MAX_FACTS);
    raw.truncate(MAX_FACTS);
    let facts = raw
        .into_iter()
        .enumerate()
        .map(|(n, (kind, subject, predicate))| Fact {
            id: n + 1,
            kind,
            subject,
            predicate,
        })
        .collect();

    Facts {
        names,
        facts,
        omitted,
    }
}
