//! Path filtering for the filesystem watcher.
//!
//! This module provides efficient filtering of filesystem paths to determine
//! which files should be tracked by the flight recorder. It combines hardcoded
//! ignore rules with .gitignore pattern matching to skip binary files, build
//! artifacts, and other non-trackable content.
//!
//! The filtering logic follows the SUPER principle: side effects (reading
//! .gitignore from disk) happen only at construction time. The `should_track`
//! method is a pure query against cached rules.
//!
//! A project can adjust filtering via the [`WatchSettings`] passed to
//! [`Filter::new`]:
//! - `force_watch_gitignore` opts out of .gitignore and hidden-file
//!   filtering. A per-project override for cases where the interesting files
//!   are exactly the ignored ones (e.g. `.env.local`, gitignored scratch
//!   directories).
//! - `unignored_dirs` opts specific hardcoded-excluded directories (e.g.
//!   `target`) back into tracking.
//!
//! `.git` is the one directory neither setting can lift — `should_track`
//! hard-refuses it before consulting `unignored_dirs` at all. Extension
//! rules and downstream magic-number binary detection are unaffected by
//! either setting.

use std::collections::BTreeSet;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

use ignore::gitignore::GitignoreBuilder;

use crate::error::WatcherError;
use crate::types::WatchSettings;

/// Directories that are excluded by default, regardless of .gitignore rules.
///
/// These are common directories containing build artifacts, dependencies,
/// and internal state that should not be part of the flight recorder. All
/// but `.git` can be opted back into tracking per-project via
/// `WatchSettings::unignored_dirs`; `.git` is permanently excluded (see the
/// hard refusal in `Filter::should_track`).
const IGNORED_DIRS: &[&str] = &[
    ".git",
    "node_modules",
    "target",
    ".next",
    "__pycache__",
    ".venv",
    "venv",
    ".tox",
    "dist",
    "build",
];

/// The one entry in `IGNORED_DIRS` that `WatchSettings::unignored_dirs` can
/// never lift. See the hard refusal at the top of `Filter::should_track`.
const PERMANENT_IGNORED_DIR: &str = ".git";

/// Comma-joined `IGNORED_DIRS` entries a user could plausibly un-ignore,
/// i.e. everything except `PERMANENT_IGNORED_DIR`. Shared by both
/// `parse_unignore_dir` error branches that list eligible names.
fn eligible_dirs_list() -> String {
    IGNORED_DIRS
        .iter()
        .filter(|&&dir| dir != PERMANENT_IGNORED_DIR)
        .copied()
        .collect::<Vec<_>>()
        .join(", ")
}

/// Validates a `--unignore-dir NAME` value.
///
/// Pure and unit-testable with no clap dependency: it matches clap's
/// `fn(&str) -> Result<String, String>` value-parser shape, so it can be
/// used directly as `value_parser = parse_unignore_dir` on the CLI's
/// `Vec<String>` argument (wired up in UD-07).
///
/// Three refusal branches, each explaining the reason rather than issuing a
/// bare rejection:
///
/// - `.git`, in any case (`.git`, `.GIT`, `.Git`, ...): permanently
///   excluded. Explains why, since "invalid" alone would invite a user to
///   keep guessing at spellings that can never work.
/// - A name that differs from an eligible `IGNORED_DIRS` entry only by
///   case, e.g. `Target`: suggests the lowercase form. `IGNORED_DIRS`
///   matching in `should_track` is exact and case-sensitive, so `Target`
///   would otherwise persist as a silent no-op.
/// - Anything else, e.g. `logs`: not on the hardcoded list at all, so there
///   is nothing for this flag to lift. Routes to `--force-watch-gitignore`,
///   the more likely real intent — a name like `logs` is almost certainly
///   being dropped by `.gitignore`, not by `IGNORED_DIRS`.
pub fn parse_unignore_dir(name: &str) -> Result<String, String> {
    // Checked case-insensitively and before the exact-match check below:
    // `.GIT` / `.Git` are still permanently excluded, and the reason for
    // refusing them is the same regardless of case, so they get the
    // refusal message rather than a "did you mean '.git'" suggestion that
    // would wrongly imply fixing the case helps.
    if name.eq_ignore_ascii_case(PERMANENT_IGNORED_DIR) {
        return Err(format!(
            "{PERMANENT_IGNORED_DIR} can never be recorded. UNF would capture git's object \
             store while git rewrites it, and restoring it could corrupt the repository. \
             Eligible directories: {}",
            eligible_dirs_list()
        ));
    }

    if IGNORED_DIRS.contains(&name) {
        return Ok(name.to_string());
    }

    let lower = name.to_lowercase();
    if lower != name && IGNORED_DIRS.contains(&lower.as_str()) {
        return Err(format!(
            "unknown directory '{name}'. Did you mean '{lower}'? Names are case-sensitive."
        ));
    }

    Err(format!(
        "'{name}' is not on UNF's excluded list, so there is nothing to un-ignore. UNF \
         already records it unless .gitignore excludes it — see --force-watch-gitignore. \
         Eligible directories: {}",
        eligible_dirs_list()
    ))
}

