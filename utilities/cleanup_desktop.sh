#!/bin/bash
# cleanup_desktop.sh
# Removes deprecated Ruake desktop files to prevent duplication in application launchers.

echo "Cleaning up Ruake desktop files..."

APP_DIR="$HOME/.local/share/applications"
DEPRECATED_FILES=(
    "$APP_DIR/ruake.desktop"
    "$APP_DIR/ruake-dev.nabaxo.ruake.desktop"
    "$APP_DIR/dev.nabaxo.ruake.desktop"
    "$APP_DIR/janq.desktop"
)

for file in "${DEPRECATED_FILES[@]}"; do
    if [ -f "$file" ]; then
        echo "Found deprecated file: $file"
        rm "$file"
        echo "Successfully removed $file"
    fi
done

# Optional: Check for other potential stale files (e.g. if we ever had other names)
# find "$APP_DIR" -name "*ruake*.desktop" -not -name "dev.nabaxo.ruake.desktop" -print

echo "Desktop cleanup complete. Please ensure 'dev.nabaxo.janq.desktop' is present after running janq daemon."
