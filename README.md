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

1. Start your terminal with the specified class:
   ```bash
   wezterm start --class wezquake
   ```
2. Run `vibullshit` to toggle visibility.

### Global Shortcut Setup

1. Open **KDE System Settings**.
2. Go to **Shortcuts** -> **Commands**.
3. Add a new command (e.g., "Toggle Quake").
4. Set Command to `/path/to/vibullshit`.
5. Assign a shortcut key (e.g., `Alt+Space`).

## License

MIT