/// Excluded directories, from `IGNORED_DIRS`, that exist as immediate
/// children of `project_root` and could be opted back into tracking with
/// `--unignore-dir`.
///
/// Performs a single `read_dir` of `project_root` — immediate children
/// only. A nested `packages/web/node_modules` is never reported; only the
/// project root's own contents are scanned. This is an I/O edge function,
/// not a pure query: call it at the CLI edge (`unf watch`, `unf status`),
/// never from `should_track`.
///
/// `.git` is deliberately omitted from the result. It exists in nearly
/// every repository, is not actionable (it can never be un-ignored — see
/// `PERMANENT_IGNORED_DIR`), and listing a permanently-excluded entry here
/// would train users to ignore the rest of the message.
///
/// Returned in `IGNORED_DIRS` order, not filesystem order, so output is
/// deterministic across runs. Values are `&'static str` borrowed from
/// `IGNORED_DIRS`, never allocated from the directory entry, so callers get
/// the canonical spelling.
///
/// Excluded directories present in the project root that this project still
/// does not record: the [`eligible_unignore_dirs`] result minus the names
/// the user opted into.
///
/// Pure — it takes the scan result rather than reading the disk itself, so
/// a caller that already has the scan does not pay for a second `read_dir`.
///
/// Lives here beside [`eligible_unignore_dirs`] because `unf watch` and
/// `unf status` both report this remainder and must agree on it. Two
/// private copies would be free to drift apart.
pub fn not_recorded_dirs<'a>(present: &[&'a str], unignored: &BTreeSet<String>) -> Vec<&'a str> {
    present
        .iter()
        .copied()
        .filter(|dir| !unignored.contains(*dir))
        .collect()
}

/// Returns an empty `Vec` if `project_root` cannot be read (missing,
/// unreadable, not a directory) rather than an error — this is a
/// discoverability aid, not a correctness check, and `unf watch` must not
/// fail because of it.
pub fn eligible_unignore_dirs(project_root: &Path) -> Vec<&'static str> {
    let entries = match std::fs::read_dir(project_root) {
        Ok(entries) => entries,
        Err(_) => return Vec::new(),
    };

    let present: HashSet<String> = entries
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.file_type().map(|ft| ft.is_dir()).unwrap_or(false))
        .filter_map(|entry| entry.file_name().into_string().ok())
        .collect();

    IGNORED_DIRS
        .iter()
        .filter(|&&dir| dir != PERMANENT_IGNORED_DIR && present.contains(dir))
        .copied()
        .collect()
}

/// File extensions for binary and non-text files that should not be tracked.
///
/// The watcher only tracks text-based files. This list covers common binary
/// formats organized by category. SVG is intentionally excluded (it's text/XML).
const IGNORED_EXTENSIONS: &[&str] = &[
    // Images (raster)
    "png", "jpg", "jpeg", "gif", "bmp", "ico", "tiff", "tif", "webp", "heic", "heif", "raw", "cr2",
    "nef", "arw", "dng", "psd", "xcf",
    // Images (vector) — SVG is text-based, keep tracking it
    "ai", "eps", // Video
    "mp4", "avi", "mkv", "mov", "wmv", "flv", "webm", "m4v", "mpg", "mpeg", "3gp", "ogv",
    // Audio
    "mp3", "wav", "flac", "aac", "ogg", "wma", "m4a", "opus", "aiff", "aif", "mid", "midi",
    // Archives & compressed
    "zip", "tar", "gz", "bz2", "xz", "7z", "rar", "zst", "lz4", "lzma", "cab", "iso", "dmg", "deb",
    "rpm", "snap", "appimage", // Executables & libraries
    "exe", "dll", "so", "dylib", "o", "a", "lib", "obj", "wasm", "elf", "bin", "com", "msi", "app",
    // Compiled/bytecode
    "pyc", "pyo", "class", "beam", "elc",
    // Rust build artifacts — .rlib starts with the ASCII `!<arch>\n` archive
    // header, which has no NUL in its first 16 bytes and isn't in
    // MAGIC_NUMBERS, so is_likely_binary alone would miss it (see the
    // rlib_header_not_caught_by_is_likely_binary test below)
    "rlib", "rmeta", "d", // Documents (binary)
    "pdf", "doc", "docx", "xls", "xlsx", "ppt", "pptx", "odt", "ods", "odp", "rtf", "pages",
    "numbers", "key", // Databases
    "sqlite", "sqlite3", "db", "mdb", "accdb", // Fonts
    "ttf", "otf", "woff", "woff2", "eot", // Data (binary)
    "parquet", "avro", "arrow", "protobuf", "msgpack", "npy", "npz", "h5", "hdf5",
    // Disk images & VMs
    "vmdk", "vdi", "qcow2", "vhd", "vhdx", // Certificates & keys (binary formats)
    "p12", "pfx", "jks", "der", // Other binary
    "swf", "fla", "blend", "fbx", "glb", "gltf",
];

/// Known binary file magic numbers (first bytes).
///
/// This is defense-in-depth: extension-based filtering is primary.
/// Magic number check catches extensionless binaries or mislabeled files.
const MAGIC_NUMBERS: &[&[u8]] = &[
    b"\x89PNG",            // PNG
    b"\xFF\xD8\xFF",       // JPEG
    b"GIF87a",             // GIF 87a
    b"GIF89a",             // GIF 89a
    b"PK\x03\x04",         // ZIP/DOCX/XLSX/JAR
    b"\x7FELF",            // ELF executable
    b"\xCF\xFA\xED\xFE",   // Mach-O (little-endian)
    b"\xFE\xED\xFA\xCF",   // Mach-O (big-endian)
    b"\xCA\xFE\xBA\xBE",   // Mach-O fat binary / Java class
    b"MZ",                 // DOS/PE executable
    b"%PDF",               // PDF
    b"\x1F\x8B",           // gzip
    b"BZh",                // bzip2
    b"\xFD7zXZ\x00",       // xz
    b"7z\xBC\xAF\x27\x1C", // 7-Zip
    b"Rar!\x1A\x07",       // RAR
    b"\x28\xB5\x2F\xFD",   // Zstandard
    b"SQLite format 3",    // SQLite
    b"RIFF",               // WAV/AVI/WebP
    b"\x00\x00\x01\x00",   // ICO
    b"OggS",               // OGG
    b"fLaC",               // FLAC
    b"ID3",                // MP3 with ID3 tag
    b"\xFF\xFB",           // MP3 frame sync
    b"\xFF\xF3",           // MP3 frame sync
    // Zlib stream headers: 78 01 (no/low compression), 78 9C (default
    // compression), 78 DA (best compression). A loose git object is a raw
    // zlib stream with no other container, so it carries one of these three
    // bytes-pairs and nothing else recognisable. Measured on this repo: 171
    // of 299 sampled loose objects (57%) have no NUL in their first 16 bytes
    // and matched no existing entry here, so is_likely_binary returned false
    // and they were treated as text. The gap applies to any loose object
    // anywhere on disk, not only inside .git. False positives are
    // implausible: a text file beginning `x` followed by a high control byte
    // is not valid UTF-8 prose.
    b"\x78\x01", // zlib, no/low compression
    b"\x78\x9C", // zlib, default compression
    b"\x78\xDA", // zlib, best compression
];

