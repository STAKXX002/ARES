use crate::changes::{diff, Change, Changes};
use crate::repo::{RepoReport, RepoState};
use std::path::Path;

fn st(changed: usize, ahead: Option<u32>, branch: &str) -> RepoState {
    RepoState {
        branch: Some(branch.into()),
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

fn rep(dir: &str, name: &str, state: Result<RepoState, String>) -> RepoReport {
    RepoReport {
        path: Path::new(dir).join(name),
        name: name.to_string(),
        state,
    }
}

fn ok(name: &str, changed: usize, ahead: Option<u32>) -> RepoReport {
    rep("/r", name, Ok(st(changed, ahead, "main")))
}

fn kinds(c: &Changes) -> Vec<(String, Change)> {
    c.changes.iter().map(|x| (x.name.clone(), x.change.clone())).collect()
}

#[test]
fn identical_scans_have_no_changes() {
    let scan = vec![ok("a", 0, Some(0)), ok("b", 3, Some(2))];
    let d = diff(&scan, &scan);
    assert!(d.is_empty());
    assert_eq!(d, Changes::default());
}

#[test]
fn two_empty_scans_have_no_changes() {
    assert!(diff(&[], &[]).is_empty());
}

#[test]
fn added_and_removed_repositories() {
    let before = vec![ok("keep", 0, None), ok("gone", 0, None)];
    let after = vec![ok("keep", 0, None), ok("fresh", 0, None)];
    let d = diff(&before, &after);
    assert_eq!(d.added, vec!["fresh"]);
    assert_eq!(d.removed, vec!["gone"]);
    assert!(d.changes.is_empty());
}

#[test]
fn same_name_in_a_different_folder_is_a_different_repo() {
    let before = vec![rep("/one", "app", Ok(st(0, None, "main")))];
    let after = vec![rep("/two", "app", Ok(st(0, None, "main")))];
    let d = diff(&before, &after);
    assert_eq!((d.added.len(), d.removed.len()), (1, 1));
}

#[test]
fn dirtiness_transitions() {
    let before = vec![ok("a", 0, None), ok("b", 4, None), ok("c", 2, None), ok("d", 5, None)];
    let after = vec![ok("a", 7, None), ok("b", 0, None), ok("c", 6, None), ok("d", 5, None)];
    let d = diff(&before, &after);
    assert_eq!(
        kinds(&d),
        vec![
            ("a".to_string(), Change::BecameDirty { files: 7 }),
            ("b".to_string(), Change::BecameClean { was: 4 }),
            ("c".to_string(), Change::FilesChanged { from: 2, to: 6 }),
        ]
    );
}

#[test]
fn unpushed_transitions() {
    let before = vec![ok("a", 0, Some(0)), ok("b", 0, Some(3)), ok("c", 0, Some(1)), ok("d", 0, Some(2))];
    let after = vec![ok("a", 0, Some(4)), ok("b", 0, Some(0)), ok("c", 0, Some(5)), ok("d", 0, Some(2))];
    let d = diff(&before, &after);
    assert_eq!(
        kinds(&d),
        vec![
            ("a".to_string(), Change::NewlyUnpushed { commits: 4 }),
            ("b".to_string(), Change::Pushed { was: 3 }),
            ("c".to_string(), Change::UnpushedChanged { from: 1, to: 5 }),
        ]
    );
}

#[test]
fn losing_the_upstream_is_not_reported_as_pushed() {
    let before = vec![ok("a", 0, Some(3))];
    let after = vec![ok("a", 0, None)];
    assert!(diff(&before, &after).is_empty());
}

#[test]
fn gaining_an_upstream_with_commits_ahead_is_newly_unpushed() {
    let before = vec![ok("a", 0, None)];
    let after = vec![ok("a", 0, Some(2))];
    assert_eq!(
        kinds(&diff(&before, &after)),
        vec![("a".to_string(), Change::NewlyUnpushed { commits: 2 })]
    );
}

#[test]
fn branch_changes_including_detached_head() {
    let before = vec![rep("/r", "a", Ok(st(0, None, "main"))), rep("/r", "b", Ok(st(0, None, "dev")))];
    let mut detached = st(0, None, "dev");
    detached.detached = true;
    detached.branch = None;
    let after = vec![rep("/r", "a", Ok(st(0, None, "feature"))), rep("/r", "b", Ok(detached))];
    let d = diff(&before, &after);
    assert_eq!(
        kinds(&d),
        vec![
            ("a".to_string(), Change::BranchChanged { from: "main".into(), to: "feature".into() }),
            ("b".to_string(), Change::BranchChanged { from: "dev".into(), to: "(detached)".into() }),
        ]
    );
}

#[test]
fn readability_transitions() {
    let bad = |n: &str| rep("/r", n, Err("boom".into()));
    let before = vec![ok("a", 0, None), bad("b"), bad("c")];
    let after = vec![bad("a"), ok("b", 0, None), bad("c")];
    let d = diff(&before, &after);
    assert_eq!(
        kinds(&d),
        vec![
            ("a".to_string(), Change::BecameUnreadable),
            ("b".to_string(), Change::ReadableAgain),
        ]
    );
}

#[test]
fn several_changes_in_one_repo_come_in_a_fixed_order() {
    let before = vec![rep("/r", "a", Ok(st(0, Some(0), "main")))];
    let after = vec![rep("/r", "a", Ok(st(3, Some(2), "dev")))];
    let d = diff(&before, &after);
    let k: Vec<Change> = d.changes.iter().map(|c| c.change.clone()).collect();
    assert_eq!(
        k,
        vec![
            Change::BecameDirty { files: 3 },
            Change::NewlyUnpushed { commits: 2 },
            Change::BranchChanged { from: "main".into(), to: "dev".into() },
        ]
    );
}

#[test]
fn changes_are_ordered_by_path_regardless_of_input_order() {
    let before = vec![ok("z", 0, None), ok("a", 0, None), ok("m", 0, None)];
    let after = vec![ok("m", 1, None), ok("z", 1, None), ok("a", 1, None)];
    let names: Vec<String> = diff(&before, &after).changes.iter().map(|c| c.name.clone()).collect();
    assert_eq!(names, vec!["a", "m", "z"]);
}
