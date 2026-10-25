use crate::changes::diff;
use crate::github::RepoRef;
use crate::render::{changes, facts, github, repos, status, RenderOptions};
use crate::repo::{Remote, RemoteReport};
use crate::store::ScanRecord;
use crate::repo::{RepoReport, RepoState};
use crate::voice::Voice;
use std::path::{Path, PathBuf};

fn st(changed: usize, ahead: Option<u32>) -> RepoState {
    RepoState {
        branch: Some("main".into()),
        detached: false,
        unborn: false,
        upstream: ahead.map(|_| "origin/main".to_string()),
        ahead,
        behind: ahead.map(|_| 0),
        staged: changed / 2,
        unstaged: changed - changed / 2,
        untracked: 0,
        conflicted: 0,
        changed_files: changed,
    }
}

fn rep(name: &str, state: Result<RepoState, String>) -> RepoReport {
    RepoReport::new(&Path::new("/r").join(name), state)
}

fn opts(voice: Voice, dirty_only: bool) -> RenderOptions {
    RenderOptions {
        voice,
        day: 20000,
        dirty_only,
    }
}

fn sample() -> Vec<RepoReport> {
    vec![
        rep("alpha", Ok(st(0, Some(0)))),
        rep("StakML", Ok(st(14, Some(2)))),
        rep("dockctl", Ok(st(0, Some(4)))),
        rep("broken", Err("fatal: not a git\nrepository".into())),
    ]
}

#[test]
fn facts_for_clean_repo() {
    assert_eq!(facts(&st(0, Some(0))), "clean");
}

#[test]
fn facts_for_dirty_and_ahead() {
    assert_eq!(
        facts(&st(14, Some(2))),
        "14 changed (7 staged, 7 unstaged, 0 untracked), 2 ahead"
    );
}

#[test]
fn facts_for_no_upstream_unborn_and_behind() {
    assert_eq!(facts(&st(0, None)), "clean, no upstream");
    let mut s = st(0, Some(0));
    s.behind = Some(3);
    assert_eq!(facts(&s), "clean, 3 behind");
    let mut u = st(0, None);
    u.unborn = true;
    assert_eq!(facts(&u), "clean, no commits yet");
}

#[test]
fn facts_mention_conflicts() {
    let mut s = st(2, Some(0));
    s.conflicted = 1;
    assert!(facts(&s).contains("1 conflicted"));
}

#[test]
fn plain_output_is_exact() {
    let out = repos(&sample(), &[], &opts(Voice::Plain, false));
    let expected = "\
4 repositories: 1 with uncommitted changes, 2 with unpushed commits. 1 could not be read.

alpha    main  clean
StakML   main  14 changed (7 staged, 7 unstaged, 0 untracked), 2 ahead
dockctl  main  clean, 4 ahead
broken   ERROR: fatal: not a git repository
";
    assert_eq!(out, expected);
}

#[test]
fn dirty_only_filters_but_keeps_errors_and_full_summary() {
    let out = repos(&sample(), &[], &opts(Voice::Plain, true));
    assert!(out.contains("4 repositories"), "summary covers all repos");
    assert!(out.contains("Showing only repositories with uncommitted changes."));
    assert!(out.contains("StakML"));
    assert!(out.contains("broken"));
    assert!(!out.contains("alpha"));
    assert!(!out.contains("dockctl"));
}

#[test]
fn dirty_only_with_nothing_dirty_prints_no_repo_lines() {
    let reports = vec![rep("alpha", Ok(st(0, Some(0))))];
    let out = repos(&reports, &[], &opts(Voice::Plain, true));
    assert!(!out.contains("alpha"));
    assert!(out.starts_with("1 repository:"));
}

#[test]
fn empty_scan_reports_no_repositories() {
    let out = repos(&[], &[], &opts(Voice::Plain, false));
    assert_eq!(out, "No repositories found.\n");
}

#[test]
fn scan_errors_become_plain_warnings() {
    let errs = vec![(PathBuf::from("/root/locked"), "Permission denied".to_string())];
    let out = repos(&[], &errs, &opts(Voice::Cheerful, false));
    assert!(out.contains("warning: could not read /root/locked: Permission denied"));
}

