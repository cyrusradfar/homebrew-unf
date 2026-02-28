#!/usr/bin/env bash
# UNFUDGED E2E Smoke Test
# Tests the full CLI lifecycle on real Homebrew installation
# Platform-agnostic: runs on Linux (Docker) or macOS (Tart VM)

set -euo pipefail

# ============================================================================
# Helpers
# ============================================================================

pass() { echo "[PASS] $1"; }
fail() { echo "[FAIL] $1"; echo "  $2"; exit 1; }

cleanup() {
  echo ""
  echo "=== Cleanup ==="
  unf stop 2>/dev/null || true
  brew uninstall "$UNF_FORMULA" 2>/dev/null || true
  rm -rf "$TEST_ROOT" || true
  rm -rf ~/.unfudged || true
  echo "Cleanup complete"
}

# ============================================================================
# Setup
# ============================================================================

echo "=== UNFUDGED E2E Smoke Test ==="
echo ""

# Detect platform and add Homebrew to PATH
if [[ "$OSTYPE" == "linux-gnu"* ]]; then
  export PATH="/home/linuxbrew/.linuxbrew/bin:$PATH"
  PLATFORM="Linux"
elif [[ "$OSTYPE" == "darwin"* ]]; then
  export PATH="/opt/homebrew/bin:$PATH"
  PLATFORM="macOS"
else
  fail "Setup" "Unsupported platform: $OSTYPE"
fi

echo "Platform: $PLATFORM"
echo "Homebrew: $(command -v brew || echo 'NOT FOUND')"

# Verify brew is available
if ! command -v brew &> /dev/null; then
  fail "Setup" "Homebrew not found in PATH"
fi
pass "Homebrew detected"

# Set formula name (defaults to unf, but can be unf-staging)
UNF_FORMULA="${UNF_FORMULA:-unf}"
echo "Formula: $UNF_FORMULA"
echo ""

# Create test root
TEST_ROOT=$(mktemp -d)
echo "Test root: $TEST_ROOT"

# Set trap for cleanup
trap cleanup EXIT

cd "$TEST_ROOT" || fail "Setup" "Cannot cd to test root"
pass "Test root created"

echo ""

# ============================================================================
# Test 1: Install
# ============================================================================

echo "=== Test 1: Install ==="
brew install "cyrusradfar/unf/$UNF_FORMULA" || fail "Install" "brew install failed"

if ! command -v unf &> /dev/null; then
  fail "Install" "unf command not found after install"
fi

VERSION=$(unf --version) || fail "Install" "unf --version failed"
echo "Installed: $VERSION"
pass "Install"
echo ""

# ============================================================================
# Test 2: Watch project A
# ============================================================================

echo "=== Test 2: Watch project A ==="
mkdir -p "$TEST_ROOT/project-a"
cd "$TEST_ROOT/project-a" || fail "Watch A" "Cannot cd to project-a"

unf watch || fail "Watch A" "unf watch failed"
sleep 2

STATUS=$(unf status) || fail "Watch A" "unf status failed"
if ! echo "$STATUS" | grep -q "Watching"; then
  fail "Watch A" "Status does not show Watching:\n$STATUS"
fi

pass "Watch project A"
echo ""

# ============================================================================
# Test 3: Watch project B
# ============================================================================

echo "=== Test 3: Watch project B ==="
mkdir -p "$TEST_ROOT/project-b"
cd "$TEST_ROOT/project-b" || fail "Watch B" "Cannot cd to project-b"

unf watch || fail "Watch B" "unf watch failed"
sleep 2

STATUS=$(unf status) || fail "Watch B" "unf status failed"
if ! echo "$STATUS" | grep -q "Watching"; then
  fail "Watch B" "Status does not show Watching:\n$STATUS"
fi

pass "Watch project B"
echo ""

# ============================================================================
# Test 4: Write files to project A
# ============================================================================

