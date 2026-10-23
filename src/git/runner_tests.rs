//! Integration tests that run real `git` in throwaway repositories.

use super::runner::{run, Query, RunError, DEFAULT_TIMEOUT};
use super::status;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static COUNTER: AtomicU32 = AtomicU32::new(0);

/// A temporary directory removed on drop.
struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!("ares-test-{}-{}-{}", std::process::id(), n, nanos));
        fs::create_dir_all(&dir).unwrap();
        TempDir(dir)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// Run git for test setup only. Isolated from the user's global config.
fn git(dir: &Path, args: &[&str]) {
    let out = Command::new("git")
        .args(["-c", "user.name=Test", "-c", "user.email=test@example.com"])
        .args(["-c", "commit.gpgsign=false"])
        .arg("-C")
        .arg(dir)
        .args(args)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .expect("git must be installed to run these tests");
    assert!(
        out.status.success(),
        "git {:?} failed: {}",
        args,
        String::from_utf8_lossy(&out.stderr)
    );
}

fn init_repo() -> TempDir {
    let t = TempDir::new();
    git(t.path(), &["init", "-q", "-b", "main"]);
    t
}

fn commit_file(t: &TempDir, name: &str, content: &str) {
    fs::write(t.path().join(name), content).unwrap();
    git(t.path(), &["add", name]);
    git(t.path(), &["commit", "-q", "-m", "add file"]);
}

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
