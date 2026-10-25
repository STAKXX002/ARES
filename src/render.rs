//! Turns repository reports into text. Pure: no I/O, no clock.
//!
//! Layout is always: headline, repository lines, optional footer, warnings.
//! The repository lines are facts and are identical for every voice. A
//! voice may add a headline, a footer, and short notes under notable
//! repositories, but it cannot alter the fact lines.

use crate::changes::{Change, Changes};
use crate::gh::activity::{Activity, CiState};
use crate::repo::{Remote, RemoteReport, RepoReport, RepoState};
use crate::store::ScanRecord;
use crate::timefmt;
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
const MAX_FULL_NAME_WIDTH: usize = 40;

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
            .map(|s| clean(&s.branch_label()).chars().count())
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
                        clean(&s.branch_label()),
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

    finish(out, &summary, scan_errors, opts)
}

/// One repository's GitHub details as a comma-separated phrase.
fn activity_details(a: &Activity, now: u64) -> String {
    let mut p: Vec<String> = Vec::new();
    if a.private {
        p.push("private".to_string());
    }
    if a.archived {
        p.push("archived".to_string());
    }
    if a.fork {
        p.push("fork".to_string());
    }
    p.push(if a.prs_capped {
        format!("{}+ open PRs", a.open_prs)
    } else {
        format!("{} open {}", a.open_prs, if a.open_prs == 1 { "PR" } else { "PRs" })
    });
    p.push(match a.open_issues {
        Some(n) => format!("{} open {}", n, if n == 1 { "issue" } else { "issues" }),
        None => "open issues unknown".to_string(),
    });
    p.push(match (&a.ci, &a.ci_error) {
        (Some(c), _) => {
            let on = c
                .branch
                .as_deref()
                .map(|b| format!(" on {}", clean(b)))
                .unwrap_or_default();
            match c.state() {
                CiState::Passing => format!("CI passing{on}"),
                CiState::Failing => format!("CI failing{on}"),
                CiState::Running => format!("CI running{on}"),
                CiState::Other(s) => format!("CI {}{on}", clean(&s)),
            }
        }
        (None, Some(_)) => "CI unavailable".to_string(),
        (None, None) => "no CI runs".to_string(),
    });
    if let Some(t) = a.pushed_at {
        p.push(format!("pushed {}", timefmt::ago(now, t)));
    }
    p.join(", ")
}

/// Render `github --online`: the offline listing plus each repository's
/// open pull requests, open issues, latest CI result, and last push.
///
/// `online[i]` belongs to `reports[i]` and is `None` for repositories that
/// are not on GitHub. When every lookup failed with the same message (gh
/// missing, not logged in, rate limited), that is said once.
pub fn github_online(
    reports: &[RemoteReport],
    online: &[Option<Result<Activity, String>>],
    scan_errors: &[(PathBuf, String)],
    now: u64,
) -> String {
    debug_assert_eq!(reports.len(), online.len());
    let mut out: Vec<String> = vec![remote_headline(reports)];

    let results: Vec<&Result<Activity, String>> = online.iter().flatten().collect();
    let checked = results.len();
    let errors: Vec<&String> = results.iter().filter_map(|r| r.as_ref().err()).collect();
    let uniform_error = match errors.first() {
        Some(first) if errors.len() == checked && errors.iter().all(|e| e == first) => Some(*first),
        _ => None,
    };

    if let Some(msg) = uniform_error {
        out.push(format!("GitHub could not be queried: {}", clean(&single_line(msg))));
    } else if checked > 0 {
        let ok: Vec<&Activity> = results.iter().filter_map(|r| r.as_ref().ok()).collect();
        let failing = ok
            .iter()
            .filter(|a| a.ci.as_ref().map(|c| c.state()) == Some(CiState::Failing))
            .count();
        let with_prs = ok.iter().filter(|a| a.open_prs > 0).count();
        let mut line = format!(
            "Checked {checked} on GitHub: {failing} with failing CI, {with_prs} with open pull requests"
        );
        if !errors.is_empty() {
            line.push_str(&format!(", {} could not be checked", errors.len()));
        }
        line.push('.');
        out.push(line);
    }

    if !reports.is_empty() {
        let name_w = reports
            .iter()
            .map(|r| clean(&r.name).chars().count())
            .max()
            .unwrap_or(0)
            .min(MAX_NAME_WIDTH);
        let full_w = reports
            .iter()
            .filter_map(|r| match &r.remote {
                Ok(Remote::GitHub(g)) => Some(clean(&g.full_name()).chars().count()),
                _ => None,
            })
            .max()
            .unwrap_or(0)
            .min(MAX_FULL_NAME_WIDTH);
        out.push(String::new());
        for (r, result) in reports.iter().zip(online) {
            let line = match (&r.remote, result) {
                (Ok(Remote::GitHub(g)), Some(Ok(a))) => format!(
                    "{:<nw$}  {:<fw$}  {}",
                    clean(&r.name),
                    clean(&g.full_name()),
                    activity_details(a, now),
                    nw = name_w,
                    fw = full_w
                ),
                (Ok(Remote::GitHub(g)), Some(Err(e))) if uniform_error.is_none() => format!(
                    "{:<nw$}  {:<fw$}  ERROR: {}",
                    clean(&r.name),
                    clean(&g.full_name()),
                    clean(&single_line(e)),
                    nw = name_w,
                    fw = full_w
                ),
                _ => format!("{:<nw$}  {}", clean(&r.name), remote_text(r), nw = name_w),
            };
            out.push(line);
        }
    }

    push_scan_warnings(&mut out, scan_errors);
    join_lines(out)
}