/// Maximum bytes to check for magic number detection.
const MAGIC_READ_SIZE: usize = 16;

/// Checks if file content starts with a known binary magic number.
///
/// Pure function: takes a byte slice, returns bool. Checks for known binary
/// file signatures and NUL bytes in the first `MAGIC_READ_SIZE` bytes.
/// This is defense-in-depth: extension-based filtering is the primary mechanism.
pub fn is_likely_binary(header: &[u8]) -> bool {
    for magic in MAGIC_NUMBERS {
        if header.len() >= magic.len() && header.starts_with(magic) {
            return true;
        }
    }
    // Also check for NUL bytes in the first MAGIC_READ_SIZE bytes
    // (text files almost never contain NUL)
    let check_len = header.len().min(MAGIC_READ_SIZE);
    header[..check_len].contains(&0)
}

/// Path filter that caches .gitignore rules and provides fast path checking.
///
/// The filter is constructed once per project root and used to check every
/// filesystem event. It combines multiple filtering strategies:
/// - Hardcoded directory exclusions (e.g., node_modules, .git)
/// - Hardcoded file extension exclusions (e.g., .png, .exe)
/// - .gitignore pattern matching (if .gitignore is present and not overridden)
/// - Hidden file filtering (with exceptions for .env files, unless overridden)
pub struct Filter {
    /// Parsed .gitignore rules, if a .gitignore file was found and loaded.
    /// Always `None` when `settings.force_watch_gitignore` is true.
    gitignore: Option<ignore::gitignore::Gitignore>,
    /// The root directory this filter was created for.
    project_root: PathBuf,
    /// Per-project overrides this filter was constructed with.
    settings: WatchSettings,
}

impl Filter {
    /// Create a new filter rooted at the given project directory.
    ///
    /// When `settings.force_watch_gitignore` is false (the default),
    /// automatically loads .gitignore if present. If the .gitignore file
    /// cannot be read or parsed, returns an error. If no .gitignore exists,
    /// the filter will still work using hardcoded rules only.
    ///
    /// When `settings.force_watch_gitignore` is true, .gitignore is not
    /// loaded at all — `should_track` never consults it, and a malformed
    /// .gitignore cannot fail construction. Hidden files are also no longer
    /// skipped. Hardcoded directory/extension rules and magic-number binary
    /// detection are unaffected by this flag.
    ///
    /// `settings.unignored_dirs` is stored verbatim and consulted by
    /// `should_track`; it has no effect on construction.
    ///
    /// # Errors
    ///
    /// Returns [`WatcherError`] if:
    /// - The project root is not a valid directory
    /// - A .gitignore file exists but cannot be parsed (only possible when
    ///   `settings.force_watch_gitignore` is false)
    pub fn new(project_root: &Path, settings: WatchSettings) -> Result<Self, WatcherError> {
        let gitignore = if settings.force_watch_gitignore {
            None
        } else {
            let gitignore_path = project_root.join(".gitignore");
            if gitignore_path.exists() {
                let mut builder = GitignoreBuilder::new(project_root);
                if let Some(err) = builder.add(&gitignore_path) {
                    return Err(WatcherError::Io(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        format!("Failed to parse .gitignore: {}", err),
                    )));
                }
                match builder.build() {
                    Ok(gi) => Some(gi),
                    Err(err) => {
                        return Err(WatcherError::Io(std::io::Error::new(
                            std::io::ErrorKind::InvalidData,
                            format!("Failed to build gitignore matcher: {}", err),
                        )))
                    }
                }
            } else {
                None
            }
        };

