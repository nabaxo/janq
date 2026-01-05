#!/bin/bash
# cleanup_desktop.sh
# Removes deprecated Ruake desktop files to prevent duplication in application launchers.

echo "Cleaning up Ruake desktop files..."

APP_DIR="$HOME/.local/share/applications"
DEPRECATED_FILE="$APP_DIR/ruake.desktop"

if [ -f "$DEPRECATED_FILE" ]; then
    echo "Found deprecated file: $DEPRECATED_FILE"
    rm "$DEPRECATED_FILE"
    if [ $? -eq 0 ]; then
        echo "Successfully removed $DEPRECATED_FILE"
    else
        echo "Error: Failed to remove $DEPRECATED_FILE"
        exit 1
    fi
else
    echo "No deprecated 'ruake.desktop' file found."
fi

# Optional: Check for other potential stale files (e.g. if we ever had other names)
# find "$APP_DIR" -name "*ruake*.desktop" -not -name "dev.nabaxo.ruake.desktop" -print

echo "Desktop cleanup complete. Please ensure 'dev.nabaxo.ruake.desktop' is present after running Ruake."
