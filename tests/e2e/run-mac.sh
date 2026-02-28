#!/bin/bash
set -euo pipefail

# ============================================================================
# UNFUDGED E2E Test Orchestrator: macOS (Tart VM)
# ============================================================================
# Manages full VM lifecycle: clone → start → setup → test → cleanup
# Runs smoke-test.sh in a clean macOS VM with Homebrew installation

# ============================================================================
# Parse Arguments
# ============================================================================

UNF_FORMULA="unf"
CLI_ONLY=false

while [[ $# -gt 0 ]]; do
  case $1 in
    --staging)
      UNF_FORMULA="unf-staging"
      shift
      ;;
    --cli-only)
      CLI_ONLY=true
      shift
      ;;
    *)
      echo "Unknown option: $1"
      echo "Usage: $0 [--staging] [--cli-only]"
      exit 1
      ;;
  esac
done

echo "=== E2E Smoke Test: macOS (Tart VM) ==="
echo "Formula: ${UNF_FORMULA}"
echo "CLI only: ${CLI_ONLY}"
echo ""

# ============================================================================
# Pre-flight Checks
# ============================================================================

echo "=== Pre-flight Checks ==="

if ! command -v tart &> /dev/null; then
  echo "[FAIL] tart not found"
  echo "Install with: brew install cirruslabs/cli/tart"
  exit 1
fi
echo "[PASS] tart installed: $(tart --version)"

if ! command -v sshpass &> /dev/null; then
  echo "[FAIL] sshpass not found"
  echo "Install with: brew install hudochenkov/sshpass/sshpass"
  exit 1
fi
echo "[PASS] sshpass installed"

echo ""

# ============================================================================
# VM Setup
# ============================================================================

# Generate unique VM name with timestamp
VM_NAME="unf-e2e-$(date +%s)"
VM_IP=""
TART_PID=""
PROXY_PID=""
PROXY_PORT=8888
PROXY_SCRIPT="/tmp/tart-proxy-${VM_NAME}.js"

echo "=== VM Lifecycle ==="
echo "VM name: ${VM_NAME}"

# Cleanup trap - ALWAYS runs on exit
cleanup() {
  local exit_code=$?
  echo ""
  echo "=== Cleanup ==="

  if [[ -n "$PROXY_PID" ]]; then
    echo "Stopping proxy (PID: ${PROXY_PID})"
    kill "$PROXY_PID" 2>/dev/null || true
    rm -f "$PROXY_SCRIPT"
  fi

  if [[ -n "$VM_NAME" ]]; then
    echo "Stopping VM: ${VM_NAME}"
    tart stop "$VM_NAME" --timeout 5 2>/dev/null || true

    echo "Deleting VM: ${VM_NAME}"
    tart delete "$VM_NAME" 2>/dev/null || true
  fi

  echo "Cleanup complete"
  exit $exit_code
}
trap cleanup EXIT

# Boot VM with retry (Tart VMs sometimes fail to get an IP on NAT networking)
MAX_BOOT_ATTEMPTS=2
START_TIME=$(date +%s)

for BOOT_ATTEMPT in $(seq 1 "$MAX_BOOT_ATTEMPTS"); do
  echo "--- Boot attempt ${BOOT_ATTEMPT}/${MAX_BOOT_ATTEMPTS} ---"

  # Clone base image (only on first attempt — re-use clone on retry)
  if [[ $BOOT_ATTEMPT -eq 1 ]]; then
    echo "Cloning base image..."
    tart clone ghcr.io/cirruslabs/macos-tahoe-base:latest "$VM_NAME"
    echo "[PASS] VM cloned"
  else
    echo "Recycling existing clone..."
  fi

  # Start VM in background (headless)
  echo "Starting VM (headless)..."
  tart run --no-graphics "$VM_NAME" &
  TART_PID=$!
  echo "[PASS] VM started (PID: ${TART_PID})"

  # Wait for IP (fail-fast: 60s instead of 90s — if no IP by then, it's not coming)
  echo "Waiting for VM IP..."
  VM_IP=""
  for i in $(seq 1 60); do
    VM_IP=$(tart ip "$VM_NAME" 2>/dev/null || true)
    if [[ -n "$VM_IP" ]]; then
      break
    fi
    sleep 1
  done

  if [[ -z "$VM_IP" ]]; then
    echo "[WARN] No IP after 60s (attempt ${BOOT_ATTEMPT})"
    if [[ $BOOT_ATTEMPT -lt $MAX_BOOT_ATTEMPTS ]]; then
      echo "Stopping VM for retry..."
      tart stop "$VM_NAME" --timeout 5 2>/dev/null || true
      sleep 2
      continue
    fi
    echo "[FAIL] Timed out waiting for VM IP after ${MAX_BOOT_ATTEMPTS} attempts"
    echo ""
    echo "This usually means the macOS Virtualization framework (vmnet) is stuck."
    echo "Try: reboot your Mac, or run 'sudo launchctl kickstart -k system/com.apple.networking.vmnet'"
    exit 1
  fi
  echo "[PASS] VM IP: ${VM_IP}"

  # Wait for SSH (timeout 60s)
  echo "Waiting for SSH..."
  SSH_READY=false
  for i in $(seq 1 60); do
    if sshpass -p admin ssh -o StrictHostKeyChecking=no -o ConnectTimeout=2 admin@"$VM_IP" true 2>/dev/null; then
      SSH_READY=true
      break
    fi
    sleep 1
  done

  if [[ "$SSH_READY" == "true" ]]; then
    echo "[PASS] SSH ready"
    break
  fi

  echo "[WARN] SSH not responsive (attempt ${BOOT_ATTEMPT})"
  if [[ $BOOT_ATTEMPT -lt $MAX_BOOT_ATTEMPTS ]]; then
    echo "Stopping VM for retry..."
    tart stop "$VM_NAME" --timeout 5 2>/dev/null || true
    sleep 2
    continue
  fi
  echo "[FAIL] SSH not responsive after ${MAX_BOOT_ATTEMPTS} attempts"
  exit 1