        Ok(Self {
            gitignore,
            project_root: project_root.to_path_buf(),
            settings,
        })
    }

    /// Returns the settings this filter was constructed with.
    pub fn settings(&self) -> &WatchSettings {
        &self.settings
    }

    /// True when the loaded `.gitignore` also excludes `dir`, so lifting
    /// `IGNORED_DIRS`'s built-in exclusion via `--unignore-dir` alone will
    /// not record anything inside it (rule 3 in `should_track` still drops
    /// it). Needed for the `unf watch` warning that `--unignore-dir target`
    /// alone does nothing on a typical Rust project.
    ///
    /// Matches `dir` as a directory, not a file — `.gitignore` patterns
    /// like `/target/` only match directory-mode lookups, and passing
    /// `is_dir: false` to the `ignore` crate's matcher would silently miss
    /// them.
    ///
    /// Always false when no `.gitignore` is loaded: either
    /// `settings.force_watch_gitignore` is set (the matcher is never
    /// built), or the project has no `.gitignore` file. A filter with
    /// nothing loaded cannot be shadowing anything.
    pub fn gitignore_shadows(&self, dir: &str) -> bool {
        match &self.gitignore {
            Some(gitignore) => gitignore.matched(dir, true).is_ignore(),
            None => false,
        }
    }

    /// Returns true if this path should be tracked by the flight recorder.
    ///
    /// **Precondition:** The caller must only pass file paths, not directory paths.
    /// This method assumes `path` is a file and does not check for directories.
    ///
    /// A path is tracked if it passes all of these checks:
    /// 1. No path component matches IGNORED_DIRS, unless that component is
    ///    in `settings.unignored_dirs` — except `.git`, which can never be
    ///    un-ignored
    /// 2. File extension is not in IGNORED_EXTENSIONS
    /// 3. If .gitignore is loaded, the path must not be ignored by it
    ///    (never loaded when `settings.force_watch_gitignore` is true)
    /// 4. Hidden files (starting with .) are skipped, except .env and .gitignore
    ///    files (skipped entirely when `settings.force_watch_gitignore` is true)
    pub fn should_track(&self, path: &Path) -> bool {
        // 1. Check if any path component matches IGNORED_DIRS, allowing for
        // per-project opt-in via settings.unignored_dirs.
        for component in path.components() {
            if let Some(comp_str) = component.as_os_str().to_str() {
                // `.git` is hard-refused here, before consulting
                // unignored_dirs, deliberately as a second layer on top of
                // CLI validation. A hand-edited projects.json setting
                // unignored_dirs to include ".git" must not be able to make
                // the daemon record git's object store. Do not "simplify"
                // this into the IGNORED_DIRS branch below.
                if comp_str == PERMANENT_IGNORED_DIR {
                    return false;
                }
                if IGNORED_DIRS.contains(&comp_str)
                    && !self.settings.unignored_dirs.contains(comp_str)
                {
                    return false;
                }
            }
        }

        // 2. Check file extension against IGNORED_EXTENSIONS (case-insensitive)
        if let Some(ext) = path.extension() {
            if let Some(ext_str) = ext.to_str() {
                let ext_lower = ext_str.to_lowercase();
                if IGNORED_EXTENSIONS.contains(&ext_lower.as_str()) {
                    return false;
                }
            }
        }

        // 3. If .gitignore loaded, check if path is ignored
        if let Some(ref gitignore) = self.gitignore {
            // Convert to relative path if it starts with project_root.
            // If strip_prefix fails, the path is absolute and outside the project root,
            // so we use the absolute path directly. The gitignore matcher handles both
            // relative and absolute paths correctly.
            let check_path = path.strip_prefix(&self.project_root).unwrap_or(path);

            // Check if the path itself is ignored
            let matched = gitignore.matched(check_path, false);
            if matched.is_ignore() {
                return false;
            }

            // Also check if any parent directory is ignored
            // This handles cases like "tmp/" where we need to check the parent
            if let Some(parent) = check_path.parent() {
                let parent_matched = gitignore.matched(parent, true);
                if parent_matched.is_ignore() {
                    return false;
                }
            }
        }

        // 4. Skip hidden files (starting with .) EXCEPT .gitignore and .env files.
        // Skipped entirely when settings.force_watch_gitignore is true, so
        // dotfiles like .env.local are tracked.
        if !self.settings.force_watch_gitignore {
            if let Some(filename) = path.file_name() {
                if let Some(name_str) = filename.to_str() {
                    if name_str.starts_with('.') {
                        // Allow .gitignore and .env files
                        if name_str == ".gitignore" || name_str == ".env" {
                            return true;
                        }
                        return false;
                    }
                }
            }
        }

        // Path passes all filters
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    /// Builds a `WatchSettings` with just `force_watch_gitignore` set,
    /// `unignored_dirs` empty. Keeps existing test call sites terse.
    fn settings(force_watch_gitignore: bool) -> WatchSettings {
        WatchSettings {
            force_watch_gitignore,
            ..Default::default()
        }
    }

    /// Builds a `WatchSettings` with the given directory names un-ignored,
    /// `force_watch_gitignore` off.
    fn settings_with_unignored(dirs: &[&str]) -> WatchSettings {
        WatchSettings {
            unignored_dirs: dirs.iter().map(|s| s.to_string()).collect(),
            ..Default::default()
        }
    }

    /// Helper to create a test filter without .gitignore
    fn filter_without_gitignore() -> Filter {
        let temp = TempDir::new().expect("create temp dir");
        Filter::new(temp.path(), settings(false)).expect("create filter")
    }

    /// Helper to create a test filter with a .gitignore file
    fn filter_with_gitignore(
        gitignore_content: &str,
        force_watch_gitignore: bool,
    ) -> (Filter, TempDir) {
        let temp = TempDir::new().expect("create temp dir");
        let gitignore_path = temp.path().join(".gitignore");
        fs::write(&gitignore_path, gitignore_content).expect("write .gitignore");
        let filter =
            Filter::new(temp.path(), settings(force_watch_gitignore)).expect("create filter");
        (filter, temp)
    }

    #[test]
    fn filter_creation_without_gitignore() {
        let temp = TempDir::new().expect("create temp dir");
        let filter = Filter::new(temp.path(), settings(false));
        assert!(filter.is_ok());
        assert!(filter.unwrap().gitignore.is_none());
    }

    #[test]
    fn filter_creation_with_gitignore() {
        let temp = TempDir::new().expect("create temp dir");
        let gitignore_path = temp.path().join(".gitignore");
        fs::write(&gitignore_path, "*.log\n").expect("write .gitignore");
        let filter = Filter::new(temp.path(), settings(false));
        assert!(filter.is_ok());
        assert!(filter.unwrap().gitignore.is_some());
    }

    #[test]
    fn ignored_directories_in_path() {
        let filter = filter_without_gitignore();
        assert!(!filter.should_track(Path::new("/project/node_modules/file.js")));
        assert!(!filter.should_track(Path::new("/project/.git/config")));
        assert!(!filter.should_track(Path::new("/project/target/debug/binary")));
        assert!(!filter.should_track(Path::new("/project/__pycache__/cache.pyc")));
    }

    #[test]
    fn ignored_extensions() {
        let filter = filter_without_gitignore();
        let temp = TempDir::new().expect("create temp dir");

        // Create test files
        let png_path = temp.path().join("image.png");
        let exe_path = temp.path().join("binary.exe");
        let sqlite_path = temp.path().join("data.sqlite");

        fs::write(&png_path, b"fake png").expect("write png");
        fs::write(&exe_path, b"fake exe").expect("write exe");
        fs::write(&sqlite_path, b"fake db").expect("write sqlite");

        assert!(!filter.should_track(&png_path));
        assert!(!filter.should_track(&exe_path));
        assert!(!filter.should_track(&sqlite_path));
    }

    #[test]
    fn case_insensitive_extension_matching() {
        let filter = filter_without_gitignore();
        let temp = TempDir::new().expect("create temp dir");

        let upper_png = temp.path().join("IMAGE.PNG");
        let mixed_jpg = temp.path().join("photo.JpG");

        fs::write(&upper_png, b"fake").expect("write upper png");
        fs::write(&mixed_jpg, b"fake").expect("write mixed jpg");

        assert!(!filter.should_track(&upper_png));
        assert!(!filter.should_track(&mixed_jpg));
    }

    #[test]
    fn unfudged_storage_is_centralized() {
        // With centralized storage, .unfudged no longer exists in the project dir.
        // The filter doesn't need to exclude it from IGNORED_DIRS anymore.
        let filter = filter_without_gitignore();
        // .git is still ignored
        assert!(!filter.should_track(Path::new("/project/.git/config")));
        // node_modules is still ignored
        assert!(!filter.should_track(Path::new("/project/node_modules/pkg.json")));
    }

    #[test]
    fn normal_text_files_pass_through() {
        let filter = filter_without_gitignore();
        let temp = TempDir::new().expect("create temp dir");

        let rs_file = temp.path().join("main.rs");
        let js_file = temp.path().join("app.js");
        let txt_file = temp.path().join("notes.txt");
        let md_file = temp.path().join("README.md");

        fs::write(&rs_file, b"fn main() {}").expect("write rs");
        fs::write(&js_file, b"console.log()").expect("write js");
        fs::write(&txt_file, b"notes").expect("write txt");
        fs::write(&md_file, b"# Title").expect("write md");

        assert!(filter.should_track(&rs_file));
        assert!(filter.should_track(&js_file));
        assert!(filter.should_track(&txt_file));
        assert!(filter.should_track(&md_file));
    }

    #[test]
    fn hidden_files_filtered_except_env() {
        let filter = filter_without_gitignore();
        let temp = TempDir::new().expect("create temp dir");

        let hidden = temp.path().join(".hidden");
        let env_file = temp.path().join(".env");
        let gitignore = temp.path().join(".gitignore");

        fs::write(&hidden, b"secret").expect("write hidden");
        fs::write(&env_file, b"KEY=value").expect("write env");
        fs::write(&gitignore, b"*.log").expect("write gitignore");

        assert!(!filter.should_track(&hidden));
        assert!(filter.should_track(&env_file));
        assert!(filter.should_track(&gitignore));
    }

    #[test]
    fn gitignore_patterns_respected() {
        let (filter, temp) = filter_with_gitignore("*.log\ntmp/\nsecret.txt\n", false);

        let log_file = temp.path().join("debug.log");
        let secret = temp.path().join("secret.txt");
        let normal = temp.path().join("code.rs");

        fs::write(&log_file, b"logs").expect("write log");
        fs::write(&secret, b"secret").expect("write secret");
        fs::write(&normal, b"code").expect("write normal");

        assert!(!filter.should_track(&log_file));
        assert!(!filter.should_track(&secret));
        assert!(filter.should_track(&normal));
    }

    #[test]
    fn gitignore_with_negation_patterns() {
        let (filter, temp) = filter_with_gitignore("*.log\n!important.log\n", false);

        let normal_log = temp.path().join("debug.log");
        let important_log = temp.path().join("important.log");

        fs::write(&normal_log, b"logs").expect("write normal log");
        fs::write(&important_log, b"important").expect("write important log");

        assert!(!filter.should_track(&normal_log));
        assert!(filter.should_track(&important_log));
    }

    #[test]
    fn gitignore_directory_patterns() {
        let (filter, temp) = filter_with_gitignore("tmp/\n", false);

        let tmp_dir = temp.path().join("tmp");
        fs::create_dir(&tmp_dir).expect("create tmp dir");
        let tmp_file = tmp_dir.join("file.txt");
        fs::write(&tmp_file, b"data").expect("write file");

        // The file should be ignored because it's in tmp/
        assert!(!filter.should_track(&tmp_file));
    }

    #[test]
    fn forced_filter_tracks_gitignored_files() {
        let (filter, temp) = filter_with_gitignore("*.log\ntmp/\n", true);

        let log_file = temp.path().join("debug.log");
        fs::write(&log_file, b"logs").expect("write log");

        let tmp_dir = temp.path().join("tmp");
        fs::create_dir(&tmp_dir).expect("create tmp dir");
        let tmp_file = tmp_dir.join("file.txt");
        fs::write(&tmp_file, b"data").expect("write file");

        assert!(filter.should_track(&log_file));
        assert!(filter.should_track(&tmp_file));
    }

    #[test]
    fn forced_filter_tracks_hidden_files() {
        let temp = TempDir::new().expect("create temp dir");
        let filter = Filter::new(temp.path(), settings(true)).expect("create filter");

        let hidden = temp.path().join(".hidden");
        let envrc = temp.path().join(".envrc");
        fs::write(&hidden, b"secret").expect("write hidden");
        fs::write(&envrc, b"export FOO=bar").expect("write envrc");

        assert!(filter.should_track(&hidden));
        assert!(filter.should_track(&envrc));
    }

    #[test]
    fn forced_filter_still_skips_ignored_dirs() {
        let temp = TempDir::new().expect("create temp dir");
        let filter = Filter::new(temp.path(), settings(true)).expect("create filter");

        assert!(!filter.should_track(Path::new("/project/node_modules/pkg.json")));
        assert!(!filter.should_track(Path::new("/project/target/debug/x")));
        assert!(!filter.should_track(Path::new("/project/.git/config")));
    }

    #[test]
    fn forced_filter_still_skips_ignored_extensions() {
        let temp = TempDir::new().expect("create temp dir");
        let filter = Filter::new(temp.path(), settings(true)).expect("create filter");
        let png = temp.path().join("logo.png");
        fs::write(&png, b"fake png").expect("write png");

        assert!(!filter.should_track(&png));
    }

    #[test]
    fn forced_filter_does_not_load_gitignore() {
        let temp = TempDir::new().expect("create temp dir");
        let gitignore_path = temp.path().join(".gitignore");
        fs::write(&gitignore_path, "*.log\n").expect("write .gitignore");

        let filter = Filter::new(temp.path(), settings(true)).expect("create filter");
        assert!(filter.gitignore.is_none());
    }

    #[test]
    fn forced_filter_tolerates_malformed_gitignore() {
        // A directory path used as a .gitignore "file" fails to parse in
        // non-forced mode, but forced mode never reads it at all.
        let temp = TempDir::new().expect("create temp dir");
        let gitignore_path = temp.path().join(".gitignore");
        fs::create_dir(&gitignore_path).expect("create dir named .gitignore");

        assert!(Filter::new(temp.path(), settings(false)).is_err());
        assert!(Filter::new(temp.path(), settings(true)).is_ok());
    }

    #[test]
    fn settings_accessor_reflects_constructed_value() {
        let temp = TempDir::new().expect("create temp dir");
        let off = Filter::new(temp.path(), settings(false)).expect("create filter");
        let on = Filter::new(temp.path(), settings(true)).expect("create filter");

        assert!(!off.settings().force_watch_gitignore);
        assert!(on.settings().force_watch_gitignore);
    }

    #[test]
    fn multiple_filtering_rules_combined() {
        let (filter, temp) = filter_with_gitignore("generated/\n", false);

        // File in node_modules (hardcoded ignore)
        let nm_file = temp.path().join("node_modules").join("pkg.json");

        // PNG file (hardcoded extension ignore)
        let png_file = temp.path().join("image.png");

        // File in gitignored directory
        let gen_dir = temp.path().join("generated");
        fs::create_dir(&gen_dir).expect("create generated dir");
        let gen_file = gen_dir.join("output.txt");
        fs::write(&gen_file, b"generated").expect("write generated");

        // Hidden file
        let hidden = temp.path().join(".hidden");
        fs::write(&hidden, b"hidden").expect("write hidden");

        // Normal file that should pass
        let normal = temp.path().join("code.rs");
        fs::write(&normal, b"code").expect("write normal");

        assert!(!filter.should_track(&nm_file));
        assert!(!filter.should_track(&png_file));
        assert!(!filter.should_track(&gen_file));
        assert!(!filter.should_track(&hidden));
        assert!(filter.should_track(&normal));
    }

    #[test]
    fn new_binary_extensions_ignored() {
        let filter = filter_without_gitignore();
        let temp = TempDir::new().expect("create temp dir");

        // Video
        let mp4 = temp.path().join("video.mp4");
        fs::write(&mp4, b"fake mp4").expect("write mp4");
        assert!(!filter.should_track(&mp4));

        // Audio
        let flac = temp.path().join("song.flac");
        fs::write(&flac, b"fake flac").expect("write flac");
        assert!(!filter.should_track(&flac));

        // Documents
        let pdf = temp.path().join("doc.pdf");
        fs::write(&pdf, b"fake pdf").expect("write pdf");
        assert!(!filter.should_track(&pdf));

        // Fonts
        let woff2 = temp.path().join("font.woff2");
        fs::write(&woff2, b"fake font").expect("write woff2");
        assert!(!filter.should_track(&woff2));

        // Archives
        let zst = temp.path().join("archive.zst");
        fs::write(&zst, b"fake zst").expect("write zst");
        assert!(!filter.should_track(&zst));
    }

    #[test]
    fn rust_build_artifact_extensions_ignored() {
        let filter = filter_without_gitignore();
        let temp = TempDir::new().expect("create temp dir");

        let rlib = temp.path().join("libfoo.rlib");
        let rmeta = temp.path().join("libfoo.rmeta");
        let dep_info = temp.path().join("libfoo.d");

        fs::write(&rlib, b"!<arch>\n").expect("write rlib");
        fs::write(&rmeta, b"fake rmeta").expect("write rmeta");
        fs::write(&dep_info, b"libfoo: src/lib.rs\n").expect("write d");

        assert!(!filter.should_track(&rlib));
        assert!(!filter.should_track(&rmeta));
        assert!(!filter.should_track(&dep_info));
    }

    #[test]
    fn unignored_dir_is_tracked() {
        let temp = TempDir::new().expect("create temp dir");
        let filter =
            Filter::new(temp.path(), settings_with_unignored(&["target"])).expect("create filter");

        assert!(filter.should_track(Path::new("/project/target/x.rs")));
    }

    #[test]
    fn unignoring_one_dir_leaves_others_excluded() {
        let temp = TempDir::new().expect("create temp dir");
        let filter =
            Filter::new(temp.path(), settings_with_unignored(&["target"])).expect("create filter");

        assert!(!filter.should_track(Path::new("/project/node_modules/x.js")));
    }

    #[test]
    fn unignore_applies_per_component_not_by_prefix() {
        let temp = TempDir::new().expect("create temp dir");
        let filter =
            Filter::new(temp.path(), settings_with_unignored(&["target"])).expect("create filter");

        // Un-ignoring `target` applies to every `target` component...
        assert!(filter.should_track(Path::new("/p/a/target/b/target/c.rs")));
        // ...but does not lift `node_modules`, even nested under `target`.
        assert!(!filter.should_track(Path::new("/p/node_modules/target/x.rs")));
    }

    #[test]
    fn unignored_dir_still_subject_to_extension_rule() {
        let temp = TempDir::new().expect("create temp dir");
        let filter =
            Filter::new(temp.path(), settings_with_unignored(&["target"])).expect("create filter");
        let png = temp.path().join("target").join("x.png");

        assert!(!filter.should_track(&png));
    }

    #[test]
    fn unignored_dir_still_subject_to_rlib_extension_rule() {
        // The Decision 7 regression: un-ignoring `target` does not resurrect
        // the .rlib UD-01 excluded.
        let temp = TempDir::new().expect("create temp dir");
        let filter =
            Filter::new(temp.path(), settings_with_unignored(&["target"])).expect("create filter");
        let rlib = temp.path().join("target").join("libfoo.rlib");

        assert!(!filter.should_track(&rlib));
    }

    #[test]
    fn unignored_dir_still_subject_to_gitignore() {
        // The Decision 7 regression test: un-ignoring `target` alone does
        // nothing when .gitignore also excludes it.
        let temp = TempDir::new().expect("create temp dir");
        fs::write(temp.path().join(".gitignore"), "/target\n").expect("write .gitignore");
        let settings = WatchSettings {
            unignored_dirs: ["target".to_string()].into_iter().collect(),
            ..Default::default()
        };
        let filter = Filter::new(temp.path(), settings).expect("create filter");
        let file = temp.path().join("target").join("notes.rs");

        assert!(!filter.should_track(&file));
    }

    #[test]
    fn unignore_plus_force_records_gitignored_dir() {
        let temp = TempDir::new().expect("create temp dir");
        fs::write(temp.path().join(".gitignore"), "/target\n").expect("write .gitignore");
        let settings = WatchSettings {
            force_watch_gitignore: true,
            unignored_dirs: ["target".to_string()].into_iter().collect(),
        };
        let filter = Filter::new(temp.path(), settings).expect("create filter");
        let file = temp.path().join("target").join("notes.rs");

        assert!(filter.should_track(&file));
    }

    #[test]
    fn git_dir_refused_even_when_unignored() {
        // The Decision 8 defence-in-depth test: construct WatchSettings
        // directly, bypassing CLI validation, to prove should_track itself
        // refuses .git.
        let temp = TempDir::new().expect("create temp dir");
        let settings = WatchSettings {
            unignored_dirs: [".git".to_string()].into_iter().collect(),
            ..Default::default()
        };
        let filter = Filter::new(temp.path(), settings).expect("create filter");

        assert!(!filter.should_track(Path::new("/project/.git/config")));
        assert!(!filter.should_track(Path::new("/project/.git/objects/ab/cd")));
    }

    #[test]
    fn svg_is_tracked() {
        let filter = filter_without_gitignore();
        let temp = TempDir::new().expect("create temp dir");
        let svg = temp.path().join("icon.svg");
        fs::write(&svg, b"<svg></svg>").expect("write svg");
        assert!(filter.should_track(&svg));
    }

    #[test]
    fn is_likely_binary_png() {
        assert!(is_likely_binary(b"\x89PNG\r\n\x1a\nsome data"));
    }

    #[test]
    fn is_likely_binary_jpeg() {
        assert!(is_likely_binary(b"\xFF\xD8\xFFsome jpeg data"));
    }

    #[test]
    fn is_likely_binary_gif() {
        assert!(is_likely_binary(b"GIF89asome gif data"));
    }

    #[test]
    fn is_likely_binary_zip() {
        assert!(is_likely_binary(b"PK\x03\x04some zip data"));
    }

    #[test]
    fn is_likely_binary_elf() {
        assert!(is_likely_binary(b"\x7FELFsome elf data"));
    }

    #[test]
    fn is_likely_binary_pdf() {
        assert!(is_likely_binary(b"%PDF-1.4 some pdf"));
    }

    #[test]
    fn is_likely_binary_sqlite() {
        assert!(is_likely_binary(b"SQLite format 3\x00"));
    }

    #[test]
    fn is_likely_binary_pe_exe() {
        assert!(is_likely_binary(b"MZ\x90\x00some exe data"));
    }

    #[test]
    fn is_likely_binary_nul_bytes() {
        assert!(is_likely_binary(b"text\x00more data"));
    }

    #[test]
    fn is_likely_binary_plain_text() {
        assert!(!is_likely_binary(b"Hello, this is plain text\n"));
    }

    #[test]
    fn is_likely_binary_utf8_text() {
        assert!(!is_likely_binary("Héllo wörld! 日本語".as_bytes()));
    }

    #[test]
    fn is_likely_binary_empty() {
        assert!(!is_likely_binary(b""));
    }

    #[test]
    fn is_likely_binary_gzip() {
        assert!(is_likely_binary(b"\x1F\x8Bsome gzip data"));
    }

    #[test]
    fn is_likely_binary_flac() {
        assert!(is_likely_binary(b"fLaCsome flac data"));
    }

    #[test]
    fn is_likely_binary_zlib_headers() {
        assert!(is_likely_binary(
            b"\x78\x01rest of a low-compression stream"
        ));
        assert!(is_likely_binary(
            b"\x78\x9Crest of a default-compression stream"
        ));
        assert!(is_likely_binary(
            b"\x78\xDArest of a best-compression stream"
        ));
    }

    #[test]
    fn is_likely_binary_real_git_loose_object() {
        // A genuine loose git object, not a hand-written stub. Captured by
        // running, from a scratch repo:
        //   printf 'unfudged flight recorder test payload for zlib
        //   magic-number detection\n' | git hash-object -w --stdin
        // then reading the raw bytes of the resulting
        // .git/objects/3b/3ddb13fc78dd699d0436ca8d2dcefd9f5c6770 file
        // (git 2.50.1, Apple Git-155). `git cat-file -p` on that hash
        // confirms it round-trips to the payload above. The stream opens
        // with the 78 01 (no/low compression) zlib header this ticket adds
        // to MAGIC_NUMBERS.
        const REAL_GIT_LOOSE_OBJECT: &[u8] = b"\x78\x01\x0d\xca\x31\x0e\x80\x20\x0c\x00\x40\x67\x5f\xd1\x0f\x90\xb8\xf9\x1e\x4a\x0b\x36\x29\xd4\xd4\x32\xe8\xeb\x65\xbe\x43\x35\x84\xf3\xd8\xe6\xa8\x93\x1a\x13\x54\x95\x76\x05\x38\x17\x73\x62\x87\xe0\x27\xe0\xce\xaf\x5a\x5e\x68\x0e\x9f\x0a\x42\xcf\x4d\x4a\x1a\xb3\xe3\x2a\xc4\xc1\x25\xc4\xc6\xfe\x03\x54\xe8\x1c\x6f";

        assert!(is_likely_binary(REAL_GIT_LOOSE_OBJECT));
    }

    #[test]
    fn rlib_header_not_caught_by_is_likely_binary() {
        // Documents WHY the extension rule exists: a real .rlib starts with
        // this ASCII `!<arch>\n` archive header, has no NUL in its first 16
        // bytes, and `!<arch>` is not in MAGIC_NUMBERS. Magic-number
        // detection alone would treat rlibs as text.
        assert!(!is_likely_binary(b"!<arch>\n1234567890`\n"));
    }

    // -- parse_unignore_dir --------------------------------------------

    #[test]
    fn parse_unignore_dir_rejects_git() {
        let err = parse_unignore_dir(".git").unwrap_err();
        assert!(
            err.contains("can never be recorded"),
            "message should give a reason, not a bare refusal: {err}"
        );
        assert!(err.contains("object store"));
        assert!(err.contains("Eligible directories:"));
    }

    #[test]
    fn parse_unignore_dir_rejects_git_regardless_of_case() {
        // The reason .git is refused (git rewrites its own object store)
        // holds regardless of spelling, so a case variant gets the same
        // refusal message, not a "did you mean '.git'" suggestion that
        // would wrongly imply fixing the case helps.
        let err = parse_unignore_dir(".GIT").unwrap_err();
        assert!(err.contains("can never be recorded"));
    }

    #[test]
    fn parse_unignore_dir_suggests_case_correction() {
        let err = parse_unignore_dir("Target").unwrap_err();
        assert!(
            err.contains("Did you mean 'target'"),
            "expected a lowercase suggestion, got: {err}"
        );
        assert!(err.contains("case-sensitive"));
    }

    #[test]
    fn parse_unignore_dir_rejects_unknown_name() {
        let err = parse_unignore_dir("logs").unwrap_err();
        assert!(err.contains("'logs' is not on UNF's excluded list"));
        assert!(
            err.contains("--force-watch-gitignore"),
            "unknown names should route to the likely real fix: {err}"
        );
        assert!(err.contains("Eligible directories:"));
    }

    #[test]
    fn parse_unignore_dir_accepts_every_eligible_dir() {
        for &dir in IGNORED_DIRS.iter().filter(|&&d| d != PERMANENT_IGNORED_DIR) {
            assert_eq!(parse_unignore_dir(dir), Ok(dir.to_string()));
        }
    }

    // -- gitignore_shadows -----------------------------------------------

    #[test]
    fn gitignore_shadows_matches_rule_three() {
        let (filter, _temp) = filter_with_gitignore("/target\n", false);
        assert!(filter.gitignore_shadows("target"));
        assert!(!filter.gitignore_shadows("dist"));
    }

    #[test]
    fn gitignore_shadows_false_when_forced() {
        let (filter, _temp) = filter_with_gitignore("/target\n", true);
        assert!(!filter.gitignore_shadows("target"));
    }

    #[test]
    fn gitignore_shadows_false_when_no_gitignore_loaded() {
        let filter = filter_without_gitignore();
        assert!(!filter.gitignore_shadows("target"));
    }

    // -- eligible_unignore_dirs -------------------------------------------

    #[test]
    fn eligible_unignore_dirs_finds_top_level_only() {
        let temp = TempDir::new().expect("create temp dir");
        fs::create_dir(temp.path().join("target")).expect("create target");
        fs::create_dir(temp.path().join("node_modules")).expect("create node_modules");
        fs::create_dir(temp.path().join("src")).expect("create src");
        fs::create_dir_all(temp.path().join("packages/web/node_modules"))
            .expect("create nested node_modules");

        let result = eligible_unignore_dirs(temp.path());

        assert_eq!(result, vec!["node_modules", "target"]);
    }

    #[test]
    fn eligible_unignore_dirs_omits_git() {
        let temp = TempDir::new().expect("create temp dir");
        fs::create_dir(temp.path().join(".git")).expect("create .git");
        fs::create_dir(temp.path().join("target")).expect("create target");

        let result = eligible_unignore_dirs(temp.path());

        assert!(!result.contains(&".git"));
        assert_eq!(result, vec!["target"]);
    }

    #[test]
    fn eligible_unignore_dirs_ignores_files() {
        let temp = TempDir::new().expect("create temp dir");
        // A *file* named "build" must not be reported as eligible.
        fs::write(temp.path().join("build"), b"not a directory").expect("write build file");

        let result = eligible_unignore_dirs(temp.path());

        assert!(result.is_empty());
    }

    #[test]
    fn eligible_unignore_dirs_does_not_error_on_unreadable_root() {
        let temp = TempDir::new().expect("create temp dir");
        let missing = temp.path().join("does-not-exist");

        let result = eligible_unignore_dirs(&missing);

        assert!(result.is_empty());
    }
}
