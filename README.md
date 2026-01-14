# janq - The Janky Quake-Style Terminal Manager sloperated in Rust

## janq is 100%, unadultareted vibe coded slop. User discretion is advised.

**janq** is a lightweight, high-performance Quake-style terminal wrapper. It manages your favorite terminal emulator (WezTerm, Alacritty, Kitty, Zed, etc.), allowing you to toggle it with a global hotkey, featuring smooth animations and multi-monitor support.

## Supported Platforms

- **Linux**: KDE Plasma 6 (Wayland via KWin scripts)
- **Windows**: Windows 10/11 (Native WinAPI)

## Key Features

- **Atomic Switching (Cross-Platform)**: Coordinated "swipe" animations—the outgoing app slides UP while the new one slides DOWN in perfect sync on both Linux and Windows.
- **Zero-Config Hotkeys (Cross-Platform)**: janq automatically registers global hotkeys. On Windows, it's native; on Linux (KDE), it syncs your TOML configuration directly with the system via D-Bus.
- **Intelligent App Resolution**: Smart fallback logic for single-app setups and strict validation for multi-app configurations.
- **Ordered Configuration**: The order of `[app]` sections in your config file determines their display order in the systray menu. The topmost application is the one that toggles when left-clicking the systray icon.
- **Robust Identification (Cross-Platform)**: Advanced weighted scoring system (Exact > Substring > Boundary > Subsequence) to reliably target the main window of complex apps like Obsidian, VS Code, and Zed.
- **High-Performance Linux Engine**: Zero-IPC liveness checks and batched window retrieval for near-instant toggling response.
- **Premium Animations**: Hardware-accelerated sliding with customizable easing (15+ curves including the premium `windows` curve).
- **Focus Restoration**: Remembers your previous window and restores focus instantly.
- **CLI Power**: Control your setup via `./janq --app <name>`.

## Installation

### Prerequisites
- **KDE Plasma 6** (Linux) or **Windows 10/11**.
- _(Optional: **musl-tools** for static Linux builds)._
- _(Optional: **mingw-w64** for Windows builds on Linux)._

### Build
```bash
make build-linux               # Binary: ./dist/janq
make build-linux-musl          # Binary: ./dist/janq (Static, recommended)
make build-windows             # Binary: ./dist/janq.exe
make build-windows-static      # Binary: ./dist/janq-portable.exe (Static/Portable, recommended)
```

> [!TIP]
> Look in `Makefile` for all the options.

### The `utilities/` Folder (For When Things Go Wrong)

The `utilities/` directory contains cleanup scripts for Linux. These exist because during development we managed to break KDE shortcuts, leave zombie processes, and generally make a mess of the desktop integration more times than we'd like to admit.

| Script | Description |
|--------|-------------|
| `full_cleanup.sh` | Nuclear option. Removes all janq/goake/ruake traces from your system. |
| `cleanup_shortcuts.sh` | Fixes KDE global shortcuts when they inevitably get stuck. |
| `cleanup_desktop.sh` | Removes desktop entries and icons. |
| `cleanup_processes.sh` | Kills any lingering daemon processes. |
| `cleanup_kwin.sh` | Removes KWin scripts. |
| `cleanup_metadata.sh` | Clears cached window IDs and metadata. |

If janq stops responding to hotkeys or you want a completely clean slate, these will save you. We know this because we've used them. A lot.

_(Sloperator note: Just use `full_cleanup.sh`.)_

## Usage

### Smart Startup & Toggling
- Run `./janq` to start the daemon.
- Subsequent calls toggle the primary window.
- Use `./janq --app name` to toggle a specific application from your config.

> [!TIP]
> **Single-App Peace of Mind**: If you only have one app configured, janq ignores typos and always picks that app. In multi-app mode, it validates your input and shows a helpful error window if an app isn't found.

### Linux (KDE)
janq generates a `.desktop` file and syncs your hotkeys to **KDE System Settings** automatically. Just run the daemon, and your shortcuts (e.g., `Meta+Grave`) will work instantly.

#### Linux Startup (Automatic)
To make janq start automatically on login:
```bash
./janq --enable-autostart
```
To disable it:
```bash
./janq --disable-autostart
```
These flags create/remove a symlink in `~/.config/autostart/` pointing to the application's desktop file.

### Windows

janq handles hotkeys natively as defined in your config. Right-click the tray icon to switch apps or quit.

#### Add janq to Windows Startup/Autostart (Manual)

To make janq start automatically when you log in:
1.  Press `Win + R`, type `shell:startup`, and press Enter.
2.  Right-click in the folder and select **New > Shortcut**.
3.  Browse to your `janq.exe` location.
4.  **Important**: To start in background mode, right-click the new shortcut, select **Properties**, and add ` --daemon` to the end of the **Target** field (e.g., `"C:\path\to\janq.exe" --daemon`).

