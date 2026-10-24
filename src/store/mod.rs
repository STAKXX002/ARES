//! Local storage for scan history, in ARES's own SQLite database.
//!
//! This is the only place ARES writes, and it writes only to its own
//! database file. The database holds repository paths and Git state, so the
//! directory is created private (0700) and the file is restricted to its
//! owner (0600).
//!
//! The schema is versioned with SQLite's `user_version`. Opening a database
//! written by a newer ARES is refused rather than risking corruption.

use crate::repo::RepoReport;
use rusqlite::{params, Connection};
use std::error::Error;
use std::fmt;
use std::fs;
use std::io;
use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
use std::path::Path;
use std::time::Duration;

/// Ordered migrations. Entry `i` upgrades the schema from version `i` to
/// `i + 1`. Never edit an existing entry once released; append a new one.
const MIGRATIONS: &[&str] = &[
    // Version 1: scans and per-repository snapshots.
    "
    CREATE TABLE scans (
        id         INTEGER PRIMARY KEY,
        started_at INTEGER NOT NULL,
        repo_count INTEGER NOT NULL
    );

    CREATE TABLE repo_snapshots (
        scan_id       INTEGER NOT NULL REFERENCES scans(id) ON DELETE CASCADE,
        path          TEXT    NOT NULL,
        name          TEXT    NOT NULL,
        error         TEXT,
        branch        TEXT,
        detached      INTEGER,
        unborn        INTEGER,
        upstream      TEXT,
        ahead         INTEGER,
        behind        INTEGER,
        staged        INTEGER,
        unstaged      INTEGER,
        untracked     INTEGER,
        conflicted    INTEGER,
        changed_files INTEGER,
        PRIMARY KEY (scan_id, path)
    ) WITHOUT ROWID;

    CREATE INDEX idx_snapshots_path ON repo_snapshots(path, scan_id);
    ",
];

/// Newest schema version this build understands.
pub const SCHEMA_VERSION: i64 = MIGRATIONS.len() as i64;

/// How many scans are kept before the oldest are deleted.
pub const MAX_SCANS: usize = 1000;

#[derive(Debug)]
pub enum StoreError {
    Io(io::Error),
    Sqlite(rusqlite::Error),
    /// The database was written by a newer version of ARES.
    NewerSchema { found: i64, supported: i64 },
}

impl fmt::Display for StoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StoreError::Io(e) => write!(f, "storage i/o error: {e}"),
            StoreError::Sqlite(e) => write!(f, "database error: {e}"),
            StoreError::NewerSchema { found, supported } => write!(
                f,
                "database schema version {found} is newer than this ares supports ({supported}); upgrade ares"
            ),
        }
    }
}

impl Error for StoreError {}

impl From<io::Error> for StoreError {
    fn from(e: io::Error) -> Self {
        StoreError::Io(e)
    }
}

impl From<rusqlite::Error> for StoreError {
    fn from(e: rusqlite::Error) -> Self {
        StoreError::Sqlite(e)
    }
}

/// A recorded scan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScanRecord {
    pub id: i64,
    /// Seconds since the Unix epoch.
    pub started_at: u64,
    pub repo_count: usize,
}

pub struct Store {
    conn: Connection,
}

impl Store {
    /// Open (creating if needed) the database at `path` and bring its
    /// schema up to date.
    pub fn open(path: &Path) -> Result<Store, StoreError> {
        if let Some(dir) = path.parent() {
            if !dir.as_os_str().is_empty() {
                fs::DirBuilder::new().recursive(true).mode(0o700).create(dir)?;
            }
        }
        let conn = Connection::open(path)?;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
        Store::init(conn)
    }

    #[cfg(test)]
    pub fn open_in_memory() -> Result<Store, StoreError> {
        Store::init(Connection::open_in_memory()?)
    }

    fn init(mut conn: Connection) -> Result<Store, StoreError> {
        conn.busy_timeout(Duration::from_secs(5))?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        migrate(&mut conn)?;
        Ok(Store { conn })
    }

