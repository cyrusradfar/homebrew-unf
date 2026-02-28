use tauri::State;

use crate::commands::unf::run_unf;
use crate::error::AppError;
use crate::state::AppState;

/// Get file content at a point in time.
/// Invokes: `unf cat FILE --json [flags]`
/// If `project` is provided, uses that instead of the selected project (for global mode).
#[tauri::command]
pub fn get_file_content(
    state: State<'_, AppState>,
    project: Option<String>,
    file: String,
    at: Option<String>,
    snapshot: Option<i64>,
) -> Result<serde_json::Value, AppError> {
    let proj = match project {
        Some(p) => p,
        None => state.selected_project()?,
    };
    let mut args: Vec<String> = vec!["cat".to_string(), file];

    if let Some(ref a) = at {
        args.push("--at".to_string());
        args.push(a.clone());
    }
    if let Some(s) = snapshot {
        args.push("--snapshot".to_string());
        args.push(s.to_string());
    }

    let arg_refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
    run_unf(&proj, &arg_refs)
}
