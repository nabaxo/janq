# Goake - Quake-Style Terminal Manager for KDE Wayland

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
- **Goake (Refactored)**: Prettier, faster, and more robust than the original.
- **Pure Go**: No CGO, no GTK, no heavy dependencies.
- **Wayland/KDE Native**: Uses D-Bus and KWin scripting for smooth window manipulation.
- **Smart Start**: Running `goake` starts the daemon or toggles the window automatically.
- **Interruptible Animations**: Toggle mid-air and witness instant reversal.
- **Stable Multi-Monitor Support**: Uses bottom-edge detection to prevent "see-sawing" between displays.
- **Terminal Dimensions (Rows/Cols)**: Set specific size for terminal-heavy workflows.
- **Auto-Respawn**: Automatically restarts the terminal if closed while the daemon is running.
- **Hot-Reloading**: Config changes are applied instantly in real-time.
- **Follow-Mouse**: Smartly summons to the monitor where your mouse is.
- **Improved Easing**: Support for 15+ curves (sine, quart, cubic, back).
- **Proper Hiding**: Opacity masking to prevent ghosting.

## Installation

1. Clone the repository:
   ```bash
   git clone <repo-url>
   cd goake
   ```
2. Build the binary:
   ```bash
   make
   ```

## Usage

1.  **Toggle/Start**:
    Simply run the binary. It handles everything.
    ```bash
    ./goake
    ```
    Bind this to your global shortcut (e.g., Meta+Grave).

2.  **Configuration**:
    Create `.goake.toml` in your home directory or `~/.config/goake/`.
    ```toml
    window_class = "wezquake"
    show_duration = 300
    show_easing = "sine-out"
    ```

3.  **Tray Icon**:
    -   **Left-Click**: Toggle.
    -   **Middle-Click**: Quit.

### 2. Global Shortcut Setup
1. Open **KDE System Settings**.
2. Go to **Shortcuts** -> **Commands**.
3. Add a new command: `/path/to/goake` (absolute path recommended).
4. Assign `Meta+Grave` (or your preferred key).

### 3. Mouse Interaction
- **Tray Icon Left-Click**: Toggle Terminal.
- **Tray Icon Middle-Click**: Quit Daemon.

## License

MIT
