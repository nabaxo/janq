#!/bin/bash

# This script finds and removes all window rules created by janq in kwinrulesrc

echo "Starting cleanup of janq KWin window rules..."

# 1. FIND AND DELETE EVERY JANQ GROUP BLOCK MANUALLY
FILE_PATH="$HOME/.config/kwinrulesrc"
if [ ! -f "$FILE_PATH" ]; then
    echo "kwinrulesrc not found at $FILE_PATH"
    exit 0
fi

echo "Force-purging janq rules via direct file manipulation..."

# We use a loop to surgically delete blocks because kwriteconfig6 --delete is failing
while true; do
    # Find the FIRST janq description and its line number
    SIG_LINE=$(grep -n "Description=janq automated icon fix" "$FILE_PATH" | head -n 1 | cut -d: -f1)

    if [ -z "$SIG_LINE" ]; then
        break
    fi

    # Find the [header] line immediately ABOVE this signature
    # We look backwards from the signature line for the first '['
    HEAD_LINE=$(head -n "$SIG_LINE" "$FILE_PATH" | grep -n "^\[" | tail -n 1 | cut -d: -f1)

    if [ -z "$HEAD_LINE" ]; then
        # Should not happen, but avoid infinite loop
        sed -i "${SIG_LINE}d" "$FILE_PATH"
        continue
    fi

    # Get the group name for logging
    GROUP_NAME=$(sed -n "${HEAD_LINE}p" "$FILE_PATH" | tr -d '[]\r')
    echo "Purging group: [$GROUP_NAME]"

    # Find where this block ends (the next [header] or end of file)
    NEXT_PARA=$(tail -n +$((HEAD_LINE + 1)) "$FILE_PATH" | grep -n "^\[" | head -n 1 | cut -d: -f1)

    if [ -n "$NEXT_PARA" ]; then
        # Found another group, stop before it
        END_LINE=$((HEAD_LINE + NEXT_PARA - 1))
    else
        # No more groups, delete to end of file
        END_LINE=$(wc -l < "$FILE_PATH")
    fi

    # Delete the block
    sed -i "${HEAD_LINE},${END_LINE}d" "$FILE_PATH"
done

# 2. CLEAN THE MASTER LIST
# Now that the blocks are physically gone, we update the General list to match
RULES_STR=$(kreadconfig6 --file kwinrulesrc --group General --key rules | tr -d '\r')
IFS=',' read -ra ADDR <<< "$RULES_STR"
NEW_RULES=()

for id in "${ADDR[@]}"; do
    if [ -z "$id" ]; then continue; fi
    # Only keep the rule if its header still exists in the file
    if grep -q "^\[$id\]" "$FILE_PATH"; then
        NEW_RULES+=("$id")
    fi
done

NEW_RULES_STR=$(IFS=,; echo "${NEW_RULES[*]}")
COUNT=${#NEW_RULES[@]}

echo "Finalizing [General] rules list and count to $COUNT..."
kwriteconfig6 --file kwinrulesrc --group General --key rules "$NEW_RULES_STR"
kwriteconfig6 --file kwinrulesrc --group General --key count "$COUNT"

# Ensure the file is flushed to disk
sync

echo "Reloading KWin..."
if command -v qdbus6 >/dev/null 2>&1; then
    qdbus6 org.kde.KWin /KWin org.kde.KWin.reconfigure
elif command -v qdbus >/dev/null 2>&1; then
    qdbus org.kde.KWin /KWin org.kde.KWin.reconfigure
fi

echo "Done."

echo "Reloading KWin..."
if command -v qdbus6 >/dev/null 2>&1; then
    qdbus6 org.kde.KWin /KWin org.kde.KWin.reconfigure
elif command -v qdbus >/dev/null 2>&1; then
    qdbus org.kde.KWin /KWin org.kde.KWin.reconfigure
fi

echo "Done. Purged $REMOVED_COUNT janq-related groups."
