//! Pure glob-based file path filtering for include/exclude pattern matching.
//!
//! `GlobFilter` implements a simple yet powerful pattern matching system:
//! - Multiple include patterns are OR'd (if any matches, path is included)
//! - Multiple exclude patterns are OR'd (if any matches, path is excluded)
//! - Exclude takes precedence over include on conflict
//! - Empty include list matches all paths (until excluded)
//! - Empty exclude list excludes no paths
//!
//! This module follows SUPER principles: pure logic with no side effects,
//! using `globset::GlobSet` for efficient pattern compilation and matching.

use globset::GlobSet;

use crate::error::UnfError;

/// A pure filter for matching file paths against glob include/exclude patterns.
///
/// # Examples
///
/// ```ignore
/// let filter = GlobFilter::new(
///     &["**/*.rs".to_string()],
///     &["**/test_*.rs".to_string()],
///     false
/// )?;
///
/// assert!(filter.matches("src/main.rs"));
/// assert!(!filter.matches("src/test_utils.rs")); // excluded by test pattern
/// assert!(!filter.matches("src/main.py"));        // not included
/// ```
#[derive(Debug, Clone)]
pub struct GlobFilter {
    /// Compiled set of include patterns (OR'd)
    include: Option<GlobSet>,
    /// Compiled set of exclude patterns (OR'd)
    exclude: Option<GlobSet>,
}

impl GlobFilter {
    /// Creates a new `GlobFilter` from include and exclude pattern lists.
    ///
    /// # Arguments
    ///
    /// * `include` - Slice of glob patterns for inclusion (empty = match all)
    /// * `exclude` - Slice of glob patterns for exclusion (empty = exclude none)
    /// * `ignore_case` - Whether to apply case-insensitive matching
    ///
    /// # Returns
    ///
    /// A new `GlobFilter` on success, or `UnfError::InvalidArgument` if any
    /// pattern is malformed.
    ///
    /// # Errors
    ///
    /// Returns `UnfError::InvalidArgument` if:
    /// - Any include pattern fails to compile
    /// - Any exclude pattern fails to compile
    pub fn new(
        include: &[String],
        exclude: &[String],
        ignore_case: bool,
    ) -> Result<Self, UnfError> {
        // Compile include patterns (if non-empty)
        let include_set = if include.is_empty() {
            None
        } else {
            let mut glob_set = globset::GlobSetBuilder::new();

            for pattern in include {
                let glob = globset::GlobBuilder::new(pattern)
                    .case_insensitive(ignore_case)
                    .build()
                    .map_err(|e| {
                        UnfError::InvalidArgument(format!(
                            "Invalid include pattern '{}': {}",
                            pattern, e
                        ))
                    })?;
                glob_set.add(glob);
            }

            Some(glob_set.build().map_err(|e| {
                UnfError::InvalidArgument(format!("Failed to compile include patterns: {}", e))
            })?)
        };

        // Compile exclude patterns (if non-empty)
        let exclude_set = if exclude.is_empty() {
            None
        } else {
            let mut glob_set = globset::GlobSetBuilder::new();

            for pattern in exclude {
                let glob = globset::GlobBuilder::new(pattern)
                    .case_insensitive(ignore_case)
                    .build()
                    .map_err(|e| {
                        UnfError::InvalidArgument(format!(
                            "Invalid exclude pattern '{}': {}",
                            pattern, e
                        ))
                    })?;
                glob_set.add(glob);
            }

            Some(glob_set.build().map_err(|e| {
                UnfError::InvalidArgument(format!("Failed to compile exclude patterns: {}", e))
            })?)
        };

        Ok(GlobFilter {
            include: include_set,
            exclude: exclude_set,
        })
    }

