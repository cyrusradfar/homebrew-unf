//! The human Mode 2 report for `unf status`.
//!
//! Pure rendering: the caller gathers values, this module returns the lines in
//! display order, and [`crate::cli::status`] prints them. Kept out of
//! `status.rs` so that file has room for the prune advisory without growing
//! past its size baseline.

use std::collections::BTreeSet;

use crate::cli::output::format_number;
use crate::cli::prune_advice::{self, PruneAdvice};

/// Everything the Mode 2 human report needs.
pub struct Mode2Report<'a> {
    pub duration: &'a str,
    pub snapshot_count: u64,
    pub file_count: u64,
    pub store_size: u64,
    pub excluded_present: &'a [&'a str],
    pub unignored: &'a BTreeSet<String>,
    pub force_watch_gitignore: bool,
    pub auto_restart: bool,
    pub advice: Option<&'a PruneAdvice>,
}

/// The report's lines, in display order:
/// stats, advice, un-ignore lines, gitignore, auto-restart.
pub fn mode2_lines(report: &Mode2Report<'_>) -> Vec<String> {
    let mut lines = vec![
        format!("Watching since {}", report.duration),
        format!("  Snapshots:  {}", format_number(report.snapshot_count)),
        format!("  Files tracked:  {}", format_number(report.file_count)),
        format!("  Store size:  {}", super::format_size(report.store_size)),
    ];

    if let Some(advice) = report.advice {
        lines.extend(prune_advice::human_lines(advice));
    }

    lines.extend(unignore_report_lines(
        report.excluded_present,
        report.unignored,
    ));

    // Only the non-default state earns a line. Respecting .gitignore is what
    // users expect, so silence means "respected".
    if report.force_watch_gitignore {
        lines.push("  Gitignore:  not applied (--force-watch-gitignore)".to_string());
    }

    lines.push(format!(
        "  Auto-restart: {}",
        if report.auto_restart {
            "enabled"
        } else {
            "disabled"
        }
    ));

    lines
}

