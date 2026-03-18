# Contributing

Requires Rust 1.75+ (edition 2021).

## Build & Test

```bash
git clone https://github.com/cyrusradfar/homebrew-unf.git
cd homebrew-unf
cargo build --release
cargo test                        # ~490 tests
cargo clippy -- -D warnings       # Zero warnings required
cargo fmt -- --check              # Format check
just kill-test-daemons            # Clean up stuck test daemons
```

Tests use `UNF_HOME` for isolation. Never use `pkill -f 'target/debug/unf'` — this kills the production daemon. Use `just kill-test-daemons` instead.

## Submitting Changes

1. Fork and create a feature branch
2. Make your changes
3. Run the checklist:
   - `cargo fmt`
   - `cargo clippy -- -D warnings` (zero warnings)
   - `cargo test` (all passing)
4. Update `CHANGELOG.md` under `Unreleased`
5. Keep commits atomic
6. Open a PR describing what and why

## Bug Reports

Include: version (`unf --version`), OS, reproduction steps, and `unf status` output if relevant.

## Feature Requests

Open an issue first. Describe the problem, not the solution.

## Code Standards

- No `unwrap()` outside tests — use `?` or `expect()` with context
- Pure, testable core logic
- Error types per module using `thiserror`
- See [CLAUDE.md](CLAUDE.md) for full standards
