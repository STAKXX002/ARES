use super::*;
use crate::repo::{RepoReport, RepoState};
use crate::testutil::TempDir;
use rusqlite::Connection;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

fn state(changed: usize, ahead: Option<u32>) -> RepoState {
    RepoState {
        branch: Some("main".into()),
        detached: false,
        unborn: false,
        upstream: ahead.map(|_| "origin/main".to_string()),
        ahead,
        behind: ahead.map(|_| 1),
        staged: 1,
        unstaged: 2,
        untracked: 3,
        conflicted: 0,
        changed_files: changed,
    }
}

fn ok(name: &str, changed: usize, ahead: Option<u32>) -> RepoReport {
    RepoReport::new(&Path::new("/r").join(name), Ok(state(changed, ahead)))
}

fn broken(name: &str) -> RepoReport {
    RepoReport::new(&Path::new("/r").join(name), Err("not a git repository".into()))
}

#[test]
fn fresh_database_is_at_the_current_schema_version() {
    let s = Store::open_in_memory().unwrap();
    assert_eq!(s.schema_version().unwrap(), SCHEMA_VERSION);
    assert_eq!(SCHEMA_VERSION, 1);
}

#[test]
fn latest_scan_is_none_when_empty() {
    let s = Store::open_in_memory().unwrap();
    assert_eq!(s.latest_scan().unwrap(), None);
}

#[test]
fn record_scan_stores_the_scan_and_every_snapshot() {
    let mut s = Store::open_in_memory().unwrap();
    let rec = s
        .record_scan(1_000, &[ok("a", 0, Some(0)), ok("b", 4, Some(2)), broken("c")])
        .unwrap();
    assert_eq!(rec.repo_count, 3);
    assert_eq!(rec.started_at, 1_000);
    assert_eq!(s.snapshot_count(rec.id).unwrap(), 3);
    assert_eq!(s.latest_scan().unwrap(), Some(rec));
}

#[test]
fn latest_scan_returns_the_newest() {
    let mut s = Store::open_in_memory().unwrap();
    s.record_scan(1, &[ok("a", 0, None)]).unwrap();
    let second = s.record_scan(2, &[ok("a", 1, None), ok("b", 0, None)]).unwrap();
    let latest = s.latest_scan().unwrap().unwrap();
    assert_eq!(latest, second);
    assert_eq!(latest.repo_count, 2);
}

#[test]
fn snapshot_fields_round_trip() {
    let mut s = Store::open_in_memory().unwrap();
    let rec = s.record_scan(5, &[ok("proj", 6, Some(3))]).unwrap();
    let row: (String, String, Option<String>, Option<String>, i64, i64, i64, i64, i64, i64) = s
        .conn
        .query_row(
            "SELECT path, name, error, upstream, ahead, behind, staged, unstaged, untracked, changed_files
             FROM repo_snapshots WHERE scan_id = ?1",
            [rec.id],
            |r| {
                Ok((
                    r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?,
                    r.get(5)?, r.get(6)?, r.get(7)?, r.get(8)?, r.get(9)?,
                ))
            },
        )
        .unwrap();
    assert_eq!(row.0, "/r/proj");
    assert_eq!(row.1, "proj");
    assert_eq!(row.2, None);
    assert_eq!(row.3.as_deref(), Some("origin/main"));
    assert_eq!((row.4, row.5, row.6, row.7, row.8, row.9), (3, 1, 1, 2, 3, 6));
}

