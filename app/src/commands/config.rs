use crate::commands::unf::run_unf_global;
use crate::error::AppError;

/// Get current config (storage location, usage).
/// Invokes: `unf config --json`
#[tauri::command]
pub async fn get_config() -> Result<serde_json::Value, AppError> {
    run_unf_global(vec!["config".to_string()]).await
}

/// Move storage to a new location.
/// Invokes: `unf config --move-storage PATH --force --json`
/// Always passes --force since the UI has its own confirmation dialog.
/// Async so it runs on a background thread instead of blocking the UI.
#[tauri::command]
pub async fn move_storage(path: String) -> Result<serde_json::Value, AppError> {
    run_unf_global(vec![
        "config".to_string(),
        "--move-storage".to_string(),
        path,
        "--force".to_string(),
    ])
    .await
}
