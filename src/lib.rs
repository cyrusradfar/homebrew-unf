//! UNFUDGED -- a high-resolution filesystem flight recorder.
//!
//! This crate provides the core library for the `unf` CLI tool. It captures
//! every text-based file change in real-time, storing content in a
//! content-addressable store with SQLite metadata, so you can rewind to any
//! point in time.

pub mod audit;
pub mod autostart;
pub mod cli;
pub mod diff;
pub mod engine;
pub mod error;
pub mod intent;
pub mod process;
pub mod registry;
pub mod sentinel;
pub mod storage;
pub mod types;
pub mod watcher;

/// Test utilities shared across modules.
///
/// All tests that set `UNF_HOME` or `HOME` env vars MUST use this shared
/// mutex to prevent concurrent interference in the same test process.
#[cfg(test)]
pub mod test_util {
    use std::sync::Mutex;

    /// Global mutex for serializing tests that modify `HOME` or `UNF_HOME` env vars.
    ///
    /// Since `global_dir()` checks `UNF_HOME` first, any test setting either env var
    /// can interfere with tests in other modules. All such tests must hold this lock.
    pub static ENV_LOCK: Mutex<()> = Mutex::new(());
}
