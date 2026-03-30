//! `unf init` command implementation.
//!
//! Initializes the flight recorder for a project and starts the background daemon.

use std::env;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Command, Stdio};

use crate::cli::OutputFormat;
use crate::engine::Engine;
use crate::error::UnfError;
use crate::process::PidFile;
use crate::storage;

/// JSON output for the init command.
#[derive(serde::Serialize)]
struct InitOutput {
    status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    snapshots_preserved: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    auto_restart: Option<bool>,
}

/// Runs the `unf init` command.
///
/// Initializes a new UNFUDGED flight recorder in the current directory.
/// Creates the `.unfudged/` directory structure, initializes the storage engine,
/// and spawns a background daemon process to watch for file changes.
///
/// # Behavior
///
/// 1. Check if `.unfudged/` already exists and daemon is running
///    - If running: print "Already watching. Use 'unf status' for details." and exit
/// 2. Create `.unfudged/` directory and initialize the Engine
/// 3. Fork a background daemon process (using hidden `__daemon` subcommand)
/// 4. Write daemon PID to `.unfudged/daemon.pid`
/// 5. Print success message with status
///
/// # Arguments
///
/// * `project_root` - The root directory to initialize (typically current directory)
/// * `format` - Output format (human or JSON)
///
/// # Returns
///
/// `Ok(())` on success, or `UnfError` if initialization fails.
///
/// # Errors
///
/// - `UnfError::Db` if database initialization fails
/// - `UnfError::Cas` if directory creation fails
/// - `UnfError::Watcher` if daemon spawn fails
pub fn run(project_root: &Path, format: OutputFormat) -> Result<(), UnfError> {
    let storage_dir = storage::resolve_storage_dir(project_root)?;
    let pid_file_path = storage::pid_path(&storage_dir);
    let pid_file = PidFile::new(pid_file_path.clone());

    // Check if already initialized and daemon is running
    if storage_dir.exists() && pid_file.is_running() {
        if format == OutputFormat::Json {
            let output = InitOutput {
                status: "already_running".to_string(),
                snapshots_preserved: None,
                auto_restart: None,
            };
            println!("{}", serde_json::to_string_pretty(&output).unwrap());
        } else {
            println!("Already watching. Use 'unf status' for details.");
        }
        return Ok(());
    }

    // Remove stopped sentinel if it exists (re-initialization)
    let stopped_path = storage::stopped_path(&storage_dir);
    let _ = std::fs::remove_file(&stopped_path);

    // Initialize engine (creates storage dir and objects/)
    let engine = if storage_dir.exists() {
        // Storage exists, open existing engine
        Engine::open(project_root, &storage_dir)?
    } else {
        // New initialization
        Engine::init(project_root, &storage_dir)?
    };

    // Spawn background daemon process
    let current_exe = env::current_exe().map_err(|e| {
        UnfError::Watcher(crate::error::WatcherError::Io(std::io::Error::other(
            format!("Failed to get current executable path: {}", e),
        )))
    })?;

    let child = Command::new(&current_exe)
        .arg("__daemon")
        .arg("--root")
        .arg(project_root)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0) // Detach so daemon survives parent exit
        .spawn()
        .map_err(|e| {
            UnfError::Watcher(crate::error::WatcherError::Io(std::io::Error::other(
                format!("Failed to spawn daemon: {}", e),
            )))
        })?;

    // Write PID file
    let pid = child.id();
    pid_file.write(pid).map_err(|e| {
        UnfError::Watcher(crate::error::WatcherError::Io(std::io::Error::other(
            format!("Failed to write PID file: {}", e),
        )))
    })?;

    // Register project in global registry and install auto-start
    // Errors are warnings, not fatal — init still succeeds
    if let Err(e) = crate::registry::register_project(project_root) {
        super::output::print_warning(&format!("Failed to register project: {}", e));
    }
    let auto_restart = match crate::autostart::install() {
        Ok(()) => crate::autostart::is_installed().unwrap_or(false),
        Err(e) => {
            super::output::print_warning(&format!("Failed to install auto-start: {}", e));
            false
        }
    };

    // Get snapshot count to determine if this is a re-initialization
    let snapshot_count = engine.get_snapshot_count().unwrap_or(0);

    // Prepare output
    let output = if snapshot_count > 0 {
        // Re-initialization: resume recording with preserved history
        InitOutput {
            status: "resumed".to_string(),
            snapshots_preserved: Some(snapshot_count),
            auto_restart: Some(auto_restart),
        }
    } else {
        // New initialization
        InitOutput {
            status: "started".to_string(),
            snapshots_preserved: None,
            auto_restart: Some(auto_restart),
        }
    };

    // Print success message
    if format == OutputFormat::Json {
        println!("{}", serde_json::to_string_pretty(&output).unwrap());
    } else if snapshot_count > 0 {
        let subject = format!(
            "{} ({} snapshots preserved)",
            project_root.display(),
            snapshot_count
        );
        super::output::print_status("Watching", &subject);
    } else {
        super::output::print_status("Watching", &project_root.display().to_string());
    }

    Ok(())
}
