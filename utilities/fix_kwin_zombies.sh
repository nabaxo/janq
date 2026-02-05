#!/bin/bash
# utilities/fix_kwin_zombies.sh
# Aggressively unloads any janq scripts from KWin memory and clears shared memory.
# Use this if janq animations get stuck or if multiple scripts appear in KWin settings.

echo "janq: Searching for zombie scripts in KWin..."

# Get all script identifiers
# The 'scripts' method returns a list of names.
# We try to unload every janq-related name multiple times to be sure.
for script in $(qdbus org.kde.KWin /Scripting org.kde.kwin.Scripting.scripts | grep janq_); do
    echo "janq: Unloading $script..."
    qdbus org.kde.KWin /Scripting org.kde.kwin.Scripting.unloadScript "$script"
done

# Clear out the shared memory files we created
echo "janq: Cleaning shared memory..."
rm /dev/shm/janq_*.js 2>/dev/null

# Kill the daemon if it's still hanging around
killall -9 janq 2>/dev/null

echo "janq: KWin script environment cleaned. You can now restart janq."
