//! Human-readable rendering for `unf prune`.
//!
//! Everything here is pure: it takes values and returns a `String`. The
//! printing happens in `cli::prune`. That split is what makes the preview a
//! prune must show before it deletes anything testable without a database.

use std::path::PathBuf;

use chrono::{DateTime, Utc};

use crate::cli::output::{format_number, format_size};
use crate::engine::PruneStats;

/// What a prune did, or would do, to one project.
///
/// The value that crosses from the executor in `cli::prune` to the renderers
/// here. `snapshots_kept` and `retained` describe what SURVIVES, which is the
/// half a user actually needs before answering a confirmation prompt.
pub struct ProjectPrune {
    /// Project root, as registered.
    pub path: PathBuf,
    /// What goes: snapshots, orphaned objects, bytes.
    pub stats: PruneStats,
    /// How many snapshots remain afterwards.
    pub snapshots_kept: u64,
    /// The window the surviving snapshots fall in, or `None` if the prune
    /// leaves the project with no history at all.
    pub retained: Option<(DateTime<Utc>, DateTime<Utc>)>,
}

/// The window that survives a prune at `cutoff`.
///
/// Pure. `delete_snapshots_before` removes `timestamp < cutoff`, so everything
/// at or after the cutoff survives and the window starts at whichever is
/// later: the oldest snapshot on record, or the cutoff itself.
///
/// Returns `None` when the project has no snapshots, or when even its newest
/// snapshot predates the cutoff — the case where a prune erases the project's
/// entire recorded history.
pub fn retained_range(
    oldest: Option<DateTime<Utc>>,
    newest: Option<DateTime<Utc>>,
    cutoff: DateTime<Utc>,
) -> Option<(DateTime<Utc>, DateTime<Utc>)> {
    let (oldest, newest) = (oldest?, newest?);
    if newest < cutoff {
        return None;
    }
    Some((oldest.max(cutoff), newest))
}

/// Renders the per-project preview shown before a wide prune deletes anything.
///
/// Leads with the identical cutoff line the real run prints, so a preview and
/// the run it previews can be compared line for line.
pub fn preview(projects: &[ProjectPrune], cutoff: DateTime<Utc>) -> String {
    let mut out = format!(
        "Pruning snapshots older than {}\n\n",
        crate::cli::format_local_time(cutoff)
    );

    for project in projects {
        out.push_str(&project_block(project));
    }

    let snapshots: u64 = projects.iter().map(|p| p.stats.snapshots_removed).sum();
    let objects: u64 = projects.iter().map(|p| p.stats.objects_removed).sum();
    let bytes: u64 = projects.iter().map(|p| p.stats.bytes_freed).sum();

    out.push_str(&format!(
        "Total: {} snapshots, {} objects, {} across {} project{}.\n",
        format_number(snapshots),
        format_number(objects),
        format_size(bytes),
        projects.len(),
        if projects.len() == 1 { "" } else { "s" }
    ));

    out
}

/// One project's block within [`preview`]: what goes, then what stays.
fn project_block(project: &ProjectPrune) -> String {
    let mut out = format!("  {}\n", project.path.display());

    out.push_str(&format!(
        "    delete  {} snapshots, {} objects, {}\n",
        format_number(project.stats.snapshots_removed),
        format_number(project.stats.objects_removed),
        format_size(project.stats.bytes_freed)
    ));

    match project.retained {
        Some((start, end)) => out.push_str(&format!(
            "    keep    {} snapshots, {} to {}\n\n",
            format_number(project.snapshots_kept),
            crate::cli::format_local_time(start),
            crate::cli::format_local_time(end)
        )),
        None => out.push_str("    keep    nothing: this removes the project's entire history\n\n"),
    }

    out
}

