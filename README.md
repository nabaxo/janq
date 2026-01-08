# Ruake - Quake-Style Terminal Manager

## Ruake is 100%, unadultareted vibe coded slop. User discretion is advised.

**Ruake** is a lightweight, high-performance Quake-style terminal wrapper. It manages your favorite terminal emulator (WezTerm, Alacritty, Kitty, Zed, etc.), allowing you to toggle it with a global hotkey, featuring smooth animations and multi-monitor support.

## Supported Platforms

- **Linux**: KDE Plasma 6 (Wayland via KWin scripts)
- **Windows**: Windows 10/11 (Native WinAPI)

## Key Features

- **Atomic Switching (Cross-Platform)**: Coordinated "swipe" animations—the outgoing app slides UP while the new one slides DOWN in perfect sync on both Linux and Windows.
- **Zero-Config Hotkeys (Cross-Platform)**: Ruake automatically registers global hotkeys. On Windows, it's native; on Linux (KDE), it syncs your TOML configuration directly with the system via D-Bus.
- **Intelligent App Resolution**: Smart fallback logic for single-app setups and strict validation for multi-app configurations.
- **Ordered Configuration**: The order of `[app]` sections in your config file determines their display order in the systray menu. The topmost application is the one that toggles when left-clicking the systray icon.
- **Robust Identification (Cross-Platform)**: Advanced scoring system (Visibility > Class > Title > Size) to reliably target the main window of complex apps like Obsidian, VS Code, and Zed.
- **Premium Animations**: Hardware-accelerated sliding with customizable easing (15+ curves)
- **Focus Restoration**: Remembers your previous window and restores focus instantly.
- **CLI Power**: Control your setup via `./ruake --app <name>`.

## Installation

### Prerequisites
- **KDE Plasma 6** (Linux) or **Windows 10/11**.
- **kdotool** (Linux/Wayland requirement).

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

### Windows Specifics: `window_class`

On Windows, the `window_class` field is highly flexible and matches against several properties. Ruake uses a **priority-based scoring system** to ensure it always grabs the correct window:

1.  **Process Name** (Highest Priority): The filename of the executable (e.g., `windowsterminal`, `wezterm-gui`, `zed`). Ruake even supports **fuzzy matching** (e.g., searching for "Windows Terminal" will correctly find `windowsterminal.exe`).
2.  **Window Class**: The technical internal class name (e.g., `CASCADIA_HOST_WINDOW_CLASS`).
3.  **Window Title**: The text shown in the title bar (e.g., "Windows Terminal").

#### Recommended setup for Windows Terminal:
If `wt` is in your system `PATH`, this is the most reliable setup:
```toml
window_class = "windowsterminal" # Or simply "Windows Terminal" (fuzzy match)
start_command = "wt"
```

#### Path Formatting (Windows)

When configuring `start_command` for local paths with backslashes or spaces, **use single quotes (`'`)** to treat the string as a literal:

```toml
window_class = "windowsterminal"
start_command = 'C:\Program Files\Terminal\wt.exe'
```

> [!TIP]
> **Pro Tip:** For most modern terminals (Windows Terminal, WezTerm, etc.), using the simple executable name (e.g., `start_command = "wt"` or `"wezterm"`) is preferred if they are in your system PATH.

## Configuration

### Search Priority

Ruake searches for a configuration file in the following order:

1.  **User Configuration** (Recommended):
    - `~/.ruake.toml` or `~/.goake.toml`
    - `~/.config/ruake/ruake.toml`
2.  **Binary Directory** (Portable Mode):
    - Same folder as the `ruake` executable.

> [!CAUTION]
> **Data Integrity**: On Linux, running a binary from a directory that contains an empty/invalid config (if found in the binary folder) will _not_ overwrite your existing shortcuts. Ruake includes a safeguard to prevent destroying your system integration.

### Setup

Create `.ruake.toml` in `~/.config/ruake/` or your home directory.

### Global settings
```toml
[window]
display_mode = "active" # follow-mouse, specific, active
width = "50%"           # Supports %, px, "0" or "unset" to disable resizing.
height = "600px"
auto_show = false       # Show window on daemon startup

[animation]
show_duration = 350
show_easing = "windows"
animate_opacity = true
```

### Single App configuration
```toml
[app]
# On Windows: Matches Process Name (e.g. "wezterm-gui") OR Window Class
window_class = "wezquake"
start_command = "wezterm --config initial_cols=160 --config initial_rows=40 start --class wezquake"
hotkey = "Meta+Grave"
```

### Multi-App configuration
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

### Default Values