/// How many repository names to list per attention group.
const MAX_LISTED: usize = 8;

fn list_names(items: &[String]) -> String {
    if items.len() <= MAX_LISTED {
        items.join(", ")
    } else {
        format!(
            "{}, +{} more",
            items[..MAX_LISTED].join(", "),
            items.len() - MAX_LISTED
        )
    }
}

/// Render the `status` overview: the headline, then which repositories need
/// attention, grouped by reason. Groups are fixed and factual.
pub fn status(
    reports: &[RepoReport],
    scan_errors: &[(PathBuf, String)],
    opts: &RenderOptions,
) -> String {
    let summary = Summary::from_reports(reports);
    let mut out: Vec<String> = vec![opts.voice.headline(&summary, seed_from("headline", opts.day))];

    let mut conflicted = Vec::new();
    let mut uncommitted = Vec::new();
    let mut unpushed = Vec::new();
    let mut behind = Vec::new();
    let mut unreadable = Vec::new();
    for r in reports {
        let name = clean(&r.name);
        match &r.state {
            Err(_) => unreadable.push(name),
            Ok(s) => {
                if s.conflicted > 0 {
                    conflicted.push(name.clone());
                }
                if s.is_dirty() {
                    uncommitted.push(format!("{} ({})", name, s.changed_files));
                }
                if s.has_unpushed() {
                    unpushed.push(format!("{} ({})", name, s.ahead.unwrap_or(0)));
                }
                if s.behind.unwrap_or(0) > 0 {
                    behind.push(format!("{} ({})", name, s.behind.unwrap_or(0)));
                }
            }
        }
    }

    let groups = [
        ("conflicted", conflicted),
        ("uncommitted", uncommitted),
        ("unpushed", unpushed),
        ("behind", behind),
        ("unreadable", unreadable),
    ];
    if groups.iter().any(|(_, v)| !v.is_empty()) {
        out.push(String::new());
        for (label, items) in &groups {
            if !items.is_empty() {
                out.push(format!("{:<13}{}", format!("{label}:"), list_names(items)));
            }
        }
    }

    finish(out, &summary, scan_errors, opts)
}

fn scan_label(s: &ScanRecord, now: u64) -> String {
    format!(
        "scan #{} ({}, {})",
        s.id,
        timefmt::ago(now, s.started_at),
        timefmt::format_utc(s.started_at)
    )
}