### Window Class & Fuzzy Matching

The `window_class` field is highly flexible. janq uses **advanced weighted fuzzy matching** to find your app even if the name isn't exact (e.g., `obs` for `Obsidian`).

- **Context-Aware Scoring**:
  - **Exact/Substring**: +5000-10000 points.
  - **Word Boundaries**: +300 bonus for matches at the start of words or following delimiters (`.`, `-`, `_`, ` `).
  - **Consecutive Bonuses**: Exponential rewards for letters that appear in sequence.
  - **Gap Penalties**: Negative scores for characters skipped between letters.
- **High-Confidence Threshold**: janq rejects weak "junk" matches (score < 500), ensuring it will spawn a new instance rather than grabbing a random visible window.
- **Zero-IPC Liveness (Linux)**: Toggling an existing window verifies its existence via `/proc` in $<0.1$ms, ensuring zero latency during animation reversals.
- **Best Practice**: While the engine is robust, using an **exact match** (e.g., `wezterm`) is always recommended for maximum speed and deterministic behavior.

#### Recommended setup for Windows Terminal:
If `wt` is in your system `PATH`, this is the most reliable setup:
```toml
window_class = "CASCADIA_HOSTING_WINDOW_CLASS" # Official hosting class
start_command = "wt"
```
> [!NOTE]
> While janq's fuzzy matcher is strong, **Windows Terminal** is a complex UWP/WinUI app. Using the official `CASCADIA_HOSTING_WINDOW_CLASS` ensures it is caught reliably even when minimized or during its complex startup sequence.

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

janq searches for a configuration file in the following order:

1.  **Binary Directory** (Portable Mode):
    - Same folder as the `janq` executable.
2.  **XDG Config Directory**:
    - `~/.config/janq/janq.toml`
    - _On Windows_: `%AppData%\Roaming\janq\janq.toml`
3.  **User Configuration**:
    - `~/.janq.toml`
    - _On Windows_: `%UserProfile%\.janq.toml`

> [!CAUTION]
> **Data Integrity**: On Linux, running a binary from a directory that contains an empty/invalid config (if found in the binary folder) will _not_ overwrite your existing shortcuts. janq includes a safeguard to prevent destroying your system integration.

### Setup

Create `.janq.toml` in `~/.config/janq/` or your home directory.

