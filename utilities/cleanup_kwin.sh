# Add to cleanup_shortcuts.sh or create cleanup_kwin.sh
if command -v qdbus6 >/dev/null 2>&1; then
    DBUS="qdbus6"
else
    DBUS="qdbus"
fi

$DBUS org.kde.KWin /Scripting org.kde.kwin.Scripting.unloadScript janq_toggle_engine || true
$DBUS org.kde.KWin /Scripting org.kde.kwin.Scripting.unloadScript janqz_toggle || true
$DBUS org.kde.KWin /Scripting org.kde.kwin.Scripting.unloadScript janq_grab || true
$DBUS org.kde.KWin /Scripting org.kde.kwin.Scripting.unloadScript janqz_grab || true