#[test]
fn unreadable_repos_store_their_error_and_null_state() {
    let mut s = Store::open_in_memory().unwrap();
    let rec = s.record_scan(5, &[broken("bad")]).unwrap();
    let (err, branch, changed): (String, Option<String>, Option<i64>) = s
        .conn
        .query_row(
            "SELECT error, branch, changed_files FROM repo_snapshots WHERE scan_id = ?1",
            [rec.id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    assert_eq!(err, "not a git repository");
    assert_eq!(branch, None);
    assert_eq!(changed, None);
}

#[test]
fn missing_upstream_is_stored_as_null_not_zero() {
    let mut s = Store::open_in_memory().unwrap();
    let rec = s.record_scan(5, &[ok("local", 0, None)]).unwrap();
    let (ahead, behind): (Option<i64>, Option<i64>) = s
        .conn
        .query_row(
            "SELECT ahead, behind FROM repo_snapshots WHERE scan_id = ?1",
            [rec.id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!((ahead, behind), (None, None));
}

#[test]
fn empty_scan_is_recorded() {
    let mut s = Store::open_in_memory().unwrap();
    let rec = s.record_scan(9, &[]).unwrap();
    assert_eq!(rec.repo_count, 0);
    assert_eq!(s.snapshot_count(rec.id).unwrap(), 0);
}

#[test]
fn duplicate_paths_in_one_scan_do_not_fail() {
    let mut s = Store::open_in_memory().unwrap();
    let rec = s
        .record_scan(1, &[ok("dup", 1, None), ok("dup", 2, None)])
        .unwrap();
    assert_eq!(s.snapshot_count(rec.id).unwrap(), 1);
}

#[test]
fn unicode_names_round_trip() {
    let mut s = Store::open_in_memory().unwrap();
    let rec = s.record_scan(1, &[ok("caf\u{e9}-\u{4e2d}\u{6587}", 0, None)]).unwrap();
    let name: String = s
        .conn
        .query_row("SELECT name FROM repo_snapshots WHERE scan_id = ?1", [rec.id], |r| r.get(0))
        .unwrap();
    assert_eq!(name, "caf\u{e9}-\u{4e2d}\u{6587}");
}

#[test]
fn retention_keeps_only_the_newest_scans_and_their_snapshots() {
    let mut s = Store::open_in_memory().unwrap();
    for i in 1..=5u64 {
        s.record_scan_keeping(i, &[ok("a", 0, None), ok("b", 0, None)], 3).unwrap();
    }
    assert_eq!(s.scan_count().unwrap(), 3);
    let latest = s.latest_scan().unwrap().unwrap();
    assert_eq!(latest.started_at, 5);
    let total_rows: i64 = s
        .conn
        .query_row("SELECT COUNT(*) FROM repo_snapshots", [], |r| r.get(0))
        .unwrap();
    assert_eq!(total_rows, 6, "snapshots of pruned scans must be deleted too");
    let oldest: i64 = s
        .conn
        .query_row("SELECT MIN(started_at) FROM scans", [], |r| r.get(0))
        .unwrap();
    assert_eq!(oldest, 3);
}

#[test]
fn retention_does_nothing_below_the_limit() {
    let mut s = Store::open_in_memory().unwrap();
    s.record_scan_keeping(1, &[ok("a", 0, None)], 10).unwrap();
    s.record_scan_keeping(2, &[ok("a", 0, None)], 10).unwrap();
    assert_eq!(s.scan_count().unwrap(), 2);
}

#[test]
fn data_persists_across_reopen_and_migration_is_idempotent() {
    let t = TempDir::new();
    let db = t.path().join("ares.db");
    {
        let mut s = Store::open(&db).unwrap();
        s.record_scan(42, &[ok("kept", 0, None)]).unwrap();
    }
    let s = Store::open(&db).unwrap();
    assert_eq!(s.schema_version().unwrap(), SCHEMA_VERSION);
    assert_eq!(s.latest_scan().unwrap().unwrap().started_at, 42);
    assert_eq!(s.scan_count().unwrap(), 1);
}

#[test]
fn a_newer_schema_is_refused_and_left_untouched() {
    let t = TempDir::new();
    let db = t.path().join("future.db");
    {
        let c = Connection::open(&db).unwrap();
        c.execute_batch("CREATE TABLE marker(x); PRAGMA user_version = 99;").unwrap();
    }
    match Store::open(&db) {
        Err(StoreError::NewerSchema { found, supported }) => {
            assert_eq!(found, 99);
            assert_eq!(supported, SCHEMA_VERSION);
        }
        Err(e) => panic!("wrong error: {e}"),
        Ok(_) => panic!("a newer database must be refused"),
    }
    let c = Connection::open(&db).unwrap();
    let v: i64 = c.query_row("PRAGMA user_version", [], |r| r.get(0)).unwrap();
    assert_eq!(v, 99);
    let n: i64 = c
        .query_row("SELECT COUNT(*) FROM sqlite_master WHERE name = 'scans'", [], |r| r.get(0))
        .unwrap();
    assert_eq!(n, 0, "no ares tables may be created in a newer database");
}

#[test]
fn database_and_directory_are_private() {
    let t = TempDir::new();
    let db = t.path().join("nested/data/ares.db");
    Store::open(&db).unwrap();
    let file_mode = fs::metadata(&db).unwrap().permissions().mode() & 0o777;
    let dir_mode = fs::metadata(db.parent().unwrap()).unwrap().permissions().mode() & 0o777;
    assert_eq!(file_mode, 0o600);
    assert_eq!(dir_mode, 0o700);
}

#[test]
fn unusable_location_is_an_io_error() {
    let t = TempDir::new();
    let blocker = t.path().join("file");
    fs::write(&blocker, "x").unwrap();
    // The parent "directory" is actually a file.
    match Store::open(&blocker.join("ares.db")) {
        Err(StoreError::Io(_)) => {}
        Err(e) => panic!("expected an io error, got {e}"),
        Ok(_) => panic!("opening under a file must fail"),
    }
}

#[test]
fn foreign_keys_are_enforced() {
    let s = Store::open_in_memory().unwrap();
    let r = s.conn.execute(
        "INSERT INTO repo_snapshots (scan_id, path, name) VALUES (999, '/x', 'x')",
        [],
    );
    assert!(r.is_err(), "orphan snapshot rows must be rejected");
}
