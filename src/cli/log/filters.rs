//! File grouping and filtering logic.

use std::path::{Path, PathBuf};

use crate::error::UnfError;
use crate::storage;
use crate::types::Snapshot;

/// Groups snapshots by file path.
///
/// Contains all snapshots for a single file, sorted chronologically (oldest first).
#[derive(Debug, Clone)]
pub struct FileGroup {
    /// The file path for all snapshots in this group.
    pub path: String,
    /// Snapshots for this file, sorted oldest-first (chronological order).
    pub entries: Vec<Snapshot>,
}

/// Kind of history scope, determined from target argument.
pub enum ScopeKind {
    All,
    File,
    Directory,
}

/// Determines the scope kind and canonical target string from a target path.
///
/// - `None` → All
/// - Path ending with `/` → Directory
/// - Path where `project_root.join(path).is_dir()` → Directory (appends `/`)
/// - Otherwise → File
pub fn determine_scope_kind(project_root: &Path, target: Option<&str>) -> (ScopeKind, String) {
    match target {
        None => (ScopeKind::All, String::new()),
        Some(path) => {
            if path.ends_with('/') {
                (ScopeKind::Directory, path.to_string())
            } else if project_root.join(path).is_dir() {
                (ScopeKind::Directory, format!("{}/", path))
            } else {
                (ScopeKind::File, path.to_string())
            }
        }
    }
}

/// Groups a flat list of snapshots by file path.
///
/// Snapshots are organized into `FileGroup` structs, one per unique file.
/// The output is sorted by most-recent activity (newest file first), while
/// entries within each group are sorted chronologically (oldest first).
///
/// # Arguments
/// * `snapshots` - A vector of snapshots to group
///
/// # Returns
/// A vector of `FileGroup` sorted by newest activity descending.
/// Each group's entries are sorted oldest-first.
/// Empty input returns empty output.
///
/// # Examples
/// ```ignore
/// let snaps = vec![snap1, snap2, snap3];
/// let groups = group_by_file(snaps);
/// // groups[0] contains the file with the most recent change
/// // groups[0].entries is sorted chronologically
/// ```
pub fn group_by_file(snapshots: Vec<Snapshot>) -> Vec<FileGroup> {
    use std::collections::BTreeMap;

    if snapshots.is_empty() {
        return Vec::new();
    }

    // Group snapshots by file path
    let mut groups: BTreeMap<String, Vec<Snapshot>> = BTreeMap::new();
    for snapshot in snapshots {
        groups
            .entry(snapshot.file_path.clone())
            .or_default()
            .push(snapshot);
    }

    // Sort entries within each group chronologically (oldest first),
    // then convert to FileGroup structs
    let mut result: Vec<FileGroup> = groups
        .into_iter()
        .map(|(path, mut entries)| {
            entries.sort_by_key(|snap| snap.timestamp);
            FileGroup { path, entries }
        })
        .collect();

    // Sort groups by most recent activity (newest file first)
    result.sort_by(|a, b| {
        let a_newest = a.entries.last().map(|snap| snap.timestamp);
        let b_newest = b.entries.last().map(|snap| snap.timestamp);
        b_newest.cmp(&a_newest) // Descending (newest first)
    });

    result
}

/// Expands `~/` to `$HOME/` and canonicalizes if the path exists.
pub(super) fn resolve_filter_path(path: &str) -> String {
    let expanded = if let Some(rest) = path.strip_prefix("~/") {
        if let Ok(home) = std::env::var("HOME") {
            format!("{}/{}", home, rest)
        } else {
            path.to_string()
        }
    } else {
        path.to_string()
    };

    // Try to canonicalize, fall back to expanded
    match std::fs::canonicalize(&expanded) {
        Ok(canonical) => canonical.to_string_lossy().to_string(),
        Err(_) => expanded,
    }
}

/// Resolves which projects to include in a global log query.
///
/// Loads the registry, applies include/exclude prefix matching, and returns
/// `(project_path, storage_dir)` pairs for accessible projects.
pub fn resolve_global_projects(
    include: &[String],
    exclude: &[String],
) -> Result<Vec<(PathBuf, PathBuf)>, UnfError> {
    let registry = crate::registry::load()?;

    if registry.projects.is_empty() {
        return Err(UnfError::InvalidArgument(
            "No projects registered. Run `unf watch` in a project directory first.".to_string(),
        ));
    }

    // Resolve filter paths
    let include_resolved: Vec<String> = include.iter().map(|p| resolve_filter_path(p)).collect();
    let exclude_resolved: Vec<String> = exclude.iter().map(|p| resolve_filter_path(p)).collect();

    let mut projects = Vec::new();

    for entry in &registry.projects {
        let path_str = entry.path.to_string_lossy().to_string();

        // Apply include filter (prefix match)
        if !include_resolved.is_empty()
            && !include_resolved.iter().any(|inc| path_str.starts_with(inc))
        {
            continue;
        }

        // Apply exclude filter (prefix match)
        if exclude_resolved.iter().any(|exc| path_str.starts_with(exc)) {
            continue;
        }

        // Resolve storage dir (skip projects with missing storage)
        match storage::resolve_storage_dir_canonical(&entry.path) {
            Ok(storage_dir) if storage_dir.exists() => {
                projects.push((entry.path.clone(), storage_dir));
            }
            _ => continue,
        }
    }

    if projects.is_empty() {
        return Err(UnfError::InvalidArgument(
            "No matching projects found.".to_string(),
        ));
    }

    Ok(projects)
}
