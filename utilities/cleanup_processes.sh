#!/bin/bash

# Utility to forcefully cleanup janq, Ruake and Goake processes and locks
echo "Stopping any existing janq, Ruake and Goake processes..."
killall -9 janq ruake goake 2>/dev/null || true

echo "Removing stale lock files..."
rm -f /tmp/janq.lock
rm -f /tmp/ruake.lock

echo "Cleanup complete."
pgrep -f janq && echo "Warning: janq process still detected." || echo "janq cleared."
pgrep -f ruake && echo "Warning: Ruake process still detected." || echo "Ruake cleared."
pgrep -f goake && echo "Warning: Goake process still detected." || echo "Goake cleared."
