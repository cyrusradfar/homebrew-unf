//! Error types for the UNFUDGED filesystem flight recorder.
//!
//! Each subsystem defines its own error type using `thiserror`. The top-level
//! [`UnfError`] aggregates them for the binary boundary. This follows the SUPER
//! principle of explicit data flow: errors are values, not panics.

use thiserror::Error;

/// Top-level error type for the `unf` binary and public API.
///
/// Aggregates subsystem errors and adds application-level variants
/// for common user-facing conditions.
#[derive(Debug, Error)]
pub enum UnfError {
    /// Error from the content-addressable storage layer.
    #[error(transparent)]
    Cas(#[from] CasError),

    /// Error from the SQLite metadata layer.
    #[error(transparent)]
    Db(#[from] DbError),

    /// Error from the filesystem watcher.
    #[error(transparent)]
    Watcher(#[from] WatcherError),

    /// The current directory is not being watched.
    #[error("Not watching. Run 'unf watch' to start.")]
    NotInitialized,

    /// A CLI argument or parameter was invalid.
    #[error("Invalid argument: {0}")]
    InvalidArgument(String),

    /// No results matched the query.
    #[error("{0}")]
    NoResults(String),
}

/// Errors from the content-addressable storage layer.
///
/// Covers I/O failures when reading or writing objects, and
/// missing-object lookups.
#[derive(Debug, Error)]
pub enum CasError {
    /// An I/O error occurred while accessing the object store.
    #[error("CAS I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// The requested content hash was not found in the object store.
    #[error("Object not found: {0}")]
    ObjectNotFound(String),
}

/// Errors from the SQLite metadata layer.
///
/// Covers raw SQLite failures and schema migration problems.
#[derive(Debug, Error)]
pub enum DbError {
    /// A SQLite operation failed.
    #[error("Database error: {0}")]
    Sqlite(#[from] rusqlite::Error),

    /// A schema migration failed.
    #[error("Migration error: {0}")]
    Migration(String),
}

/// Errors from the filesystem watcher subsystem.
///
/// Covers notification library failures and underlying I/O errors.
#[derive(Debug, Error)]
pub enum WatcherError {
    /// The notification library reported an error.
    #[error("Watcher error: {0}")]
    Notify(#[from] notify::Error),

    /// An I/O error occurred in the watcher.
    #[error("Watcher I/O error: {0}")]
    Io(#[from] std::io::Error),
}

/// Convenience type alias used throughout the codebase.
pub type Result<T> = std::result::Result<T, UnfError>;

/// Process exit codes for the `unf` binary.
///
/// Maps specific error conditions to appropriate exit codes for shell scripting
/// and automation.
#[repr(i32)]
#[derive(Debug, Clone, Copy)]
pub enum ExitCode {
    /// General or unknown error.
    GeneralError = 1,
    /// Project not initialized (missing `.unfudged/`).
    NotInitialized = 2,
    /// Invalid argument or time specification.
    InvalidArgument = 3,
    /// No results matching query.
    NoResults = 4,
}

impl From<&UnfError> for ExitCode {
    fn from(err: &UnfError) -> Self {
        match err {
            UnfError::NotInitialized => ExitCode::NotInitialized,
            UnfError::InvalidArgument(_) => ExitCode::InvalidArgument,
            UnfError::NoResults(_) => ExitCode::NoResults,
            _ => ExitCode::GeneralError,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn not_initialized_message() {
        let err = UnfError::NotInitialized;
        assert_eq!(err.to_string(), "Not watching. Run 'unf watch' to start.");
    }

    #[test]
    fn invalid_argument_message() {
        let err = UnfError::InvalidArgument("bad --flag value".to_string());
        assert_eq!(err.to_string(), "Invalid argument: bad --flag value");
    }

    #[test]
    fn no_results_message() {
        let err = UnfError::NoResults("No history for test.txt.".to_string());
        assert_eq!(err.to_string(), "No history for test.txt.");
    }

    #[test]
    fn cas_io_error_converts() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "gone");
        let cas_err = CasError::from(io_err);
        let unf_err = UnfError::from(cas_err);
        assert!(unf_err.to_string().contains("CAS I/O error"));
    }

    #[test]
    fn cas_object_not_found_message() {
        let err = CasError::ObjectNotFound("deadbeef".to_string());
        assert_eq!(err.to_string(), "Object not found: deadbeef");
    }

    #[test]
    fn db_migration_error_message() {
        let err = DbError::Migration("v2 schema failed".to_string());
        assert_eq!(err.to_string(), "Migration error: v2 schema failed");
    }

    #[test]
    fn db_error_converts_to_unf_error() {
        let db_err = DbError::Migration("failed".to_string());
        let unf_err = UnfError::from(db_err);
        assert!(unf_err.to_string().contains("Migration error"));
    }

    #[test]
    fn watcher_io_error_converts() {
        let io_err = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "denied");
        let watch_err = WatcherError::from(io_err);
        let unf_err = UnfError::from(watch_err);
        assert!(unf_err.to_string().contains("Watcher I/O error"));
    }

    #[test]
    fn error_is_send_and_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<UnfError>();
        assert_send_sync::<CasError>();
        assert_send_sync::<DbError>();
        assert_send_sync::<WatcherError>();
    }

    #[test]
    fn exit_code_not_initialized() {
        let err = UnfError::NotInitialized;
        let code = ExitCode::from(&err);
        assert_eq!(code as i32, 2);
    }

    #[test]
    fn exit_code_invalid_argument() {
        let err = UnfError::InvalidArgument("bad time".to_string());
        let code = ExitCode::from(&err);
        assert_eq!(code as i32, 3);
    }

    #[test]
    fn exit_code_no_results() {
        let err = UnfError::NoResults("No history".to_string());
        let code = ExitCode::from(&err);
        assert_eq!(code as i32, 4);
    }

    #[test]
    fn exit_code_general_error() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "gone");
        let cas_err = CasError::from(io_err);
        let unf_err = UnfError::from(cas_err);
        let code = ExitCode::from(&unf_err);
        assert_eq!(code as i32, 1);
    }
}
