use tauri::State;

use crate::commands::unf::{run_unf, run_unf_global};
use crate::error::AppError;
use crate::state::AppState;

/// List all registered projects.
/// Invokes: `unf list --json`
#[tauri::command]
pub async fn list_projects() -> Result<serde_json::Value, AppError> {
    run_unf_global(vec!["list".to_string()]).await
}

/// Select a project and return its status.
/// Invokes: `unf status --json` in the project directory.
#[tauri::command]
pub async fn select_project(
    state: State<'_, AppState>,
    path: String,
) -> Result<serde_json::Value, AppError> {
    state.set_selected_project(path.clone())?;
    run_unf(path, vec!["status".to_string()]).await
}

/// Get the status of the currently selected project.
/// Invokes: `unf status --json` in the project directory.
#[tauri::command]
pub async fn get_project_status(state: State<'_, AppState>) -> Result<serde_json::Value, AppError> {
    let project = state.selected_project()?;
    run_unf(project, vec!["status".to_string()]).await
}

/// Remove (unwatch) a project from the registry.
/// Invokes: `unf unwatch --project PATH --json`
#[tauri::command]
pub async fn remove_project(path: String) -> Result<serde_json::Value, AppError> {
    run_unf_global(vec!["unwatch".to_string(), "--project".to_string(), path]).await
}
