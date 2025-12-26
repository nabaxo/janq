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
- **Gouake (Refactored)**: Prettier, faster, and more robust than the original.
- **Pure Go**: No CGO, no GTK, no heavy dependencies.
- **Wayland/KDE Native**: Uses D-Bus and KWin scripting for smooth window manipulation.
- **Smart Start**: Running `gouake` starts the daemon or toggles the window automatically.
- **Interruptible Animations**: Toggle mid-animation to instantly reverse the window.
- **Advanced Easing**: Support for `sine`, `quart`, `cubic`, and `back` curves.
- **Coordinate Robustness**: Intelligent multi-monitor and follow-mouse support.
- **Proper Hiding**: Windows are physically moved off-screen and minimized to avoid artifacts.

## Installation

1. Clone the repository:
   ```bash
   git clone <repo-url>
   cd gouake
   ```
2. Build the binary:
   ```bash
   make
   ```

## Usage

1.  **Toggle/Start**:
    Simply run the binary. It handles everything.
    ```bash
    ./gouake
    ```
    Bind this to your global shortcut (e.g., Meta+Grave).

2.  **Configuration**:
    Create `.gouake.toml` in your home directory or `~/.config/gouake/`.
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
3. Add a new command: `/path/to/gouake` (absolute path recommended).
4. Assign `Meta+Grave` (or your preferred key).

### 3. Mouse Interaction
- **Tray Icon Left-Click**: Toggle Terminal.
- **Tray Icon Middle-Click**: Quit Daemon.

## License

MIT
