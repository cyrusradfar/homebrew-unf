#!/bin/bash
# Test that release/staging workflows can push formula updates to main
# even when main has diverged (the scenario that broke v0.17.8–v0.17.11).
#
# Simulates:
#   1. A repo with a release tag
#   2. Staging workflow pushes a commit to main after the tag
#   3. Release workflow checks out the tag (detached HEAD)
#   4. Verifies the "fetch + checkout main" approach succeeds
#
# Usage: ./scripts/test-workflow-push.sh

set -euo pipefail

TMPDIR=$(mktemp -d)
REMOTE="$TMPDIR/remote.git"
PASSED=0
FAILED=0

cleanup() { rm -rf "$TMPDIR"; }
trap cleanup EXIT

setup_repo() {
    git init --bare "$REMOTE" -q 2>/dev/null
    local work="$TMPDIR/setup"
    git clone "$REMOTE" "$work" -q 2>/dev/null
    cd "$work"
    git config user.name "test"
    git config user.email "test@test.com"

    mkdir -p Formula Casks
    echo "v1" > version.txt
    echo "old formula" > Formula/unf.rb
    git add -A && git commit -m "v1.0.0" -q
    git tag v1.0.0
    git push origin main v1.0.0 -q 2>/dev/null

    # Staging workflow pushes after tag (this is what causes the divergence)
    echo "staging formula" > Formula/unf-staging.rb
    git add -A && git commit -m "Staging: unf v1.0.0" -q
    git push origin main -q 2>/dev/null
}

clone_at_tag() {
    local name="$1"
    local dir="$TMPDIR/$name"
    git clone "$REMOTE" "$dir" -q 2>/dev/null
    cd "$dir"
    git config user.name "github-actions[bot]"
    git config user.email "github-actions[bot]@users.noreply.github.com"
    git checkout v1.0.0 -q 2>/dev/null  # detached HEAD (like actions/checkout@v4)
}

pass() { echo "  PASS: $1"; PASSED=$((PASSED + 1)); }
fail() { echo "  FAIL: $1"; FAILED=$((FAILED + 1)); }

# --- Setup ---
echo "Setting up test repo..."
setup_repo

# --- Test 1: Old approach fails ---
echo ""
echo "Test 1: Detached HEAD push (old approach) should fail"
clone_at_tag "old-approach"
echo "release formula" > Formula/unf.rb
git add Formula/unf.rb && git commit -m "Update unf to 1.0.0" -q
if git push origin HEAD:main 2>/dev/null; then
    fail "push should have been rejected"
else
    pass "push rejected as expected"
fi

# --- Test 2: New approach succeeds ---
echo ""
echo "Test 2: Fetch + checkout main (new approach) should succeed"
clone_at_tag "new-approach"
git fetch origin main -q 2>/dev/null
git checkout -B main origin/main -q
echo "release formula" > Formula/unf.rb
git add Formula/unf.rb && git commit -m "Update unf to 1.0.0" -q
if git push origin main 2>/dev/null; then
    pass "push succeeded"
else
    fail "push should have succeeded"
fi

# --- Test 3: History is correct ---
echo ""
echo "Test 3: All commits present on main after push"
COMMIT_COUNT=$(git log --oneline main | wc -l | tr -d ' ')
if [ "$COMMIT_COUNT" -eq 3 ]; then
    pass "3 commits: initial + staging + release"
else
    fail "expected 3 commits, got $COMMIT_COUNT"
fi

# --- Test 4: Staging formula preserved ---
echo ""
echo "Test 4: Staging formula not clobbered by release push"
if [ -f Formula/unf-staging.rb ]; then
    pass "staging formula still exists"
else
    fail "staging formula was lost"
fi

# --- Test 5: No-op push when formula unchanged ---
echo ""
echo "Test 5: No commit created when formula is unchanged"
clone_at_tag "no-op"
git fetch origin main -q 2>/dev/null
git checkout -B main origin/main -q
# Don't change anything
git add Formula/ Casks/ 2>/dev/null || true
if git diff --cached --quiet; then
    pass "no commit needed (diff is clean)"
else
    fail "unexpected diff when nothing changed"
fi

# --- Summary ---
echo ""
echo "========================================="
echo "Results: $PASSED passed, $FAILED failed"
echo "========================================="
[ "$FAILED" -eq 0 ] || exit 1