/// Render the `changes` report comparing two recorded scans.
pub fn changes(
    previous: &ScanRecord,
    current: &ScanRecord,
    found: &Changes,
    now: u64,
    opts: &RenderOptions,
) -> String {
    let mut out: Vec<String> = vec![
        format!(
            "Comparing {} with {}.",
            scan_label(previous, now),
            scan_label(current, now)
        ),
        String::new(),
    ];

    if found.is_empty() {
        out.push("No changes between these scans.".to_string());
    } else {
        let names = |v: &[String]| v.iter().map(|n| clean(n)).collect::<Vec<_>>();
        let mut groups: Vec<(&str, Vec<String>)> = vec![
            ("new:", names(&found.added)),
            ("gone:", names(&found.removed)),
        ];
        let mut pick_kind = |label: &'static str, f: &dyn Fn(&Change) -> Option<String>| {
            let items: Vec<String> = found
                .changes
                .iter()
                .filter_map(|c| f(&c.change).map(|d| format!("{}{}", clean(&c.name), d)))
                .collect();
            groups.push((label, items));
        };
        pick_kind("became dirty:", &|c| match c {
            Change::BecameDirty { files } => Some(format!(" ({files} files)")),
            _ => None,
        });
        pick_kind("became clean:", &|c| match c {
            Change::BecameClean { was } => Some(format!(" (was {was})")),
            _ => None,
        });
        pick_kind("files changed:", &|c| match c {
            Change::FilesChanged { from, to } => Some(format!(" ({from} -> {to})")),
            _ => None,
        });
        pick_kind("newly unpushed:", &|c| match c {
            Change::NewlyUnpushed { commits } => Some(format!(" ({commits} commits)")),
            _ => None,
        });
        pick_kind("pushed:", &|c| match c {
            Change::Pushed { was } => Some(format!(" (was {was})")),
            _ => None,
        });
        pick_kind("unpushed changed:", &|c| match c {
            Change::UnpushedChanged { from, to } => Some(format!(" ({from} -> {to})")),
            _ => None,
        });
        pick_kind("branch changed:", &|c| match c {
            Change::BranchChanged { from, to } => {
                Some(format!(" ({} -> {})", clean(from), clean(to)))
            }
            _ => None,
        });
        pick_kind("unreadable now:", &|c| match c {
            Change::BecameUnreadable => Some(String::new()),
            _ => None,
        });
        pick_kind("readable again:", &|c| match c {
            Change::ReadableAgain => Some(String::new()),
            _ => None,
        });

        for (label, items) in &groups {
            if !items.is_empty() {
                out.push(format!("{:<19}{}", label, list_names(items)));
            }
        }
    }

    let total = found.added.len() + found.removed.len() + found.changes.len();
    if let Some(remark) = opts.voice.changes_remark(total, seed_from("remark", opts.day)) {
        out.push(String::new());
        out.push(remark);
    }

    let mut text = out.join("\n");
    text.push('\n');
    text
}

/// "17 repositories: 16 on GitHub, 0 with another remote, 1 with no remote."
fn remote_headline(reports: &[RemoteReport]) -> String {
    let (mut github, mut other, mut none, mut errors) = (0, 0, 0, 0);
    for r in reports {
        match &r.remote {
            Ok(Remote::GitHub(_)) => github += 1,
            Ok(Remote::Other) => other += 1,
            Ok(Remote::None) => none += 1,
            Err(_) => errors += 1,
        }
    }
    if reports.is_empty() {
        return "No repositories found.".to_string();
    }
    let mut head = format!(
        "{}: {} on GitHub, {} with another remote, {} with no remote.",
        if reports.len() == 1 {
            "1 repository".to_string()
        } else {
            format!("{} repositories", reports.len())
        },
        github,
        other,
        none
    );
    if errors > 0 {
        head.push_str(&format!(" {errors} could not be read."));
    }
    head
}

fn push_scan_warnings(out: &mut Vec<String>, scan_errors: &[(PathBuf, String)]) {
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
}

fn join_lines(out: Vec<String>) -> String {
    let mut text = out.join("\n");
    text.push('\n');
    text
}

/// What the offline `github` report shows for a repository.
fn remote_text(r: &RemoteReport) -> String {
    match &r.remote {
        Ok(Remote::GitHub(g)) => g.full_name(),
        Ok(Remote::Other) => "(another remote)".to_string(),
        Ok(Remote::None) => "(no remote)".to_string(),
        Err(e) => format!("ERROR: {}", clean(&single_line(e))),
    }
}

/// Render the `github` report: which repositories are on GitHub.
///
/// Plain facts only. Remotes that are not GitHub are shown without their
/// URL, which could contain credentials.
pub fn github(reports: &[RemoteReport], scan_errors: &[(PathBuf, String)]) -> String {
    let mut out: Vec<String> = vec![remote_headline(reports)];
    if !reports.is_empty() {
        let name_w = reports
            .iter()
            .map(|r| clean(&r.name).chars().count())
            .max()
            .unwrap_or(0)
            .min(MAX_NAME_WIDTH);
        out.push(String::new());
        for r in reports {
            out.push(format!("{:<nw$}  {}", clean(&r.name), remote_text(r), nw = name_w));
        }
    }
    push_scan_warnings(&mut out, scan_errors);
    join_lines(out)
}

/// Append the optional footer and any scan warnings, then join the lines.
fn finish(
    mut out: Vec<String>,
    summary: &Summary,
    scan_errors: &[(PathBuf, String)],
    opts: &RenderOptions,
) -> String {
    if let Some(footer) = opts.voice.footer(summary, seed_from("footer", opts.day)) {
        out.push(String::new());
        out.push(footer);
    }

    push_scan_warnings(&mut out, scan_errors);
    join_lines(out)
}