### Global settings
```toml
[window]
display_mode = "active" # follow-mouse, specific, active
width = "50%"           # Supports %, px, "0" or "unset" to disable resizing.
height = "600px"
auto_show = false       # Show window on daemon startup

[animation]
show_duration = 350
show_easing = "cubic-bezier(0, 1, 1, 0)" # Or "bezier(0, 1, 1, 0)" or "(0, 1, 1, 0)"
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

> [!WARNING]
> Configuring a multiwindow app as `window_class` will act supremely janky. Do not do it. Or do. I'm not your mom. ¯\_(ツ)_/¯

### Default Values

| Section | Option | Default | Description | Per-App |
| :--- | :--- | :--- | :--- | :--- |
| `[app]` | `window_class` | **Required** | Window class/name to match for toggling | — |
| | `start_command` | **Required** | Command to launch the application | — |
| | `hotkey` | `"Meta+Grave"` | Global hotkey(s) to toggle the app | — |
| `[window]` | `display_mode` | `"follow-mouse"` | Monitor selection: `follow-mouse`, `active`, or `specific` | ✗ no |
| | `display_index` | `0` | Monitor index when `display_mode = "specific"` | ✗ no |
| | `width` | — | Window width (`%` or `px`) | ✓ yes |
| | `height` | — | Window height (`%` or `px`) | ✓ yes |
| | `keep_above` | `false` | Keep window above all others | ✗ no |
| | `force_priority` | `false` | (Linux) Use KWin Fullscreen state to sit on top of other fullscreen apps. **Note: janq removes window borders/chrome unconditionally for all managed windows.** | ✗ no |
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
| `expo`* | Exponential curve for a snappier, "high-speed" feeling. |
| `linear` | Direct, constant movement. |
| `ease`* | Smooth acceleration and deceleration. |
| `sine`* | Subtler sine-wave curve. |
| `cubic`* | Sharper deceleration. |
| `quart`* | Very sharp deceleration (popular for UI). |
| `back`* | Overshoots slightly before settling. |
| `cubic-bezier` | Custom CSS-style curve: `cubic-bezier(x1, y1, x2, y2)`. |

\* Supports `-in`, `-out`, and `-in-out` variants (e.g., `back-in`, `ease-out`, `quart-in-out`). The short name defaults to `-in-out`. **If an invalid string is provided, janq falls back to an `ease-out` curve.**

> [!TIP]
> **Custom Bezier Shortcuts**: You can also use `bezier(x1, y1, x2, y2)` or just `(x1, y1, x2, y2)` for brevity.

### Display Modes

The `display_mode` setting in the `[window]` section determines which monitor **janq** uses to display your applications.

| Mode | Description |
| :--- | :--- |
| `follow-mouse` | (**Default**) The window appears on the monitor where the mouse cursor is currently located. |
| `active` | The window appears on the monitor that currently has keyboard focus (the active window). |
| `specific` | The window always appears on a specific monitor, defined by `display_index`. |

> [!NOTE]
> When using `display_mode = "specific"`, you must also set `display_index` (0-indexed) to the desired monitor.

### Keycodes

janq supports a wide range of keycodes for defining hotkeys. Keys are case-insensitive.

**Modifiers:** `Ctrl`, `Alt`, `Shift`, `Meta` (Super/Windows/Cmd).
*   **Aliases:** `Control`, `Super`, `Win`, `Cmd`.
Multiple modifiers can be combined (e.g., `Meta+Shift+F`, `Ctrl+Alt+T`, or `ctrl+alt+shift+meta+z`).

> [!NOTE]
> **Multi-Hotkey Support**: `janq` supports up to **four hotkeys** per application on both Windows and Linux.

> [!TIP]
> **Single Hotkey Support**: You can define hotkeys without any modifiers (e.g., `hotkey = "F1"` or `hotkey = "PageUp"`).
>
> [!IMPORTANT]
> **Global Hijacking**: If you use a single character key (like `hotkey = "s"`) as a global shortcut, it will act globally across your system while the daemon is running. Pressing that key will toggle your application instead of typing the character. We recommend using **Function keys (`F1`-`F12`)** or **Navigation keys** for single-key hotkeys.

**Standard Keys:**
*   **Alphanumeric:** `a`-`z`, `0`-`9` (Case-insensitive)
*   **Function:** `f1`-`f12`
*   **Navigation:** `up` (`arrowup`), `down` (`arrowdown`), `left` (`arrowleft`), `right` (`arrowright`), `home`, `end`, `pgup` (`pageup`), `pgdn` (`pagedown`)
*   **Editing:** `insert`, `delete` (`del`), `backspace`, `tab`, `enter` (`return`), `space`, `esc` (`escape`), `capslock` (`caps_lock`)

**Punctuation & Symbols:**
*   `grave` / `backtick` / `` ` ``
*   `minus` ( `-` ), `equal` ( `=` )
*   `bracketleft` ( `[` ), `bracketright` ( `]` )
*   `backslash` ( `\` ), `slash` ( `/` )
*   `semicolon` ( `;` ), `quote` ( `'` )
*   `comma` ( `,` ), `period` ( `.` )

**International / Special:**
*   `section` ( `§` ) - *Mapped to `IntlBackslash` (the key below Esc on some ISO keyboards)*
*   `plusminus` / `±`
*   `dead_grave`

## Related Projects
- **zbus**: Facilitating D-Bus communication.
- **KWin Scripting API**: Direct integration for Wayland window management on Linux.

## Known Issues and other notes

### Linux: Hotkey registration delay
On KDE Plasma, there's a small intentional delay (~500ms) when registering or updating hotkeys. This is a workaround for a race condition in KWin's `GlobalShortcutsRegistry` that can cause crashes with rapid D-Bus operations. The delay only affects startup and config reloads, not toggle performance.

### App Compatibility: Opacity Animations
Some applications (especially Electron-based ones like Obsidian, VS Code, or Discord(Sloperator note: maybe?)) may experience unreliable or non-functional `animate_opacity`, particularly on Linux. This is often due to how these apps manage their own rendering buffers or "occlusion" optimizations that conflict with compositor-level transparency signals during motion.

**Note:** Just test and find out if enabling `animate_opacity` works for your particular app. If you notice flickering, "blank" windows during toggle, or if the animation just feels sluggish or weird, Just don't enable `animate_opacity` for that specific app in your config.

### Linux: Animation Artifacts (Ghosting / Jitter)
**If you experience intense jittering or "fighting" animations**, you likely have a third-party KWin effect active (like "Geometry Change") that is competing with janq to animate the window. To fix this:
- Open your KWin effect settings.
- Find the conflicting effect (e.g., "Geometry Change").
- Add the window classes managed by janq (e.g., `wezterm`, `obsidian`) to the effect's **Exclusion List**.

While **janq** is optimized for high-refresh displays and uses `ForceBlur` to stabilize transitions, some degree of content lag is currently an inherent platform limitation for these types of apps.

## License
Copyright (c) 2026 Nebez Kassem

Licensed under the [MIT](LICENSE) license.