#[test]
fn fact_lines_are_identical_across_voices() {
    let names = ["alpha", "StakML", "dockctl", "broken"];
    let fact_lines = |voice| -> Vec<String> {
        repos(&sample(), &[], &opts(voice, false))
            .lines()
            .filter(|l| names.iter().any(|n| l.starts_with(n)))
            .map(|l| l.to_string())
            .collect()
    };
    let plain = fact_lines(Voice::Plain);
    assert_eq!(plain.len(), 4);
    assert_eq!(fact_lines(Voice::Cheerful), plain);
    assert_eq!(fact_lines(Voice::Deadpan), plain);
}

#[test]
fn flavored_voices_add_notes_for_notable_repos_only() {
    let plain = repos(&sample(), &[], &opts(Voice::Plain, false));
    assert!(
        !plain.lines().any(|l| l.starts_with("    ")),
        "plain has no indented notes"
    );

    for v in [Voice::Cheerful, Voice::Deadpan] {
        let out = repos(&sample(), &[], &opts(v, false));
        let notes: Vec<&str> = out.lines().filter(|l| l.starts_with("    ")).collect();
        // StakML has 14 changed files, dockctl has 4 unpushed commits.
        assert_eq!(notes.len(), 2, "{v:?}: {out}");
        assert!(notes.iter().any(|n| n.contains("14")));
        assert!(notes.iter().any(|n| n.contains('4')));
    }
}

#[test]
fn wording_is_stable_within_a_day_and_all_ascii() {
    for v in Voice::ALL {
        let a = repos(&sample(), &[], &opts(v, false));
        let b = repos(&sample(), &[], &opts(v, false));
        assert_eq!(a, b);
        assert!(a.is_ascii(), "{v:?}");
    }
}

#[test]
fn detached_head_is_labelled() {
    let mut s = st(0, None);
    s.detached = true;
    s.branch = None;
    let out = repos(&[rep("x", Ok(s))], &[], &opts(Voice::Plain, false));
    assert!(out.contains("(detached)"));
}

#[test]
fn output_always_ends_with_one_newline() {
    for v in Voice::ALL {
        let out = repos(&sample(), &[], &opts(v, false));
        assert!(out.ends_with('\n'));
        assert!(!out.ends_with("\n\n"));
    }
}

#[test]
fn control_characters_in_names_and_errors_are_neutralized() {
    let reports = vec![
        rep("evil\x1b[31mred", Ok(st(0, Some(0)))),
        rep("bad\x07bell", Err("fatal:\x1b[2Jboom\nnext line".into())),
    ];
    let errs = vec![(PathBuf::from("/tmp/\x1b]0;pwned\x07dir"), "denied\x1b[0m".to_string())];
    for v in Voice::ALL {
        let out = repos(&reports, &errs, &opts(v, false));
        assert!(
            !out.chars().any(|c| c.is_control() && c != '\n'),
            "{v:?} leaked a control character: {out:?}"
        );
        assert!(out.contains("evil?[31mred"));
        assert!(out.contains("bad?bell"));
        assert!(out.contains("warning: could not read /tmp/?]0;pwned?dir"));
    }
}

#[test]
fn status_plain_output_is_exact() {
    let out = status(&sample(), &[], &opts(Voice::Plain, false));
    let expected = "\
4 repositories: 1 with uncommitted changes, 2 with unpushed commits. 1 could not be read.

uncommitted: StakML (14)
unpushed:    StakML (2), dockctl (4)
unreadable:  broken
";
    assert_eq!(out, expected);
}

#[test]
fn status_with_nothing_to_report_is_just_the_headline() {
    let reports = vec![rep("a", Ok(st(0, Some(0)))), rep("b", Ok(st(0, None)))];
    let out = status(&reports, &[], &opts(Voice::Plain, false));
    assert_eq!(out.lines().count(), 1);
    assert!(out.starts_with("2 repositories:"));
}

#[test]
fn status_groups_conflicts_and_behind() {
    let mut c = st(2, Some(0));
    c.conflicted = 1;
    let mut b = st(0, Some(0));
    b.behind = Some(5);
    let reports = vec![rep("merging", Ok(c)), rep("stale", Ok(b))];
    let out = status(&reports, &[], &opts(Voice::Plain, false));
    assert!(out.contains("conflicted:  merging\n"));
    assert!(out.contains("behind:      stale (5)\n"));
}

