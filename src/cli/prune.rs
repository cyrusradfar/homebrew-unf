//! CLI handler for the `unf prune` command.
//!
//! Removes old snapshots and cleans up orphaned CAS objects.
//! Supports both single-project and all-projects modes.

use std::path::Path;

use chrono::DateTime;

use crate::cli::OutputFormat;
use crate::engine::Engine;
use crate::error::UnfError;
use crate::registry;
use crate::storage;

/// JSON output for the prune command.
#[derive(serde::Serialize)]
struct PruneOutput {
    dry_run: bool,
    snapshots_removed: u64,
    objects_removed: u64,
    bytes_freed: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    registry_entries_cleaned: Option<u64>,
}

/// Runs the prune command for a single project or all projects.
pub fn run(
    project_root: &Path,
    older_than: &str,
    dry_run: bool,
    all_projects: bool,
    format: OutputFormat,
) -> Result<(), UnfError> {
    // Parse older_than using the existing parse_time_spec
    let cutoff = super::parse_time_spec(older_than)?;

    if all_projects {
        run_all_projects(cutoff, dry_run, format)
    } else {
        run_single_project(project_root, cutoff, dry_run, format)
    }
}

/// Prunes a single project.
fn run_single_project(
    project_root: &Path,
    cutoff: DateTime<chrono::Utc>,
    dry_run: bool,
    format: OutputFormat,
) -> Result<(), UnfError> {
    // Resolve storage dir and check if initialized
    let storage_dir = storage::resolve_storage_dir(project_root)?;
    if !storage_dir.exists() {
        return Err(UnfError::NotInitialized);
    }

    // Open engine and run prune
    let engine = Engine::open(project_root, &storage_dir)?;
    let stats = engine.prune(cutoff, dry_run)?;

    // Format and output results
    let output = PruneOutput {
        dry_run,
        snapshots_removed: stats.snapshots_removed,
        objects_removed: stats.objects_removed,
        bytes_freed: stats.bytes_freed,
        registry_entries_cleaned: None,
    };

    if format == OutputFormat::Json {
        println!("{}", serde_json::to_string_pretty(&output).unwrap());
    } else {
        print_human_output(dry_run, cutoff, &stats);
    }

    Ok(())
}

/// Prunes all registered projects.
fn run_all_projects(
    cutoff: DateTime<chrono::Utc>,
    dry_run: bool,
    format: OutputFormat,
) -> Result<(), UnfError> {
    let registry = registry::load()?;

    let mut total_snapshots = 0u64;
    let mut total_objects = 0u64;
    let mut total_bytes = 0u64;

    // Iterate all projects and prune each
    for entry in &registry.projects {
        let storage_dir = match storage::resolve_storage_dir_canonical(&entry.path) {
            Ok(d) => d,
            Err(e) => {
                eprintln!(
                    "Warning: Could not resolve storage for {}: {}",
                    entry.path.display(),
                    e
                );
                continue;
            }
        };

        // Skip projects that haven't been initialized
        if !storage_dir.exists() {
            continue;
        }

        // Try to open engine and prune
        match Engine::open(&entry.path, &storage_dir) {
            Ok(engine) => match engine.prune(cutoff, dry_run) {
                Ok(stats) => {
                    total_snapshots += stats.snapshots_removed;
                    total_objects += stats.objects_removed;
                    total_bytes += stats.bytes_freed;
                }
                Err(e) => {
                    eprintln!("Warning: Prune failed for {}: {}", entry.path.display(), e);
                }
            },
            Err(e) => {
                eprintln!(
                    "Warning: Could not open engine for {}: {}",
                    entry.path.display(),
                    e
                );
            }
        }
    }

    // Prune stale registry entries (only if not dry-run)
    let registry_entries_cleaned = if !dry_run {
        registry::prune_stale_entries()? as u64
    } else {
        0
    };

    // Format and output results
    let output = PruneOutput {
        dry_run,
        snapshots_removed: total_snapshots,
        objects_removed: total_objects,
        bytes_freed: total_bytes,
        registry_entries_cleaned: Some(registry_entries_cleaned),
    };

    if format == OutputFormat::Json {
        println!("{}", serde_json::to_string_pretty(&output).unwrap());
    } else {
        print_human_output_with_registry(
            dry_run,
            cutoff,
            total_snapshots,
            total_objects,
            total_bytes,
            registry_entries_cleaned,
        );
    }

    Ok(())
}

/// Prints human-readable prune output for a single project.
fn print_human_output(
    dry_run: bool,
    cutoff: DateTime<chrono::Utc>,
    stats: &crate::engine::PruneStats,
) {
    print!("{}", human_output(dry_run, cutoff, stats));
}

/// Builds the human-readable prune output for a single project.
///
/// Pure: no I/O. Always leads with the resolved cutoff so a dry run and a
/// real run report the identical boundary — the whole point of a preview
/// for the tool's one destructive command.
fn human_output(
    dry_run: bool,
    cutoff: DateTime<chrono::Utc>,
    stats: &crate::engine::PruneStats,
) -> String {
    let dry_run_prefix = if dry_run { "[dry run] " } else { "" };
    let mut out = format!(
        "Pruning snapshots older than {}\n",
        super::format_local_time(cutoff)
    );

    if stats.snapshots_removed == 0 && stats.objects_removed == 0 {
        out.push_str(&format!("{}Nothing to prune.\n", dry_run_prefix));
        return out;
    }

    if stats.snapshots_removed > 0 {
        out.push_str(&format!(
            "{}Pruned {} snapshots.\n",
            dry_run_prefix,
            format_number(stats.snapshots_removed)
        ));
    }

    if stats.objects_removed > 0 {
        out.push_str(&format!(
            "{}Removed {} orphaned objects ({} freed).\n",
            dry_run_prefix,
            format_number(stats.objects_removed),
            format_size(stats.bytes_freed)
        ));
    }

    out
}

