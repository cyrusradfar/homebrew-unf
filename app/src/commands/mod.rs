pub mod config;
pub mod content;
pub mod daemon;
pub mod diff;
pub mod history;
pub mod project;
pub mod unf;

#[cfg(test)]
mod async_guard {
    //! Enforces AR-03: every `#[tauri::command]` in `app/src/commands/` must be `async fn`.
    //!
    //! Sync Tauri commands execute inline on the WKWebView URL-scheme-handler thread
    //! (the macOS main/UI thread), so any blocking call freezes the app window. This
    //! guard fails CI if a future contributor reintroduces a sync command.
    use regex::Regex;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::OnceLock;

    /// One scan failure: source label, the line number where `#[tauri::command]` was
    /// found, and the first non-attribute/doc line that should have been `async fn`.
    #[derive(Debug)]
    struct ScanError {
        source: String,
        attr_line: usize,
        offending: String,
    }

    fn attr_re() -> &'static Regex {
        static RE: OnceLock<Regex> = OnceLock::new();
        RE.get_or_init(|| Regex::new(r"^\s*#\[\s*tauri::command(\s*\(.*\))?\s*\]\s*$").unwrap())
    }

    fn async_fn_re() -> &'static Regex {
        static RE: OnceLock<Regex> = OnceLock::new();
        // Accepts: `async fn`, `pub async fn`, `pub(crate) async fn`, `pub(super) async fn`, etc.
        RE.get_or_init(|| Regex::new(r"^\s*(pub(\([a-z]+\))?\s+)?async\s+fn\s+(\w+)").unwrap())
    }

    fn other_attr_re() -> &'static Regex {
        static RE: OnceLock<Regex> = OnceLock::new();
        RE.get_or_init(|| Regex::new(r"^\s*#\[").unwrap())
    }

    /// Scan a single source file's contents for sync `#[tauri::command]` declarations.
    ///
    /// Returns the list of offending sites; an empty `Vec` means the file is clean.
    /// Pure over `&str` — callers supply contents (real file or inline fixture).
    fn scan(source: &str, label: &str) -> Vec<ScanError> {
        let lines: Vec<&str> = source.lines().collect();
        let mut errors = Vec::new();

        for (idx, line) in lines.iter().enumerate() {
            if !attr_re().is_match(line) {
                continue;
            }
            // Walk forward, skipping blank lines, other attribute lines, and doc comments.
            let mut j = idx + 1;
            while j < lines.len() {
                let l = lines[j];
                let trimmed = l.trim_start();
                if trimmed.is_empty()
                    || trimmed.starts_with("///")
                    || trimmed.starts_with("//!")
                    || other_attr_re().is_match(l)
                {
                    j += 1;
                    continue;
                }
                break;
            }
            let candidate = lines.get(j).copied().unwrap_or("");
            if !async_fn_re().is_match(candidate) {
                errors.push(ScanError {
                    source: label.to_string(),
                    attr_line: idx + 1,
                    offending: candidate.to_string(),
                });
            }
        }
        errors
    }

    fn commands_dir() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("src/commands")
    }

    /// The real guard: walk every `.rs` file under `app/src/commands/` and fail if
    /// any `#[tauri::command]` is not followed by `async fn`.
    #[test]
    fn all_tauri_commands_are_async() {
        let dir = commands_dir();
        let mut all_errors: Vec<ScanError> = Vec::new();
        let mut scanned = 0usize;

        for entry in fs::read_dir(&dir).expect("read commands dir") {
            let entry = entry.expect("dir entry");
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) != Some("rs") {
                continue;
            }
            let contents = fs::read_to_string(&path)
                .unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
            let label = path.display().to_string();
            all_errors.extend(scan(&contents, &label));
            scanned += 1;
        }

        assert!(scanned > 0, "no .rs files found in {}", dir.display());

        if !all_errors.is_empty() {
            let mut msg = String::from(
                "Found sync `#[tauri::command]` declarations. Tauri commands MUST be `async fn`:\n",
            );
            for err in &all_errors {
                msg.push_str(&format!(
                    "  {}:{}  ->  {}\n",
                    err.source,
                    err.attr_line,
                    err.offending.trim_end()
                ));
            }
            panic!("{msg}");
        }
    }

    // ---- Fixture tests (inline strings, not real files) ----

    #[test]
    fn fixture_doc_comment_between_attr_and_fn() {
        let src = "#[tauri::command]\n/// Doc\npub async fn f() {}\n";
        assert!(scan(src, "fixture").is_empty());
    }

    #[test]
    fn fixture_stacked_allow_attr() {
        let src = "#[allow(clippy::too_many_arguments)]\n#[tauri::command]\npub async fn f() {}\n";
        assert!(scan(src, "fixture").is_empty());
    }

    #[test]
    fn fixture_renamed_command() {
        let src = "#[tauri::command(rename = \"foo\")]\npub async fn f() {}\n";
        assert!(scan(src, "fixture").is_empty());
    }

    #[test]
    fn fixture_pub_crate_async() {
        let src = "#[tauri::command]\npub(crate) async fn f() {}\n";
        assert!(scan(src, "fixture").is_empty());
    }

    #[test]
    fn fixture_bare_async() {
        let src = "#[tauri::command]\nasync fn f() {}\n";
        assert!(scan(src, "fixture").is_empty());
    }

    #[test]
    fn fixture_sync_pub_fn() {
        let src = "#[tauri::command]\npub fn f() {}\n";
        let errs = scan(src, "fixture");
        assert_eq!(errs.len(), 1, "expected one scan error, got {errs:?}");
        assert_eq!(errs[0].attr_line, 1);
        assert!(errs[0].offending.contains("pub fn f()"));
    }
}
