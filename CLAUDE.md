# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Vision

**UNFUDGED** (UNF\*) is a high-resolution "Flight Recorder" for the developer's filesystem. It captures every text-based file change in real-time as a continuous safety net. Whether it's a human error, a botched refactor, or an AI agent mass-overwriting files, UNF\* lets you rewind to a state of integrity in seconds.

**Key value props:**
- Zero-Commit Workflow: If you saved it, UNF\* has it
- AI Panic Room: Hardware-level Undo for autonomous agents
- State Insurance: Recover "ghost files" never tracked by VCS
- High-Resolution Diffs: Minute-by-minute file evolution

## Architecture

Client-Daemon model in Rust:
- **Watcher** (daemon): FSEvents/inotify/ReadDirectoryChangesW with 3-second smart debounce, text-only filtering via magic-number binary detection
- **Engine** (storage): Content-Addressable Storage using BLAKE3 hashing, SQLite metadata (ACID), flat-file object store
- **CLI** (`unf`): Time-and-state oriented commands (not commit-oriented)
- **AI Safety**: Burst analytics (50+ files in <1s triggers automatic "Pre-Burst" restoration point)
- **Desktop App**: Tauri-based macOS menu bar app (source in `app/`)

Resource targets: <1% CPU, <100MB RAM. Local-first, no data leaves the machine. Append-only immutable logs.

### Retention Decay
- Phase 1 (24h): Every change preserved
- Phase 2 (7d): Thinned to 1 snapshot/hour
- Phase 3 (30d): Thinned to 1 snapshot/day

### CLI Interface (`unf`)
```
unf watch                      # Start watching current directory (register + start daemon)
unf unwatch                    # Stop watching current directory (deregister from daemon)
unf status                     # Watcher status + recent snapshot stats
unf log <file>                 # High-res timeline of file versions
unf diff --at "5m"             # Changes since N minutes ago
unf diff --include "*.rs"      # Filter diff to matching files
unf restore --at TIME          # Restore files to a point in time
unf restore --at TIME <file>   # Restore a specific file
unf cat --at TIME <file>       # Print file contents at a point in time
unf list --at TIME             # List tracked files at a point in time
unf prune --before TIME        # Remove snapshots older than a threshold
unf stop                       # Stop the global daemon
unf restart                    # Restart the global daemon
```

Time formats: `"5m"`, `"2h"`, `"1d"`, `"2025-06-15 14:30:00"`, or any `humantime` duration.

## Development

### Build & Test
```bash
cargo build                    # Build debug
cargo build --release          # Build release
cargo test                     # Run all tests
cargo test <test_name>         # Run single test
cargo test -- --nocapture      # Run tests with stdout
cargo clippy -- -D warnings    # Lint (zero warnings policy)
cargo fmt                      # Format
cargo fmt -- --check           # Check formatting without modifying
just test                      # Run tests + auto-cleanup of leaked test daemons
just kill-test-daemons         # Kill stuck test daemons (NEVER kills production)
```

### Desktop App
```bash
cd app
npm ci --prefix ui
cargo install tauri-cli --locked
cargo tauri dev
```

### Daemon Safety
- NEVER use `pkill -f 'target/debug/unf'` or broad patterns -- this kills the production daemon
- Test daemons use `UNF_HOME` pointed at temp dirs, so they run as `__daemon --root /private/var/folders/...`
- Use `just kill-test-daemons` to safely clean up stuck test processes
- If the production daemon is accidentally killed: `unf restart`

### Pre-Commit Checklist
- `cargo fmt -- --check` passes
- `cargo clippy -- -D warnings` passes (zero warnings)
- `cargo test` passes (100% pass rate, zero tolerance)

### Pre-Push Checklist
- All pre-commit checks pass
- `cargo build --release` succeeds
- Cross-platform concerns documented if applicable

## SUPER Principles (Functional Design)

All code follows SUPER:
- **S**ide Effects at Edge: I/O (filesystem, SQLite, OS events) isolated at boundaries; core logic is pure
- **U**ncoupled Logic: Small, focused, composable functions and modules
- **P**ure & Total Functions: Predictable, handle all inputs, return values (not panics)
- **E**xplicit Data Flow: Clear transformation pipelines, no hidden state
- **R**eplaceable by Value: Referential transparency where possible

### In Practice for Rust
- Side effects (file I/O, SQLite, OS watcher APIs) live in boundary modules, not in core logic
- Core hashing, diffing, retention, and CAS logic are pure functions taking data in, returning data out
- Use `Result<T, E>` everywhere; `unwrap()` is banned outside tests
- Prefer owned types for clarity; use borrows for performance in hot paths
- Newtype wrappers for domain concepts (e.g., `ContentHash(String)`, `SnapshotId(u64)`)

## Coding Standards

### Rust-Specific
- No `unwrap()` outside of tests -- use `?`, `expect()` with context, or proper error handling
- No magic numbers -- use named constants
- No `unsafe` without documented justification and review
- Error types per module using `thiserror`; `anyhow` at binary boundaries only
- Prefer `&str` over `String` in function signatures when ownership isn't needed
- All public items have doc comments

### Project Structure
- Binary entry point in `src/main.rs` (thin: arg parsing + dispatch)
- Library logic in `src/lib.rs` and submodules
- CLI command handlers in `src/cli/`
- Watcher subsystem in `src/watcher/`
- Storage engine in `src/engine/`
- Integration tests in `tests/`
- E2E tests in `tests/e2e/`
- Desktop app in `app/`
- Homebrew formula in `Formula/`, cask in `Casks/`