echo "=== Test 4: Write files to project A ==="
cd "$TEST_ROOT/project-a" || fail "Write A" "Cannot cd to project-a"

echo "version one" > file1.txt
echo "hello world" > file2.txt
mkdir -p src
echo "fn main() {}" > src/main.rs

echo "Waiting for debounce + processing..."
sleep 5

pass "Write files to project A"
echo ""

# ============================================================================
# Test 5: Write files to project B
# ============================================================================

echo "=== Test 5: Write files to project B ==="
cd "$TEST_ROOT/project-b" || fail "Write B" "Cannot cd to project-b"

echo '{"key": "value"}' > config.json
echo "some notes" > notes.txt

echo "Waiting for debounce + processing..."
sleep 5

pass "Write files to project B"
echo ""

# ============================================================================
# Test 6: Modify and delete in project A
# ============================================================================

echo "=== Test 6: Modify and delete in project A ==="
cd "$TEST_ROOT/project-a" || fail "Modify A" "Cannot cd to project-a"

echo "version two" > file1.txt
rm file2.txt

echo "Waiting for debounce + processing..."
sleep 5

pass "Modify and delete in project A"
echo ""

# ============================================================================
# Test 7: Modify in project B
# ============================================================================

echo "=== Test 7: Modify in project B ==="
cd "$TEST_ROOT/project-b" || fail "Modify B" "Cannot cd to project-b"

echo '{"key": "updated"}' > config.json

echo "Waiting for debounce + processing..."
sleep 5

pass "Modify in project B"
echo ""

# ============================================================================
# Test 8: Log (project A)
# ============================================================================

echo "=== Test 8: Log (project A) ==="
cd "$TEST_ROOT/project-a" || fail "Log A" "Cannot cd to project-a"

LOG_OUTPUT=$(unf log file1.txt --json) || fail "Log A" "unf log failed"

# Parse JSON to verify at least 2 entries with id fields
if command -v jq &> /dev/null; then
  ENTRY_COUNT=$(echo "$LOG_OUTPUT" | jq '.entries | length') || fail "Log A" "Failed to parse JSON with jq"
  if [[ "$ENTRY_COUNT" -lt 2 ]]; then
    fail "Log A" "Expected at least 2 entries, got $ENTRY_COUNT"
  fi

  # Verify each entry has an id > 0
  IDS=$(echo "$LOG_OUTPUT" | jq '.entries[].id') || fail "Log A" "Failed to extract IDs"
  for id in $IDS; do
    if [[ "$id" -le 0 ]]; then
      fail "Log A" "Found invalid id: $id"
    fi
  done
else
  # Fallback: grep for "id": pattern
  ID_COUNT=$(echo "$LOG_OUTPUT" | grep -c '"id":' || true)
  if [[ "$ID_COUNT" -lt 2 ]]; then
    fail "Log A" "Expected at least 2 id fields, got $ID_COUNT"
  fi
fi

pass "Log (project A)"
echo ""

# ============================================================================
# Test 9: Log (project B)
# ============================================================================

echo "=== Test 9: Log (project B) ==="
cd "$TEST_ROOT/project-b" || fail "Log B" "Cannot cd to project-b"

LOG_OUTPUT=$(unf log --json) || fail "Log B" "unf log failed"

# Verify non-empty output
if [[ -z "$LOG_OUTPUT" ]]; then
  fail "Log B" "Empty log output"
fi

# Verify has entries
if command -v jq &> /dev/null; then
  ENTRY_COUNT=$(echo "$LOG_OUTPUT" | jq '.entries | length') || fail "Log B" "Failed to parse JSON"
  if [[ "$ENTRY_COUNT" -eq 0 ]]; then
    fail "Log B" "No entries found"
  fi
else
  if ! echo "$LOG_OUTPUT" | grep -q '"id":'; then
    fail "Log B" "No entries found (no id fields)"
  fi
fi

pass "Log (project B)"
echo ""

# ============================================================================
# Test 10: Diff (project A)
# ============================================================================

