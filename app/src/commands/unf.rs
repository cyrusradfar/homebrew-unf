use std::process::Command;

use crate::error::AppError;

/// Resolve the full path to the `unf` binary.
/// macOS GUI apps don't inherit the shell PATH, so we check common locations.
fn find_unf() -> String {
    let candidates = [
        "/opt/homebrew/bin/unf",
        "/usr/local/bin/unf",
    ];
    for path in &candidates {
        if std::path::Path::new(path).exists() {
            return path.to_string();
        }
    }
    // Fall back to bare name (works if launched from terminal)
    "unf".to_string()
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
    serde_json::from_str(&json_str).map_err(|e| AppError::ParseError(e.to_string()))
}
