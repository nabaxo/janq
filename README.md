# Goake - Quake-Style Terminal Manager for KDE Wayland

A standalone Go tool to manage a terminal window (like WezTerm) as a quake-style dropdown terminal on KDE Plasma Wayland.

## Features

- **Dropdown Animations**: Smooth sliding toggle with configurable easing and duration
- **Opacity Animation**: Optional fade in/out effect with adjustable timing
- **Multi-Monitor Support**: Follows mouse cursor across displays with smooth transitions
- **Configurable**: Managed via `.goake.toml`
- **Lightweight**: Zero-runtime dependencies other than D-Bus and KWin
- **Pure Go**: No CGO, no GTK, no heavy dependencies
- **Smart Start**: Running `goake` starts the daemon or toggles the window automatically
- **Interruptible Animations**: Toggle mid-animation for instant reversal
- **Auto-Respawn**: Automatically restarts the terminal if closed while daemon is running
- **Hot-Reloading**: Config changes are applied instantly in real-time
- **15+ Easing Curves**: sine, quart, cubic, back, and more

## Prerequisites

- KDE Plasma (Wayland session)
- `wezterm` (or any terminal you specify in config)
- Go (if building from source)

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

1. **Toggle/Start**:
   Simply run the binary. It handles everything.
   ```bash
   ./goake
   ```
   Bind this to your global shortcut (e.g., Meta+Grave).

2. **Configuration**:
   Create `.goake.toml` in your home directory or `~/.config/goake/`.

## Configuration

```toml
# Terminal settings
window_class = "wezquake"
start_command = "wezterm start --class wezquake"
hotkey = "Meta+Grave"
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

## Global Shortcut Setup

1. Open **KDE System Settings**
2. Go to **Shortcuts** → **Commands**
3. Add a new command: `/path/to/goake` (absolute path recommended)
4. Assign `Meta+Grave` (or your preferred key)

## Tray Icon

- **Left-Click**: Toggle terminal
- **Middle-Click**: Quit daemon

## License

MIT
