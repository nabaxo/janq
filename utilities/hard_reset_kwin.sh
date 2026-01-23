#!/bin/bash
# utilities/hard_reset_kwin.sh
# Aggressively cleans up Janq's KWin state

echo "--- Stopping Janq ---"
killall janq 2>/dev/null || true

echo "--- Unloading KWin Scripts ---"
SCRIPTS=("janq_toggle_engine" "janq_restore_script" "janq_grab" "janqz_toggle" "janqz_grab")
for script in "${SCRIPTS[@]}"; do
    qdbus org.kde.KWin /Scripting org.kde.kwin.Scripting.unloadScript "$script" 2>/dev/null || true
done

echo "--- Forcing KWin Reconfigure ---"
qdbus org.kde.KWin /KWin org.kde.KWin.reconfigure || true

echo "--- Cleaning D-Bus Service ---"
qdbus --session | grep org.janq | xargs -I {} qdbus {} / org.freedesktop.DBus.Peer.Ping 2>/dev/null || true

echo "--- Done. If things are still frozen, try 'kwin_x11 --replace &' or 'kwin_wayland --replace &' ---"
