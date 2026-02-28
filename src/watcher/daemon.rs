//! Daemon state and event routing for the single-daemon architecture.
//!
//! This module defines the core types for the global daemon that manages
//! multiple watched projects with a single filesystem watcher. It also
//! provides the pure `route_event` function for dispatching filesystem
//! events to their owning project.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::mpsc::Receiver;

use notify::{Event, RecommendedWatcher, RecursiveMode, Watcher};

use super::debounce::Debouncer;
use super::filter::Filter;
use crate::engine::Engine;
use crate::error::{UnfError, WatcherError};
use crate::storage;

/// Represents a single watched project within the global daemon.
///
/// Each registered project has its own engine (CAS + SQLite), path filter,
/// and event debouncer. The daemon holds one `ProjectContext` per watched
/// directory.
pub struct ProjectContext {
    /// Canonical path to the project root.
    pub root: PathBuf,
    /// Engine instance for this project (CAS + SQLite).
    pub engine: Engine,
    /// Path filter (gitignore + extension-based).
    pub filter: Filter,
    /// Event debouncer for batching filesystem events.
    pub debouncer: Debouncer,
}

/// Global state for the single-daemon architecture.
///
/// The daemon manages multiple projects with a single filesystem watcher.
/// Projects are dynamically added/removed via SIGUSR1 signal + registry.
pub struct DaemonState {
    /// Single filesystem watcher shared across all projects.
    pub watcher: RecommendedWatcher,
    /// Map from canonical project root to its context.
    pub projects: HashMap<PathBuf, ProjectContext>,
    /// Channel receiver for filesystem events from all watched directories.
    pub rx: Receiver<Result<Event, notify::Error>>,
}

impl DaemonState {
    /// Synchronizes the daemon's project set with the registry.
    ///
    /// Loads the current registry, identifies projects to add and remove,
    /// and applies those changes. Log errors during addition but continue
    /// (fail gracefully for individual projects).
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success. Returns error only for fatal failures
    /// (e.g., registry I/O error).
    pub fn sync_with_registry(&mut self) -> Result<(), UnfError> {
        // Load current registry
        let registry = crate::registry::load()?;

        // Collect registered paths into a set
        let registered_set: HashSet<PathBuf> = registry
            .projects
            .iter()
            .map(|entry| entry.path.clone())
            .collect();

        // Find projects to remove: keys in self.projects not in registry
        let to_remove: Vec<PathBuf> = self
            .projects
            .keys()
            .filter(|path| !registered_set.contains(*path))
            .cloned()
            .collect();

        // Find projects to add: paths in registry not in self.projects
        let to_add: Vec<PathBuf> = registered_set
            .iter()
            .filter(|path| !self.projects.contains_key(*path))
            .cloned()
            .collect();

        // Remove projects
        for path in to_remove {
            self.remove_project(&path);
        }

        // Add projects (log errors but continue)
        for path in to_add {
            match self.add_project(&path) {
                Ok(()) => {
                    // Project added successfully
                }
                Err(err) => {
                    // Log and continue
                    eprintln!("Failed to add project {}: {}", path.display(), err);
                }
            }
        }

        Ok(())
    }

    /// Adds a new project to the daemon's watch set.
    ///
    /// Creates an Engine, Filter, and Debouncer for the project, and watches
    /// the directory recursively. Checks for a stopped sentinel before proceeding.
    ///
    /// # Arguments
    ///
    /// * `path` - Canonical path to the project root
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success, or an error if setup fails.
    pub fn add_project(&mut self, path: &Path) -> Result<(), UnfError> {
        // Check for stopped sentinel
        let storage_dir = storage::resolve_storage_dir_canonical(path)?;
        if storage::stopped_path(&storage_dir).exists() {
            return Ok(());
        }

        // Create Engine
        let engine = Engine::open(path, &storage_dir)?;

        // Create Filter
        let filter = Filter::new(path)?;

        // Create Debouncer
        let debouncer = Debouncer::new();

        // Watch the path
        self.watcher
            .watch(path, RecursiveMode::Recursive)
            .map_err(WatcherError::from)?;

        // Insert into projects map
        let project_context = ProjectContext {
            root: path.to_path_buf(),
            engine,
            filter,
            debouncer,
        };
        self.projects.insert(path.to_path_buf(), project_context);

        Ok(())
    }

    /// Removes a project from the daemon's watch set.
    ///
    /// Drains any pending events from the debouncer (discarding them),
    /// unwatches the path, and removes the project from the map.
    /// This is a best-effort operation: errors from unwatching are ignored.
    ///
    /// # Arguments
    ///
    /// * `path` - Canonical path to the project root
    pub fn remove_project(&mut self, path: &Path) {
        if let Some(mut context) = self.projects.remove(path) {
            // Force drain the debouncer to discard any pending events
            let _ = context.debouncer.force_drain();

            // Unwatch the path (ignore errors — path may already be gone)
            let _ = self.watcher.unwatch(path);
        }
    }
}

