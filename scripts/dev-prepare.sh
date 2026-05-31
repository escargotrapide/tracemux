#!/usr/bin/env bash
# Prepare the Tauri dev environment:
#   1. Build tracemux-cli (debug)
#   2. Copy binary to app-tauri/src-tauri/binaries/ (Tauri sidecar)
#   3. Generate a placeholder icon.png and icon.ico (if not already present)
#
# Run this once before `just dev-tauri` or `just dev-all`.
set -euo pipefail

RELEASE=""
while [[ $# -gt 0 ]]; do
    case "$1" in
        --release) RELEASE="--release"; shift ;;
        *) echo "Unknown option: $1" >&2; exit 1 ;;
    esac
done

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PROFILE="debug"; [[ -n "$RELEASE" ]] && PROFILE="release"

# 1. Build
echo "Building tracemux-cli..."
cargo build $RELEASE -p tracemux-cli

# 2. Copy sidecar
TRIPLE="$(rustc -vV | sed -n 's/^host: //p')"
BIN_DIR="$ROOT/app-tauri/src-tauri/binaries"
mkdir -p "$BIN_DIR"
cp "$ROOT/target/$PROFILE/tracemux" "$BIN_DIR/tracemux-${TRIPLE}"
echo "  Sidecar: $BIN_DIR/tracemux-${TRIPLE}"

# 3. Placeholder icons
ICON_DIR="$ROOT/app-tauri/src-tauri/icons"
mkdir -p "$ICON_DIR"

if [[ ! -f "$ICON_DIR/icon.png" ]]; then
    # Minimal valid 1x1 RGB PNG (base64-encoded)
    echo "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAIAAACQd1PeAAAADklEQVQI12P4z8BQDwADhQGAWjR9awAAAABJRU5ErkJggg==" \
        | base64 -d > "$ICON_DIR/icon.png"
    echo "  icon.png (placeholder): $ICON_DIR/icon.png"
fi

if [[ ! -f "$ICON_DIR/icon.ico" ]]; then
    # Minimal valid ICO (hex bytes piped through xxd)
    printf '\x00\x00\x01\x00\x01\x00\x01\x01\x00\x00\x01\x00\x18\x00\x30\x00\x00\x00\x16\x00\x00\x00\x28\x00\x00\x00\x01\x00\x00\x00\x02\x00\x00\x00\x01\x00\x18\x00\x00\x00\x00\x00\x08\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00' \
        > "$ICON_DIR/icon.ico"
    echo "  icon.ico (placeholder): $ICON_DIR/icon.ico"
fi

echo "dev-prepare complete. You can now run: just dev-tauri"
