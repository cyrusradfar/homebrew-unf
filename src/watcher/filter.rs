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

use std::path::{Path, PathBuf};

use ignore::gitignore::GitignoreBuilder;

use crate::error::WatcherError;

/// Directories that should never be tracked, regardless of .gitignore rules.
///
/// These are common directories containing build artifacts, dependencies,
/// and internal state that should not be part of the flight recorder.
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
    "pyc", "pyo", "class", "beam", "elc", // Documents (binary)
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
/// - .gitignore pattern matching (if .gitignore is present)
/// - Hidden file filtering (with exceptions for .env files)
pub struct Filter {
    /// Parsed .gitignore rules, if a .gitignore file was found.
    gitignore: Option<ignore::gitignore::Gitignore>,
    /// The root directory this filter was created for.
    project_root: PathBuf,
}

impl Filter {
    /// Create a new filter rooted at the given project directory.
    ///
    /// Automatically loads .gitignore if present. If the .gitignore file
    /// cannot be read or parsed, returns an error. If no .gitignore exists,
    /// the filter will still work using hardcoded rules only.
    ///
    /// # Errors
    ///
    /// Returns [`WatcherError`] if:
    /// - The project root is not a valid directory
    /// - A .gitignore file exists but cannot be parsed
    pub fn new(project_root: &Path) -> Result<Self, WatcherError> {
        let gitignore_path = project_root.join(".gitignore");
        let gitignore = if gitignore_path.exists() {
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
        };

        Ok(Self {
            gitignore,
            project_root: project_root.to_path_buf(),
        })
    }

    /// Returns true if this path should be tracked by the flight recorder.
    ///
    /// **Precondition:** The caller must only pass file paths, not directory paths.
    /// This method assumes `path` is a file and does not check for directories.
    ///
    /// A path is tracked if it passes all of these checks:
    /// 1. No path component matches IGNORED_DIRS
    /// 2. File extension is not in IGNORED_EXTENSIONS
    /// 3. If .gitignore is loaded, the path must not be ignored by it
    /// 4. Hidden files (starting with .) are skipped, except .env and .gitignore files
    pub fn should_track(&self, path: &Path) -> bool {
        // 1. Check if any path component matches IGNORED_DIRS
        for component in path.components() {
            if let Some(comp_str) = component.as_os_str().to_str() {
                if IGNORED_DIRS.contains(&comp_str) {
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
            // Convert to relative path if it starts with project_root
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

        // 4. Skip hidden files (starting with .) EXCEPT .gitignore and .env files
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

        // Path passes all filters
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    /// Helper to create a test filter without .gitignore
    fn filter_without_gitignore() -> Filter {
        let temp = TempDir::new().expect("create temp dir");
        Filter::new(temp.path()).expect("create filter")
    }

    /// Helper to create a test filter with a .gitignore file
    fn filter_with_gitignore(gitignore_content: &str) -> (Filter, TempDir) {
        let temp = TempDir::new().expect("create temp dir");
        let gitignore_path = temp.path().join(".gitignore");
        fs::write(&gitignore_path, gitignore_content).expect("write .gitignore");
        let filter = Filter::new(temp.path()).expect("create filter");
        (filter, temp)
    }

    #[test]
    fn filter_creation_without_gitignore() {
        let temp = TempDir::new().expect("create temp dir");
        let filter = Filter::new(temp.path());
        assert!(filter.is_ok());
        assert!(filter.unwrap().gitignore.is_none());
    }

    #[test]
    fn filter_creation_with_gitignore() {
        let temp = TempDir::new().expect("create temp dir");
        let gitignore_path = temp.path().join(".gitignore");
        fs::write(&gitignore_path, "*.log\n").expect("write .gitignore");
        let filter = Filter::new(temp.path());
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
        let (filter, temp) = filter_with_gitignore("*.log\ntmp/\nsecret.txt\n");

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
        let (filter, temp) = filter_with_gitignore("*.log\n!important.log\n");

        let normal_log = temp.path().join("debug.log");
        let important_log = temp.path().join("important.log");

        fs::write(&normal_log, b"logs").expect("write normal log");
        fs::write(&important_log, b"important").expect("write important log");

        assert!(!filter.should_track(&normal_log));
        assert!(filter.should_track(&important_log));
    }

    #[test]
    fn gitignore_directory_patterns() {
        let (filter, temp) = filter_with_gitignore("tmp/\n");

        let tmp_dir = temp.path().join("tmp");
        fs::create_dir(&tmp_dir).expect("create tmp dir");
        let tmp_file = tmp_dir.join("file.txt");
        fs::write(&tmp_file, b"data").expect("write file");

        // The file should be ignored because it's in tmp/
        assert!(!filter.should_track(&tmp_file));
    }

    #[test]
    fn multiple_filtering_rules_combined() {
        let (filter, temp) = filter_with_gitignore("generated/\n");

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
}