    /// Tests whether a file path matches the filter.
    ///
    /// # Arguments
    ///
    /// * `path` - The file path to test (e.g., "src/main.rs")
    ///
    /// # Returns
    ///
    /// `true` if the path should be included (passes include filter and
    /// does not match exclude filter), `false` otherwise.
    ///
    /// # Logic
    ///
    /// 1. If include patterns exist and path doesn't match any, return `false`
    /// 2. If exclude patterns exist and path matches any, return `false`
    /// 3. Otherwise return `true`
    pub fn matches(&self, path: &str) -> bool {
        // If include set exists, path must match at least one include pattern
        if let Some(ref include) = self.include {
            if !include.is_match(path) {
                return false;
            }
        }

        // If exclude set exists, path must NOT match any exclude pattern
        if let Some(ref exclude) = self.exclude {
            if exclude.is_match(path) {
                return false;
            }
        }

        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_filter_matches_all() {
        let filter = GlobFilter::new(&[], &[], false).unwrap();
        assert!(filter.matches("src/main.rs"));
        assert!(filter.matches("README.md"));
        assert!(filter.matches("path/to/any/file.txt"));
    }

    #[test]
    fn include_only_single_pattern() {
        let filter = GlobFilter::new(&["**/*.rs".to_string()], &[], false).unwrap();
        assert!(filter.matches("src/main.rs"));
        assert!(filter.matches("tests/unit.rs"));
        assert!(!filter.matches("README.md"));
        assert!(!filter.matches("Cargo.toml"));
    }

    #[test]
    fn include_multiple_patterns_are_ored() {
        let filter = GlobFilter::new(
            &["**/*.rs".to_string(), "**/*.toml".to_string()],
            &[],
            false,
        )
        .unwrap();
        assert!(filter.matches("src/main.rs"));
        assert!(filter.matches("Cargo.toml"));
        assert!(!filter.matches("README.md"));
    }

    #[test]
    fn exclude_only_single_pattern() {
        let filter = GlobFilter::new(&[], &["**/*.tmp".to_string()], false).unwrap();
        assert!(filter.matches("src/main.rs"));
        assert!(filter.matches("README.md"));
        assert!(!filter.matches("data/cache.tmp"));
    }

    #[test]
    fn exclude_multiple_patterns_are_ored() {
        let filter = GlobFilter::new(
            &[],
            &["**/*.tmp".to_string(), "**/*.bak".to_string()],
            false,
        )
        .unwrap();
        assert!(filter.matches("src/main.rs"));
        assert!(!filter.matches("file.tmp"));
        assert!(!filter.matches("file.bak"));
    }

    #[test]
    fn exclude_wins_over_include() {
        let filter = GlobFilter::new(
            &["**/*.rs".to_string()],
            &["**/*test*.rs".to_string()],
            false,
        )
        .unwrap();
        assert!(filter.matches("src/main.rs"));
        assert!(!filter.matches("src/test_utils.rs"));
        assert!(!filter.matches("tests/integration_test.rs"));
    }

    #[test]
    fn both_include_and_exclude() {
        let filter = GlobFilter::new(
            &["src/**".to_string(), "tests/**".to_string()],
            &["**/*.tmp".to_string()],
            false,
        )
        .unwrap();
        assert!(filter.matches("src/main.rs"));
        assert!(filter.matches("tests/unit.rs"));
        assert!(!filter.matches("README.md")); // not in include
        assert!(!filter.matches("src/cache.tmp")); // in include but excluded
        assert!(!filter.matches("data/file.tmp")); // excluded (not in include either)
    }

    #[test]
    fn case_insensitive_matching() {
        let filter = GlobFilter::new(
            &["**/*.RS".to_string()],
            &[],
            true, // ignore_case = true
        )
        .unwrap();
        assert!(filter.matches("src/main.rs"));
        assert!(filter.matches("src/main.RS"));
        assert!(filter.matches("src/main.Rs"));
    }

    #[test]
    fn case_sensitive_matching() {
        let filter = GlobFilter::new(
            &["**/*.RS".to_string()],
            &[],
            false, // ignore_case = false
        )
        .unwrap();
        assert!(!filter.matches("src/main.rs"));
        assert!(filter.matches("src/main.RS"));
    }

    #[test]
    fn glob_wildcard_single_star() {
        // Note: In globset, * matches path separators, so "src/*.rs" matches "src/subdir/other.rs"
        // To match only files directly in src/, use a more specific pattern or exclude subdirs
        let filter = GlobFilter::new(&["src/*.rs".to_string()], &[], false).unwrap();
        assert!(filter.matches("src/main.rs"));
        // In globset, * is a path glob and matches across directories
        assert!(filter.matches("src/subdir/other.rs"));
    }

    #[test]
    fn glob_wildcard_double_star() {
        let filter = GlobFilter::new(&["src/**/*.rs".to_string()], &[], false).unwrap();
        assert!(filter.matches("src/main.rs"));
        assert!(filter.matches("src/subdir/other.rs"));
        assert!(filter.matches("src/a/b/c/deep.rs"));
    }

    #[test]
    fn invalid_include_pattern() {
        let result = GlobFilter::new(
            &["[invalid".to_string()], // invalid regex bracket
            &[],
            false,
        );
        assert!(result.is_err());
    }

    #[test]
    fn invalid_exclude_pattern() {
        let result = GlobFilter::new(
            &[],
            &["[invalid".to_string()], // invalid regex bracket
            false,
        );
        assert!(result.is_err());
    }

    #[test]
    fn complex_pattern_combination() {
        let filter = GlobFilter::new(
            &["src/**/*.rs".to_string(), "tests/**".to_string()],
            &["**/.*".to_string(), "**/__pycache__/**".to_string()],
            false,
        )
        .unwrap();
        assert!(filter.matches("src/main.rs"));
        assert!(filter.matches("tests/unit/test.rs"));
        assert!(!filter.matches("README.md")); // not in include
        assert!(!filter.matches("src/.hidden.rs")); // excluded
        assert!(!filter.matches("tests/__pycache__/module.pyc")); // excluded
    }
}