/// Builds the human-readable prune output for a single project.
///
/// Pure: no I/O. Always leads with the resolved cutoff so a dry run and a
/// real run report the identical boundary — the whole point of a preview
/// for the tool's one destructive command.
pub fn human_output(dry_run: bool, cutoff: DateTime<Utc>, stats: &PruneStats) -> String {
    let dry_run_prefix = if dry_run { "[dry run] " } else { "" };
    let mut out = format!(
        "Pruning snapshots older than {}\n",
        crate::cli::format_local_time(cutoff)
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

/// Builds the human-readable prune output for all projects.
///
/// Pure: no I/O. Same cutoff-first contract as [`human_output`].
pub fn human_output_with_registry(
    dry_run: bool,
    cutoff: DateTime<Utc>,
    total_snapshots: u64,
    total_objects: u64,
    total_bytes: u64,
    registry_entries: u64,
) -> String {
    let dry_run_prefix = if dry_run { "[dry run] " } else { "" };
    let mut out = format!(
        "Pruning snapshots older than {}\n",
        crate::cli::format_local_time(cutoff)
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

#[cfg(test)]
mod tests {
    use super::*;

    fn at(rfc3339: &str) -> DateTime<Utc> {
        rfc3339.parse().expect("valid RFC3339 timestamp")
    }

    /// Extracts the "Pruning snapshots older than ..." line from output.
    fn cutoff_line(output: &str) -> &str {
        output
            .lines()
            .find(|l| l.starts_with("Pruning snapshots older than "))
            .unwrap_or_else(|| panic!("no cutoff line found in output: {:?}", output))
    }

    fn sample(path: &str, removed: u64, kept: u64, retained_from: Option<&str>) -> ProjectPrune {
        ProjectPrune {
            path: PathBuf::from(path),
            stats: PruneStats {
                snapshots_removed: removed,
                objects_removed: removed / 2,
                bytes_freed: removed * 1024,
            },
            snapshots_kept: kept,
            retained: retained_from.map(|s| (at(s), at("2026-08-30T12:00:00Z"))),
        }
    }

    /// A dry run must report the exact same resolved cutoff as a real run.
    /// `prune` is the one destructive command in this tool — if the dry-run
    /// preview ever drifted from what a real run would delete against, the
    /// preview would be worse than useless.
    #[test]
    fn dry_run_and_real_run_report_same_cutoff() {
        let cutoff = at("2020-06-15T12:00:00Z");
        let stats = PruneStats {
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
        assert!(cutoff_line(&dry_run_output).ends_with(&crate::cli::format_local_time(cutoff)));
    }

    /// Same equality check for the all-projects human output path.
    #[test]
    fn dry_run_and_real_run_report_same_cutoff_all_projects() {
        let cutoff = at("2020-06-15T12:00:00Z");

        let dry_run_output = human_output_with_registry(true, cutoff, 0, 0, 0, 0);
        let real_run_output = human_output_with_registry(false, cutoff, 0, 0, 0, 0);

        assert_eq!(
            cutoff_line(&dry_run_output),
            cutoff_line(&real_run_output),
            "dry-run cutoff must match the real-run cutoff exactly"
        );
    }

    /// The per-project preview leads with the same cutoff line the real run
    /// prints, so what you were shown and what was deleted are comparable.
    #[test]
    fn preview_reports_same_cutoff_as_real_run() {
        let cutoff = at("2020-06-15T12:00:00Z");
        let projects = vec![sample("/p/one", 10, 5, Some("2020-06-15T12:00:00Z"))];

        assert_eq!(
            cutoff_line(&preview(&projects, cutoff)),
            cutoff_line(&human_output_with_registry(false, cutoff, 10, 5, 0, 0))
        );
    }

    /// The cutoff line is present even when there's something to prune, not
    /// just on the "Nothing to prune" fast path.
    #[test]
    fn cutoff_line_present_when_snapshots_pruned() {
        let cutoff = at("2020-06-15T12:00:00Z");
        let stats = PruneStats {
            snapshots_removed: 3,
            objects_removed: 1,
            bytes_freed: 42,
        };

        let output = human_output(false, cutoff, &stats);
        assert_eq!(
            cutoff_line(&output),
            format!(
                "Pruning snapshots older than {}",
                crate::cli::format_local_time(cutoff)
            )
        );
        assert!(output.contains("Pruned 3 snapshots."));
    }

    /// Every project gets its own line, with counts, bytes and what survives.
    #[test]
    fn preview_lists_every_project_and_totals_them() {
        let cutoff = at("2026-08-23T12:00:00Z");
        let projects = vec![
            sample("/p/one", 1000, 40, Some("2026-08-23T12:00:00Z")),
            sample("/p/two", 24, 6, Some("2026-08-25T12:00:00Z")),
        ];

        let out = preview(&projects, cutoff);

        assert!(out.contains("/p/one"), "{}", out);
        assert!(out.contains("/p/two"), "{}", out);
        assert!(
            out.contains("delete  1,000 snapshots, 500 objects"),
            "{}",
            out
        );
        assert!(out.contains("keep    40 snapshots,"), "{}", out);
        assert!(
            out.contains("Total: 1,024 snapshots, 512 objects, 1.0 MB across 2 projects."),
            "{}",
            out
        );
    }

    /// A project the prune would empty completely says so in words. A number
    /// of retained snapshots reading "0" is far too easy to skim past.
    #[test]
    fn preview_names_a_project_it_would_empty() {
        let cutoff = at("2026-08-23T12:00:00Z");
        let projects = vec![sample("/p/stale", 900, 0, None)];

        let out = preview(&projects, cutoff);
        assert!(
            out.contains("keep    nothing: this removes the project's entire history"),
            "{}",
            out
        );
    }

    #[test]
    fn retained_range_starts_at_the_cutoff_when_history_predates_it() {
        let cutoff = at("2026-08-23T12:00:00Z");
        let range = retained_range(
            Some(at("2026-01-01T00:00:00Z")),
            Some(at("2026-08-30T00:00:00Z")),
            cutoff,
        );
        assert_eq!(range, Some((cutoff, at("2026-08-30T00:00:00Z"))));
    }

    #[test]
    fn retained_range_starts_at_the_oldest_snapshot_when_nothing_is_pruned() {
        let oldest = at("2026-08-28T00:00:00Z");
        let range = retained_range(Some(oldest), Some(at("2026-08-30T00:00:00Z")), {
            at("2026-08-23T12:00:00Z")
        });
        assert_eq!(range, Some((oldest, at("2026-08-30T00:00:00Z"))));
    }

    /// Nothing survives when even the newest snapshot predates the cutoff.
    #[test]
    fn retained_range_is_none_when_everything_is_older_than_the_cutoff() {
        assert_eq!(
            retained_range(
                Some(at("2026-01-01T00:00:00Z")),
                Some(at("2026-02-01T00:00:00Z")),
                at("2026-08-23T12:00:00Z"),
            ),
            None
        );
    }

    #[test]
    fn retained_range_is_none_for_an_empty_project() {
        assert_eq!(retained_range(None, None, at("2026-08-23T12:00:00Z")), None);
    }
}