#[test]
fn status_truncates_long_lists_with_a_count() {
    let reports: Vec<RepoReport> = (0..11)
        .map(|i| rep(&format!("r{i:02}"), Ok(st(1, Some(0)))))
        .collect();
    let out = status(&reports, &[], &opts(Voice::Plain, false));
    assert!(out.contains("r07 (1), +3 more"), "{out}");
    assert!(!out.contains("r08"));
}

#[test]
fn status_group_lines_are_identical_across_voices() {
    let labels = ["conflicted:", "uncommitted:", "unpushed:", "behind:", "unreadable:"];
    let groups = |voice| -> Vec<String> {
        status(&sample(), &[], &opts(voice, false))
            .lines()
            .filter(|l| labels.iter().any(|x| l.starts_with(x)))
            .map(|l| l.to_string())
            .collect()
    };
    let plain = groups(Voice::Plain);
    assert_eq!(plain.len(), 3);
    assert_eq!(groups(Voice::Cheerful), plain);
    assert_eq!(groups(Voice::Deadpan), plain);
}

#[test]
fn status_neutralizes_control_characters() {
    let reports = vec![rep("evil\x1b[2Jname", Ok(st(3, Some(0))))];
    for v in Voice::ALL {
        let out = status(&reports, &[], &opts(v, false));
        assert!(!out.chars().any(|c| c.is_control() && c != '\n'), "{v:?}");
        assert!(out.contains("evil?[2Jname (3)"));
    }
}

const T0: u64 = 1_700_000_000;

fn scan(id: i64, at: u64, n: usize) -> ScanRecord {
    ScanRecord { id, started_at: at, repo_count: n }
}

fn changes_for(before: &[RepoReport], after: &[RepoReport], voice: Voice) -> String {
    changes(
        &scan(1, T0, before.len()),
        &scan(2, T0 + 7_200, after.len()),
        &diff(before, after),
        T0 + 7_200,
        &opts(voice, false),
    )
}

fn before_after() -> (Vec<RepoReport>, Vec<RepoReport>) {
    let before = vec![
        rep("a", Ok(st(0, Some(0)))),
        rep("b", Ok(st(4, Some(0)))),
        rep("c", Ok(st(0, Some(3)))),
        rep("gone", Ok(st(0, None))),
    ];
    let after = vec![
        rep("a", Ok(st(12, Some(0)))),
        rep("b", Ok(st(0, Some(0)))),
        rep("c", Ok(st(0, Some(0)))),
        rep("fresh", Ok(st(0, None))),
    ];
    (before, after)
}

#[test]
fn changes_plain_output_is_exact() {
    let (b, a) = before_after();
    let expected = "Comparing scan #1 (2 hours ago, 2023-11-14 22:13 UTC) with scan #2 (just now, 2023-11-15 00:13 UTC).

new:               fresh
gone:              gone
became dirty:      a (12 files)
became clean:      b (was 4)
pushed:            c (was 3)
";
    assert_eq!(changes_for(&b, &a, Voice::Plain), expected);
}

#[test]
fn changes_with_nothing_different_says_so_plainly() {
    let (b, _) = before_after();
    let out = changes_for(&b, &b, Voice::Plain);
    assert!(out.starts_with("Comparing scan #1 (2 hours ago"));
    assert!(out.ends_with("\n\nNo changes between these scans.\n"));
}

#[test]
fn changes_group_lines_are_identical_across_voices() {
    let (b, a) = before_after();
    let labels = ["new:", "gone:", "became dirty:", "became clean:", "pushed:"];
    let groups = |voice| -> Vec<String> {
        changes_for(&b, &a, voice)
            .lines()
            .filter(|l| labels.iter().any(|x| l.starts_with(x)))
            .map(|l| l.to_string())
            .collect()
    };
    let plain = groups(Voice::Plain);
    assert_eq!(plain.len(), 5);
    assert_eq!(groups(Voice::Cheerful), plain);
    assert_eq!(groups(Voice::Deadpan), plain);
}

#[test]
fn flavored_voices_add_one_closing_remark_and_plain_adds_none() {
    let (b, a) = before_after();
    let plain = changes_for(&b, &a, Voice::Plain);
    for v in [Voice::Cheerful, Voice::Deadpan] {
        let out = changes_for(&b, &a, v);
        assert!(out.starts_with(plain.trim_end()), "{v:?} must keep every plain line");
        assert!(out.len() > plain.len());
        assert!(out.is_ascii());
    }
}

