use tauri::State;

use crate::commands::unf::{run_unf, run_unf_global};
use crate::error::AppError;
use crate::state::AppState;

/// List all registered projects.
/// Invokes: `unf list --json`
#[tauri::command]
pub fn list_projects() -> Result<serde_json::Value, AppError> {
    run_unf_global(&["list"])
}

/// Select a project and return its status.
/// Invokes: `unf status --json` in the project directory.
#[tauri::command]
pub fn select_project(
    state: State<'_, AppState>,
    path: String,
) -> Result<serde_json::Value, AppError> {
    state.set_selected_project(path.clone())?;
    run_unf(&path, &["status"])
}

/// Get the status of the currently selected project.
/// Invokes: `unf status --json` in the project directory.
#[tauri::command]
pub fn get_project_status(state: State<'_, AppState>) -> Result<serde_json::Value, AppError> {
    let project = state.selected_project()?;
    run_unf(&project, &["status"])
}

/// Remove (unwatch) a project from the registry.
/// Invokes: `unf unwatch --project PATH --json`
#[tauri::command]
pub fn remove_project(path: String) -> Result<serde_json::Value, AppError> {
    run_unf_global(&["unwatch", "--project", &path])
}
