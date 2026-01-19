#!/bin/bash
# cleanup_errors.sh
# Removes janq error temp files from /tmp

echo "Cleaning up janq error files..."

ERROR_FILES=(
    "/tmp/janq_error.txt"
)

for file in "${ERROR_FILES[@]}"; do
    if [ -f "$file" ]; then
        rm "$file"
        echo "Removed: $file"
    fi
done

echo "Error file cleanup complete."
