#!/bin/bash
echo "--- purgin Goake ---"
pkill -f goake
rm -f /home/nabaxo/.goake.toml
rm -f /home/nabaxo/.goake.toml.goake.toml
rm -f /home/nabaxo/.local/share/applications/*goake*.desktop
rm -f /home/nabaxo/.local/share/dbus-1/services/*goake*.service
rm -rf /home/nabaxo/.config/goake

# Scrub kglobalshortcutsrc for goake
python3 -c "
import os
path = os.path.expanduser('~/.config/kglobalshortcutsrc')
if os.path.exists(path):
    with open(path, 'r') as f:
        lines = f.readlines()
    new_lines = []
    skip = False
    for line in lines:
        if line.strip().startswith('[') and 'goake' in line.lower():
            skip = True
            continue
        if skip and line.strip().startswith('['):
            skip = False
        if not skip:
            new_lines.append(line)
    with open(path, 'w') as f:
        f.writelines(new_lines)
" 2>/dev/null

kbuildsycoca6 --noincremental
echo "--- Goake purged ---"
