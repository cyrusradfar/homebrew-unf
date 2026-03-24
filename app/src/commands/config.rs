use crate::commands::unf::run_unf_global;
use crate::error::AppError;

/// Get current config (storage location, usage).
/// Invokes: `unf config --json`
#[tauri::command]
pub fn get_config() -> Result<serde_json::Value, AppError> {
    run_unf_global(&["config"])
}

/// Move storage to a new location.
/// Invokes: `unf config --move-storage PATH --json`
#[tauri::command]
pub fn move_storage(path: String) -> Result<serde_json::Value, AppError> {
    run_unf_global(&["config", "--move-storage", &path])
}