done

VM_READY_TIME=$(date +%s)
VM_BOOT_DURATION=$((VM_READY_TIME - START_TIME))
echo "[TIMING] VM boot took ${VM_BOOT_DURATION}s"
echo ""

# ============================================================================
# HTTP Proxy (VPN split tunneling workaround)
# ============================================================================
# Tart VMs on NAT networking often can't reach the internet when the host
# has a VPN active. We start a local HTTP CONNECT proxy and use a reverse
# SSH tunnel so the VM routes traffic through the host.

echo "=== Starting HTTP Proxy ==="

cat > "$PROXY_SCRIPT" << 'PROXY_EOF'
const http = require('http');
const net = require('net');
const { URL } = require('url');

const server = http.createServer((req, res) => {
  const url = new URL(req.url);
  const proxy = http.request({
    hostname: url.hostname,
    port: url.port || 80,
    path: url.pathname + url.search,
    method: req.method,
    headers: {...req.headers, host: url.host}
  }, (proxyRes) => {
    res.writeHead(proxyRes.statusCode, proxyRes.headers);
    proxyRes.pipe(res);
  });
  proxy.on('error', (e) => { res.writeHead(502); res.end(e.message); });
  req.pipe(proxy);
});

server.on('connect', (req, cSocket, head) => {
  const [host, port] = req.url.split(':');
  const sSocket = net.connect(parseInt(port) || 443, host, () => {
    cSocket.write('HTTP/1.1 200 OK\r\n\r\n');
    sSocket.write(head);
    sSocket.pipe(cSocket);
    cSocket.pipe(sSocket);
  });
  sSocket.on('error', () => cSocket.end());
});

server.listen(8888, '127.0.0.1', () => console.log('Proxy ready on :8888'));
PROXY_EOF

node "$PROXY_SCRIPT" &
PROXY_PID=$!
sleep 1

# Verify proxy started
if ! kill -0 "$PROXY_PID" 2>/dev/null; then
  echo "[FAIL] Proxy server failed to start"
  exit 1
fi
echo "[PASS] Proxy running (PID: ${PROXY_PID})"
echo ""

# ============================================================================
# SSH Helper Functions
# ============================================================================

# SSH with reverse tunnel for proxy access
PROXY_ENV="export http_proxy=http://127.0.0.1:${PROXY_PORT} https_proxy=http://127.0.0.1:${PROXY_PORT} ALL_PROXY=http://127.0.0.1:${PROXY_PORT};"

ssh_cmd() {
  sshpass -p admin ssh \
    -o StrictHostKeyChecking=no \
    -o UserKnownHostsFile=/dev/null \
    -o LogLevel=ERROR \
    -R "${PROXY_PORT}:127.0.0.1:${PROXY_PORT}" \
    admin@"$VM_IP" "$@"
}

# SSH with proxy env vars pre-set (for commands that need internet)
ssh_inet() {
  ssh_cmd "${PROXY_ENV} $*"
}

scp_to() {
  sshpass -p admin scp -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -o LogLevel=ERROR "$1" admin@"$VM_IP":"$2"
}

# ============================================================================
# VM Provisioning
# ============================================================================

echo "=== VM Provisioning ==="

# Test connectivity: try direct first, fall back to proxy tunnel
echo "Testing internet connectivity..."
if ssh_cmd 'curl -s --max-time 15 https://github.com > /dev/null && echo OK' 2>/dev/null | grep -q OK; then
  echo "[PASS] Direct internet connectivity (no proxy needed)"
  PROXY_ENV=""
