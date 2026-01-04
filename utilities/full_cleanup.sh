#!/bin/bash
set -e

# Get the directory of this script
DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" >/dev/null 2>&1 && pwd )"

echo "=== STARTING FULL RUAKE RESET ==="

# 1. Clean processes and lock files first
if [ -f "$DIR/cleanup_processes.sh" ]; then
    echo "--- Running process cleanup ---"
    bash "$DIR/cleanup_processes.sh"
else
    echo "Error: cleanup_processes.sh not found in $DIR"
    exit 1
fi

# 2. Clean D-Bus and shortcuts
if [ -f "$DIR/cleanup_shortcuts.sh" ]; then
    echo "--- Running shortcut and D-Bus cleanup ---"
    bash "$DIR/cleanup_shortcuts.sh"
else
    echo "Error: cleanup_shortcuts.sh not found in $DIR"
    exit 1
fi

# 3. Clean Metadata (Trash, Crashes, Cache)
if [ -f "$DIR/cleanup_metadata.sh" ]; then
    echo "--- Running metadata and cache cleanup ---"
    bash "$DIR/cleanup_metadata.sh"
else
    echo "Warning: cleanup_metadata.sh not found in $DIR, skipping metadata cleanup."
fi

echo "=== FULL RESET COMPLETE ==="
echo "You can now rebuild and restart the daemon."
