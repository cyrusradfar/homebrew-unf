use crate::commands::unf::{run_unf, run_unf_global};
use crate::error::AppError;

/// Watch a new project directory.
/// Invokes: `unf watch --json` in the given directory.
#[tauri::command]
pub async fn watch_project(path: String) -> Result<serde_json::Value, AppError> {
    run_unf(path, vec!["watch".to_string()]).await
}

/// Unwatch a project directory.
/// Invokes: `unf unwatch --project PATH --json`
#[tauri::command]
pub async fn unwatch_project(path: String) -> Result<serde_json::Value, AppError> {
    run_unf_global(vec!["unwatch".to_string(), "--project".to_string(), path]).await
}

/// Stop the global daemon.
/// Invokes: `unf stop --json`
#[tauri::command]
pub async fn stop_daemon() -> Result<serde_json::Value, AppError> {
    run_unf_global(vec!["stop".to_string()]).await
}

/// Restart the global daemon.
/// Invokes: `unf restart --json`
#[tauri::command]
pub async fn restart_daemon() -> Result<serde_json::Value, AppError> {
    run_unf_global(vec!["restart".to_string()]).await
}

/// Get daemon status.
/// Invokes: `unf status --json` globally.
#[tauri::command]
pub async fn get_daemon_status() -> Result<serde_json::Value, AppError> {
    run_unf_global(vec!["status".to_string()]).await
}
