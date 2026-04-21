use tauri::State;

use crate::commands::unf::{run_unf, run_unf_global};
use crate::error::AppError;
use crate::state::AppState;

/// Get paginated log entries.
/// Invokes: `unf log [target] --json [flags]`
#[allow(clippy::too_many_arguments)]
#[tauri::command]
pub async fn get_log(
    state: State<'_, AppState>,
    target: Option<String>,
    since: Option<String>,
    until: Option<String>,
    limit: Option<u32>,
    cursor: Option<String>,
    include: Option<Vec<String>>,
    exclude: Option<Vec<String>>,
    group_by_file: Option<bool>,
) -> Result<serde_json::Value, AppError> {
    let project = state.selected_project()?;
    let mut args: Vec<String> = vec!["log".to_string()];

    if let Some(t) = target {
        args.push(t);
    }
    if let Some(s) = since {
        args.push("--since".to_string());
        args.push(s);
    }
    if let Some(u) = until {
        args.push("--until".to_string());
        args.push(u);
    }
    if let Some(l) = limit {
        args.push("--limit".to_string());
        args.push(l.to_string());
    }
    if let Some(c) = cursor {
        args.push("--cursor".to_string());
        args.push(c);
    }
    if let Some(patterns) = include {
        for pattern in patterns {
            args.push("--include".to_string());
            args.push(pattern);
        }
    }
    if let Some(patterns) = exclude {
        for pattern in patterns {
            args.push("--exclude".to_string());
            args.push(pattern);
        }
    }
    if group_by_file == Some(true) {
        args.push("--group-by-file".to_string());
    }

    run_unf(project, args).await
}

/// Get global (cross-project) log entries.
/// Invokes: `unf log --global --json [flags]`
#[allow(clippy::too_many_arguments)]
#[tauri::command]
pub async fn get_global_log(
    since: Option<String>,
    until: Option<String>,
    limit: Option<u32>,
    include: Option<Vec<String>>,
    exclude: Option<Vec<String>>,
    group_by_file: Option<bool>,
    include_project: Option<Vec<String>>,
    exclude_project: Option<Vec<String>>,
) -> Result<serde_json::Value, AppError> {
    let mut args: Vec<String> = vec!["log".to_string(), "--global".to_string()];

    if let Some(s) = since {
        args.push("--since".to_string());
        args.push(s);
    }
    if let Some(u) = until {
        args.push("--until".to_string());
        args.push(u);
    }
    if let Some(l) = limit {
        args.push("--limit".to_string());
        args.push(l.to_string());
    }
    if let Some(patterns) = include {
        for pattern in patterns {
            args.push("--include".to_string());
            args.push(pattern);
        }
    }
    if let Some(patterns) = exclude {
        for pattern in patterns {
            args.push("--exclude".to_string());
            args.push(pattern);
        }
    }
    if group_by_file == Some(true) {
        args.push("--group-by-file".to_string());
    }
    if let Some(projects) = include_project {
        for p in projects {
            args.push("--include-project".to_string());
            args.push(p);
        }
    }
    if let Some(projects) = exclude_project {
        for p in projects {
            args.push("--exclude-project".to_string());
            args.push(p);
        }
    }

    run_unf_global(args).await
}

/// Get density histogram data for the time scrubber.
/// Invokes: `unf log --density --json --buckets N [flags]`
#[tauri::command]
pub async fn get_density(
    state: State<'_, AppState>,
    buckets: Option<u32>,
    since: Option<String>,
    include: Option<Vec<String>>,
    exclude: Option<Vec<String>>,
) -> Result<serde_json::Value, AppError> {
    let project = state.selected_project()?;
    let mut args: Vec<String> = vec!["log".to_string(), "--density".to_string()];

    let bucket_count = buckets.unwrap_or(100);
    args.push("--buckets".to_string());
    args.push(bucket_count.to_string());

    if let Some(s) = since {
        args.push("--since".to_string());
        args.push(s);
    }
    if let Some(patterns) = include {
        for pattern in patterns {
            args.push("--include".to_string());
            args.push(pattern);
        }
    }
    if let Some(patterns) = exclude {
        for pattern in patterns {
            args.push("--exclude".to_string());
            args.push(pattern);
        }
    }

    run_unf(project, args).await
}

/// Get global density histogram data for the time scrubber.
/// Invokes: `unf log --global --density --json --buckets N [flags]`
#[tauri::command]
pub async fn get_global_density(
    buckets: Option<u32>,
    since: Option<String>,
    include: Option<Vec<String>>,
    exclude: Option<Vec<String>>,
) -> Result<serde_json::Value, AppError> {
    let mut args: Vec<String> = vec![
        "log".to_string(),
        "--global".to_string(),
        "--density".to_string(),
    ];

    let bucket_count = buckets.unwrap_or(100);
    args.push("--buckets".to_string());
    args.push(bucket_count.to_string());

    if let Some(s) = since {
        args.push("--since".to_string());
        args.push(s);
    }
    if let Some(patterns) = include {
        for pattern in patterns {
            args.push("--include".to_string());
            args.push(pattern);
        }
    }
    if let Some(patterns) = exclude {
        for pattern in patterns {
            args.push("--exclude".to_string());
            args.push(pattern);
        }
    }

    run_unf_global(args).await
}
