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
curl -fsSLO https://downloads.unfudged.io/releases/v0.21.0/unf_0.21.0_amd64.deb
sudo dpkg -i unf_0.21.0_amd64.deb
```

For ARM64:
```bash
curl -fsSLO https://downloads.unfudged.io/releases/v0.21.0/unf_0.21.0_arm64.deb
sudo dpkg -i unf_0.21.0_arm64.deb
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
| `unf watch --force-watch-gitignore` | Also record gitignored files and hidden dotfiles |
| `unf watch --unignore-dir <NAME>` | Also record an excluded directory, such as `target`, or a git path, such as `.git/hooks` (repeatable) |
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
- **Respects `.gitignore`** — Files your `.gitignore` excludes are not recorded, and hidden dotfiles are skipped. Run `unf watch --force-watch-gitignore` to record them anyway. The setting stays on for that project until you run plain `unf watch` again. Warning: secrets in ignored files, such as `.env.local`, go into the recording.
- **Excluded directories** — UNF skips `.git`, `node_modules`, `target`, `.next`, `__pycache__`, `.venv`, `venv`, `.tox`, `dist`, and `build` in every project. These directories are large and they change on every build. Run `unf watch --unignore-dir target` to record one of them in a project; repeat the flag for more. UNF skips `.git` as a whole directory, and the flag refuses that bare name. Recording git's object store while git rewrites it could damage the repository.
- **Git paths** — Git does not track the files inside `.git`, so git cannot recover them. UNF records three of them on request: `unf watch --unignore-dir .git/hooks`, `--unignore-dir .git/config`, and `--unignore-dir .git/info/exclude`. Use this when an agent writes a hook or changes a remote. Every other path inside `.git` stays refused, and the flag gives you the reason. `.git/objects` holds gigabytes that git rewrites constantly. A restore of a stale `.git/refs` entry would point the repository at the wrong commit. `.git/index`, `.git/logs`, and `.git/modules` are refused too. UNF refuses them twice: once in the flag, and again in the recorder, so an edited settings file cannot record the object store.
  - **Cost is low.** This is not like `target`. A new repository holds 14 hooks, all of them inert `.sample` files that git never changes. UNF records them once, then records a hook only when you or an agent edits it. `.git/config` changes when a remote or a setting changes, which is rare.
  - **One flag is enough.** `.gitignore` never applies inside `.git`, so a git path does not need `--force-watch-gitignore`.
  - **Submodules are not covered.** A submodule keeps its `.git` as a file, not as a directory. That file points into the parent `.git/modules` directory, and UNF refuses that path. So `--unignore-dir .git/hooks` records the hooks of the current repository only.
  - **Restore care.** `unf restore` can put back an old `.git/config` and revert a remote URL. UNF always takes a safety snapshot before it restores, so you can get the current file back.
- **Both flags together** — Most projects also list these directories in `.gitignore`, so two rules exclude the same directory. `--unignore-dir` lifts UNF's built-in rule only. For `target` in a typical Rust project, run `unf watch --unignore-dir target --force-watch-gitignore`. With one flag alone, the directory stays unrecorded. `unf watch` warns you when this happens. Expect the store to grow: `unf status` shows the size, and `unf prune --older-than 7d` frees space.
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
