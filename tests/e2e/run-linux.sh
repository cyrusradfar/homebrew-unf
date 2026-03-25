#!/bin/bash
set -euo pipefail

# Parse arguments
UNF_FORMULA="unf"
FROM_SOURCE=false

while [[ $# -gt 0 ]]; do
  case $1 in
    --staging)
      UNF_FORMULA="unf-staging"
      shift
      ;;
    --from-source)
      FROM_SOURCE=true
      shift
      ;;
    *)
      echo "Unknown option: $1"
      echo "Usage: $0 [--staging] [--from-source]"
      exit 1
      ;;
  esac
done

echo "=== E2E Smoke Test: Linux (Docker) ==="
echo "Formula: ${UNF_FORMULA}"
echo "From source: ${FROM_SOURCE}"
echo ""

# Resolve script directory so this works from any cwd
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# Repo root is two levels up from tests/e2e/
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

if [[ "$FROM_SOURCE" == "true" ]]; then
    # Build Docker image with Rust toolchain for from-source builds
    docker build --platform linux/amd64 -f "$SCRIPT_DIR/Dockerfile.e2e-source" -t unf-e2e-source "$SCRIPT_DIR"

    # Run container with the repo mounted read-only; build inside container
    docker run --rm \
        --platform linux/amd64 \
        -v "$REPO_ROOT:/src:ro" \
        -v "$SCRIPT_DIR/smoke-test.sh:/home/linuxbrew/smoke-test.sh:ro" \
        -e UNF_FORMULA="${UNF_FORMULA}" \
        -e FROM_SOURCE=true \
        -u linuxbrew \
        unf-e2e-source \
        bash -c 'cp -r /src /tmp/build && cd /tmp/build && cargo build --release 2>&1 && sudo cp target/release/unf /usr/local/bin/unf && bash /home/linuxbrew/smoke-test.sh'
else
    # Build Docker image (x86_64 — matches the Linux release binary)
    docker build --platform linux/amd64 -f "$SCRIPT_DIR/Dockerfile.e2e" -t unf-e2e "$SCRIPT_DIR"

    # Run container
    docker run --rm \
        --platform linux/amd64 \
        -v "$SCRIPT_DIR/smoke-test.sh:/home/linuxbrew/smoke-test.sh:ro" \
        -e UNF_FORMULA="${UNF_FORMULA}" \
        -u linuxbrew \
        unf-e2e \
        bash /home/linuxbrew/smoke-test.sh
fi

# Exit with container's exit code
exit $?
