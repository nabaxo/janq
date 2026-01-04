# Ruake - Quake-Style Terminal Manager

**Ruake** is a lightweight, high-performance Quake-style terminal wrapper. It manages your favorite terminal emulator (WezTerm, Alacritty, Kitty, Zed, etc.), allowing you to toggle it with a global hotkey, featuring smooth animations and multi-monitor support.

## Supported Platforms

- **Linux**: KDE Plasma 6 (Wayland via KWin scripts)
- **Windows**: Windows 10/11 (Native WinAPI)

## Key Features

- **Atomic Switching (Cross-Platform)**: Coordinated "swipe" animations—the outgoing app slides UP while the new one slides DOWN in perfect sync on both Linux and Windows.
- **Zero-Config Hotkeys (Cross-Platform)**: Ruake automatically registers global hotkeys. On Windows, it's native; on Linux (KDE), it syncs your TOML configuration directly with the system via D-Bus.
- **Intelligent App Resolution**: Smart fallback logic for single-app setups and strict validation for multi-app configurations.
- **Robust Identification (Cross-Platform)**: Advanced scoring system (Visibility > Class > Title > Size) to reliably target the main window of complex apps like Obsidian, VS Code, and Zed.
- **Premium Animations**: Hardware-accelerated sliding with customizable easing (e.g., "windows" curves).
- **Focus Restoration**: Remembers your previous window and restores focus instantly.
- **CLI Power**: Control your setup via `./ruake --app <name>`.

## Installation

### Prerequisites
- **KDE Plasma 6** (Linux) or **Windows 10/11**.
- **kdotool** (Linux/Wayland requirement).
- A terminal (WezTerm, Zed, VS Code, etc.).

### Build
```bash
make build-linux   # Binary: ./dist/ruake
make build-windows # Binary: ./dist/ruake.exe
```

## Usage

### Smart Startup & Toggling
- Run `./ruake` to start the daemon.
- Subsequent calls toggle the primary window.
- Use `./ruake --app name` to toggle a specific application from your config.

> [!TIP]
> **Single-App Peace of Mind**: If you only have one app configured, Ruake ignores typos and always picks that app. In multi-app mode, it validates your input and shows a helpful error window if an app isn't found.

### Linux (KDE)
Ruake generates a `.desktop` file and syncs your hotkeys to **KDE System Settings** automatically. Just run the daemon, and your shortcuts (e.g., `Meta+Grave`) will work instantly.

### Windows
Ruake handles hotkeys natively as defined in your config. Right-click the tray icon to switch apps or quit.

## Configuration

Create `.ruake.toml` in `~/.config/ruake/` or your home directory.

### Single App
```toml
[app]
window_class = "wezquake"
start_command = "wezterm --config initial_cols=160 --config initial_rows=40 start --class wezquake"
hotkey = "Meta+Grave"
```

### Multi-App
```toml
[app.terminal]
window_class = "wezquake"
start_command = "wezterm --config initial_cols=160 --config initial_rows=40 start --class wezquake"
hotkey = ["Meta+Grave", "Ctrl+Grave"]

[app.zed]
window_class = "zed"
start_command = "zed"
hotkey = "Meta+Z"
```

## Global settings
```toml
[window]
display_mode = "active" # follow-mouse, specific, active
width = "50%"           # Supports %, px, "0" or "unset" to disable resizing.
height = "600px"

[animation]
show_duration = 350
show_easing = "windows"
animate_opacity = true
```

### Easing Modes

| Mode | Short Name | Description |
| :--- | :--- | :--- |
| `windows` | - | (Default) High-end cubic-bezier curve matching modern Windows 11 animations. |
| `linear` | - | Direct, constant movement. |
| `ease-in-out` | `ease` | Smooth acceleration and deceleration. |
| `sine-in-out` | `sine` | Subtler version of `ease-out`. |
| `cubic-in-out` | `cubic` | Sharper deceleration. |
| `quart-in-out` | `quart` | Very sharp deceleration (popular for UI). |
| `back-in-out` | `back` | Slightly overshoots before settling. |
| `ease-in` | - | Starts slow, accelerates at the end. |
| `ease-out` | - | Starts fast, decelerates to a stop. |
| `back-in` | - | Anticipates movement by pulling back slightly before sliding. |
| `back-out` | - | Slightly overshoots before settling. |

> [!TIP]
> **Shortcuts**: You can use short names (e.g., `back`, `quart`) as a shortcut for the `-in-out` variant.

> [!NOTE]
> All `*-in`, `*-out`, and `*-in-out` variants (e.g. `quart-in-out`) are supported.

## Related Projects
- **kdotool**: Powering the Wayland window management on Linux.
- **zbus**: Facilitating D-Bus communication.

## Known Issues

### Linux: Hotkey registration delay
On KDE Plasma, there's a small intentional delay (~500ms) when registering or updating hotkeys. This is a workaround for a race condition in KWin's `GlobalShortcutsRegistry` that can cause crashes with rapid D-Bus operations. The delay only affects startup and config reloads, not toggle performance.

## License
MIT
