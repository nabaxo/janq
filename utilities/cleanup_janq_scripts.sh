#!/bin/bash

# janq_reset_kwin.sh
# Safely unloads all janq-related scripts from KWin memory.
# Use this if janq crashes or if focus_watcher scripts are stuck.

echo "janq: Looking for orphaned KWin scripts..."

# Get all script names and IDs from KWin Scripting object
# We look for scripts starting with 'janq_'
SCRIPTS=$(qdbus org.kde.KWin /Scripting org.kde.kwin.Scripting.scripts)

for script in $SCRIPTS; do
    if [[ $script == janq_* ]]; then
        echo "janq: Unloading $script..."
        qdbus org.kde.KWin /Scripting org.kde.kwin.Scripting.unloadScript "$script"
    fi
done

# Also kill any leftover janq processes
killall -9 janq 2>/dev/null

echo "janq: Cleanup complete. You can now restart janq."
