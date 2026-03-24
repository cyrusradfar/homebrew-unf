use crate::commands::unf::{run_unf, run_unf_global};
use crate::error::AppError;

/// Watch a new project directory.
/// Invokes: `unf watch --json` in the given directory.
#[tauri::command]
pub fn watch_project(path: String) -> Result<serde_json::Value, AppError> {
    run_unf(&path, &["watch"])
}

/// Unwatch a project directory.
/// Invokes: `unf unwatch --project PATH --json`
#[tauri::command]
pub fn unwatch_project(path: String) -> Result<serde_json::Value, AppError> {
    run_unf_global(&["unwatch", "--project", &path])
}

/// Stop the global daemon.
/// Invokes: `unf stop --json`
#[tauri::command]
pub fn stop_daemon() -> Result<serde_json::Value, AppError> {
    run_unf_global(&["stop"])
}

/// Restart the global daemon.
/// Invokes: `unf restart --json`
#[tauri::command]
pub fn restart_daemon() -> Result<serde_json::Value, AppError> {
    run_unf_global(&["restart"])
}

/// Get daemon status.
/// Invokes: `unf status --json` globally.
#[tauri::command]
pub fn get_daemon_status() -> Result<serde_json::Value, AppError> {
    run_unf_global(&["status"])
}