echo "=== Test 10: Diff (project A) ==="
cd "$TEST_ROOT/project-a" || fail "Diff A" "Cannot cd to project-a"

DIFF_OUTPUT=$(unf diff --at 15s) || fail "Diff A" "unf diff failed"

if ! echo "$DIFF_OUTPUT" | grep -q "file1.txt"; then
  fail "Diff A" "Diff output does not mention file1.txt:\n$DIFF_OUTPUT"
fi

pass "Diff (project A)"
echo ""

# ============================================================================
# Test 11: Cat (project A)
# ============================================================================

echo "=== Test 11: Cat (project A) ==="
cd "$TEST_ROOT/project-a" || fail "Cat A" "Cannot cd to project-a"

CAT_OUTPUT=$(unf cat file1.txt --at 15s) || fail "Cat A" "unf cat failed"

if ! echo "$CAT_OUTPUT" | grep -q "version one"; then
  fail "Cat A" "Expected 'version one', got:\n$CAT_OUTPUT"
fi

pass "Cat (project A)"
echo ""

# ============================================================================
# Test 12: Restore (project A)
# ============================================================================

echo "=== Test 12: Restore (project A) ==="
cd "$TEST_ROOT/project-a" || fail "Restore A" "Cannot cd to project-a"

unf restore --at 15s -y || fail "Restore A" "unf restore failed"

# Verify file1.txt was restored to "version one"
CURRENT_CONTENT=$(cat file1.txt)
if [[ "$CURRENT_CONTENT" != "version one" ]]; then
  fail "Restore A" "Expected 'version one', got: $CURRENT_CONTENT"
fi

# Verify file2.txt was restored
if [[ ! -f file2.txt ]]; then
  fail "Restore A" "file2.txt was not restored"
fi

FILE2_CONTENT=$(cat file2.txt)
if [[ "$FILE2_CONTENT" != "hello world" ]]; then
  fail "Restore A" "file2.txt has wrong content: $FILE2_CONTENT"
fi

pass "Restore (project A)"
echo ""

# ============================================================================
# Test 13: List projects
# ============================================================================

echo "=== Test 13: List projects ==="

LIST_OUTPUT=$(unf list --json) || fail "List" "unf list failed"

# Verify both project paths are in the output
if ! echo "$LIST_OUTPUT" | grep -q "project-a"; then
  fail "List" "project-a not found in list output"
fi

if ! echo "$LIST_OUTPUT" | grep -q "project-b"; then
  fail "List" "project-b not found in list output"
fi

# If jq available, verify JSON structure
if command -v jq &> /dev/null; then
  PROJECT_COUNT=$(echo "$LIST_OUTPUT" | jq '.projects | length') || fail "List" "Failed to parse JSON"
  if [[ "$PROJECT_COUNT" -lt 2 ]]; then
    fail "List" "Expected at least 2 projects, got $PROJECT_COUNT"
  fi
fi

pass "List projects"
echo ""

# ============================================================================
# Test 14: Stop daemon
# ============================================================================

echo "=== Test 14: Stop daemon ==="

unf stop || fail "Stop" "unf stop failed"
sleep 1

# Verify no daemon process running
if pgrep -f "unf __daemon" > /dev/null; then
  fail "Stop" "Daemon still running after stop"
fi

pass "Stop daemon"
echo ""

# ============================================================================
# Test 15: Uninstall
# ============================================================================

echo "=== Test 15: Uninstall ==="

brew uninstall "$UNF_FORMULA" || fail "Uninstall" "brew uninstall failed"

# Clear shell's command cache and verify binary is gone
hash -r 2>/dev/null || true
if command -v unf &> /dev/null; then
  fail "Uninstall" "unf command still available after uninstall"
fi

pass "Uninstall"
echo ""

# ============================================================================
# Summary
# ============================================================================

echo "==================================="
echo "ALL TESTS PASSED"
echo "==================================="
