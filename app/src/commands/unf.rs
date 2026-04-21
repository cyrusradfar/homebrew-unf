use std::process::Command;
use std::sync::OnceLock;

use crate::error::AppError;

/// Resolve the full path to the `unf` binary (cached for process lifetime).
/// macOS GUI apps don't inherit the shell PATH, so we check common locations.
/// Note: if `unf` is installed after app launch, restart the app to pick it up.
fn find_unf() -> &'static str {
    static UNF_PATH: OnceLock<String> = OnceLock::new();
    UNF_PATH.get_or_init(|| {
        for path in ["/opt/homebrew/bin/unf", "/usr/local/bin/unf"] {
            if std::path::Path::new(path).exists() {
                return path.to_string();
            }
        }
        "unf".to_string()
    })
}

/// Run `unf` with the given args in the given project directory.
/// Appends `--json` automatically. Returns parsed JSON.
///
/// Runs the blocking subprocess on Tauri's blocking thread pool so the
/// caller's async task (and on macOS the main/UI thread) stays responsive.
pub async fn run_unf(
    project_dir: String,
    args: Vec<String>,
) -> Result<serde_json::Value, AppError> {
    tauri::async_runtime::spawn_blocking(move || {
        #[cfg(any(test, debug_assertions))]
        if let Ok(ms) = std::env::var("UNF_SLOW_MS") {
            if let Ok(n) = ms.parse::<u64>() {
                std::thread::sleep(std::time::Duration::from_millis(n));
            }
        }

        let arg_refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();

        let mut cmd = Command::new(find_unf());
        cmd.current_dir(&project_dir);
        cmd.args(&arg_refs);
        cmd.arg("--json");

        let output = cmd
            .output()
            .map_err(|e| AppError::SpawnFailed(e.to_string()))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();
            return Err(AppError::UnfError(stderr));
        }

        let json_str = String::from_utf8_lossy(&output.stdout);
        serde_json::from_str(&json_str).map_err(|e| AppError::ParseError(e.to_string()))
    })
    .await
    .map_err(|e| AppError::JoinFailed(e.to_string()))?
}

/// Run `unf` without project context (e.g., `unf list`).
/// Appends `--json` automatically. Returns parsed JSON.
///
/// Runs the blocking subprocess on Tauri's blocking thread pool so the
/// caller's async task (and on macOS the main/UI thread) stays responsive.
pub async fn run_unf_global(args: Vec<String>) -> Result<serde_json::Value, AppError> {
    tauri::async_runtime::spawn_blocking(move || {
        #[cfg(any(test, debug_assertions))]
        if let Ok(ms) = std::env::var("UNF_SLOW_MS") {
            if let Ok(n) = ms.parse::<u64>() {
                std::thread::sleep(std::time::Duration::from_millis(n));
            }
        }

        let arg_refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();

        let mut cmd = Command::new(find_unf());
        cmd.args(&arg_refs);
        cmd.arg("--json");

        let output = cmd
            .output()
            .map_err(|e| AppError::SpawnFailed(e.to_string()))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();
            return Err(AppError::UnfError(stderr));
        }

        let json_str = String::from_utf8_lossy(&output.stdout);

        // Some commands (e.g. --move-storage) emit newline-delimited JSON events.
        // Parse the last non-empty line as the final result.
        match serde_json::from_str(&json_str) {
            Ok(v) => Ok(v),
            Err(_) => {
                let last_line = json_str
                    .lines()
                    .rev()
                    .find(|l| !l.trim().is_empty())
                    .unwrap_or("");
                serde_json::from_str(last_line).map_err(|e| AppError::ParseError(e.to_string()))
            }
        }
    })
    .await
    .map_err(|e| AppError::JoinFailed(e.to_string()))?
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    #[test]
    fn concurrent_run_unf_global_parallelizes() {
        // SAFETY: manipulates process-wide env; run with --test-threads=1 if
        // other tests ever read UNF_SLOW_MS. Currently no other test does.
        std::env::set_var("UNF_SLOW_MS", "1500");

        let start = Instant::now();
        tauri::async_runtime::block_on(async {
            let a = run_unf_global(vec!["list".to_string()]);
            let b = run_unf_global(vec!["status".to_string()]);
            let _ = tokio::join!(a, b);
        });
        let elapsed = start.elapsed();

        std::env::remove_var("UNF_SLOW_MS");

        assert!(
            elapsed < std::time::Duration::from_millis(2500),
            "Concurrent run_unf_global calls took {elapsed:?}; expected <2500ms. \
             If this regressed to ~3000ms, spawn_blocking is not offloading the \
             blocking work — commands are serializing on the caller thread."
        );
    }
}