/// Routes a filesystem event path to its owning project root.
///
/// Finds the most specific (longest prefix) project root that contains
/// the given path. Returns `None` if no project owns the path.
///
/// This selects by longest prefix so that nested projects (e.g., `/a/b`
/// inside `/a`) are correctly routed to the innermost project.
///
/// Pure function: no I/O, no side effects.
///
/// # Arguments
///
/// * `path` - The filesystem path from the watcher event
/// * `project_roots` - Slice of registered project root paths
///
/// # Returns
///
/// A reference to the matching project root, or `None` if no project
/// contains the given path.
///
/// # Examples
///
/// ```
/// use std::path::{Path, PathBuf};
/// use unfudged::watcher::daemon::route_event;
///
/// let roots = vec![
///     PathBuf::from("/home/user/project-a"),
///     PathBuf::from("/home/user/project-b"),
/// ];
/// let result = route_event(Path::new("/home/user/project-a/src/main.rs"), &roots);
/// assert_eq!(result, Some(&PathBuf::from("/home/user/project-a")));
/// ```
pub fn route_event<'a>(path: &Path, project_roots: &'a [PathBuf]) -> Option<&'a PathBuf> {
    project_roots
        .iter()
        .filter(|root| path.starts_with(root))
        .max_by_key(|root| root.components().count())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn route_event_no_match() {
        let roots = vec![
            PathBuf::from("/home/user/project-a"),
            PathBuf::from("/home/user/project-b"),
        ];
        let path = Path::new("/completely/different/path/file.rs");
        assert_eq!(route_event(path, &roots), None);
    }

    #[test]
    fn route_event_single_match() {
        let roots = vec![
            PathBuf::from("/home/user/project-a"),
            PathBuf::from("/home/user/project-b"),
        ];
        let path = Path::new("/home/user/project-a/src/main.rs");
        assert_eq!(
            route_event(path, &roots),
            Some(&PathBuf::from("/home/user/project-a"))
        );
    }

    #[test]
    fn route_event_nested_projects() {
        let roots = vec![
            PathBuf::from("/a"),
            PathBuf::from("/a/b"),
            PathBuf::from("/a/b/c"),
        ];
        // File in /a/b/c should route to /a/b/c (longest prefix)
        let path = Path::new("/a/b/c/file.rs");
        assert_eq!(route_event(path, &roots), Some(&PathBuf::from("/a/b/c")));

        // File in /a/b (but not /a/b/c) should route to /a/b
        let path2 = Path::new("/a/b/other.rs");
        assert_eq!(route_event(path2, &roots), Some(&PathBuf::from("/a/b")));

        // File directly in /a should route to /a
        let path3 = Path::new("/a/top-level.rs");
        assert_eq!(route_event(path3, &roots), Some(&PathBuf::from("/a")));
    }

    #[test]
    fn route_event_exact_root() {
        let roots = vec![PathBuf::from("/home/user/project")];
        // Path IS the root itself
        let path = Path::new("/home/user/project");
        assert_eq!(
            route_event(path, &roots),
            Some(&PathBuf::from("/home/user/project"))
        );
    }

    #[test]
    fn route_event_empty_projects() {
        let roots: Vec<PathBuf> = vec![];
        let path = Path::new("/any/path/file.rs");
        assert_eq!(route_event(path, &roots), None);
    }

    #[test]
    fn route_event_prefix_boundary_no_false_match() {
        // Ensure /home/user/proj does NOT match /home/user/project/file.rs
        // because starts_with checks component boundaries on PathBuf.
        let roots = vec![PathBuf::from("/home/user/proj")];
        let path = Path::new("/home/user/project/file.rs");
        assert_eq!(route_event(path, &roots), None);
    }

    #[test]
    fn route_event_multiple_candidates_picks_longest() {
        let roots = vec![
            PathBuf::from("/workspace"),
            PathBuf::from("/workspace/monorepo"),
            PathBuf::from("/workspace/monorepo/packages/core"),
        ];
        let path = Path::new("/workspace/monorepo/packages/core/src/lib.rs");
        assert_eq!(
            route_event(path, &roots),
            Some(&PathBuf::from("/workspace/monorepo/packages/core"))
        );
    }

    #[test]
    fn remove_project_nonexistent_is_noop() {
        // Create a real daemon state with notify::recommended_watcher
        let (tx, _rx) = std::sync::mpsc::channel();
        let watcher = notify::recommended_watcher(move |_| {
            let _ = tx.send(Ok(Default::default()));
        })
        .expect("create watcher");

        let mut state = DaemonState {
            watcher,
            projects: HashMap::new(),
            rx: _rx,
        };

        let path = PathBuf::from("/nonexistent/project");

        // Should not panic when removing a project that doesn't exist
        state.remove_project(&path);

        // projects map should still be empty
        assert!(state.projects.is_empty());
    }

    #[test]
    fn remove_project_existing_removes_from_map() {
        // This test verifies that remove_project removes an entry from the map.
        // We can't easily test the watcher unwatch without mocking, so we focus
        // on the map semantics.
        use tempfile::TempDir;

        let (tx, _rx) = std::sync::mpsc::channel();
        let watcher = notify::recommended_watcher(move |_| {
            let _ = tx.send(Ok(Default::default()));
        })
        .expect("create watcher");

        let mut state = DaemonState {
            watcher,
            projects: HashMap::new(),
            rx: _rx,
        };

        // Create temporary directories for the project
        let project_temp = TempDir::new().expect("create project dir");
        let storage_temp = TempDir::new().expect("create storage dir");

        let path = project_temp.path().to_path_buf();

        // Initialize engine with real project and storage dirs
        let engine =
            Engine::init(&path, storage_temp.path()).expect("initialize engine for testing");

        let context = ProjectContext {
            root: path.clone(),
            engine,
            filter: Filter::new(&path).expect("create filter"),
            debouncer: Debouncer::new(),
        };

        state.projects.insert(path.clone(), context);
        assert_eq!(state.projects.len(), 1);

        // Remove the project
        state.remove_project(&path);

        // Should be removed from the map
        assert!(state.projects.is_empty());
    }
}
