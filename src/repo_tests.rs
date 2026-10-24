use crate::git::status;
use crate::config::MAX_PARALLEL_LIMIT;
use crate::repo::{inspect, inspect_all, RepoReport, RepoState, MAX_WORKERS};
use crate::testutil::{commit_file, git, init_repo, TempDir};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

const T: Duration = Duration::from_secs(10);
const H: &str = "1111111111111111111111111111111111111111";

fn z(records: &[&str]) -> String {
    let mut s = records.join("\0");
    s.push('\0');
    s
}

fn state(records: &[&str]) -> RepoState {
    RepoState::from_status(&status::parse(&z(records)).unwrap())
}

#[test]
fn from_status_copies_branch_and_counts() {
    let m = format!("1 .M N... 100644 100644 100644 {H} {H} a.rs");
    let a = format!("1 A. N... 000000 100644 100644 {H} {H} b.rs");
    let s = state(&[
        "# branch.oid abc",
        "# branch.head dev",
        "# branch.upstream origin/dev",
        "# branch.ab +2 -1",
        &m,
        &a,
        "? c.txt",
    ]);
    assert_eq!(s.branch.as_deref(), Some("dev"));
    assert_eq!(s.upstream.as_deref(), Some("origin/dev"));
    assert_eq!((s.ahead, s.behind), (Some(2), Some(1)));
    assert_eq!((s.staged, s.unstaged, s.untracked, s.conflicted), (1, 1, 1, 0));
    assert_eq!(s.changed_files, 3);
    assert!(s.is_dirty());
    assert!(s.has_unpushed());
}

#[test]
fn file_both_staged_and_modified_counts_once_in_changed_files() {
    let e = format!("1 MM N... 100644 100644 100644 {H} {H} a.rs");
    let s = state(&["# branch.oid abc", "# branch.head main", &e]);
    assert_eq!((s.staged, s.unstaged), (1, 1));
    assert_eq!(s.changed_files, 1);
}

#[test]
fn no_upstream_leaves_ahead_and_behind_unknown() {
    let s = state(&["# branch.oid abc", "# branch.head main"]);
    assert_eq!((s.ahead, s.behind), (None, None));
    assert!(!s.has_unpushed());
}

#[test]
fn unborn_and_detached_flags() {
    let s = state(&["# branch.oid (initial)", "# branch.head main"]);
    assert!(s.unborn);
    let d = state(&["# branch.oid abc", "# branch.head (detached)"]);
    assert!(d.detached);
    assert_eq!(d.branch, None);
}

#[test]
fn ignored_files_do_not_make_a_repo_dirty() {
    let s = state(&["# branch.oid abc", "# branch.head main", "! target/"]);
    assert!(!s.is_dirty());
}

#[test]
fn report_name_is_the_directory_name() {
    let r = RepoReport::new(Path::new("/home/u/code/StakML"), Err("x".into()));
    assert_eq!(r.name, "StakML");
    assert!(r.is_error());
    assert!(!r.is_dirty());
}

#[test]
fn inspect_clean_repo() {
    let t = init_repo();
    commit_file(&t, "a.txt", "hi\n");
    let r = inspect(t.path(), T);
    let s = r.state.unwrap();
    assert!(!s.is_dirty());
    assert_eq!(s.branch.as_deref(), Some("main"));
}

#[test]
fn inspect_dirty_repo() {
    let t = init_repo();
    commit_file(&t, "a.txt", "hi\n");
    fs::write(t.path().join("a.txt"), "changed\n").unwrap();
    fs::write(t.path().join("new.txt"), "x\n").unwrap();
    let r = inspect(t.path(), T);
    assert!(r.is_dirty());
    assert_eq!(r.state.unwrap().changed_files, 2);
}

#[test]
fn inspect_non_repo_reports_error_instead_of_panicking() {
    let t = TempDir::new();
    let r = inspect(t.path(), T);
    assert!(r.is_error());
    assert!(r.state.unwrap_err().contains("not a git repository"));
}

#[test]
fn inspect_sees_commits_ahead_of_upstream() {
    let origin = init_repo();
    commit_file(&origin, "a.txt", "one\n");

    let holder = TempDir::new();
    git(
        holder.path(),
        &["clone", "-q", origin.path().to_str().unwrap(), "work"],
    );
    let work = holder.path().join("work");
    fs::write(work.join("b.txt"), "two\n").unwrap();
    git(&work, &["add", "b.txt"]);
    git(&work, &["commit", "-q", "-m", "local commit"]);

    let s = inspect(&work, T).state.unwrap();
    assert_eq!(s.upstream.as_deref(), Some("origin/main"));
    assert_eq!((s.ahead, s.behind), (Some(1), Some(0)));
    assert!(s.has_unpushed());
    assert!(!s.is_dirty());
}

/// Build `n` directories: most are repos, every third is dirty, and every
/// seventh is a plain directory that is not a repo (so it reports an error).
fn mixed_dirs(n: usize) -> (TempDir, Vec<PathBuf>) {
    let t = TempDir::new();
    let mut paths = Vec::new();
    for i in 0..n {
        let dir = t.path().join(format!("r{i:02}"));
        fs::create_dir_all(&dir).unwrap();
        if i % 7 == 6 {
            paths.push(dir);
            continue;
        }
        git(&dir, &["init", "-q", "-b", "main"]);
        fs::write(dir.join("f.txt"), "x\n").unwrap();
        git(&dir, &["add", "f.txt"]);
        git(&dir, &["commit", "-q", "-m", "init"]);
        if i % 3 == 0 {
            fs::write(dir.join("loose.txt"), "y\n").unwrap();
        }
        paths.push(dir);
    }
    (t, paths)
}

#[test]
fn inspect_all_of_nothing_is_empty() {
    assert!(inspect_all(&[], T, 4).is_empty());
}

#[test]
fn inspect_all_matches_sequential_results_in_order() {
    let (_t, paths) = mixed_dirs(20);
    let expected: Vec<RepoReport> = paths.iter().map(|p| inspect(p, T)).collect();
    assert!(expected.iter().any(|r| r.is_error()));
    assert!(expected.iter().any(|r| r.is_dirty()));

    for workers in [1, 2, 4, 16] {
        let got = inspect_all(&paths, T, workers);
        assert_eq!(got, expected, "workers = {workers}");
    }
}

#[test]
fn inspect_all_handles_more_workers_than_repos() {
    let (_t, paths) = mixed_dirs(3);
    let expected: Vec<RepoReport> = paths.iter().map(|p| inspect(p, T)).collect();
    assert_eq!(inspect_all(&paths, T, 16), expected);
}

#[test]
fn inspect_all_clamps_unreasonable_worker_counts() {
    let (_t, paths) = mixed_dirs(4);
    let expected: Vec<RepoReport> = paths.iter().map(|p| inspect(p, T)).collect();
    assert_eq!(inspect_all(&paths, T, 0), expected);
    assert_eq!(inspect_all(&paths, T, 100_000), expected);
}

#[test]
fn worker_limit_matches_the_config_limit() {
    assert_eq!(MAX_WORKERS, MAX_PARALLEL_LIMIT);
}
