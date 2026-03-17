# UNFUDGED

A filesystem flight recorder. Captures every text-based file change in real-time so you can rewind to any saved state in seconds.

Whether you fat-finger a delete, botch a refactor, or an AI agent mass-overwrites your files — `unf` has your back.

## Why

- **Zero-commit workflow** — if you saved it, `unf` has it. No staging, no committing, no branches.
- **AI safety net** — autonomous agents can wreck a codebase in seconds. `unf` gives you a hardware-level undo.
- **Ghost file recovery** — recover files that were never tracked by git.
- **High-resolution diffs** — see exactly what changed, minute by minute.

## Install

### Homebrew (macOS / Linux)

```bash
brew install cyrusradfar/unf/unf
```

### Desktop app (macOS)

```bash
brew install --cask cyrusradfar/unf/unfudged
```

### Build from source

```bash
git clone https://github.com/cyrusradfar/homebrew-unf.git
cd homebrew-unf
cargo build --release
# Binary at target/release/unf
```

## Quick start

```bash
cd ~/my-project
unf watch       # Start recording file changes
# ... work normally ...
unf log src/main.rs          # See every saved version
unf diff --at "5m"           # What changed in the last 5 minutes?
unf restore --at "10m ago"   # Roll back to 10 minutes ago
```

## CLI reference

| Command | Description |
|---------|-------------|
| `unf watch` | Start watching the current directory (registers + starts daemon) |
| `unf unwatch` | Stop watching the current directory |
| `unf status` | Watcher status and recent snapshot stats |
| `unf log <file>` | Timeline of all recorded versions of a file |
| `unf diff --at <time>` | Show changes since a point in time |
| `unf diff --include <glob>` | Filter diff to matching files |
| `unf restore --at <time>` | Restore files to a point in time |
| `unf restore --at <time> <file>` | Restore a specific file |
| `unf cat --at <time> <file>` | Print a file's contents at a point in time |
| `unf list --at <time>` | List tracked files at a point in time |
| `unf prune --before <time>` | Remove snapshots older than a threshold |
| `unf stop` | Stop the global daemon |
| `unf restart` | Restart the global daemon |

Time formats: `"5m"`, `"2h"`, `"1d"`, `"2025-06-15 14:30:00"`, or any `humantime` duration.

## Architecture

```
unf watch
  |
  v
[Daemon] -- FSEvents / inotify / ReadDirectoryChangesW
  |
  v
[Engine] -- BLAKE3 hash --> Content-Addressable Store (flat files)
         -- metadata   --> SQLite (WAL mode, ACID)
```

- **Client-Daemon model** — `unf watch` starts a single global daemon that watches all registered directories. Each CLI command talks to the daemon or reads storage directly.
- **Content-Addressable Storage** — file contents are hashed with BLAKE3 and stored as flat files. Identical content is stored once regardless of how many snapshots reference it.
- **SQLite metadata** — snapshot timestamps, file paths, and content hashes are stored in SQLite with WAL mode for concurrent reads.
- **Smart debounce** — 3-second debounce window batches rapid saves into a single snapshot.
- **Text-only** — binary files are detected by magic number and skipped. Only text-based files are recorded.
- **Retention decay** — snapshots thin out over time: every change for 24h, hourly for 7d, daily for 30d.

Resource targets: <1% CPU, <100MB RAM. Local-first — no data leaves your machine.

## Desktop app

The Tauri-based desktop app (macOS) provides a menu bar interface for quick status checks and controls. Source is in `app/`.

```bash
cd app
npm ci --prefix ui
cargo install tauri-cli --locked
cargo tauri dev
```

## Development

```bash
cargo build                # Debug build
cargo test                 # Run all tests (~400)
cargo clippy -- -D warnings # Lint (zero warnings policy)
cargo fmt -- --check       # Format check
just test                  # Run tests + clean up leaked test daemons
just kill-test-daemons     # Kill stuck test daemons (safe — never touches production)
```

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or <http://www.apache.org/licenses/LICENSE-2.0>)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or <http://opensource.org/licenses/MIT>)

at your option.
