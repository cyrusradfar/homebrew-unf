#!/bin/bash
# UNFUDGED E2E Test Setup for Tauri WebDriver
#
# This script installs all dependencies needed to run E2E tests
# for the UNFUDGED Tauri desktop app via WebdriverIO.
#
# Usage:
#   ./tests/e2e/setup-tauri-driver.sh
#
# Requirements:
#   - Homebrew (for package management)
#   - Xcode Command Line Tools (for compilation)

set -euo pipefail

# Color codes for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Logging helpers
info() {
  echo -e "${GREEN}[INFO]${NC} $1"
}

warn() {
  echo -e "${YELLOW}[WARN]${NC} $1"
}

error() {
  echo -e "${RED}[ERROR]${NC} $1"
  exit 1
}

# ============================================================================
# macOS Platform Detection
# ============================================================================

if [[ ! "$OSTYPE" == "darwin"* ]]; then
  error "This script only supports macOS. Detected: $OSTYPE"
fi

info "Detected macOS platform"

# ============================================================================
# Check Homebrew
# ============================================================================

if ! command -v brew &>/dev/null; then
  error "Homebrew not found. Please install from https://brew.sh"
fi

info "Homebrew found: $(brew --version | head -1)"

# ============================================================================
# Check/Install Node.js
# ============================================================================

if ! command -v node &>/dev/null; then
  info "Node.js not found, installing via Homebrew..."
  brew install node
else
  info "Node.js found: $(node --version)"
fi

# ============================================================================
# Check/Install Rust (required for tauri-driver)
# ============================================================================

if ! command -v rustc &>/dev/null; then
  info "Rust not found, installing..."
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
  source "$HOME/.cargo/env"
else
  info "Rust found: $(rustc --version)"
fi

# ============================================================================
# Check/Install tauri-driver
# ============================================================================

if ! command -v tauri-driver &>/dev/null; then
  info "tauri-driver not found, installing via cargo..."
  cargo install tauri-driver --locked
else
  info "tauri-driver found: $(tauri-driver --version 2>/dev/null || echo 'version check failed')"
fi

# ============================================================================
# Verify UNFUDGED.app is installed
# ============================================================================

UNFUDGED_APP="/Applications/UNFUDGED.app"
if [[ ! -d "$UNFUDGED_APP" ]]; then
  warn "UNFUDGED.app not found at $UNFUDGED_APP"
  warn "Install with: brew install --cask cyrusradfar/unf/unfudged"
else
  info "UNFUDGED.app found at $UNFUDGED_APP"
fi

# ============================================================================
# Install Node.js dependencies
# ============================================================================

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
E2E_DIR="$SCRIPT_DIR"

info "Installing Node.js dependencies in $E2E_DIR..."
cd "$E2E_DIR"

if [[ ! -f "package.json" ]]; then
  error "package.json not found in $E2E_DIR"
fi

npm install

# ============================================================================
# Summary
# ============================================================================

echo ""
info "Setup complete!"
echo ""
echo "Next steps:"
echo "1. Ensure UNFUDGED.app is installed:"
echo "   brew install --cask cyrusradfar/unf/unfudged"
echo ""
echo "2. Run E2E tests:"
echo "   cd $E2E_DIR"
echo "   npm run test:app"
echo ""
echo "3. For debug output:"
echo "   npm run test:app:debug"
echo ""
