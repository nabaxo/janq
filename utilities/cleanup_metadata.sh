#!/bin/bash
set -e

echo "=== STARTING METADATA CLEANUP ==="

# 1. Clear KDE Metadata Trash
echo "[1/3] Clearing indexed metadata from Trash..."
rm -rf ~/.local/share/Trash/files/* 2>/dev/null || true
rm -rf ~/.local/share/Trash/info/* 2>/dev/null || true

# 2. Clear DrKonqi Crash Reports
echo "[2/3] Clearing DrKonqi crash reports..."
# These reports are often indexed by KDE and can cause "zombie" registrations
rm -rf ~/.cache/drkonqi/crashes/ruake.* 2>/dev/null || true
rm -rf ~/.cache/drkonqi/crashes/goake.* 2>/dev/null || true
rm -rf ~/.cache/drkonqi/crashes/dev.nabaxo.* 2>/dev/null || true

# 3. Refresh System Configuration Cache
echo "[3/3] Forcing KDE system configuration cache refresh..."
kbuildsycoca6 --noincremental || echo "Warning: kbuildsycoca6 failed, metadata might still be stale."

echo "=== METADATA CLEANUP COMPLETE ==="
