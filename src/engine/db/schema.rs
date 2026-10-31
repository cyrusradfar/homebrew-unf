//! Schema initialization and migrations.

use crate::error::DbError;
use rusqlite::Connection;

/// Current schema version.
///
/// Increment this when making schema changes, and add migration logic
/// to `init_schema()`.
const SCHEMA_VERSION: i64 = 3;

/// Every table and index this binary needs, created idempotently.
///
/// Kept out of `init_schema` so the function stays readable and under the
/// function-length gate; the SQL is unchanged from its previous inline form.
const SCHEMA_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS schema_version (
    version INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS snapshots (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    file_path   TEXT    NOT NULL,
    content_hash TEXT   NOT NULL,
    size_bytes  INTEGER NOT NULL,
    timestamp   TEXT    NOT NULL,
    event_type  TEXT    NOT NULL,
    line_count  INTEGER NOT NULL DEFAULT 0,
    lines_added INTEGER NOT NULL DEFAULT 0,
    lines_removed INTEGER NOT NULL DEFAULT 0
);

CREATE INDEX IF NOT EXISTS idx_snapshots_file_time
    ON snapshots (file_path, timestamp DESC);

CREATE INDEX IF NOT EXISTS idx_snapshots_time
    ON snapshots (timestamp DESC);

CREATE TABLE IF NOT EXISTS meta (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
"#;

/// The v1 → v2 step: the three line-statistics columns.
const V1_TO_V2_ALTERS: &str = r#"
ALTER TABLE snapshots ADD COLUMN line_count INTEGER NOT NULL DEFAULT 0;
ALTER TABLE snapshots ADD COLUMN lines_added INTEGER NOT NULL DEFAULT 0;
ALTER TABLE snapshots ADD COLUMN lines_removed INTEGER NOT NULL DEFAULT 0;
"#;

/// Initializes the database schema if it does not exist.
///
/// This function is idempotent: it uses `IF NOT EXISTS` clauses to avoid
/// errors on repeated calls. Schema version tracking is included for
/// future migration support.
///
/// # Arguments
/// * `conn` - SQLite connection to initialize
///
/// # Errors
/// Returns `DbError::Sqlite` if schema creation fails.
pub fn init_schema(conn: &Connection) -> Result<(), DbError> {
    conn.execute_batch(SCHEMA_SQL)?;

    // Insert schema version if the table is empty
    let version_exists: bool =
        conn.query_row("SELECT EXISTS(SELECT 1 FROM schema_version)", [], |row| {
            row.get(0)
        })?;

    if !version_exists {
        conn.execute(
            "INSERT INTO schema_version (version) VALUES (?1)",
            [SCHEMA_VERSION],
        )?;
        return Ok(());
    }

    let current_version: i64 =
        conn.query_row("SELECT version FROM schema_version", [], |row| row.get(0))?;

    // Only a v1 database needs the ALTERs. They are not idempotent, so the
    // guard must stay equality-based.
    if current_version == 1 && SCHEMA_VERSION > 1 {
        conn.execute_batch(V1_TO_V2_ALTERS)?;
    }

    // Advance every older database in one step. `WHERE version < ?1` keeps the
    // update monotonic: a future database is never downgraded by this binary.
    conn.execute(
        "UPDATE schema_version SET version = ?1 WHERE version < ?1",
        [SCHEMA_VERSION],
    )?;

    Ok(())
}
