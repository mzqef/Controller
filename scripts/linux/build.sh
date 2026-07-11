#!/usr/bin/env bash
#
# Build IntelliBoard for Linux (release profile).
#
# This script:
#   1. Checks for common system dependencies.
#   2. Runs `cargo build --release`.
#   3. Stages the deployable bundle into target/release/dist/.
#
# Usage:
#   ./scripts/linux/build.sh
#
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/../.." && pwd)"

cd "$ROOT_DIR"

echo "=== IntelliBoard Linux Build ==="
echo ""

# -------------------------------------------------------------------
# 1. Dependency check (informational — does not auto-install)
# -------------------------------------------------------------------
echo "[1/3] Checking system dependencies..."

check_dep() {
    if command -v "$1" &>/dev/null; then
        echo "  ✓ $1 found"
    else
        echo "  ✗ $1 NOT found (recommended)"
    fi
}

check_dep xdotool
check_dep wmctrl
check_dep pkg-config

# Check for X11 / Wayland dev libraries via pkg-config
if pkg-config --exists xcb 2>/dev/null; then
    echo "  ✓ libxcb found"
else
    echo "  ⚠ libxcb not found via pkg-config (install libxcb-randr0-dev / libxcb-devel)"
fi

# Check for /dev/uinput (for global hotkeys via rdev::grab)
if [ -e /dev/uinput ]; then
    if [ -w /dev/uinput ]; then
        echo "  ✓ /dev/uinput is writable"
    else
        echo "  ⚠ /dev/uinput exists but is NOT writable (hotkeys need: sudo usermod -aG input \$USER)"
    fi
else
    echo "  ⚠ /dev/uinput not found (hotkeys will be unavailable)"
fi

echo ""

# -------------------------------------------------------------------
# 2. Build
# -------------------------------------------------------------------
echo "[2/3] Building release binaries..."
cargo build --release
echo ""

# -------------------------------------------------------------------
# 3. Stage bundle
# -------------------------------------------------------------------
echo "[3/3] Staging deployable bundle..."
DIST_DIR="target/release/dist"
rm -rf "$DIST_DIR"
mkdir -p "$DIST_DIR/resources"

cp target/release/IntelliBoard "$DIST_DIR/"
cp target/release/memory_graph_ui "$DIST_DIR/" 2>/dev/null || echo "  (memory_graph_ui not built, skipping)"
cp target/release/functions_config_ui "$DIST_DIR/" 2>/dev/null || echo "  (functions_config_ui not built, skipping)"
cp target/release/hotkey_config_ui "$DIST_DIR/" 2>/dev/null || echo "  (hotkey_config_ui not built, skipping)"

# Config directory (build.rs already copies to target/release/config)
if [ -d "target/release/config" ]; then
    cp -r target/release/config "$DIST_DIR/"
elif [ -d "config" ]; then
    cp -r config "$DIST_DIR/"
fi

# Resources (icon)
if [ -d "resources" ]; then
    cp resources/icon.png "$DIST_DIR/resources/" 2>/dev/null || true
    cp resources/icon.ico "$DIST_DIR/resources/" 2>/dev/null || true
fi

# Optional files
[ -f README.md ] && cp README.md "$DIST_DIR/"
[ -f LICENSE ] && cp LICENSE "$DIST_DIR/"
[ -f .env.example ] && cp .env.example "$DIST_DIR/" || true

echo ""
echo "=== Build complete ==="
echo "Bundle staged at: $DIST_DIR"
echo ""
echo "To run from the bundle:"
echo "  cd $DIST_DIR"
echo "  ./IntelliBoard"
echo ""
echo "To install system-wide (optional):"
echo "  sudo mkdir -p /opt/intelliboard"
echo "  sudo cp -r $DIST_DIR/* /opt/intelliboard/"
echo "  sudo cp scripts/linux/intelliboard.desktop /usr/share/applications/"