else
  echo "[INFO] Direct connectivity failed, trying reverse proxy tunnel..."
  if ssh_inet 'curl -s --max-time 15 https://github.com > /dev/null && echo OK' 2>/dev/null | grep -q OK; then
    echo "[PASS] Internet connectivity via proxy tunnel"
  else
    echo "[FAIL] No internet connectivity (neither direct nor proxy)"
    exit 1
  fi
fi
echo ""

# Check if Homebrew is already installed
if ssh_cmd 'command -v brew' &>/dev/null; then
  echo "[SKIP] Homebrew already installed"
else
  echo "Installing Homebrew..."
  SETUP_START=$(date +%s)

  # Install Homebrew non-interactively (needs internet)
  ssh_inet 'NONINTERACTIVE=1 /bin/bash -c "$(curl -fsSL https://raw.githubusercontent.com/Homebrew/install/HEAD/install.sh)"' || {
    echo "[FAIL] Homebrew installation failed"
    exit 1
  }

  # Add Homebrew to shell profile
  ssh_cmd 'echo "eval \"\$(/opt/homebrew/bin/brew shellenv)\"" >> ~/.zprofile'

  SETUP_END=$(date +%s)
  SETUP_DURATION=$((SETUP_END - SETUP_START))
  echo "[TIMING] Homebrew setup took ${SETUP_DURATION}s"
fi

# Verify Homebrew is functional
BREW_VERSION=$(ssh_cmd 'eval "$(/opt/homebrew/bin/brew shellenv)" && brew --version | head -n1') || {
  echo "[FAIL] Homebrew not functional"
  exit 1
}
echo "[PASS] ${BREW_VERSION}"
echo ""

# ============================================================================
# CLI Smoke Test
# ============================================================================

echo "=== CLI Smoke Test ==="

# Copy smoke test script to VM
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
echo "Copying smoke-test.sh to VM..."
scp_to "$SCRIPT_DIR/smoke-test.sh" "~/smoke-test.sh"
echo "[PASS] Script copied"

# Run smoke test
echo ""
echo "--- Starting smoke test ---"
TEST_START=$(date +%s)

ssh_cmd "${PROXY_ENV} UNF_FORMULA=${UNF_FORMULA} bash ~/smoke-test.sh"
TEST_EXIT_CODE=$?

TEST_END=$(date +%s)
TEST_DURATION=$((TEST_END - TEST_START))

echo "--- Smoke test complete ---"
echo ""

if [[ $TEST_EXIT_CODE -ne 0 ]]; then
  echo "[FAIL] CLI smoke test failed (exit code: ${TEST_EXIT_CODE})"
  exit $TEST_EXIT_CODE
fi

echo "[PASS] CLI smoke test passed"
echo "[TIMING] CLI test took ${TEST_DURATION}s"
echo ""

# ============================================================================
# Desktop App Test (Optional)
# ============================================================================

