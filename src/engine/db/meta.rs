//! Key/value metadata stored beside the snapshot history.
//!
//! The table has no schema of its own: keys are constants, and the value is
//! always TEXT. `last_prune_at` holds an RFC 3339 UTC timestamp that is parsed
//! at the read site, so a corrupt row degrades to "never pruned" instead of
//! failing the read.

use rusqlite::{Connection, OptionalExtension};

use crate::error::DbError;

/// Key holding the RFC 3339 time of the last successful real prune.
pub(crate) const LAST_PRUNE_AT: &str = "last_prune_at";

/// Reads one metadata value, or `None` when the key has never been written.
///
/// A database that predates the `meta` table returns `DbError::Sqlite`; callers
/// that read foreign stores handle that as `None`.
pub(crate) fn get_meta(conn: &Connection, key: &str) -> Result<Option<String>, DbError> {
    let value = conn
        .query_row("SELECT value FROM meta WHERE key = ?1", [key], |row| {
            row.get(0)
        })
        .optional()?;
    Ok(value)
}

/// Writes one metadata value, replacing any previous value for the key.
pub(crate) fn set_meta(conn: &Connection, key: &str, value: &str) -> Result<(), DbError> {
    conn.execute(
        "INSERT INTO meta (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        [key, value],
    )?;
    Ok(())
}
