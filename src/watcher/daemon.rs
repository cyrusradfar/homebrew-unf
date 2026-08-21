//! Daemon state and event routing for the single-daemon architecture.
//!
//! This module defines the core types for the global daemon that manages
//! multiple watched projects with a single filesystem watcher. It also
//! provides the pure `route_event` function for dispatching filesystem
//! events to their owning project.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::Receiver;

use notify::{Event, RecommendedWatcher, RecursiveMode, Watcher};

use super::debounce::Debouncer;
use super::filter::Filter;
use crate::engine::Engine;
use crate::error::{UnfError, WatcherError};
use crate::storage;
use crate::types::WatchSettings;

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

        // Map each registered path to its settings. This doubles as the
        // "registered set" for the remove/add diff below, so the registry is
        // loaded exactly once per sync.
        let registered: HashMap<PathBuf, WatchSettings> = registry
            .projects
            .iter()
            .map(|entry| (entry.path.clone(), entry.settings.clone()))
            .collect();

        // Find projects to remove: keys in self.projects not in registry
        let to_remove: Vec<PathBuf> = self
            .projects
            .keys()
            .filter(|path| !registered.contains_key(*path))
            .cloned()
            .collect();

        // Find projects to add: paths in registry not in self.projects
        let to_add: Vec<(PathBuf, WatchSettings)> = registered
            .iter()
            .filter(|(path, _)| !self.projects.contains_key(*path))
            .map(|(path, settings)| (path.clone(), settings.clone()))
            .collect();

        // Find projects to reload: present in both sets, but the registry's
        // force_watch_gitignore flag no longer matches the Filter the
        // running context holds. Only the Filter is rebuilt (below) — the
        // Engine, Debouncer, and OS watch registration are left untouched so
        // unrelated projects keep their pending debounced events and this
        // project's watch is not churned for a path that did not change.
        //
        // This compares only force_watch_gitignore, matching the pre-UD-03
        // trigger condition. Comparing the whole WatchSettings (so a change
        // to unignored_dirs also triggers a reload) is UD-05's job.
        let to_reload: Vec<(PathBuf, WatchSettings)> = self
            .projects
            .iter()
            .filter_map(|(path, ctx)| {
                registered.get(path).and_then(|want| {
                    (want.force_watch_gitignore != ctx.filter.settings().force_watch_gitignore)
                        .then(|| (path.clone(), want.clone()))
                })
            })
            .collect();

        // Remove projects
        for path in to_remove {
            self.remove_project(&path);
        }

        // Add projects (log errors but continue)
        for (path, settings) in to_add {
            match self.add_project(&path, settings) {
                Ok(()) => {
                    // Project added successfully
                }
                Err(err) => {
                    // Log and continue
                    eprintln!("Failed to add project {}: {}", path.display(), err);
                }
            }
        }

        // Reload filters for projects whose flag changed (log and continue)
        for (path, settings) in to_reload {
            match Filter::new(&path, settings) {
                Ok(filter) => {
                    if let Some(ctx) = self.projects.get_mut(&path) {
                        ctx.filter = filter;
                    }
                }
                Err(err) => {
                    eprintln!("Failed to reload filter for {}: {}", path.display(), err);
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
    /// * `settings` - The registry entry's settings for this path.
    ///   `sync_with_registry` reads this from the registry it already
    ///   loaded, so it is passed in rather than read again here.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success, or an error if setup fails.
    pub fn add_project(&mut self, path: &Path, settings: WatchSettings) -> Result<(), UnfError> {
        // Check for stopped sentinel
        let storage_dir = storage::resolve_storage_dir_canonical(path)?;
        if storage::stopped_path(&storage_dir).exists() {
            return Ok(());
        }

        // Create Engine
        let engine = Engine::open(path, &storage_dir)?;

        // Create Filter
        let filter = Filter::new(path, settings)?;

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
            filter: Filter::new(&path, WatchSettings::default()).expect("create filter"),
            debouncer: Debouncer::new(),
        };

        state.projects.insert(path.clone(), context);
        assert_eq!(state.projects.len(), 1);

        // Remove the project
        state.remove_project(&path);

        // Should be removed from the map
        assert!(state.projects.is_empty());
    }

    /// Locks `UNF_HOME` for the duration of `f` and points it at a fresh
    /// temp dir, so `crate::registry::load`/`register_project` operate on
    /// an isolated registry rather than the real `~/.unfudged/projects.json`.
    /// Mirrors the `with_test_home` helper in `intent.rs`.
    fn with_test_registry<F: FnOnce()>(f: F) {
        let _guard = crate::test_util::ENV_LOCK.lock().unwrap();
        let temp = tempfile::TempDir::new().expect("create temp UNF_HOME");
        let original = std::env::var("UNF_HOME").ok();
        std::env::set_var("UNF_HOME", temp.path());

        f();

        if let Some(val) = original {
            std::env::set_var("UNF_HOME", val);
        } else {
            std::env::remove_var("UNF_HOME");
        }
    }

    /// Builds a `DaemonState` with a real `notify` watcher and no projects.
    fn new_test_state() -> DaemonState {
        let (tx, rx) = std::sync::mpsc::channel();
        let watcher = notify::recommended_watcher(move |_| {
            let _ = tx.send(Ok(Default::default()));
        })
        .expect("create watcher");

        DaemonState {
            watcher,
            projects: HashMap::new(),
            rx,
        }
    }

    #[test]
    fn sync_reloads_filter_on_flag_change() {
        use std::time::Instant;
        use tempfile::TempDir;

        with_test_registry(|| {
            let mut state = new_test_state();

            let project_temp = TempDir::new().expect("create project dir");
            let storage_temp = TempDir::new().expect("create storage dir");
            let path = project_temp
                .path()
                .canonicalize()
                .expect("canonicalize project path");

            let engine =
                Engine::init(&path, storage_temp.path()).expect("initialize engine for testing");

            let mut context = ProjectContext {
                root: path.clone(),
                engine,
                filter: Filter::new(&path, WatchSettings::default()).expect("create filter"),
                debouncer: Debouncer::new(),
            };
            // Queue a pending debounced event. If sync rebuilds the whole
            // ProjectContext (the rejected alternative) instead of just the
            // Filter, this event is lost.
            context.debouncer.push(
                PathBuf::from("test.txt"),
                crate::types::EventType::Modify,
                Instant::now(),
            );
            assert!(context.debouncer.has_pending());

            state.projects.insert(path.clone(), context);

            // Register the project with the flag off, matching the Filter above.
            crate::registry::register_project(&path, Some(WatchSettings::default()))
                .expect("register project with flag off");

            // No-op sync: flag matches, so the Filter and debouncer must be
            // left exactly as they are.
            state
                .sync_with_registry()
                .expect("sync with unchanged flag");
            assert!(
                !state.projects[&path]
                    .filter
                    .settings()
                    .force_watch_gitignore,
                "flag unchanged must leave the filter as-is"
            );
            assert!(
                state.projects[&path].debouncer.has_pending(),
                "no-op sync must not touch the debouncer"
            );

            // Flip the flag in the registry and sync again.
            crate::registry::register_project(
                &path,
                Some(WatchSettings {
                    force_watch_gitignore: true,
                    ..Default::default()
                }),
            )
            .expect("flip flag to true");
            state
                .sync_with_registry()
                .expect("sync with flag change false -> true");

            assert!(
                state.projects[&path]
                    .filter
                    .settings()
                    .force_watch_gitignore,
                "flag flip false -> true must rebuild the Filter"
            );
            assert!(
                state.projects[&path].debouncer.has_pending(),
                "reloading the Filter must not drop the pending debounced event"
            );

            // Flip back and confirm the reverse direction also rebuilds.
            crate::registry::register_project(&path, Some(WatchSettings::default()))
                .expect("flip flag back to false");
            state
                .sync_with_registry()
                .expect("sync with flag change true -> false");
            assert!(
                !state.projects[&path]
                    .filter
                    .settings()
                    .force_watch_gitignore,
                "flag flip true -> false must rebuild the Filter back"
            );

            let _ = state.watcher.unwatch(&path);
        });
    }

    #[test]
    fn sync_add_project_uses_registry_flag() {
        use tempfile::TempDir;

        with_test_registry(|| {
            let mut state = new_test_state();

            let project_temp = TempDir::new().expect("create project dir");
            let path = project_temp
                .path()
                .canonicalize()
                .expect("canonicalize project path");

            // Pre-initialize storage, mirroring `unf watch` having run before
            // the daemon picks the project up on the next registry sync.
            let storage_dir =
                storage::resolve_storage_dir_canonical(&path).expect("resolve storage dir");
            Engine::init(&path, &storage_dir).expect("initialize engine for testing");

            // Register with the flag ON before the daemon has ever seen this
            // project, so `add_project` must read it rather than default to
            // `false`.
            crate::registry::register_project(
                &path,
                Some(WatchSettings {
                    force_watch_gitignore: true,
                    ..Default::default()
                }),
            )
            .expect("register project with flag on");

            state.sync_with_registry().expect("sync to add project");

            let ctx = state.projects.get(&path).expect("project was added");
            assert!(
                ctx.filter.settings().force_watch_gitignore,
                "add_project must build the Filter from the registry's flag, not hardcode false"
            );

            let _ = state.watcher.unwatch(&path);
        });
    }
}
