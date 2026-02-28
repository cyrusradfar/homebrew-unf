//! `unf unwatch` command implementation.
//!
//! Stops watching a specific project directory and deregisters it from the
//! global daemon. The per-project storage and history are preserved.

use std::fs;
use std::path::Path;

use crate::cli::OutputFormat;
use crate::error::UnfError;
use crate::storage;

/// JSON output for the unwatch command.
#[derive(serde::Serialize)]
struct UnwatchOutput {
    status: String,
}

pub fn run(project_root: &Path, format: OutputFormat) -> Result<(), UnfError> {
    // Use the canonical variant because the path may come from the registry
    // for orphaned projects whose directories no longer exist on disk.
    let storage_dir = storage::resolve_storage_dir_canonical(project_root)?;

    // Remove from intent registry (source of truth)
    if let Err(e) = crate::intent::remove_project(project_root) {
        super::output::print_warning(&format!("Failed to remove intent: {}", e));
    }

    // Always unregister from registry first (even if storage dir is gone)
    if let Err(e) = crate::registry::unregister_project(project_root) {
        super::output::print_warning(&format!("Failed to unregister project: {}", e));
    }

    // Audit log
    crate::audit::log_event("UNWATCH", &project_root.display().to_string());

    // Check if storage exists
    if !storage_dir.exists() {
        if format == OutputFormat::Json {
            let output = UnwatchOutput {
                status: "unwatched".to_string(),
            };
            println!("{}", serde_json::to_string_pretty(&output).unwrap());
        } else {
            super::output::print_status("Unwatching", &project_root.display().to_string());
        }
        return Ok(());
    }

    // Create stopped sentinel
    let stopped_path = storage::stopped_path(&storage_dir);
    let _ = fs::write(&stopped_path, b"");

    // Signal global daemon to reload (it will remove this project)
    if let Ok(global_pid_path) = storage::global_pid_path() {
        if let Ok(pid_str) = fs::read_to_string(&global_pid_path) {
            if let Ok(pid) = pid_str.trim().parse::<u32>() {
                if crate::process::is_alive(pid) {
                    let _ = crate::process::send_signal(pid, signal_hook::consts::SIGUSR1);
                }
            }
        }
    }

    // Clean up per-project PID file
    let pid_path = storage::pid_path(&storage_dir);
    let _ = fs::remove_file(&pid_path);

    // If no more registered projects, remove auto-start
    if let Ok(false) = crate::registry::has_projects() {
        let _ = crate::autostart::remove();
    }

    if format == OutputFormat::Json {
        let output = UnwatchOutput {
            status: "unwatched".to_string(),
        };
        println!("{}", serde_json::to_string_pretty(&output).unwrap());
    } else {
        super::output::print_status("Unwatching", &project_root.display().to_string());
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn unwatch_nonexistent_storage() {
        let _guard = crate::test_util::ENV_LOCK.lock().unwrap();
        let unf_home = TempDir::new().expect("create unf_home");
        let project = TempDir::new().expect("create project");
        std::env::set_var("UNF_HOME", unf_home.path());

        let result = run(project.path(), OutputFormat::Human);
        assert!(result.is_ok());
        std::env::remove_var("UNF_HOME");
    }
}
