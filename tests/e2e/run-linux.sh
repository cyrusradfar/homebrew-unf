#!/bin/bash
set -euo pipefail

# Parse arguments
UNF_FORMULA="unf"
if [[ "${1:-}" == "--staging" ]]; then
    UNF_FORMULA="unf-staging"
fi

echo "=== E2E Smoke Test: Linux (Docker) ==="
echo "Formula: ${UNF_FORMULA}"
echo ""

# Build Docker image (x86_64 — matches the Linux release binary)
docker build --platform linux/amd64 -f tests/e2e/Dockerfile.e2e -t unf-e2e tests/e2e/

# Run container
docker run --rm \
    --platform linux/amd64 \
    -v "$(pwd)/tests/e2e/smoke-test.sh:/home/linuxbrew/smoke-test.sh:ro" \
    -e UNF_FORMULA="${UNF_FORMULA}" \
    -u linuxbrew \
    unf-e2e \
    bash /home/linuxbrew/smoke-test.sh

# Exit with container's exit code
exit $?
