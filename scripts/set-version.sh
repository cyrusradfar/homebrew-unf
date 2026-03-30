#!/bin/bash
set -euo pipefail

VERSION="${1:?Usage: $0 <version>}"

# Validate semver format
if ! echo "$VERSION" | grep -qE '^[0-9]+\.[0-9]+\.[0-9]+$'; then
    echo "Error: Version must be in X.Y.Z format"
    exit 1
fi

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT_DIR="$(dirname "$SCRIPT_DIR")"

# Cross-platform sed: macOS uses sed -i '' while Linux uses sed -i
if [[ "$OSTYPE" == "darwin"* ]]; then
    SED_ARGS=('-i' '')
else
    SED_ARGS=('-i')
fi

# Update root Cargo.toml
sed "${SED_ARGS[@]}" "s/^version = \".*\"/version = \"$VERSION\"/" "$ROOT_DIR/Cargo.toml"

# Update app/Cargo.toml (only the first version line)
# Use awk to replace only the first occurrence of version line
awk -v ver="$VERSION" 'NR==3 {print "version = \"" ver "\""; next} {print}' "$ROOT_DIR/app/Cargo.toml" > "$ROOT_DIR/app/Cargo.toml.tmp" && mv "$ROOT_DIR/app/Cargo.toml.tmp" "$ROOT_DIR/app/Cargo.toml"

# Update app/tauri.conf.json
# Use python3 for reliable JSON editing
python3 -c "
import json, sys
with open('$ROOT_DIR/app/tauri.conf.json', 'r') as f:
    data = json.load(f)
data['version'] = '$VERSION'
with open('$ROOT_DIR/app/tauri.conf.json', 'w') as f:
    json.dump(data, f, indent=2)
    f.write('\n')
"

echo "Version updated to $VERSION in:"
echo "  Cargo.toml"
echo "  app/Cargo.toml"
echo "  app/tauri.conf.json"
