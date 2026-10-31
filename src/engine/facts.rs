//! Read-only facts about a project's store.
//!
//! `unf status` measures every registered project, including projects whose
//! daemon is not running. That rules out [`crate::engine::Engine::open`],
//! which creates a missing database, enables WAL, and migrates the schema.
//! Everything here is either a pure filesystem walk or a read-only SQLite
//! connection.

use std::fs;
use std::path::Path;
use std::time::Duration;

use chrono::{DateTime, Utc};
use rusqlite::{Connection, OpenFlags};

use crate::error::{CasError, DbError, UnfError};
use crate::storage;

/// The prune-relevant facts of one project's store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StorageFacts {
    /// When the project was last pruned for real, from `meta.last_prune_at`.
    pub last_prune_at: Option<DateTime<Utc>>,
    /// The timestamp of the oldest snapshot, or `None` when empty.
    pub oldest_snapshot_at: Option<DateTime<Utc>>,
}

/// Sums a directory tree. A missing path is 0, not an error.
///
/// A directory that vanishes between `stat` and `read_dir` still fails: the
/// missing-path case is the one callers rely on, and a half-read tree must not
/// silently under-report.
pub fn dir_size(path: &Path) -> Result<u64, UnfError> {
    let mut total = 0u64;
    walk_dir_sum(path, &mut total)?;
    Ok(total)
}

/// Recursively walks a directory tree and sums the size of all files.
fn walk_dir_sum(path: &Path, total: &mut u64) -> Result<(), UnfError> {
    if path.is_file() {
        let metadata = fs::metadata(path).map_err(CasError::Io)?;
        *total += metadata.len();
    } else if path.is_dir() {
        for entry in fs::read_dir(path).map_err(CasError::Io)? {
            let entry = entry.map_err(CasError::Io)?;
            walk_dir_sum(&entry.path(), total)?;
        }
    }
    Ok(())
}

/// Reads a project's facts through a read-only connection.
///
/// Returns `None` when the database does not exist. Never creates or migrates
/// it, never enables WAL, and never calls `init_schema`. A database that
/// cannot be read returns `Err`, which lets the caller skip the project.
///
/// # Errors
///
/// Returns `UnfError::Db` when the database exists but cannot be read as a
/// SQLite database of this schema.
pub fn read_storage_facts(storage_dir: &Path) -> Result<Option<StorageFacts>, UnfError> {
    let db_path = storage::db_path(storage_dir);
    if !db_path.is_file() {
        return Ok(None);
    }

    let conn = Connection::open_with_flags(&db_path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(DbError::from)?;
    conn.busy_timeout(Duration::from_secs(2))
        .map_err(DbError::from)?;

    let last_prune_at = read_last_prune_at(&conn)?;
    let oldest_snapshot_at = crate::engine::db::get_oldest_snapshot_time(&conn)?;

    Ok(Some(StorageFacts {
        last_prune_at,
        oldest_snapshot_at,
    }))
}

/// Reads `meta.last_prune_at`, tolerating a database that predates the table.
fn read_last_prune_at(conn: &Connection) -> Result<Option<DateTime<Utc>>, DbError> {
    if !has_meta_table(conn)? {
        return Ok(None);
    }

    let Some(value) =
        crate::engine::db::meta::get_meta(conn, crate::engine::db::meta::LAST_PRUNE_AT)?
    else {
        return Ok(None);
    };

    Ok(DateTime::parse_from_rfc3339(&value)
        .ok()
        .map(|time| time.with_timezone(&Utc)))
}

/// Whether the `meta` table exists. A v2 database opened before the upgrade
/// has not been migrated yet, so the table can be absent.
fn has_meta_table(conn: &Connection) -> Result<bool, DbError> {
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'meta'",
        [],
        |row| row.get(0),
    )?;
    Ok(count > 0)
}
