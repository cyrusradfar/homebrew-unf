//! Low-level diff utilities for computing line-level statistics.
//!
//! This module provides pure functions for analyzing text differences.

use similar::{ChangeTag, TextDiff};

/// Line-level diff statistics between two text blobs.
#[derive(Debug, Clone, Default)]
pub struct DiffStats {
    pub lines_added: u32,
    pub lines_removed: u32,
}

/// Computes line-level diff statistics between two byte slices.
///
/// Pure function: no I/O, no side effects. Both inputs are treated
/// as UTF-8 (lossy conversion). Uses the `similar` crate.
pub fn compute_diff_stats(old: &[u8], new: &[u8]) -> DiffStats {
    let old_str = String::from_utf8_lossy(old);
    let new_str = String::from_utf8_lossy(new);
    let diff = TextDiff::from_lines(old_str.as_ref(), new_str.as_ref());
    let mut stats = DiffStats::default();
    for change in diff.iter_all_changes() {
        match change.tag() {
            ChangeTag::Insert => stats.lines_added += 1,
            ChangeTag::Delete => stats.lines_removed += 1,
            ChangeTag::Equal => {}
        }
    }
    stats
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diff_stats_identical_content() {
        let content = b"line1\nline2\nline3\n";
        let stats = compute_diff_stats(content, content);
        assert_eq!(stats.lines_added, 0);
        assert_eq!(stats.lines_removed, 0);
    }

    #[test]
    fn diff_stats_pure_additions() {
        let old = b"";
        let new = b"line1\nline2\n";
        let stats = compute_diff_stats(old, new);
        assert_eq!(stats.lines_added, 2);
        assert_eq!(stats.lines_removed, 0);
    }

    #[test]
    fn diff_stats_pure_deletions() {
        let old = b"line1\nline2\n";
        let new = b"";
        let stats = compute_diff_stats(old, new);
        assert_eq!(stats.lines_added, 0);
        assert_eq!(stats.lines_removed, 2);
    }

    #[test]
    fn diff_stats_mixed_changes() {
        let old = b"line1\nline2\nline3\n";
        let new = b"line1\nmodified\nline4\n";
        let stats = compute_diff_stats(old, new);
        // line1 stays (equal)
        // line2 is removed, line3 is removed (2 deletions)
        // "modified" and "line4" are added (2 insertions)
        assert_eq!(stats.lines_added, 2);
        assert_eq!(stats.lines_removed, 2);
    }

    #[test]
    fn diff_stats_empty_inputs() {
        let old = b"";
        let new = b"";
        let stats = compute_diff_stats(old, new);
        assert_eq!(stats.lines_added, 0);
        assert_eq!(stats.lines_removed, 0);
    }
}
