# Add to cleanup_shortcuts.sh or create cleanup_kwin.sh
qdbus org.kde.KWin /Scripting org.kde.kwin.Scripting.unloadScript janq_toggle_engine || true
qdbus org.kde.KWin /Scripting org.kde.kwin.Scripting.unloadScript janqz_toggle || true
qdbus org.kde.KWin /Scripting org.kde.kwin.Scripting.unloadScript janq_grab || true
qdbus org.kde.KWin /Scripting org.kde.kwin.Scripting.unloadScript janqz_grab || true