if [[ "$CLI_ONLY" == "false" ]]; then
  echo "=== Desktop App Test ==="
  APP_TEST_START=$(date +%s)

  # Re-install unf (CLI smoke test uninstalled it)
  echo "Re-installing $UNF_FORMULA for app test..."
  CASK_FORMULA="${UNF_FORMULA/unf/unfudged}"
  ssh_inet "eval \"\$(/opt/homebrew/bin/brew shellenv)\" && brew install cyrusradfar/unf/$UNF_FORMULA" || {
    echo "[FAIL] Re-install of $UNF_FORMULA failed"
    exit 1
  }

  # Install the desktop app cask
  echo "Installing desktop app cask: $CASK_FORMULA..."
  ssh_inet "eval \"\$(/opt/homebrew/bin/brew shellenv)\" && brew install --cask cyrusradfar/unf/$CASK_FORMULA" || {
    echo "[FAIL] Cask install of $CASK_FORMULA failed"
    exit 1
  }
  echo "[PASS] Desktop app installed"

  # Create test projects with data for the app to display
  echo "Creating test data for app..."
  ssh_cmd 'eval "$(/opt/homebrew/bin/brew shellenv)" && \
    mkdir -p ~/app-test/project-a ~/app-test/project-b && \
    cd ~/app-test/project-a && unf watch && \
    cd ~/app-test/project-b && unf watch && \
    sleep 2 && \
    echo "first version" > ~/app-test/project-a/file1.txt && \
    echo "hello" > ~/app-test/project-a/file2.txt && \
    mkdir -p ~/app-test/project-a/src && \
    echo "fn main() {}" > ~/app-test/project-a/src/main.rs && \
    echo "{}" > ~/app-test/project-b/config.json && \
    echo "notes" > ~/app-test/project-b/notes.txt && \
    sleep 5 && \
    echo "second version" > ~/app-test/project-a/file1.txt && \
    echo "updated" > ~/app-test/project-b/config.json && \
    sleep 5'
  echo "[PASS] Test data created"

  # Install Node + test dependencies (needs internet)
  echo "Setting up app test dependencies..."
  ssh_inet 'eval "$(/opt/homebrew/bin/brew shellenv)" && \
    command -v node || brew install node && \
    command -v rustc || (curl --proto "=https" --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y && source "$HOME/.cargo/env") && \
    command -v tauri-driver || (source "$HOME/.cargo/env" && cargo install tauri-driver --locked)'
  echo "[PASS] Dependencies installed"

  # Copy test files to VM
  echo "Copying app test files to VM..."
  ssh_cmd 'mkdir -p ~/e2e-tests/screenshots'
  scp_to "$SCRIPT_DIR/package.json" "~/e2e-tests/package.json"
  scp_to "$SCRIPT_DIR/tsconfig.json" "~/e2e-tests/tsconfig.json"
  scp_to "$SCRIPT_DIR/wdio.conf.ts" "~/e2e-tests/wdio.conf.ts"
  scp_to "$SCRIPT_DIR/app-test.spec.ts" "~/e2e-tests/app-test.spec.ts"
  scp_to "$SCRIPT_DIR/helpers.ts" "~/e2e-tests/helpers.ts"

  # Install npm dependencies (needs internet)
  ssh_inet 'eval "$(/opt/homebrew/bin/brew shellenv)" && cd ~/e2e-tests && npm install' || {
    echo "[FAIL] npm install failed"
    exit 1
  }
  echo "[PASS] npm dependencies installed"

  # Run app tests
  echo ""
  echo "--- Starting desktop app tests ---"
  # Start tauri-driver in background (WebDriver server on port 4444)
  # tauri-driver may not be supported on all macOS versions — check first
  DRIVER_CHECK=$(ssh_cmd 'source "$HOME/.cargo/env" && tauri-driver --version 2>&1' || true)
  if echo "$DRIVER_CHECK" | grep -qi "not supported"; then
    echo "[SKIP] tauri-driver not supported on this VM platform"
    echo "[SKIP] Desktop app tests skipped (CLI tests passed)"
    APP_EXIT_CODE=0
  else
    ssh_cmd 'source "$HOME/.cargo/env" && tauri-driver &'
    sleep 2  # Wait for tauri-driver to bind port

    ssh_cmd 'eval "$(/opt/homebrew/bin/brew shellenv)" && source "$HOME/.cargo/env" && cd ~/e2e-tests && npm run test:app'
    APP_EXIT_CODE=$?

    # Stop tauri-driver
    ssh_cmd 'pkill -f tauri-driver 2>/dev/null || true'
  fi

  APP_TEST_END=$(date +%s)
  APP_TEST_DURATION=$((APP_TEST_END - APP_TEST_START))

  echo "--- Desktop app tests complete ---"
  echo ""

  if [[ $APP_EXIT_CODE -ne 0 ]]; then
    # Pull screenshots for debugging
    echo "Pulling screenshots from VM..."
    sshpass -p admin scp -o StrictHostKeyChecking=no -o LogLevel=ERROR -r admin@"$VM_IP":~/e2e-tests/screenshots/ "$SCRIPT_DIR/screenshots/" 2>/dev/null || true
    echo "[FAIL] Desktop app tests failed (exit code: ${APP_EXIT_CODE})"
    exit $APP_EXIT_CODE
  fi

  echo "[PASS] Desktop app tests passed"
  echo "[TIMING] App test took ${APP_TEST_DURATION}s"

  # Cleanup app test data
  ssh_cmd 'eval "$(/opt/homebrew/bin/brew shellenv)" && unf stop 2>/dev/null; rm -rf ~/app-test ~/e2e-tests ~/.unfudged' || true
  echo ""
fi

# ============================================================================
# Summary
# ============================================================================

TOTAL_END=$(date +%s)
TOTAL_DURATION=$((TOTAL_END - START_TIME))

echo "==================================="
echo "E2E TEST COMPLETE"
echo "==================================="
echo "VM boot:        ${VM_BOOT_DURATION}s"
echo "CLI test:       ${TEST_DURATION}s"
if [[ "$CLI_ONLY" == "false" ]] && [[ -n "${APP_TEST_DURATION:-}" ]]; then
  echo "App test:       ${APP_TEST_DURATION}s"
fi
echo "Total time:     ${TOTAL_DURATION}s"
echo ""
echo "Exit code: 0"