#[test]
fn changes_report_covers_the_remaining_groups() {
    let mut detached = st(0, None);
    detached.detached = true;
    detached.branch = None;
    let before = vec![
        rep("files", Ok(st(2, None))),
        rep("up", Ok(st(0, Some(1)))),
        rep("br", Ok(st(0, None))),
        rep("ok", Ok(st(0, None))),
        rep("bad", Err("x".into())),
    ];
    let after = vec![
        rep("files", Ok(st(9, None))),
        rep("up", Ok(st(0, Some(4)))),
        rep("br", Ok(detached)),
        rep("ok", Err("boom".into())),
        rep("bad", Ok(st(0, None))),
    ];
    let out = changes_for(&before, &after, Voice::Plain);
    assert!(out.contains("files changed:     files (2 -> 9)\n"), "{out}");
    assert!(out.contains("unpushed changed:  up (1 -> 4)\n"), "{out}");
    assert!(out.contains("branch changed:    br (main -> (detached))\n"), "{out}");
    assert!(out.contains("unreadable now:    ok\n"), "{out}");
    assert!(out.contains("readable again:    bad\n"), "{out}");
}

#[test]
fn changes_truncate_long_lists() {
    let before: Vec<RepoReport> = (0..11).map(|i| rep(&format!("r{i:02}"), Ok(st(0, None)))).collect();
    let after: Vec<RepoReport> = (0..11).map(|i| rep(&format!("r{i:02}"), Ok(st(1, None)))).collect();
    let out = changes_for(&before, &after, Voice::Plain);
    assert!(out.contains("r07 (1 files), +3 more"), "{out}");
}

#[test]
fn changes_neutralize_control_characters() {
    let before = vec![rep("evil\x1b[2Jname", Ok(st(0, None)))];
    let mut moved = st(0, None);
    moved.branch = Some("b\x07ell".into());
    let after = vec![rep("evil\x1b[2Jname", Ok(moved)), rep("new\x1bone", Ok(st(0, None)))];
    for v in Voice::ALL {
        let out = changes_for(&before, &after, v);
        assert!(!out.chars().any(|c| c.is_control() && c != '\n'), "{v:?}: {out:?}");
        assert!(out.contains("evil?[2Jname (main -> b?ell)"));
        assert!(out.contains("new?one"));
    }
}

fn remote_rep(name: &str, remote: Result<Remote, String>) -> RemoteReport {
    RemoteReport {
        path: Path::new("/r").join(name),
        name: name.to_string(),
        remote,
    }
}

fn on_github(owner: &str, name: &str) -> Result<Remote, String> {
    Ok(Remote::GitHub(RepoRef {
        owner: owner.into(),
        name: name.into(),
    }))
}

#[test]
fn github_report_output_is_exact() {
    let reports = vec![
        remote_rep("ARES", on_github("STAKXX002", "ARES")),
        remote_rep("scratch", Ok(Remote::None)),
        remote_rep("mirror", Ok(Remote::Other)),
        remote_rep("broken", Err("not a git repository\nsecond line".into())),
    ];
    let expected = "\
4 repositories: 1 on GitHub, 1 with another remote, 1 with no remote. 1 could not be read.

ARES     STAKXX002/ARES
scratch  (no remote)
mirror   (another remote)
broken   ERROR: not a git repository second line
";
    assert_eq!(github(&reports, &[]), expected);
}

#[test]
fn github_report_for_nothing_and_for_one_repo() {
    assert_eq!(github(&[], &[]), "No repositories found.\n");
    let one = vec![remote_rep("solo", on_github("o", "n"))];
    assert!(github(&one, &[]).starts_with("1 repository: 1 on GitHub, 0 with another remote, 0 with no remote.\n"));
}

#[test]
fn github_report_neutralizes_control_characters_and_shows_scan_warnings() {
    let reports = vec![remote_rep("evil\x1b[2Jname", Ok(Remote::None))];
    let errs = vec![(PathBuf::from("/x/\x07bad"), "denied".to_string())];
    let out = github(&reports, &errs);
    assert!(!out.chars().any(|c| c.is_control() && c != '\n'));
    assert!(out.contains("evil?[2Jname"));
    assert!(out.contains("warning: could not read /x/?bad: denied"));
}
