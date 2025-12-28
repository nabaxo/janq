# Rustake - Quake-Style Terminal Manager

A standalone Rust tool to manage a terminal window (like WezTerm) as a quake-style dropdown terminal.
Rewritten from Goake (Go) to Rust for better performance and safety.

## Supported Platforms

- **Linux**: KDE Plasma (Wayland only, via KWin scripts)
- **Windows**: Windows 10/11 (via WinAPI)

## Features

- **Dropdown Animations**: Smooth sliding toggle with configurable easing and duration
- **Opacity Animation**: Optional fade in/out effect with adjustable timing
- **Multi-Monitor Support**: Follows mouse cursor across displays with smooth transitions
- **Configurable**: Managed via `.goake.toml`
- **Lightweight**: Zero-runtime dependencies other than OS APIs
- **Pure Rust**: Blazing fast, safe, and efficient
- **Smart Start**: Running `rustake` starts the daemon or toggles the window automatically
- **Interruptible Animations**: Toggle mid-animation for instant reversal
- **Auto-Respawn**: Automatically restarts the terminal if closed while daemon is running
- **Hot-Reloading**: Config changes are applied instantly in real-time
- **Native IPC**: Uses D-Bus (Linux) and Named Pipes (Windows)
- **Global Hotkeys**: Built-in hotkey daemon on Windows (Linux relies on system shortcuts)
- **System Tray**: Native tray icon for management (Windows only)

## Prerequisites

- **Linux**: KDE Plasma (Wayland session), `wezterm` (or any terminal)
- **Windows**: `pwsh`, `cmd`, or `wezterm`, Windows 10/11
- Rust (Cargo) environment (if building from source)

## Installation

1. Clone the repository:
   ```bash
   git clone <repo-url>
   cd rustake
   ```
2. Build the binary:
   ```bash
   make
   # Or manually:
   cargo build --release
   ```
3. The binary will be at `target/release/rustake`.

## Usage

### Windows
1. Run `rustake.exe`. It will start in the background and show a tray icon.
2. Use the hotkey configured in `.goake.toml` (default `Meta+Grave`) to toggle.
3. Managing: Right-click the tray icon to Quit.

### Linux
1. **Toggle/Start**:
   Simply run the binary. It handles everything.
   ```bash
   ./target/release/rustake
   ```
2. **Global Shortcut Setup**:
   - Open **KDE System Settings**
   - Go to **Shortcuts** → **Commands**
   - Add legacy command: `/path/to/rustake` (absolute path recommended)
   - Assign `Meta+Grave` (or your preferred key)

## Configuration

Create `.goake.toml` in your home directory (`~` or `%USERPROFILE%`).

```toml
# Terminal settings
# Terminal settings
window_class = "wezquake"      # Linux: Window Class | Windows: Process Name (e.g. "wezterm-gui" or "wezquake")
start_command = "wezterm start --class wezquake" # On Windows, if using default `wezterm-gui`, change window_class above.
hotkey = ["Meta+Grave", "Meta+Space"] # Windows Only: Global hotkey(s)
keep_above = false

# Display settings
display_mode = "follow-mouse"  # "follow-mouse", "specific", or "active"
display_index = 0              # Only used if display_mode = "specific"

# Size (percentage or terminal dimensions)
width_percent = 40
height_percent = 40
width_cols = 120               # Takes precedence over percentage if > 0
height_rows = 40

# Animation
show_duration = 350            # milliseconds
hide_duration = 350
show_easing = "ease-out-cubic"
hide_easing = "ease-in-quart"

# Opacity animation
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