| Section | Option | Default | Description | Per-App |
| :--- | :--- | :--- | :--- | :--- |
| `[app]` | `window_class` | **Required** | Window class/name to match for toggling | — |
| | `start_command` | **Required** | Command to launch the application | — |
| | `hotkey` | `"Meta+Grave"` | Global hotkey(s) to toggle the app | — |
| `[window]` | `display_mode` | `"follow-mouse"` | Monitor selection: `follow-mouse`, `active`, or `specific` | ✗ no |
| | `display_index` | `0` | Monitor index when `display_mode = "specific"` | ✗ no |
| | `width` | — | Window width (`%` or `px`) | ✓ yes |
| | | `height` | — | Window height (`%` or `px`) | ✓ yes |
| | `keep_above` | `false` | Keep window above all others | ✗ no |
| | `force_priority` | `false` | (Linux) Use KWin Fullscreen state to sit on top of other fullscreen apps. **Note: Ruake removes window borders/chrome unconditionally for all managed windows.** | ✗ no |
| | `auto_show` | `false` | Show window on daemon startup | ✗ no |
| `[animation]` | `show_duration` | `350` (ms) | Duration of the show animation | ✗ no |
| | `hide_duration` | `350` (ms) | Duration of the hide animation | ✗ no |
| | `show_easing` | `"ease"` | Easing curve for showing | ✗ no |
| | `hide_easing` | `"ease"` | Easing curve for hiding | ✗ no |
| | `animate_opacity` | `false` | Fade opacity during animations | ✓ yes |
| | `show_opacity_point` | `0.2` | Animation progress (0-1) by which the window becomes fully opaque | ✗ no |
| | `hide_opacity_point` | `0.8` | Animation progress (0-1) when fade-out starts | ✗ no |

### Easing Modes

| Mode | Description |
| :--- | :--- |
| `windows` | High-end cubic-bezier curve matching modern Windows 11 animations. |
| `linear` | Direct, constant movement. |
| `ease`* | Smooth acceleration and deceleration. |
| `sine`* | Subtler sine-wave curve. |
| `cubic`* | Sharper deceleration. |
| `quart`* | Very sharp deceleration (popular for UI). |
| `back`* | Overshoots slightly before settling. |

\* Supports `-in`, `-out`, and `-in-out` variants (e.g., `back-in`, `ease-out`, `quart-in-out`). The short name defaults to `-in-out`. **If an invalid string is provided, Ruake falls back to an `ease-out` curve.**

### Premium Features

#### Sticky Swipe (Linux)
When using `display_mode = "active"`, Ruake features "Sticky Swipe." If you toggle between two different applications while one is already visible, the new application will appear on the **same monitor** as the outgoing one, even if your mouse or focus has moved. This keeps the coordinated "swipe" animation grounded on a single screen for a more premium feel.

### Keycodes

Ruake supports a wide range of keycodes for defining hotkeys. Keys are case-insensitive.

**Modifiers:** `Ctrl`, `Alt`, `Shift`, `Meta` (Super/Windows/Cmd).
Multiple modifiers can be combined (e.g., `Meta+Shift+F`, `Ctrl+Alt+T`, or `ctrl+alt+shift+meta+z`, whatever floats your boat).

**Standard Keys:**
*   **Alphanumeric:** `a`-`z`, `0`-`9`
*   **Function:** `f1`-`f12`
*   **Navigation:** `up`, `down`, `left`, `right`, `home`, `end`, `pgup` (pageup), `pgdn` (pagedown)
*   **Editing:** `insert`, `delete` (del), `backspace`, `tab`, `enter` (return), `space`, `esc` (escape)

**Punctuation & Symbols:**
*   `grave` / `backtick` ( ` )
*   `minus` ( - ), `equal` ( = )
*   `bracketleft` ( [ ), `bracketright` ( ] )
*   `backslash` ( \ ), `slash` ( / )
*   `semicolon` ( ; ), `quote` ( ' )
*   `comma` ( , ), `period` ( . )

**International / Special:**
*   `section` ( § ) - *Mapped to `IntlBackslash` (the key below Esc on some ISO keyboards)*
*   `plusminus`
*   `dead_grave`

## Related Projects
- **kdotool**: Powering the Wayland window management on Linux.
- **zbus**: Facilitating D-Bus communication.

## Known Issues

### Linux: Hotkey registration delay
On KDE Plasma, there's a small intentional delay (~500ms) when registering or updating hotkeys. This is a workaround for a race condition in KWin's `GlobalShortcutsRegistry` that can cause crashes with rapid D-Bus operations. The delay only affects startup and config reloads, not toggle performance.

## License
MIT
