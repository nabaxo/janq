# Ruake - Quake-Style Terminal Manager

**Ruake** (formerly Rustake/Goake) is a lightweight, high-performance Quake-style terminal wrapper written in Rust. It manages your favorite terminal emulator (WezTerm, Alacritty, Kitty, etc.), allowing you to toggle it with a global hotkey, featuring smooth animations and multi-monitor support.

## Supported Platforms

- **Linux**: KDE Plasma (Wayland only, via KWin scripts)
- **Windows**: Windows 10/11 (via WinAPI)

## Features

- **Fast & Lightweight**: Written in Rust for minimal resource usage.
- **Cross-Platform**: Works on **Linux** (X11/Wayland via KWin/XDO) and **Windows** (Native Win32).
- **Smooth Animations**: Hardware-accelerated slide animations (ease-out-quart).
- **Focus Restoration**: Remembers your previous window and restores focus instantly when the terminal hides.
- **Smart Start**: Running `ruake` starts the daemon or toggles the window automatically.
- **DPI Aware**: Correctly handles multi-monitor setups with mixed DPIs.

## Installation

### Prerequisites

- **Rust**: Ensure you have `cargo` installed.
- **Terminal**: WezTerm is the default, but you can configure any supported terminal.
- **Linux**: `libxdo-dev` (if using X11/XDO), `kdotool` (for Wayland/KWin).

### Build from Source

1. Clone the repository:
   ```bash
   git clone https://github.com/nabaxo/ruake.git
   cd ruake
   ```

2. Build for your platform:
   - **Linux**:
     ```bash
     make build-linux
     ```
   - **Windows** (MinGW):
     ```bash
     make build-windows
     ```

3. The binary will be at `target/release/ruake` (Linux) or `target/release/ruake.exe` (Windows).

## Usage

### Windows

1. Run `ruake.exe`. It will start in the background and show a tray icon.
2. Press **F12** (default) to toggle the terminal.
3. Right-click the tray icon to Quit or Toggle.

### Linux

1. Run the binary:
   ```bash
   ./target/release/ruake
   ```
2. **Global Shortcut Setup**:
   - Open **KDE System Settings**
   - Go to **Shortcuts** → **Commands**
   - Add legacy command: `/path/to/ruake` (absolute path recommended)
   - Assign `Meta+Grave` (or your preferred key)

## Configuration

Create `.ruake.toml` (or `.goake.toml`) in your home directory (`~` or `%USERPROFILE%`).

```toml
[general]
# Terminal settings
window_class = "wezquake"      # Linux: Window Class | Windows: Process Name (e.g. "wezterm-gui" or "wezquake")
start_command = "wezterm start --class wezquake" # Command to launch the terminal
hotkey = ["Meta+Grave", "Meta+Space"] # Global hotkey(s) (Windows Only)

[window]
# Display settings
display_mode = "follow-mouse"  # "follow-mouse", "specific", or "active"
display_index = 0              # Only used if display_mode = "specific"
keep_above = false             # Keep window on top (Windows)

# Size (percentage or terminal dimensions)
width_percent = 40
height_percent = 40
width_cols = 160               # Takes precedence over percentage if > 0
height_rows = 40

[animation]
# Timing
show_duration = 350            # milliseconds
hide_duration = 350
show_easing = "windows"        # Easing function
hide_easing = "windows"

# Opacity
animate_opacity = true         # Set to false to disable fade effect
show_opacity_point = 0.2       # Fade-in completes at 20% of animation
hide_opacity_point = 0.8       # Fade-out starts at 80% of animation
```

### Easing Functions

- `linear`
- `ease-in`, `ease-out`, `ease-in-out`
- `sine-in`, `sine-out`, `sine-in-out`
- `cubic-in`, `cubic-out`, `cubic-in-out`
- `quart-in`, `quart-out`, `quart-in-out`
- `back-in`, `back-out`, `back-in-out`
- `windows` for `cubic-bezier(0.25, 0, 0, 1)`

## License

MIT
