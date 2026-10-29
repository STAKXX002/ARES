//! Integration tests that run real `git` in throwaway repositories.

use crate::git::runner::{run, run_with_limit_for_test, Query, RunError};
use crate::git::status;
use crate::tests::util::{commit_file, git, init_repo, TempDir};
use std::fs;
use std::time::Duration;

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(10);

fn status_of(t: &TempDir) -> status::Status {
    let raw = run(t.path(), Query::Status, DEFAULT_TIMEOUT).unwrap();
    status::parse(&raw).unwrap()
}

#[test]
fn fresh_repo_has_no_commit_and_is_clean() {
    let t = init_repo();
    let s = status_of(&t);
    assert_eq!(s.branch.oid, None);
    assert_eq!(s.branch.head.as_deref(), Some("main"));
    assert!(!s.is_dirty());
}

#[test]
fn clean_repo_after_commit() {
    let t = init_repo();
    commit_file(&t, "a.txt", "hello\n");
    let s = status_of(&t);
    assert!(s.branch.oid.is_some());
    assert_eq!(s.branch.upstream, None);
    assert!(!s.is_dirty());
}

#[test]
fn modified_and_untracked_files_are_reported() {
    let t = init_repo();
    commit_file(&t, "a.txt", "hello\n");
    fs::write(t.path().join("a.txt"), "changed\n").unwrap();
    fs::write(t.path().join("new file.txt"), "x\n").unwrap();
    let s = status_of(&t);
    assert_eq!((s.staged(), s.unstaged(), s.untracked_count()), (0, 1, 1));
    assert_eq!(s.untracked, vec!["new file.txt".to_string()]);
}

#[test]
fn staged_rename_is_reported_with_original_path() {
    let t = init_repo();
    commit_file(&t, "old.txt", "some content that is long enough to detect a rename\n");
    git(t.path(), &["mv", "old.txt", "new.txt"]);
    let s = status_of(&t);
    assert_eq!(s.entries.len(), 1);
    assert_eq!(s.entries[0].path, "new.txt");
    assert_eq!(s.entries[0].orig_path.as_deref(), Some("old.txt"));
}

#[test]
fn status_does_not_rewrite_the_index() {
    let t = init_repo();
    commit_file(&t, "a.txt", "same\n");

    // Rewrite identical content so the file's stat data no longer matches
    // the index. A plain `git status` would refresh and rewrite the index.
    std::thread::sleep(std::time::Duration::from_millis(1100));
    fs::write(t.path().join("a.txt"), "same\n").unwrap();

    let index = t.path().join(".git").join("index");
    let before = fs::read(&index).unwrap();
    let s = status_of(&t);
    let after = fs::read(&index).unwrap();

    assert!(!s.is_dirty());
    assert_eq!(before, after, "ARES must not modify the repository index");
}

#[test]
fn non_repository_returns_failed_error() {
    let t = TempDir::new();
    match run(t.path(), Query::Status, DEFAULT_TIMEOUT) {
        Err(RunError::Failed { stderr, .. }) => assert!(stderr.contains("not a git repository")),
        other => panic!("expected Failed, got {other:?}"),
    }
}

#[test]
fn missing_directory_returns_failed_error() {
    let t = TempDir::new();
    let gone = t.path().join("does-not-exist");
    assert!(matches!(
        run(&gone, Query::Status, DEFAULT_TIMEOUT),
        Err(RunError::Failed { .. })
    ));
}

#[test]
fn oversized_git_output_is_rejected_instead_of_buffered() {
    let t = init_repo();
    for i in 0..50 {
        fs::write(t.path().join(format!("untracked-file-{i}.txt")), "x").unwrap();
    }
    // The real limit is large; a tiny one proves the guard is wired in.
    match run_with_limit_for_test(t.path(), Query::Status, DEFAULT_TIMEOUT, 100) {
        Err(RunError::OutputTooLarge { limit }) => assert_eq!(limit, 100),
        other => panic!("expected OutputTooLarge, got {other:?}"),
    }
    // And the same repository is fine under the real limit.
    assert!(run(t.path(), Query::Status, DEFAULT_TIMEOUT).is_ok());
}
