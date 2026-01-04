#!/bin/bash
set -e

echo "=== STARTING ULTIMATE RUAKE CLEANUP ==="

# 1. Kill Ruake
echo "[1/5] Killing Ruake daemon..."
pkill -9 ruake || true
sleep 1

# 2. Backup Config
echo "[2/5] Backing up kglobalshortcutsrc..."
cp ~/.config/kglobalshortcutsrc ~/.config/kglobalshortcutsrc.cleanup_backup_$(date +%s)

# 3. D-Bus Unregister Loop
echo "[3/5] Unregistering via D-Bus..."
# We iterate all potential component names we might have used
COMPONENTS=(
    "ruake-dev.nabaxo.ruake.desktop"
    "dev.nabaxo.ruake.desktop"
    "dev_nabaxo_ruake_desktop"
    "ruake"
    "dev.nabaxo.goake.desktop"
    "dev_nabaxo_goake_desktop"
    "goake"
)

for comp in "${COMPONENTS[@]}"; do
    if qdbus org.kde.kglobalaccel /kglobalaccel allActionsForComponent "$comp" &>/dev/null; then
        echo "  Found component: $comp"
        # Extract actions (handle qdbus formatting mess)
        # Using python for reliable parsing of qdbus output might be safer, but let's try strict grep/sed
        # qdbus output for allActionsForComponent is roughly: [Argument: a(sa(ai)) {[...], [...]}]
        # We'll use a loop based on known app names from config if listing fails

        # Try listing with simple grep
        qdbus --literal org.kde.kglobalaccel /kglobalaccel allActionsForComponent "$comp" | \
        grep -o '"[^"]*"' | \
        grep -v "Toggle" | grep -v "Ruake" | grep -v "Goake" | \
        cut -d'"' -f2 | \
        while read action; do
             if [[ -n "$action" ]]; then
                 echo "    Unregistering action: $action"
                 qdbus org.kde.kglobalaccel /kglobalaccel unregister "$comp" "$action" || true
             fi
        done

        # Explicitly unregister known apps just in case
        KNOWN_APPS=("wezquake" "kcalc" "zed" "vscode" "_launch")
        for app in "${KNOWN_APPS[@]}"; do
             qdbus org.kde.kglobalaccel /kglobalaccel unregister "$comp" "$app" >/dev/null 2>&1 || true
        done
    fi
done

# 4. Config File Purge
echo "[4/5] Scrubbing kglobalshortcutsrc..."
# Remove lines containing 'ruake' (case insensitive)
sed -i '/ruake/Id' ~/.config/kglobalshortcutsrc
sed -i '/goake/Id' ~/.config/kglobalshortcutsrc
# Remove blocks for dev.nabaxo.ruake.desktop if they remain (empty headers)
sed -i '/\[ruake-dev.nabaxo.ruake.desktop\]/d' ~/.config/kglobalshortcutsrc
sed -i '/\[dev.nabaxo.ruake.desktop\]/d' ~/.config/kglobalshortcutsrc
sed -i '/\[dev.nabaxo.goake.desktop\]/d' ~/.config/kglobalshortcutsrc
# Remove ghost entries (app keys that shouldn't be in unrelated sections)
# Don't use kwriteconfig6 as it escapes tabs incorrectly
for key in "wezquake" "kcalc" "zed" "vscode"; do
    # Remove lines that start with these app names (and are full entries, not partial matches)
    sed -i "/^${key}=/d" ~/.config/kglobalshortcutsrc
done
# Check if successful
if grep -qi "ruake" ~/.config/kglobalshortcutsrc; then
    echo "WARNING: Failed to fully scrub 'ruake' from config file."
    grep -i "ruake" ~/.config/kglobalshortcutsrc
else
    echo "  Config file scrubbed successfully."
fi


if grep -qi "goake" ~/.config/kglobalshortcutsrc; then
    echo "WARNING: Failed to fully scrub 'goake' from config file."
    grep -i "goake" ~/.config/kglobalshortcutsrc
else
    echo "  Config file scrubbed successfully."
fi

# 5. System Refresh
echo "[5/5] Refreshing KDE System Configuration Cache..."
kbuildsycoca6 --noincremental

echo "=== CLEANUP COMPLETE ==="
