# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/),
and this project adheres to [Semantic Versioning](https://semver.org/).

## [Unreleased]

## [0.18.1] - 2026-03-31
### Fixed
- `unf restart` no longer leaves a duplicate sentinel running
- `unf config` now reports the correct project count (was undercounting)
- `unf status` in an unwatched directory no longer says "stopped unexpectedly." Three cases now: never watched, previously watched but inactive, and actively watching

## [0.18.0] - 2026-03-31
### Changed
- Desktop app: "All" tab is now permanent (can't be closed). New "+" button to add project tabs
- Internal: split large modules (`log`, `db`) into smaller files. No behavior change

### Fixed
- Corrected `--include` flag reference in README (belongs to `log`, not `diff`)
- Fixed invalid time format in website demo (`14:32:07` is not a valid `--at` value)
- Website no longer claims every mutating command supports `--dry-run` (only `restore` and `prune` do)
- Stale Debian install URLs in README

### Added
- CI workflow on GitHub Actions

## [0.17.16] - 2026-03-28
### Fixed
- Sentinel detects and respawns zombie daemons (crashed processes that `kill(pid,0)` couldn't distinguish from alive)
- `flock` guard prevents multiple sentinels from accumulating
- `spawn_daemon()` retains `Child` handle to prevent zombie creation

### Added
- Data freshness check: sentinel verifies snapshots are actively being recorded, restarts daemon on first staleness
- E2E tests for zombie detection, flock, and stop-no-loop (Linux + macOS)

## [0.17.11] - 2026-03-18
### Fixed
- Histogram time range: drag-to-create was immediately cleared by $effect (session selection unaffected)
### Added
- Range selection state machine extracted to `rangeSelection.ts` with 21 unit tests (vitest)
- Resource leak tests refined (removed flaky health check from SQLite FD test)

## [0.17.9] - 2026-03-18
### Added
- Linux ARM64 (aarch64-unknown-linux-gnu) build target and .deb package
- CHANGELOG.md, CONTRIBUTING.md
- Dual MIT/Apache-2.0 license (matching ripgrep/bat/fd)
### Changed
- Homebrew formula supports ARM64 Linux
- All docs polished for OSS readiness

## [0.17.8] - 2026-03-17
### Added
- .deb package in staging and release pipelines
- Public CLAUDE.md for contributors
- Download analytics tooling (CloudFront logs)

### Changed
- Source-available release on homebrew-unf
- Resource leak tests (FD, RSS, SQLite cleanup)
- Website: Google Analytics, OG image, clean URLs, mobile nav

## [0.17.7] - 2026-02-25
### Changed
- Website: SFW/NSFW toggle with neumorphic UI, cross-tab sync

## [0.17.6] - 2026-02-19
### Changed
- Website: hero animation polish, two-pane app mockup redesign

## [0.17.5] - 2026-02-19
### Fixed
- Commands now work from subdirectories of watched projects (ancestor walk)
- Desktop app resolves `unf` binary path for GUI launch

## [0.17.4] - 2026-02-18
### Fixed
- Desktop app: resolve `unf` binary path (macOS GUI apps don't inherit shell PATH)

## [0.17.3] - 2026-02-18
### Changed
- Shared test infrastructure, E2E script fixes

## [0.17.2] - 2026-02-18
### Changed
- `unf list` output rewritten with dynamic column alignment, colored status, time ranges

## [0.17.1] - 2026-02-18
### Changed
- Website: CSS extraction, contact page, unified terminal styling

## [0.17.0] - 2026-02-17
### Added
- AWS infrastructure: S3 buckets, CloudFront, ACM cert, Route 53 DNS
- Release pipeline: staging-to-production promotion (no rebuild)
- CloudFront access logging for download analytics

### Changed
- Homebrew formulas point to downloads.unfudged.io

## [0.16.2] - 2026-02-17
### Fixed
- Flaky concurrent worktree load test marked `#[ignore]` on CI

## [0.16.1] - 2026-02-17
### Changed
- Website: eliminated AI writing patterns, sharpened narrative

## [0.16.0] - 2026-02-17
### Added
- Tech whitepaper page (tech.html)
- Use case carousel with AI agent disaster scenarios

## [0.15.0] - 2026-02-16
### Added
- Tauri desktop app (macOS, universal DMG)
- Homebrew Cask: `brew install --cask cyrusradfar/unf/unfudged`
- Landing page

## [0.14.0] - 2026-02-16
### Changed
- Release binaries hosted on public homebrew-unf repo

## [0.13.0] - 2026-02-16
### Added
- `unf recap` command for AI agent context reconstruction (--json, --global, --since)

## [0.12.0] - 2026-02-15
### Added
- Session detection: `unf log --sessions`, `unf diff --session`, `unf restore --session`

## [0.11.0] - 2026-02-15
### Added
- Desktop app: global density histogram
- Interactive time-range filtering

### Fixed
- Per-row scrollbars in diff/raw view

## [0.10.0] - 2026-02-15
### Added
- Sentinel watchdog (two-process reliability)
- Intent registry (intent.json)
- Audit log (audit.log)

## [0.9.0] - 2026-02-15
### Added
- `unf log --global` with cross-project merge
- `--include-project` / `--exclude-project` filters
- Desktop app: Global tab, project-aware views

## [0.8.1] - 2026-02-14
### Fixed
- Flaky load tests: fixed JSON parsing for paginated output, poll-based waits

## [0.8.0] - 2026-02-13
### Added
- Desktop app: multi-filter autocomplete with keyboard nav

## [0.7.1] - 2026-02-13
### Added
- Unified filter status bar

### Fixed
- Diff view infinite loop
- Search breaking row expansion

## [0.7.0] - 2026-02-13
### Added
- Contextual diff viewer with Shiki syntax highlighting
- Word-level intra-line diffs, language-aware context
- Session detection on histogram

## [0.6.1] - 2026-02-13
### Fixed
- Tab switch stale data
- Svelte 5 effect contamination
- File tree entry cap removed

## [0.6.0] - 2026-02-12
### Changed
- UI: 2-column + topbar layout
- Resizable sidebar, histogram range selector, tab persistence

## [0.5.7] - 2026-02-12
### Fixed
- `unf diff --at` single-point comparison
- Unified diff output with hunk headers and context
- Test isolation with UNF_HOME

## [0.5.6] - 2026-02-11
### Changed
- Removed per-project PID fallbacks
- Hidden legacy `init` command (use `watch` instead)

## [0.5.5] - 2026-02-11
### Fixed
- Boot resilience: clear stopped sentinels
- Orphan cleanup: prune missing projects

## [0.5.4] - 2026-02-11
### Fixed
- `unf stop` preserves registry and autostart

## [0.5.3] - 2026-02-11
### Changed
- Unified CLI output patterns
- Registry auto-recovery from corrupt files

## [0.5.2] - 2026-02-11
### Changed
- `unf watch` 386x faster on large repos

## [0.5.1] - 2026-02-11
### Changed
- Always show line deltas in `unf log` output (no more hidden columns)

## [0.5.0] - 2026-02-11
### Added
- Enhanced `unf log` with inline stats
- Magic-number binary detection
- Single global daemon

### Changed
- 398 tests passing

## [0.4.4] - 2026-02-11
### Fixed
- `unf stop` kills stuck CLI processes
- Infinite loop in `unf log` with glob filters

## [0.4.3] - 2026-02-10
### Added
- Load/stress tests (burst capture, debounce, multi-worktree)

## [0.4.2] - 2026-02-10
### Added
- `--project` global flag for remote project management
- `unf list --verbose` with tracked files and activity times

## [0.4.1] - 2026-02-10
### Added
- Self-documenting `--help` with examples
- E2E test infrastructure

## [0.3.0] - 2026-02-09
### Added
- Developer README
- Stress test validation

## [0.2.0] - 2026-02-09
### Added
- `unf log` with keyset pagination, file/directory/project scoping
- `--since` time filter, TTY-aware output

## [0.1.0] - 2026-02-09
### Added
- Initial release: 6 CLI commands
- Background daemon with OS-native file watching
- BLAKE3 content-addressable storage
- SQLite metadata with WAL mode
- .gitignore filtering, 3-second debounce
- 158 tests
