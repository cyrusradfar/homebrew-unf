# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/),
and this project adheres to [Semantic Versioning](https://semver.org/).

## [Unreleased]

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
- Website copy overhaul: eliminated AI writing patterns, sharpened narrative

## [0.16.0] - 2026-02-17
### Added
- Tech whitepaper page (tech.html) targeting HN audience
- Use case carousel (10 cards, 7 AI agent disaster scenarios)

## [0.15.0] - 2026-02-16
### Added
- Tauri desktop app distribution (universal macOS DMG)
- Homebrew Cask (`brew install --cask cyrusradfar/unf/unfudged`)
- Single-page marketing website

## [0.14.0] - 2026-02-16
### Changed
- Release binaries hosted on public homebrew-unf repo (private repo can't serve Homebrew)

## [0.13.0] - 2026-02-16
### Added
- `unf recap` command for AI agent context reconstruction (--json, --global, --since)

## [0.12.0] - 2026-02-15
### Added
- Session detection: `unf log --sessions`, `unf diff --session`, `unf restore --session`
- Gap-based activity boundary computation from snapshot timestamps

## [0.11.0] - 2026-02-15
### Added
- Desktop app: global density histogram with session diamonds
- Interactive time-range filtering

### Fixed
- Per-row scrollbars in diff/raw view no longer occlude text

## [0.10.0] - 2026-02-15
### Added
- Sentinel watchdog (two-process reliability model)
- Intent registry (intent.json) for immutable watch/unwatch source of truth
- Audit log (audit.log) for diagnostic events

## [0.9.0] - 2026-02-15
### Added
- `unf log --global` with K-way merge across projects
- `--include-project` / `--exclude-project` filters
- Desktop app: Global tab as default, project-aware timeline/diff views

## [0.8.1] - 2026-02-14
### Fixed
- Flaky load tests: fixed JSON parsing for paginated output, poll-based waits

## [0.8.0] - 2026-02-13
### Added
- Desktop app: multi-filter autocomplete replacing single-file filter
- FilterAutocomplete component with keyboard nav, ARIA roles

## [0.7.1] - 2026-02-13
### Added
- Unified filter status bar with human-friendly time descriptions

### Fixed
- Diff view infinite loop from reactive cycle
- Search breaking row expansion

## [0.7.0] - 2026-02-13
### Added
- Contextual diff viewer with Shiki syntax highlighting
- Word-level intra-line diffs
- Language-aware context expansion (16+ languages)
- Session detection diamonds on histogram

## [0.6.1] - 2026-02-13
### Fixed
- Tab switch stale data race
- Svelte 5 effect cross-contamination
- 1000-entry cap removed from fileTree for accurate counts

## [0.6.0] - 2026-02-12
### Changed
- Major UI overhaul: 2-column + topbar layout (replaces 3-column + scrubber)
- Resizable sidebar, dual-handle histogram range selector, tab persistence

## [0.5.7] - 2026-02-12
### Fixed
- `unf diff --at` single-point delegation to two-point comparison
- Unified diff output with @@ hunk headers, 3-line context, colored output
- All tests use UNF_HOME isolation

## [0.5.6] - 2026-02-11
### Changed
- Removed per-project PID fallbacks
- Hidden legacy `init` command (use `watch` instead)

## [0.5.5] - 2026-02-11
### Fixed
- Boot resilience: `__boot` clears stopped sentinels for reboot recovery
- Orphan cleanup: prune entries where project dir no longer exists

## [0.5.4] - 2026-02-11
### Fixed
- `unf stop` no longer destroys project registry or removes autostart

## [0.5.3] - 2026-02-11
### Added
- Centralized CLI output module with formatting helpers

### Changed
- Unified CLI output patterns across all 12 commands
- Registry auto-recovery from corrupt projects.json

## [0.5.2] - 2026-02-11
### Changed
- 386x faster `unf watch` on large repos (removed cosmetic file count walk)

## [0.5.1] - 2026-02-11
### Changed
- Always show line deltas in `unf log` output (no more hidden columns)

## [0.5.0] - 2026-02-11
### Added
- Enhanced `unf log` with inline stats (line count, diff delta, file size)
- Magic-number binary detection in snapshot flow
- Single global daemon managing all watched projects

### Changed
- 398 tests passing (306 unit + 68 integration + 8 prune + 16 doc)

## [0.4.4] - 2026-02-11
### Fixed
- `unf stop` kills stuck CLI processes holding DB open (lsof + SIGKILL)
- Infinite loop in `unf log` pagination with glob filters

## [0.4.3] - 2026-02-10
### Added
- 8 automated load/stress tests (burst capture, debounce, multi-worktree)

## [0.4.2] - 2026-02-10
### Added
- `--project` global flag for remote project management
- `unf list --verbose` with tracked files and activity times

## [0.4.1] - 2026-02-10
### Added
- Self-documenting `--help` with examples via `include_str!`
- Double-blind E2E test infrastructure

## [0.3.0] - 2026-02-09
### Added
- Developer-focused README
- Stress test validation (full build→sabotage→restore cycle)

## [0.2.0] - 2026-02-09
### Added
- `unf log` with keyset cursor pagination, file/directory/project scoping
- `--since` time filter, TTY-aware pagination, ANSI color output

## [0.1.0] - 2026-02-09
### Added
- Initial release: 6 CLI commands (init, status, log, diff, restore, stop)
- Background daemon with OS-native file watching (FSEvents/inotify/ReadDirectoryChangesW)
- BLAKE3 content-addressable storage
- SQLite metadata with WAL mode
- .gitignore filtering, 3-second debounce
- 158 tests (127 unit + 18 integration + 13 doc)
