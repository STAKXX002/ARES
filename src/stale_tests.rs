use crate::repo::{RepoReport, RepoState};
use crate::stale::{classify, StaleItem};
use std::path::Path;

const DAY: u64 = 86_400;
const NOW: u64 = 1_700_000_000;

fn state(changed: usize, ahead: Option<u32>) -> RepoState {
    RepoState {
        branch: Some("main".into()),
        detached: false,
        unborn: false,
        upstream: ahead.map(|_| "origin/main".to_string()),
        ahead,
        behind: ahead.map(|_| 0),
        staged: 0,
        unstaged: changed,
        untracked: 0,
        conflicted: 0,
        changed_files: changed,
    }
}

fn repo(name: &str, changed: usize, ahead: Option<u32>) -> RepoReport {
    RepoReport {
        path: Path::new("/r").join(name),
        name: name.into(),
        state: Ok(state(changed, ahead)),
    }
}

fn ago(days: u64) -> Result<Option<u64>, String> {
    Ok(Some(NOW - days * DAY))
}

#[test]
fn nothing_is_stale_when_everything_is_recent() {
    let input = vec![(repo("a", 0, None), ago(1)), (repo("b", 0, None), ago(30))];
    let r = classify(&input, NOW, 90);
    assert!(r.inactive.is_empty());
    assert_eq!((r.total, r.active()), (2, 2));
}

#[test]
fn the_threshold_is_inclusive_and_counts_whole_days() {
    let input = vec![
        (repo("just-under", 0, None), Ok(Some(NOW - 90 * DAY + 1))),
        (repo("exactly", 0, None), ago(90)),
        (repo("over", 0, None), ago(91)),
    ];
    let r = classify(&input, NOW, 90);
    let names: Vec<&str> = r.inactive.iter().map(|i| i.name.as_str()).collect();
    assert_eq!(names, vec!["over", "exactly"]);
    assert_eq!(r.inactive[1].age_days, 90);
    assert_eq!(r.active(), 1);
}

#[test]
fn quiet_repositories_are_listed_longest_quiet_first_with_ties_by_path() {
    let input = vec![
        (repo("mid", 0, None), ago(100)),
        (repo("zed", 0, None), ago(200)),
        (repo("old", 0, None), ago(300)),
        (repo("alpha", 0, None), ago(200)),
    ];
    let r = classify(&input, NOW, 90);
    let names: Vec<&str> = r.inactive.iter().map(|i| i.name.as_str()).collect();
    assert_eq!(names, vec!["old", "alpha", "zed", "mid"]);
}

#[test]
fn unfinished_work_and_unpushed_commits_are_carried_along() {
    let input = vec![(repo("busy", 7, Some(3)), ago(120))];
    let r = classify(&input, NOW, 90);
    assert_eq!(
        r.inactive,
        vec![StaleItem {
            name: "busy".into(),
            path: Path::new("/r").join("busy"),
            age_days: 120,
            dirty_files: 7,
            unpushed: 3,
        }]
    );
}

#[test]
fn an_unknown_upstream_counts_as_nothing_unpushed() {
    let r = classify(&[(repo("x", 0, None), ago(400))], NOW, 90);
    assert_eq!(r.inactive[0].unpushed, 0);
}

#[test]
fn repositories_without_commits_are_reported_separately() {
    let input = vec![(repo("fresh", 2, None), Ok(None)), (repo("old", 0, None), ago(200))];
    let r = classify(&input, NOW, 90);
    assert_eq!(r.no_commits, vec!["fresh"]);
    assert_eq!(r.inactive.len(), 1);
    assert_eq!(r.active(), 0);
}

#[test]
fn unreadable_state_or_history_is_reported_not_guessed() {
    let broken_state = RepoReport {
        path: Path::new("/r/b1").into(),
        name: "b1".into(),
        state: Err("boom".into()),
    };
    let input = vec![
        (broken_state, ago(500)),
        (repo("b2", 0, None), Err("git log failed".into())),
        (repo("ok", 0, None), ago(500)),
    ];
    let r = classify(&input, NOW, 90);
    assert_eq!(r.unreadable, vec!["b1", "b2"]);
    assert_eq!(r.inactive.len(), 1);
}

#[test]
fn a_commit_dated_in_the_future_is_not_stale_and_does_not_underflow() {
    let r = classify(&[(repo("future", 0, None), Ok(Some(NOW + 10 * DAY)))], NOW, 1);
    assert!(r.inactive.is_empty());
    assert_eq!(r.active(), 1);
}

#[test]
fn an_empty_scan_classifies_to_nothing() {
    let r = classify(&[], NOW, 90);
    assert_eq!((r.total, r.active()), (0, 0));
    assert_eq!(r.days, 90);
}

#[test]
fn the_buckets_always_add_up_to_the_total() {
    let input = vec![
        (repo("a", 0, None), ago(1)),
        (repo("b", 0, None), ago(500)),
        (repo("c", 0, None), Ok(None)),
        (repo("d", 0, None), Err("x".into())),
    ];
    let r = classify(&input, NOW, 90);
    assert_eq!(r.active() + r.inactive.len() + r.no_commits.len() + r.unreadable.len(), r.total);
}
