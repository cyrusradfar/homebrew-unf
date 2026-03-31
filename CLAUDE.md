# CLAUDE.md

Guidance for Claude Code (claude.ai/code) when contributing to this repository.

## What is UNFUDGED?

**UNFUDGED** (UNF\*) is a filesystem flight recorder. It captures every text-based file change in real-time so you can rewind to any saved state in seconds.

**Use cases:**
- Zero-commit recovery: recover from mistakes without git overhead
- AI safety: hardware-level undo for agent-caused chaos
- Ghost file recovery: recover files never tracked by git
- Minute-by-minute diffs: see exactly what changed

## Architecture

Client-Daemon model in Rust:
- **Daemon** (`src/daemon/`): OS-native watcher (FSEvents/inotify/ReadDirectoryChangesW). 3-second debounce. Magic-number binary detection.
- **Engine** (`src/engine/`): Content-Addressable Storage with BLAKE3, SQLite metadata, flat-file objects.
- **CLI** (`src/cli/`): Time-oriented commands (not commit-oriented).
- **Desktop App** (`app/`): Tauri-based macOS menu bar app.

Resource targets: <1% CPU, <100MB RAM. Local-first, no data leaves the machine.

### Retention Decay
- Phase 1 (24h): Every change preserved
- Phase 2 (7d): Thinned to 1 snapshot/hour
- Phase 3 (30d): Thinned to 1 snapshot/day

### CLI Commands
- `unf watch` — Start watching (register + start daemon)
- `unf unwatch` — Stop watching
- `unf status` — Watcher status and snapshot stats
- `unf log <file>` — Timeline of file versions
- `unf diff --at "5m"` — Changes in last 5 minutes
- `unf restore --at TIME` — Restore to a point in time
- `unf cat --at TIME <file>` — Print file contents at TIME
- `unf list -v` — List tracked files (verbose mode)
- `unf prune --older-than TIME` — Remove old snapshots
- `unf stop`, `unf restart` — Daemon control

Time format: `"5m"`, `"2h"`, `"1d"`, or RFC3339 timestamp.

## Development

### Build & Test
```bash
cargo build --release          # Release build
cargo test                     # Run all tests (~490)
cargo clippy -- -D warnings    # Zero warnings required
cargo fmt -- --check           # Format check
just test                      # Tests + cleanup leaked daemons
just kill-test-daemons         # Kill stuck test processes
```

### Desktop App
```bash
cd app && npm ci --prefix ui
cargo install tauri-cli --locked
cargo tauri dev
```

### Daemon Safety
Never use broad `pkill -f 'target/debug/unf'` — this kills the production daemon. Test daemons use `UNF_HOME` for isolation. Use `just kill-test-daemons` to clean up test processes. If the production daemon is killed, run `unf restart`.

### Code Submission
- `cargo fmt -- --check` ✓
- `cargo clippy -- -D warnings` ✓ (zero warnings)
- `cargo test` ✓ (100% pass rate)
- Update `CHANGELOG.md` under `Unreleased`

## Design Principles

**SUPER**: Side effects at edge, Uncoupled logic, Pure functions, Explicit data flow, Replaceable by value.

In practice:
- Side effects (I/O, SQLite, OS APIs) at boundaries; core logic pure
- Small, focused, composable functions
- `Result<T, E>` everywhere; no `unwrap()` outside tests
- Newtype wrappers for domain concepts (`ContentHash`, `SnapshotId`)

## Coding Standards

**Rust:**
- No `unwrap()` outside tests — use `?` or `expect()`
- Named constants, not magic numbers
- No `unsafe` without justification
- `Result<T, E>` per module using `thiserror`
- Prefer `&str` over `String` in signatures
- Doc comments on public items

**Structure:**
- `src/main.rs` — thin entry point (arg parsing, dispatch)
- `src/lib.rs` — library logic and submodules
- `src/cli/` — command handlers
- `src/daemon/` — OS watcher implementations
- `src/engine/` — storage engine
- `tests/` — integration and E2E tests
- `app/` — Tauri desktop app
- `Formula/`, `Casks/` — Homebrew definitions
