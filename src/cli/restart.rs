//! `unf restart` command implementation.
//!
//! Restarts the global daemon and sentinel. Stops the current sentinel and
//! daemon, removes stopped markers, clears per-project stopped sentinels,
//! then starts the sentinel (which manages daemon lifecycle).

use std::fs;
use std::path::Path;
use std::thread;
use std::time::Duration;

use crate::cli::OutputFormat;
use crate::error::UnfError;
use crate::storage;

/// JSON output for the restart command.
#[derive(serde::Serialize)]
struct RestartOutput {
    status: String,
}

pub fn run(_project_root: &Path, format: OutputFormat) -> Result<(), UnfError> {
    // Kill sentinel first (prevents it from respawning daemon during restart)
    if let Err(e) = crate::sentinel::kill_sentinel() {
        super::output::print_warning(&format!("Failed to stop sentinel: {}", e));
    }

    // Stop daemon if running
    let global_pid_path = storage::global_pid_path()?;
    if let Ok(pid_str) = fs::read_to_string(&global_pid_path) {
        if let Ok(pid) = pid_str.trim().parse::<u32>() {
            if crate::process::is_alive(pid) {
                let _ = crate::process::terminate(pid);
                for _ in 0..20 {
                    if !crate::process::is_alive(pid) {
                        break;
                    }
                    thread::sleep(Duration::from_millis(100));
                }
            }
        }
        let _ = fs::remove_file(&global_pid_path);
    }

    // Remove global stopped marker
    if let Ok(stopped_path) = storage::global_stopped_path() {
        let _ = fs::remove_file(&stopped_path);
    }

    // Check if there are projects to watch (check both registry and intent)
    let has_registry_projects = crate::registry::has_projects().unwrap_or(false);
    let has_intent_projects = crate::intent::load()
        .map(|i| !i.projects.is_empty())
        .unwrap_or(false);

    if !has_registry_projects && !has_intent_projects {
        if format == OutputFormat::Json {
            let output = RestartOutput {
                status: "no_projects".to_string(),
            };
            println!("{}", serde_json::to_string_pretty(&output).unwrap());
        } else {
            println!("No projects registered. Use 'unf watch' first.");
        }
        return Ok(());
    }

    // Remove per-project stopped sentinels for all registered projects
    if let Ok(registry) = crate::registry::load() {
        for entry in &registry.projects {
            if let Ok(storage_dir) = storage::resolve_storage_dir_canonical(&entry.path) {
                let stopped = storage::stopped_path(&storage_dir);
                let _ = fs::remove_file(&stopped);
            }
        }
    }

    // Start sentinel (sentinel will start daemon)
    crate::sentinel::ensure_sentinel_running()?;

    // Audit log
    crate::audit::log_event("RESTART", "sentinel and daemon");

    if format == OutputFormat::Json {
        let output = RestartOutput {
            status: "restarted".to_string(),
        };
        println!("{}", serde_json::to_string_pretty(&output).unwrap());
    } else {
        super::output::print_status("Restarted", "daemon");
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn restart_no_projects() {
        // This is a basic sanity test. Full integration testing happens in WS3-08.
        // Just verify that the function signature compiles and is runnable.
        // The actual daemon behavior is tested in integration tests.
    }
}
