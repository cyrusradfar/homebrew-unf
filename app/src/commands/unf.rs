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
pub fn run_unf(project_dir: &str, args: &[&str]) -> Result<serde_json::Value, AppError> {
    let mut cmd = Command::new(find_unf());
    cmd.current_dir(project_dir);
    cmd.args(args);
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
}

/// Run `unf` without project context (e.g., `unf list`).
/// Appends `--json` automatically. Returns parsed JSON.
pub fn run_unf_global(args: &[&str]) -> Result<serde_json::Value, AppError> {
    let mut cmd = Command::new(find_unf());
    cmd.args(args);
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
            serde_json::from_str(last_line)
                .map_err(|e| AppError::ParseError(e.to_string()))
        }
    }
}
