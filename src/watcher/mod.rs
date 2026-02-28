//! Filesystem watcher for the UNFUDGED flight recorder.
//!
//! This module contains the core watcher subsystems:
//! - Path filtering with .gitignore support
//! - Event debouncer for batching filesystem events
//! - OS-specific watcher implementations (future)

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::channel;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::time::{Duration, Instant};

use notify::{Config, EventKind, RecommendedWatcher, Watcher};

use crate::engine::Engine;
use crate::error::{UnfError, WatcherError};
use crate::types::EventType;

pub mod daemon;
pub mod debounce;
pub mod filter;

/// Maps notify event kinds to our domain EventType.
///
/// Returns `None` for event kinds we don't care about (Access, Other, etc.).
/// Renames are not explicitly handled here; they appear as separate Delete + Create events.
fn map_notify_event(kind: &EventKind) -> Option<EventType> {
    match kind {
        EventKind::Create(_) => Some(EventType::Create),
        EventKind::Modify(_) => Some(EventType::Modify),
        EventKind::Remove(_) => Some(EventType::Delete),
        _ => None, // Ignore Access, Other events
    }
}

/// Processes a batch of debounced events, creating snapshots for each.
///
/// Converts absolute paths to relative paths and creates snapshots via the engine.
/// Skips binary files (returns None) and logs errors but does not crash on individual snapshot failures.
fn process_batch(
    batch: Vec<(std::path::PathBuf, EventType)>,
    engine: &Engine,
    project_root: &Path,
) {
    for (path, event_type) in batch {
        let rel_path = match path.strip_prefix(project_root) {
            Ok(p) => p.to_string_lossy().to_string(),
            Err(_) => {
                eprintln!("Skipping path outside project root: {:?}", path);
                continue;
            }
        };
        match engine.create_snapshot(&rel_path, event_type) {
            Ok(Some(_snapshot)) => {
                // Successfully created snapshot
            }
            Ok(None) => {
                // Binary file, skipped silently
            }
            Err(e) => {
                eprintln!("Failed to create snapshot for {}: {}", rel_path, e);
            }
        }
    }
}

/// Run the global multi-project daemon loop. Blocks until shutdown signal.
///
/// This is the main event loop for the daemon process. It manages multiple watched
/// projects loaded from the registry, routes filesystem events to the correct project,
/// and creates snapshots via each project's Engine.
///
/// The daemon:
/// - Watches all registered projects simultaneously with a single `notify::RecommendedWatcher`
/// - Loads the initial project set from the registry
/// - Responds to SIGTERM for graceful shutdown
/// - Responds to SIGUSR1 to reload the registry and add/remove projects dynamically
/// - Runs with a 500ms timeout for event polling to balance responsiveness with CPU usage
///
/// # Returns
///
/// Returns `Ok(())` on clean shutdown via SIGTERM.
/// Returns `Err` if the watcher or registry initialization fails.
///
/// # Errors
///
/// - `UnfError::Registry` if the registry cannot be loaded initially
/// - `UnfError::Watcher` if the filesystem watcher fails to start
/// - `UnfError::Db` or `UnfError::Cas` if engine initialization fails for a project
///
/// # Example
///
/// ```no_run
/// use unfudged::watcher::run_daemon;
///
/// // Called by the daemon process (not tied to any specific project)
/// run_daemon().expect("Daemon failed");
/// ```
pub fn run_daemon() -> Result<(), UnfError> {
    // 1. Create watcher with channel
    let (tx, rx) = channel();
    let watcher = RecommendedWatcher::new(tx, Config::default()).map_err(WatcherError::Notify)?;

    // 2. Create DaemonState
    let mut state = daemon::DaemonState {
        watcher,
        projects: HashMap::new(),
        rx,
    };

    // 3. Initial sync with registry (populates projects)
    state.sync_with_registry()?;

    // 4. Set up signal handlers using signal-hook
    let shutdown = Arc::new(AtomicBool::new(false));
    let reload = Arc::new(AtomicBool::new(false));

    // SIGTERM -> shutdown
    signal_hook::flag::register(signal_hook::consts::SIGTERM, shutdown.clone())
        .map_err(|e| UnfError::Watcher(WatcherError::Io(e)))?;
    // SIGUSR1 -> reload registry (add/remove projects)
    signal_hook::flag::register(signal_hook::consts::SIGUSR1, reload.clone())
        .map_err(|e| UnfError::Watcher(WatcherError::Io(e)))?;

    // 5. Event loop
    loop {
        // Check shutdown
        if shutdown.load(Ordering::SeqCst) {
            flush_all_pending(&mut state);
            return Ok(());
        }

        // Check reload signal
        if reload.swap(false, Ordering::SeqCst) {
            if let Err(e) = state.sync_with_registry() {
                eprintln!("Failed to sync registry: {}", e);
            }
        }

        // Receive events with 500ms timeout
        match state.rx.recv_timeout(Duration::from_millis(500)) {
            Ok(Ok(event)) => {
                let project_roots: Vec<PathBuf> = state.projects.keys().cloned().collect();
                for path in event.paths {
                    if let Some(project_root) = daemon::route_event(&path, &project_roots) {
                        let project_root = project_root.clone();
                        if let Some(ctx) = state.projects.get_mut(&project_root) {
                            if ctx.filter.should_track(&path) {
                                if let Some(event_type) = map_notify_event(&event.kind) {
                                    ctx.debouncer.push(path, event_type, Instant::now());
                                }
                            }
                        }
                    }
                }
            }
            Ok(Err(e)) => {
                eprintln!("Watcher error: {}", e);
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                // Expected, continue to debouncer check
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                flush_all_pending(&mut state);
                return Err(UnfError::Watcher(WatcherError::Io(std::io::Error::new(
                    std::io::ErrorKind::BrokenPipe,
                    "Watcher channel disconnected",
                ))));
            }
        }

        // Drain ready debouncers for all projects
        let now = Instant::now();
        for ctx in state.projects.values_mut() {
            if let Some(batch) = ctx.debouncer.drain_if_ready(now) {
                process_batch(batch, &ctx.engine, &ctx.root);
            }
        }
    }
}

/// Flushes all pending debouncer batches for all projects.
///
/// This is called before daemon shutdown to ensure no events are lost.
fn flush_all_pending(state: &mut daemon::DaemonState) {
    for ctx in state.projects.values_mut() {
        if ctx.debouncer.has_pending() {
            if let Some(batch) = ctx.debouncer.force_drain() {
                process_batch(batch, &ctx.engine, &ctx.root);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn map_notify_event_create() {
        let kind = EventKind::Create(notify::event::CreateKind::File);
        assert_eq!(map_notify_event(&kind), Some(EventType::Create));
    }

    #[test]
    fn map_notify_event_modify() {
        let kind = EventKind::Modify(notify::event::ModifyKind::Data(
            notify::event::DataChange::Any,
        ));
        assert_eq!(map_notify_event(&kind), Some(EventType::Modify));
    }

    #[test]
    fn map_notify_event_remove() {
        let kind = EventKind::Remove(notify::event::RemoveKind::File);
        assert_eq!(map_notify_event(&kind), Some(EventType::Delete));
    }

    #[test]
    fn map_notify_event_access_ignored() {
        let kind = EventKind::Access(notify::event::AccessKind::Read);
        assert_eq!(map_notify_event(&kind), None);
    }

    #[test]
    fn map_notify_event_other_ignored() {
        let kind = EventKind::Other;
        assert_eq!(map_notify_event(&kind), None);
    }
}