/// The un-ignore lines of the human status report, indented and ready to
/// print in order.
///
/// Line 1 names what this project also records. Line 2 names what it still
/// skips, and line 3 is the paste-ready hint for opting one of them in —
/// naming a real directory rather than a `<NAME>` placeholder, so the reader
/// does no substitution.
///
/// Each line appears only when it applies, matching the `Gitignore:`
/// precedent: the default state is the expected state and earns no line. A
/// project with an empty set and no excluded directories in its root — the
/// common case — gets an empty `Vec` and prints nothing.
fn unignore_report_lines(present: &[&str], unignored: &BTreeSet<String>) -> Vec<String> {
    let mut lines = Vec::new();

    if !unignored.is_empty() {
        let names: Vec<&str> = unignored.iter().map(String::as_str).collect();
        lines.push(format!("  Un-ignored:  {}", names.join(", ")));
    }

    let not_recorded = crate::watcher::filter::not_recorded_dirs(present, unignored);
    if let Some(first) = not_recorded.first() {
        lines.push(format!("  Not recorded:  {}", not_recorded.join(", ")));
        lines.push(format!("    {}", crate::cli::output::unignore_hint(first)));
    }

    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::prune_advice::{PruneAdvice, Urgency};

    static EMPTY_UNIGNORED: BTreeSet<String> = BTreeSet::new();

    /// Builds a `BTreeSet` of un-ignored directory names from string slices.
    fn unignored(dirs: &[&str]) -> BTreeSet<String> {
        dirs.iter().map(|d| (*d).to_string()).collect()
    }

    fn advice(urgency: Urgency) -> PruneAdvice {
        PruneAdvice {
            urgency,
            total_store_bytes: 612 * 1024 * 1024,
            projects_measured: 4,
            overdue_projects: 1,
            oldest_overdue_days: 94,
            current_project_overdue_days: Some(94),
        }
    }

    fn minimal_report<'a>(advice: Option<&'a PruneAdvice>) -> Mode2Report<'a> {
        Mode2Report {
            duration: "3 days ago",
            snapshot_count: 12,
            file_count: 4,
            store_size: 2048,
            excluded_present: &[],
            unignored: &EMPTY_UNIGNORED,
            force_watch_gitignore: false,
            auto_restart: false,
            advice,
        }
    }

    #[test]
    fn mode2_lines_start_with_the_stats() {
        let lines = mode2_lines(&minimal_report(None));

        assert_eq!(lines[0], "Watching since 3 days ago");
        assert_eq!(lines[1], "  Snapshots:  12");
        assert_eq!(lines[2], "  Files tracked:  4");
        assert_eq!(lines[3], "  Store size:  2.0 KB");
        assert_eq!(lines[4], "  Auto-restart: disabled");
    }

    #[test]
    fn advice_sits_immediately_after_store_size() {
        let advice = advice(Urgency::Strong);
        let lines = mode2_lines(&minimal_report(Some(&advice)));

        assert_eq!(lines[3], "  Store size:  2.0 KB");
        assert!(lines[4].starts_with("  Warning:"), "{:?}", lines);
        assert!(
            lines.iter().any(|line| line.contains("--dry-run")),
            "{:?}",
            lines
        );
    }

    #[test]
    fn full_order_is_stats_advice_unignore_gitignore_auto_restart() {
        let advice = advice(Urgency::Suggest);
        let mut report = minimal_report(Some(&advice));
        let present = ["target"];
        let unignored = unignored(&["target"]);
        report.excluded_present = &present;
        report.unignored = &unignored;
        report.force_watch_gitignore = true;
        report.auto_restart = true;

        let lines = mode2_lines(&report);

        let store = lines
            .iter()
            .position(|line| line.starts_with("  Store size:"))
            .expect("store line");
        let prune = lines
            .iter()
            .position(|line| line.starts_with("  Prune:"))
            .expect("advice line");
        let unignore = lines
            .iter()
            .position(|line| line.starts_with("  Un-ignored:"))
            .expect("un-ignore line");
        let gitignore = lines
            .iter()
            .position(|line| line.starts_with("  Gitignore:"))
            .expect("gitignore line");
        let auto_restart = lines
            .iter()
            .position(|line| line.starts_with("  Auto-restart:"))
            .expect("auto-restart line");

        assert!(store < prune);
        assert!(prune < unignore);
        assert!(unignore < gitignore);
        assert!(gitignore < auto_restart);
    }

    #[test]
    fn no_advice_keeps_the_report_unchanged() {
        let lines = mode2_lines(&minimal_report(None));
        assert!(
            !lines
                .iter()
                .any(|line| line.contains("Prune:") || line.contains("Warning:")),
            "{:?}",
            lines
        );
    }

    #[test]
    fn not_recorded_dirs_omits_unignored_ones() {
        let present = ["node_modules", "target", "dist"];

        assert_eq!(
            crate::watcher::filter::not_recorded_dirs(&present, &unignored(&["target"])),
            vec!["node_modules", "dist"]
        );
    }

    #[test]
    fn not_recorded_dirs_empty_when_everything_is_unignored() {
        let present = ["target", "dist"];

        assert!(crate::watcher::filter::not_recorded_dirs(
            &present,
            &unignored(&["target", "dist"])
        )
        .is_empty());
    }

    #[test]
    fn unignore_report_lines_silent_in_the_default_case() {
        // Nothing opted in and nothing excluded present: silence means the
        // default, exactly like the `Gitignore:` line.
        assert!(unignore_report_lines(&[], &BTreeSet::new()).is_empty());
    }

    #[test]
    fn unignore_report_lines_reports_only_the_unignored_list() {
        // Everything present is already opted in, so no "Not recorded" line
        // and no hint.
        let lines = unignore_report_lines(&["target"], &unignored(&["target"]));

        assert_eq!(lines, vec!["  Un-ignored:  target".to_string()]);
    }

    /// The `Un-ignored:` line carries `.git` paths beside bare names.
    /// Entries sort `.git` first and are `, `-joined, so the slashes inside
    /// an entry never read as the separator between two entries.
    #[test]
    fn unignore_report_lines_render_git_paths_beside_names() {
        let lines = unignore_report_lines(&["target"], &unignored(&["target", ".git/hooks"]));

        assert_eq!(lines[0], "  Un-ignored:  .git/hooks, target");
        // `.git` is never in `present`, so it adds no "Not recorded" line.
        assert_eq!(lines.len(), 1);
    }

    #[test]
    fn unignore_report_lines_reports_only_the_remainder() {
        let lines = unignore_report_lines(&["node_modules", "dist"], &BTreeSet::new());

        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0], "  Not recorded:  node_modules, dist");
        assert_eq!(
            lines[1],
            "    Record one with `unf watch --unignore-dir node_modules`. See `unf watch --help`."
        );
    }

    #[test]
    fn unignore_report_lines_full_report_is_ordered() {
        let lines =
            unignore_report_lines(&["node_modules", "target", "dist"], &unignored(&["target"]));

        assert_eq!(
            lines,
            vec![
                "  Un-ignored:  target".to_string(),
                "  Not recorded:  node_modules, dist".to_string(),
                "    Record one with `unf watch --unignore-dir node_modules`. See `unf watch --help`."
                    .to_string(),
            ]
        );
    }

    #[test]
    fn unignore_report_hint_names_a_real_directory() {
        // A `<NAME>` placeholder would make the reader do the substitution.
        let lines = unignore_report_lines(&["dist"], &BTreeSet::new());

        assert!(
            lines[1].contains("--unignore-dir dist"),
            "hint was not paste-ready: {}",
            lines[1]
        );
    }
}
