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
show_duration = 300       # Show animation duration in ms
hide_duration = 300       # Hide animation duration in ms
show_easing = "ease-out"  # "linear", "ease-in", "ease-out", "ease-in-out"
hide_easing = "ease-in"
```

## Usage

### 1. Start the Daemon / Toggle Visibility
Run the program directly:
```bash
./vibullshit
```
- **First Run**:
  - Starts as a background daemon.
  - **Auto-checks** if `wezquake` is running.
  - If missing, **auto-starts** the terminal using `start_command` from config.
  - Minimizes the window to tray.
- **Subsequent Runs**: It instantly toggles the terminal window.

Add this command to your **KDE Autostart** setttings.

### 2. Global Shortcut Setup
1. Open **KDE System Settings**.
2. Go to **Shortcuts** -> **Commands**.
3. Add a new command: `/path/to/vibullshit` (absolute path recommended).
4. Assign `Meta+Grave` (or your preferred key).

### 3. Mouse Interaction
- **Tray Icon Left-Click**: Toggle Terminal.
- **Tray Icon Middle-Click**: Quit Daemon.

## License

MIT