/// Prints human-readable prune output for all projects.
fn print_human_output_with_registry(
    dry_run: bool,
    cutoff: DateTime<chrono::Utc>,
    total_snapshots: u64,
    total_objects: u64,
    total_bytes: u64,
    registry_entries: u64,
) {
    print!(
        "{}",
        human_output_with_registry(
            dry_run,
            cutoff,
            total_snapshots,
            total_objects,
            total_bytes,
            registry_entries
        )
    );
}

/// Builds the human-readable prune output for all projects.
///
/// Pure: no I/O. Same cutoff-first contract as [`human_output`].
fn human_output_with_registry(
    dry_run: bool,
    cutoff: DateTime<chrono::Utc>,
    total_snapshots: u64,
    total_objects: u64,
    total_bytes: u64,
    registry_entries: u64,
) -> String {
    let dry_run_prefix = if dry_run { "[dry run] " } else { "" };
    let mut out = format!(
        "Pruning snapshots older than {}\n",
        super::format_local_time(cutoff)
    );

    if total_snapshots == 0 && total_objects == 0 && registry_entries == 0 {
        out.push_str(&format!("{}Nothing to prune.\n", dry_run_prefix));
        return out;
    }

    if total_snapshots > 0 {
        out.push_str(&format!(
            "{}Pruned {} snapshots across all projects.\n",
            dry_run_prefix,
            format_number(total_snapshots)
        ));
    }

    if total_objects > 0 {
        out.push_str(&format!(
            "{}Removed {} orphaned objects ({} freed).\n",
            dry_run_prefix,
            format_number(total_objects),
            format_size(total_bytes)
        ));
    }

    if registry_entries > 0 {
        out.push_str(&format!(
            "{}Cleaned {} stale registry entries.\n",
            dry_run_prefix,
            format_number(registry_entries)
        ));
    }

    out
}

use super::output::{format_number, format_size};

// Tests for format_size and format_number are in output.rs

#[cfg(test)]
mod tests {
    use super::*;

    /// Extracts the "Pruning snapshots older than ..." line from output.
    fn cutoff_line(output: &str) -> &str {
        output
            .lines()
            .find(|l| l.starts_with("Pruning snapshots older than "))
            .unwrap_or_else(|| panic!("no cutoff line found in output: {:?}", output))
    }

    /// A dry run must report the exact same resolved cutoff as a real run.
    /// `prune` is the one destructive command in this tool — if the dry-run
    /// preview ever drifted from what a real run would delete against, the
    /// preview would be worse than useless.
    #[test]
    fn dry_run_and_real_run_report_same_cutoff() {
        let cutoff: DateTime<chrono::Utc> = "2020-06-15T12:00:00Z"
            .parse()
            .expect("valid RFC3339 timestamp");
        let stats = crate::engine::PruneStats {
            snapshots_removed: 0,
            objects_removed: 0,
            bytes_freed: 0,
        };

        let dry_run_output = human_output(true, cutoff, &stats);
        let real_run_output = human_output(false, cutoff, &stats);

        assert_eq!(
            cutoff_line(&dry_run_output),
            cutoff_line(&real_run_output),
            "dry-run cutoff must match the real-run cutoff exactly"
        );

        // Also sanity-check it round-trips through the shared formatter that
        // `unf log` uses, so the printed cutoff is re-usable as `--older-than`.
        assert!(cutoff_line(&dry_run_output).ends_with(&super::super::format_local_time(cutoff)));
    }

    /// Same equality check for the all-projects human output path.
    #[test]
    fn dry_run_and_real_run_report_same_cutoff_all_projects() {
        let cutoff: DateTime<chrono::Utc> = "2020-06-15T12:00:00Z"
            .parse()
            .expect("valid RFC3339 timestamp");

        let dry_run_output = human_output_with_registry(true, cutoff, 0, 0, 0, 0);
        let real_run_output = human_output_with_registry(false, cutoff, 0, 0, 0, 0);

        assert_eq!(
            cutoff_line(&dry_run_output),
            cutoff_line(&real_run_output),
            "dry-run cutoff must match the real-run cutoff exactly"
        );
    }

    /// The cutoff line is present even when there's something to prune, not
    /// just on the "Nothing to prune" fast path.
    #[test]
    fn cutoff_line_present_when_snapshots_pruned() {
        let cutoff: DateTime<chrono::Utc> = "2020-06-15T12:00:00Z"
            .parse()
            .expect("valid RFC3339 timestamp");
        let stats = crate::engine::PruneStats {
            snapshots_removed: 3,
            objects_removed: 1,
            bytes_freed: 42,
        };

        let output = human_output(false, cutoff, &stats);
        assert_eq!(
            cutoff_line(&output),
            format!(
                "Pruning snapshots older than {}",
                super::super::format_local_time(cutoff)
            )
        );
        assert!(output.contains("Pruned 3 snapshots."));
    }
}
