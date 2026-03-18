#!/bin/bash
set -euo pipefail

echo "Uninstalling UNFUDGED..."
echo ""

# Stop brew service first (cleanly unloads launchd plist)
if brew services list 2>/dev/null | grep -q "^unf "; then
  echo "Stopping brew service..."
  brew services stop unf 2>/dev/null || true
fi

# Stop sentinel (prevents it from respawning the daemon)
if pgrep -f "unf __sentinel" &>/dev/null; then
  echo "Stopping sentinel..."
  pkill -f "unf __sentinel" 2>/dev/null || true
  sleep 1
fi

# Stop the daemon
if command -v unf &>/dev/null; then
  echo "Stopping daemon..."
  unf stop 2>/dev/null || true
fi
if pgrep -f "unf __daemon" &>/dev/null; then
  echo "Sending SIGTERM to daemon..."
  pkill -f "unf __daemon" 2>/dev/null || true
  sleep 1
fi

# Uninstall casks
for cask in unfudged unfudged-staging; do
  if brew list --cask "$cask" &>/dev/null; then
    echo "Removing cask: $cask"
    brew uninstall --cask "$cask"
  fi
done

# Uninstall formulas
for formula in unf unf-staging; do
  if brew list "$formula" &>/dev/null; then
    echo "Removing formula: $formula"
    brew uninstall "$formula"
  fi
done

# Kill any remaining processes
pkill -f "unf __sentinel" 2>/dev/null || true
pkill -f "unf __daemon" 2>/dev/null || true
sleep 1

# Remove LaunchAgents (both legacy and brew-managed)
for plist in \
  "$HOME/Library/LaunchAgents/com.unfudged.sentinel.plist" \
  "$HOME/Library/LaunchAgents/com.unfudged.daemon.plist" \
  "$HOME/Library/LaunchAgents/homebrew.mxcl.unf.plist"; do
  if [ -f "$plist" ]; then
    echo "Removing LaunchAgent: $(basename "$plist")"
    launchctl bootout "gui/$(id -u)" "$plist" 2>/dev/null || true
    rm -f "$plist"
  fi
done

# Clear stopped marker so reinstall can start cleanly
rm -f "$HOME/.unfudged/stopped"

# Verify clean
echo ""
CLEAN=true
for formula in unf unf-staging; do
  if brew list "$formula" &>/dev/null; then
    echo "WARNING: $formula is still installed"
    CLEAN=false
  fi
done
for cask in unfudged unfudged-staging; do
  if brew list --cask "$cask" &>/dev/null; then
    echo "WARNING: $cask is still installed"
    CLEAN=false
  fi
done
if pgrep -f "unf __daemon" &>/dev/null; then
  echo "WARNING: daemon is still running"
  CLEAN=false
fi

if $CLEAN; then
  echo "Clean. All UNFUDGED components removed."
  echo "Snapshots preserved in ~/.unfudged/"
else
  echo "Some components could not be removed."
  exit 1
fi
