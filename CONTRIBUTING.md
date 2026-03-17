# Contributing to UNFUDGED

Thanks for helping. Here's how to get started.

## Building from Source

Requires Rust 1.75+ (edition 2021).

```bash
git clone https://github.com/cyrusradfar/homebrew-unf.git
cd homebrew-unf
cargo build --release
./target/release/unf --version
```

## Running Tests

```bash
cargo test                          # Full suite (~490 tests)
cargo test <name>                   # Single test
cargo clippy -- -D warnings         # Zero warnings (required)
cargo fmt -- --check                # Formatting check
just kill-test-daemons              # Clean up stuck test daemons
```

**Important**: Tests run in isolation using `UNF_HOME` pointed at temp dirs. Never use `pkill -f 'target/debug/unf'` — this kills the production daemon. Use `just kill-test-daemons` instead, which is safe.

## Submitting Changes

1. Fork and create a feature branch
2. Make your changes
3. Run the pre-submit checklist:
   - `cargo fmt` (must pass)
   - `cargo clippy -- -D warnings` (zero warnings)
   - `cargo test` (all passing)
4. Update `CHANGELOG.md` under `Unreleased`
5. Keep commits atomic and focused
6. Open a PR with a clear description of what changed and why

## Bug Reports

Include:
- `unf --version`
- OS and version
- Steps to reproduce
- Output from `unf status` if relevant

## Feature Requests

Open an issue first. Describe the problem you're solving, not just the solution. Changes follow the SUPER principles: side effects at boundaries, pure logic in core.

## Code Standards

- No `unwrap()` outside tests — use `?` or `expect()` with context
- Error types per module using `thiserror`
- Keep core logic pure and testable
- See CLAUDE.md for full standards

## Daemon Safety

The daemon watches real files. Be careful in development.

- Use `just kill-test-daemons` to clean up test processes
- Never broad `pkill` patterns
- If the production daemon is killed, run `unf restart` to recover