    /// Schema version of the open database.
    #[cfg(test)]
    pub fn schema_version(&self) -> Result<i64, StoreError> {
        Ok(self
            .conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))?)
    }

    /// Record a scan and its per-repository snapshots in one transaction,
    /// keeping only the newest `MAX_SCANS` scans.
    pub fn record_scan(&mut self, at: u64, reports: &[RepoReport]) -> Result<ScanRecord, StoreError> {
        self.record_scan_keeping(at, reports, MAX_SCANS)
    }

    pub fn record_scan_keeping(
        &mut self,
        at: u64,
        reports: &[RepoReport],
        keep: usize,
    ) -> Result<ScanRecord, StoreError> {
        let tx = self.conn.transaction()?;
        tx.execute(
            "INSERT INTO scans (started_at, repo_count) VALUES (?1, ?2)",
            params![at as i64, reports.len() as i64],
        )?;
        let id = tx.last_insert_rowid();

        {
            let mut insert = tx.prepare(
                "INSERT INTO repo_snapshots (
                    scan_id, path, name, error, branch, detached, unborn, upstream,
                    ahead, behind, staged, unstaged, untracked, conflicted, changed_files
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)
                 ON CONFLICT(scan_id, path) DO NOTHING",
            )?;
            for r in reports {
                let path = r.path.to_string_lossy();
                match &r.state {
                    Err(e) => {
                        insert.execute(params![
                            id, path, r.name, e, None::<String>, None::<bool>, None::<bool>,
                            None::<String>, None::<i64>, None::<i64>, None::<i64>, None::<i64>,
                            None::<i64>, None::<i64>, None::<i64>
                        ])?;
                    }
                    Ok(s) => {
                        insert.execute(params![
                            id,
                            path,
                            r.name,
                            None::<String>,
                            s.branch,
                            s.detached,
                            s.unborn,
                            s.upstream,
                            s.ahead.map(i64::from),
                            s.behind.map(i64::from),
                            s.staged as i64,
                            s.unstaged as i64,
                            s.untracked as i64,
                            s.conflicted as i64,
                            s.changed_files as i64
                        ])?;
                    }
                }
            }
        }

        // Retention: delete everything older than the newest `keep` scans.
        // Snapshots go with them through ON DELETE CASCADE.
        tx.execute(
            "DELETE FROM scans WHERE id <= (
                 SELECT id FROM scans ORDER BY id DESC LIMIT 1 OFFSET ?1
             )",
            params![keep as i64],
        )?;
        tx.commit()?;

        Ok(ScanRecord {
            id,
            started_at: at,
            repo_count: reports.len(),
        })
    }

    /// The most recently recorded scan, if any.
    pub fn latest_scan(&self) -> Result<Option<ScanRecord>, StoreError> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, started_at, repo_count FROM scans ORDER BY id DESC LIMIT 1")?;
        let mut rows = stmt.query([])?;
        match rows.next()? {
            None => Ok(None),
            Some(row) => Ok(Some(ScanRecord {
                id: row.get(0)?,
                started_at: row.get::<_, i64>(1)? as u64,
                repo_count: row.get::<_, i64>(2)? as usize,
            })),
        }
    }

    /// Number of snapshot rows stored for a scan.
    #[cfg(test)]
    pub fn snapshot_count(&self, scan_id: i64) -> Result<usize, StoreError> {
        Ok(self.conn.query_row(
            "SELECT COUNT(*) FROM repo_snapshots WHERE scan_id = ?1",
            params![scan_id],
            |r| r.get::<_, i64>(0),
        )? as usize)
    }

    #[cfg(test)]
    pub fn scan_count(&self) -> Result<usize, StoreError> {
        Ok(self
            .conn
            .query_row("SELECT COUNT(*) FROM scans", [], |r| r.get::<_, i64>(0))? as usize)
    }
}

/// Apply any migrations the database has not seen yet, each in its own
/// transaction so a failure leaves the previous version intact.
fn migrate(conn: &mut Connection) -> Result<(), StoreError> {
    let current: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    if current > SCHEMA_VERSION {
        return Err(StoreError::NewerSchema {
            found: current,
            supported: SCHEMA_VERSION,
        });
    }
    for (i, sql) in MIGRATIONS.iter().enumerate().skip(current as usize) {
        let tx = conn.transaction()?;
        tx.execute_batch(sql)?;
        tx.execute_batch(&format!("PRAGMA user_version = {}", i + 1))?;
        tx.commit()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests;
