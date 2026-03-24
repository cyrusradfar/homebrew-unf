# Run all tests with automatic cleanup of leaked test processes
test *ARGS:
    #!/usr/bin/env bash
    set +e
    cargo test {{ARGS}}
    TEST_EXIT=$?
    just kill-test-daemons
    exit $TEST_EXIT

# Kill test daemons AND sentinels in temp dirs (safe — never touches production)
kill-test-daemons:
    #!/usr/bin/env bash
    pkill -f '__daemon --root /private/var/folders' 2>/dev/null || true
    pkill -f '__daemon --root /tmp/unf-e2e' 2>/dev/null || true
    # Kill test sentinels via PID files in temp dirs
    for f in /private/var/folders/*/*/*/.tmp*/sentinel.pid /tmp/unf-e2e-*/sentinel.pid; do
        [ -f "$f" ] && kill $(cat "$f") 2>/dev/null || true
    done
    # Kill orphaned debug-build sentinels (production uses /opt/homebrew/bin/unf)
    pkill -f 'target/debug/unf __sentinel' 2>/dev/null || true
    echo "Test daemons and sentinels killed."

# E2E smoke test on Linux (Docker)
test-e2e-linux:
    tests/e2e/run-linux.sh

# E2E smoke test on macOS (Tart VM) — CLI + desktop app
test-e2e-mac:
    tests/e2e/run-mac.sh

# E2E CLI-only on macOS (skip desktop app tests)
test-e2e-mac-cli:
    tests/e2e/run-mac.sh --cli-only

# E2E smoke tests on both platforms
test-e2e: test-e2e-linux test-e2e-mac

# E2E with staging formula
test-e2e-staging:
    tests/e2e/run-linux.sh --staging
    tests/e2e/run-mac.sh --staging

# Test that release/staging workflow push logic handles diverged main
test-workflow-push:
    ./scripts/test-workflow-push.sh

# Download stats from CloudFront logs (optional: filter by version)
download-stats *VERSION:
    ./scripts/download-stats.sh {{VERSION}}
