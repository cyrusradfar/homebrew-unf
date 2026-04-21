use tauri::State;

use crate::commands::unf::run_unf;
use crate::error::AppError;
use crate::state::AppState;

/// Get diff output for a time point or range.
/// Invokes: `unf diff --json [flags]`
/// If `project` is provided, uses that instead of the selected project (for global mode).
#[allow(clippy::too_many_arguments)]
#[tauri::command]
pub async fn get_diff(
    state: State<'_, AppState>,
    project: Option<String>,
    at: Option<String>,
    from: Option<String>,
    to: Option<String>,
    file: Option<String>,
    snapshot: Option<i64>,
    context: Option<usize>,
) -> Result<serde_json::Value, AppError> {
    let proj = match project {
        Some(p) => p,
        None => state.selected_project()?,
    };
    let mut args: Vec<String> = vec!["diff".to_string()];

    if let Some(id) = snapshot {
        args.push("--snapshot".to_string());
        args.push(id.to_string());
    } else {
        if let Some(a) = at {
            args.push("--at".to_string());
            args.push(a);
        }
        if let Some(f) = from {
            args.push("--from".to_string());
            args.push(f);
        }
        if let Some(t) = to {
            args.push("--to".to_string());
            args.push(t);
        }
        if let Some(fi) = file {
            args.push(fi);
        }
    }

    if let Some(ctx) = context {
        args.push("--context".to_string());
        args.push(ctx.to_string());
    }

    run_unf(proj, args).await
}
