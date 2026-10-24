use crate::render::{facts, repos, RenderOptions};
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
