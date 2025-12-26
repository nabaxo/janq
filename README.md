# vibullshit - Quake-Style Terminal Manager for KDE Wayland

A standalone Go tool to manage a terminal window (like WezTerm) as a quake-style dropdown terminal on KDE Plasma Wayland.

## Features

- **Dropdown Animations**: Smooth sliding toggle using KWin scripting.
- **Display Awareness**: Supports "follow-mouse", "active screen", or specific monitor indices.
- **Configurable**: Managed via `config.toml`.
- **Lightweight**: Zero-runtime dependencies other than D-Bus and KWin.

## Prerequisites

- KDE Plasma (Wayland session)
- `wezterm` (or any terminal you specify in config)
- Go (if building from source)

## Installation

1. Clone the repository:
   ```bash
   git clone <repo-url>
   cd vibullshit
   ```
2. Build the binary:
   ```bash
   make
   ```

## Configuration

Edit `config.toml` to suit your needs:

```toml
window_class = "wezquake"
display_mode = "follow-mouse"
width_percent = 100
height_percent = 40
animation_duration = 300
animation_type = "slide"
```

## Usage

### 1. Start the Daemon
To have the tray icon and enable remote toggling (required for global shortcuts), start the program in daemon mode:
```bash
./vibullshit --daemon
```
You can add this to your KDE Autostart settings.

### 2. Prepare your Terminal
Start your terminal with the specified class:
```bash
wezterm start --class wezquake
```

### 3. Toggle Visibility
- **Via Tray Icon**:
    - **Left-Click**: Toggle Terminal.
    - **Middle-Click**: Quit Daemon.
    - *(Note: Right-click menu is not supported in pure D-Bus mode).*
- **Via Command Line**: Run `./vibullshit` (it will communicate with the running daemon).
- **Via Global Shortcut**: Map a key to `./vibullshit` in KDE Settings.

## Global Shortcut Setup

1. Open **KDE System Settings**.
2. Go to **Shortcuts** -> **Commands**.
3. Add a new command (e.g., "Toggle Quake").
- Set Command to `/path/to/vibullshit`.
- Assign a shortcut key (e.g., `Meta+Grave`, which is the Meta key plus the key right below Escape).

## License

MIT
