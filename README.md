# UNFUDGED

A filesystem flight recorder. Captures every text-based file change in real-time.

**Recover from any mistake in seconds:** fat-finger deletes, botched refactors, AI agent chaos. If you saved it, `unf` has it.

- **Zero-commit workflow** — if you saved it, `unf` has it. No git overhead.
- **Hardware-level undo** — recover from autonomous agent disasters in seconds.
- **Ghost file recovery** — recover files never tracked by git.
- **Minute-by-minute diffs** — see exactly what changed.

## Install

### Homebrew (macOS & Linux)

```bash
brew install cyrusradfar/unf/unf
```

### Debian / Ubuntu

```bash
curl -fsSLO https://downloads.unfudged.io/releases/v0.18.4/unf_0.18.4_amd64.deb
sudo dpkg -i unf_0.18.4_amd64.deb
```

For ARM64:
```bash
curl -fsSLO https://downloads.unfudged.io/releases/v0.18.4/unf_0.18.4_arm64.deb
sudo dpkg -i unf_0.18.4_arm64.deb
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
unf restore --at 10m         # Roll back to 10 minutes ago
```

## CLI reference

| Command | Description |
|---------|-------------|
| `unf watch` | Start watching the current directory (registers + starts daemon) |
| `unf unwatch` | Stop watching the current directory |
| `unf status` | Watcher status and recent snapshot stats |
| `unf log <file>` | Timeline of all recorded versions of a file |
| `unf log --since <time> --until <time>` | Filter log to a time range |
| `unf diff --at <time>` | Show changes since a point in time |
| `unf log --include <glob>` | Filter log to matching files |
| `unf restore --at <time>` | Restore files to a point in time |
| `unf restore --at <time> <file>` | Restore a specific file |
| `unf cat --at <time> <file>` | Print a file's contents at a point in time |
| `unf list` | List all watched projects |
| `unf prune --older-than <time>` | Remove snapshots older than a threshold |
| `unf config` | Show storage location and disk usage |
| `unf config --move-storage <path>` | Move storage to a new location |
| `unf recap` | Summarize recent session activity |
| `unf stop` | Stop the global daemon |
| `unf restart` | Restart the global daemon |

Time formats: `5m`, `2h`, `1d`, or ISO 8601 (`2026-02-09T20:17:00Z`).

## How it works

- **Daemon model** — `unf watch` starts a global daemon that watches all registered directories using OS-native APIs (FSEvents/inotify/ReadDirectoryChangesW).
- **Content-Addressable Storage** — Files are hashed with BLAKE3. Identical content is stored once; snapshots reference it by hash.
- **SQLite metadata** — Timestamps, paths, and hashes in SQLite with WAL mode for concurrent access.
- **Smart batching** — 3-second debounce window prevents rapid saves from bloating storage.
- **Text-only** — Binary files are detected and skipped. Only text snapshots are kept.
- **Manual pruning** — `unf prune --older-than 30d` to reclaim space. Automatic retention decay is planned.

Resource targets: <1% CPU, <100MB RAM. Local-first, zero data leaves the machine.

## Desktop app development

The macOS app is Tauri-based. To build locally:

```bash
cd app && npm ci --prefix ui
cargo install tauri-cli --locked
cargo tauri dev
```

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) for build, test, and submission guidelines.

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or <http://www.apache.org/licenses/LICENSE-2.0>)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or <http://opensource.org/licenses/MIT>)

at your option.
