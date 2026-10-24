//! Turns repository reports into text. Pure: no I/O, no clock.
//!
//! Layout is always: headline, repository lines, optional footer, warnings.
//! The repository lines are facts and are identical for every voice. A
//! voice may add a headline, a footer, and short notes under notable
//! repositories, but it cannot alter the fact lines.

use crate::repo::{RepoReport, RepoState};
use crate::voice::{seed_from, Summary, Voice};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy)]
pub struct RenderOptions {
    pub voice: Voice,
    /// Days since the Unix epoch. Varies wording from day to day.
    pub day: u64,
    /// Show only repositories with uncommitted changes (and any that could
    /// not be read, since their state is unknown).
    pub dirty_only: bool,
}

const MAX_NAME_WIDTH: usize = 30;
const MAX_BRANCH_WIDTH: usize = 20;

/// The factual description of a repository's state.
pub fn facts(s: &RepoState) -> String {
    let mut parts: Vec<String> = Vec::new();
    if s.changed_files == 0 {
        parts.push("clean".to_string());
    } else {
        parts.push(format!(
            "{} changed ({} staged, {} unstaged, {} untracked)",
            s.changed_files, s.staged, s.unstaged, s.untracked
        ));
    }
    if s.conflicted > 0 {
        parts.push(format!("{} conflicted", s.conflicted));
    }
    if s.unborn {
        parts.push("no commits yet".to_string());
    } else if s.upstream.is_none() {
        parts.push("no upstream".to_string());
    } else {
        if s.ahead.unwrap_or(0) > 0 {
            parts.push(format!("{} ahead", s.ahead.unwrap_or(0)));
        }
        if s.behind.unwrap_or(0) > 0 {
            parts.push(format!("{} behind", s.behind.unwrap_or(0)));
        }
    }
    parts.join(", ")
}

fn branch_label(s: &RepoState) -> String {
    if s.detached {
        "(detached)".to_string()
    } else {
        s.branch.clone().unwrap_or_else(|| "?".to_string())
    }
}

fn single_line(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Replace control characters (such as ESC) with `?`.
///
/// Directory names and git error text come from the filesystem and from
/// repositories, which may be attacker-controlled. Printing them raw would
/// let a crafted name inject terminal escape sequences into the output.
fn clean(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_control() { '?' } else { c })
        .collect()
}

/// Render the full `repos` report.
pub fn repos(
    reports: &[RepoReport],
    scan_errors: &[(PathBuf, String)],
    opts: &RenderOptions,
) -> String {
    let summary = Summary::from_reports(reports);
    let mut out: Vec<String> = Vec::new();
    out.push(opts.voice.headline(&summary, seed_from("headline", opts.day)));

    let shown: Vec<&RepoReport> = reports
        .iter()
        .filter(|r| !opts.dirty_only || r.is_dirty() || r.is_error())
        .collect();

    if opts.dirty_only && summary.total > 0 {
        out.push("Showing only repositories with uncommitted changes.".to_string());
    }

    if !shown.is_empty() {
        let name_w = shown
            .iter()
            .map(|r| clean(&r.name).chars().count())
            .max()
            .unwrap_or(0)
            .min(MAX_NAME_WIDTH);
        let branch_w = shown
            .iter()
            .filter_map(|r| r.state.as_ref().ok())
            .map(|s| clean(&branch_label(s)).chars().count())
            .max()
            .unwrap_or(0)
            .min(MAX_BRANCH_WIDTH);

        out.push(String::new());
        for r in &shown {
            match &r.state {
                Ok(s) => {
                    out.push(format!(
                        "{:<nw$}  {:<bw$}  {}",
                        clean(&r.name),
                        clean(&branch_label(s)),
                        facts(s),
                        nw = name_w,
                        bw = branch_w
                    ));
                    if let Some(note) = opts.voice.repo_note(r, seed_from(&r.name, opts.day)) {
                        out.push(format!("    {note}"));
                    }
                }
                Err(e) => out.push(format!(
                    "{:<nw$}  ERROR: {}",
                    clean(&r.name),
                    clean(&single_line(e)),
                    nw = name_w
                )),
            }
        }
    }

    if let Some(footer) = opts.voice.footer(&summary, seed_from("footer", opts.day)) {
        out.push(String::new());
        out.push(footer);
    }

    if !scan_errors.is_empty() {
        out.push(String::new());
        for (path, msg) in scan_errors {
            out.push(format!(
                "warning: could not read {}: {}",
                clean(&path.display().to_string()),
                clean(&single_line(msg))
            ));
        }
    }

    let mut text = out.join("\n");
    text.push('\n');
    text
}
