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
use crate::process::PidFile;
use crate::storage;

/// JSON output for the restart command.
#[derive(serde::Serialize)]
struct RestartOutput {
    status: String,
    /// Auto-start state *after* the command ran. Absent on the `no_projects`
    /// path, where auto-start is deliberately left alone.
    #[serde(skip_serializing_if = "Option::is_none")]
    auto_restart: Option<bool>,
    /// True only when this command installed auto-start that was missing.
    #[serde(skip_serializing_if = "Option::is_none")]
    auto_restart_enabled: Option<bool>,
}

#[allow(clippy::cognitive_complexity)]
// TODO(v0.18): reduce complexity
pub fn run(_project_root: &Path, format: OutputFormat) -> Result<(), UnfError> {
    // Kill sentinel first (prevents it from respawning daemon during restart)
    if let Err(e) = crate::sentinel::kill_sentinel() {
        super::output::print_warning(&format!("Failed to stop sentinel: {}", e));
    }

    // Stop daemon if running
    let global_pid_path = storage::global_pid_path()?;
    let pid_file = PidFile::new(global_pid_path);
    if let Ok(Some(pid)) = pid_file.read() {
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
    let _ = pid_file.remove();

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
                auto_restart: None,
                auto_restart_enabled: None,
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

    // Reinstall auto-start if it went missing. Runs *after* the no-projects
    // guard (so it cannot undo `unf unwatch`) and *before* the sentinel starts:
    // on macOS `launchctl load -w` starts the sentinel itself, so installing
    // first leaves exactly one sentinel, owned by launchd.
    let (autostart_enabled_now, autostart_present) = match crate::autostart::ensure_installed() {
        Ok(pair) => pair,
        Err(e) => {
            // Cosmetic-tier: the daemon is already restarting. Warn, do not fail.
            super::output::print_warning(&format!("Failed to enable auto-start: {}", e));
            (false, false)
        }
    };

    // Start sentinel (sentinel will start daemon)
    crate::sentinel::ensure_sentinel_running()?;

    // Audit log
    crate::audit::log_event("RESTART", "sentinel and daemon");

    if format == OutputFormat::Json {
        let output = RestartOutput {
            status: "restarted".to_string(),
            auto_restart: Some(autostart_present),
            auto_restart_enabled: Some(autostart_enabled_now),
        };
        println!("{}", serde_json::to_string_pretty(&output).unwrap());
    } else {
        super::output::print_status("Restarted", "daemon");
        // Report only when we acted *and* it worked. Silence is the healthy path.
        if autostart_enabled_now && autostart_present {
            super::output::print_status("Enabled", "auto-restart on login");
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::RestartOutput;

    #[test]
    fn restart_no_projects() {
        // This is a basic sanity test. Full integration testing happens in WS3-08.
        // Just verify that the function signature compiles and is runnable.
        // The actual daemon behavior is tested in integration tests.
    }

    /// The `no_projects` path must not mention auto-start at all: the command
    /// deliberately leaves it alone when nothing is registered.
    #[test]
    fn no_projects_json_omits_autostart_fields() {
        let json = serde_json::to_string(&RestartOutput {
            status: "no_projects".to_string(),
            auto_restart: None,
            auto_restart_enabled: None,
        })
        .expect("serialize");

        assert!(!json.contains("auto_restart"), "got: {}", json);
        assert!(json.contains("\"status\":\"no_projects\""), "got: {}", json);
    }

    /// Auto-start already present: state is reported, but we did not act.
    #[test]
    fn already_installed_json_reports_state_without_enable() {
        let json = serde_json::to_string(&RestartOutput {
            status: "restarted".to_string(),
            auto_restart: Some(true),
            auto_restart_enabled: Some(false),
        })
        .expect("serialize");

        assert!(json.contains("\"auto_restart\":true"), "got: {}", json);
        assert!(
            json.contains("\"auto_restart_enabled\":false"),
            "got: {}",
            json
        );
    }

    /// The `status` field is never removed or renamed — the shipped Mac app
    /// reads it and is not being rebuilt.
    #[test]
    fn restarted_json_keeps_status_field() {
        let json = serde_json::to_string(&RestartOutput {
            status: "restarted".to_string(),
            auto_restart: Some(true),
            auto_restart_enabled: Some(true),
        })
        .expect("serialize");

        let parsed: serde_json::Value = serde_json::from_str(&json).expect("parse");
        assert_eq!(parsed["status"], "restarted");
    }
}
