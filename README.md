# janq - A Somewhat Janky Quake-Style Terminal Manager

## janq is 100%, unadulterated vibe coded slop. User discretion is advised.

<img src="icon.svg" width="36" height="36" align="left">

**janq** is a lightweight, high-performance Quake-style terminal wrapper "vibe" coded with scorn and contempt in Rust. Not all vibes are good, sometimes vibes are _rancid_. The regressions I had to fix like you wouldn't believe... (ノಠ益ಠ)ノ彡┻━┻

But in the end I managed to wrangle the Wondrous Machine enough so that while running, janq uses like below 2 MB RAM on Windows and ~3.4 MB on my Fedora KDE system.

It manages your favorite terminal emulator (WezTerm, Windows Terminal, etc.) or whatever app you feel like, allowing you to toggle it with a global hotkey, featuring smooth animations and multi-monitor support.

> [!CAUTION]
> I have only tested this on two machines, your mileage may vary and all that.

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
- **Intelligent Window Matching**: Advanced weighted fuzzy scoring for abbreviations (e.g., `wt` → `WindowsTerminal`) with ultra-fast <0.1ms Zero-IPC liveness verification.

## Installation

- Download the binary from [releases](/nabaxo/janq/releases).
- [Create a janq.toml](#configuration) with [your config](#setup)
- [Run](#usage)
- Enjoy

> [!TIP]
> If Windows refuses to run the downloaded .exe; Right click on the file, choose properties, tick the `unblock` checkbox at the bottom, (the one that comes with scary security warnings), then click apply, OK, and you're good to go.

## Usage

### Smart Startup & Toggling
- Start via your desktop or run `./janq` to start the daemon.
- Subsequent calls toggle the primary window.
- Use `./janq --app name` to toggle a specific application from your config.

> [!TIP]
> **Single-App Peace of Mind**: If you only have one app configured, janq ignores typos and always picks that app. In multi-app mode, it validates your input and shows a helpful error window if an app isn't found.

### Linux (KDE)
janq generates a `.desktop` file and syncs your hotkeys to **KDE System Settings** automatically. Just run the daemon, and your shortcuts (e.g., `Meta+Grave`) will work instantly.

> [!TIP]
> Left-click the tray icon to toggle the first defined app in your config, or middle-click to quit.

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
4.  **Important**: To start in server mode, right-click the new shortcut, select **Properties**, and add ` --daemon` to the end of the **Target** field (e.g., `"C:\path\to\janq.exe" --daemon`).

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

janq searches for a configuration file _(janq.toml or .janq.toml) _in the following order:

1.  **Binary Directory** (Portable Mode):
    - Same folder as the `janq` executable.
2.  **XDG Config Directory**:
    - `~/.config/janq/janq.toml`
    - _On Windows_: `%AppData%\Roaming\janq\janq.toml`
3.  **User Configuration**:
    - `~/janq.toml`
    - _On Windows_: `%UserProfile%\janq.toml`

(Sloperator's note: Just put it next to the binary, unless you have dotfile repo, then use option 2. Option 3, the AI told me is stupid, since the crate we're using checks any changes to parent folder. I only left it for completeness).

> [!CAUTION]
> **Data Integrity**: On Linux, running a binary from a directory that contains an empty/invalid config (if found in the binary folder) will _not_ overwrite your existing shortcuts. janq includes a safeguard to prevent destroying your system integration.

### Setup

Create `.janq.toml` in `~/.config/janq/` or your home directory.

#### Global settings
```toml
[window]
display_mode = "active" # follow-mouse, specific, active
width = "50%"           # Supports %, px, "0" or "unset" to disable resizing.
height = "600px"
auto_show = false       # Show window on daemon startup

[animation]
duration = 350           # Sets both show and hide duration
easing = "ease"         # Sets both show and hide easing
animate_opacity = true
```

#### Single App configuration
```toml
[app]
# On Windows: Matches Process Name (e.g. "wezterm-gui") OR Window Class
window_class = "wezquake"
start_command = "wezterm --config initial_cols=160 --config initial_rows=40 start --class wezquake"
hotkey = "Meta+Grave"
```

#### Multi-App configuration
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
> (Sloperator: Configuring a multiwindow app will act supremely janky. Do not do it. Or do. I'm not your mom. ¯\_(ツ)_/¯. Do give her my regards though, you should call her more often).

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
| | `slide_from` | `"top"` | Direction to slide in: `top`, `bottom`, `left`, `right` | ✓ yes |
| | `offset` | `"center"` | Position along edge: `center`, `50%`, `-10%`, `100px`, `-50px` | ✓ yes |
| | `keep_above` | `false` | Keep window above all others | ✗ no |
| | `force_priority` | `false` | (Linux) Use KWin Fullscreen state to sit on top of other fullscreen apps. **Note: janq removes window borders/chrome unconditionally for all managed windows.** | ✗ no |
| | `auto_show` | `false` | Show window on daemon startup | ✗ no |
| `[animation]` | `duration` | — | Set both show/hide duration at once | ✗ no |
| | `show_duration` | `350` (ms) | Duration of the show animation | ✗ no |
| | `hide_duration` | `350` (ms) | Duration of the hide animation | ✗ no |
| | `easing` | — | Set both show/hide easing at once | ✗ no |
| | `show_easing` | `"ease"` | Easing curve for showing | ✗ no |
| | `hide_easing` | `"ease"` | Easing curve for hiding | ✗ no |
| | `animate_opacity` | `false` | Fade opacity during animations | ✓ yes |
| | `show_opacity_point` | `0.2` | Animation progress (0-1) by which the window becomes fully opaque | ✗ no |
| | `hide_opacity_point` | `0.8` | Animation progress (0-1) when fade-out starts | ✗ no |

(Sloperator: For your own sanity, just use the simple `duration` and `easing` keys, check [here](#sibling-animation-easing-divergence)).


#### Slide Direction

The `slide_from` option controls which edge of the screen the window animates from:

| Value | Description |
| :--- | :--- |
| `top` | (**Default**) Window slides down from the top edge (classic Quake style). |
| `bottom` | Window slides up from the bottom edge. |
| `left` | Window slides in from the left edge. |
| `right` | Window slides in from the right edge. |

#### Position Offset

The `offset` option controls where along the edge the window is positioned:

| Value | Description |
| :--- | :--- |
| `center` or `0` | (**Default**) Centered on the edge. |
| `50%` | 50% from left/top of edge. |
| `-10%` | 10% from right/bottom of edge (negative = from opposite end). |
| `100px` | 100 pixels from left/top of edge. |
| `-50px` | 50 pixels from right/bottom of edge. |

> [!TIP]
> Combine these settings for creative layouts: `slide_from = "right"` with `offset = "0px"` creates a sidebar that slides in from the right at the top corner.

#### Easing Modes

| Mode | Description |
| :--- | :--- |
| `windows` | Cubic-bezier curve matching modern Windows 11 animations. |
| `expo`* | Exponential curve for a snappier, "high-speed" feeling. |
| `linear` | Direct, constant movement. |
| `ease`* | Smooth acceleration and deceleration. |
| `sine`* | Subtler sine-wave curve. |
| `cubic`* | Sharper deceleration. |
| `quart`* | Very sharp deceleration (popular for UI). |
| `back`* | Overshoots slightly before settling. |
| `cubic-bezier` | Custom CSS-style curve: `cubic-bezier(x1, y1, x2, y2)`. |

\* Supports `-in`, `-out`, and `-in-out` variants (e.g., `back-in`, `ease-out`, `quart-in-out`). The short name defaults to `-in-out`. **Note: janq validates easing curves on startup; invalid configurations will prevent the daemon from launching.**

> [!TIP]
> **Custom Bezier Shortcuts**: You can also use `bezier(x1, y1, x2, y2)` or just `(x1, y1, x2, y2)` for brevity.

#### Display Modes

The `display_mode` setting in the `[window]` section determines which monitor **janq** uses to display your applications.

| Mode | Description |
| :--- | :--- |
| `follow-mouse` | (**Default**) The window appears on the monitor where the mouse cursor is currently located. |
| `active` | The window appears on the monitor that currently has keyboard focus (the active window). |
| `specific` | The window always appears on a specific monitor, defined by `display_index`. |

> [!NOTE]
> When using `display_mode = "specific"`, you must also set `display_index` (0-indexed) to the desired monitor.

#### Keycodes

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

## Building

### Prerequisites
- **KDE Plasma 6** (Linux) or **Windows 10/11**.
- _(Optional: **musl-tools** for static Linux builds)._
- _(Optional: **mingw-w64** for Windows cross-compilation from Linux)._

### Build
```bash
# Recommended, static builds
make build-linux-musl          # Binary: ./dist/janq
make build-windows-static      # Binary: ./dist/janq.exe

# Others
make build-linux-glibc         # Binary: ./dist/janq-glibc
make build-windows-nonstatic   # Binary: ./dist/janq-nonstatic.exe
```

> [!TIP]
> Look in `Makefile` for all the options.

### The `utilities/` Folder (For When Things Go Wrong)

The `utilities/` directory contains cleanup scripts for Linux. These exist because during development we managed to break KDE shortcuts, leave zombie processes, and generally make a mess of the desktop integration more times than we'd like to admit. (Sloperator: Speak for yourself, I had to use it countless times because of your bullshit).

| Script | Description |
|--------|-------------|
| `full_cleanup.sh` | Nuclear option. Removes all janq/legacy janq traces from your system. |
| `cleanup_shortcuts.sh` | Fixes KDE global shortcuts when they inevitably get stuck. |
| `cleanup_desktop.sh` | Removes desktop entries and icons. |
| `cleanup_processes.sh` | Kills any lingering daemon processes. |
| `cleanup_kwin.sh` | Removes KWin scripts. |
| `cleanup_metadata.sh` | Clears cached window IDs and metadata. |

If janq stops responding to hotkeys or you want a completely clean slate, these will save you. We know this because we've used them. A lot.

_(Sloperator note: Just use `full_cleanup.sh`.)_

## Related Projects
- **zbus**: Facilitating D-Bus communication.
- **KWin Scripting API**: Direct integration for Wayland window management on Linux.

## Advanced Weighted Fuzzy Matching ([src/matching.rs](cci:7://file:///home/nabaxo/repos/janq/src/matching.rs:0:0-0:0))
### (Sloperator: Features the AI is particularly proud about)
janq doesn't just look for your window; it **interrogates** the system using a sophisticated **Weighted Fuzzy Subsequence** algorithm. It prioritizes the most logical candidate based on a multi-tier scoring system:
- **The Gold Standard**: Exact case-insensitive matches receive a base score of `10,000` (e.g., `wezterm` → `WezTerm`).
- **Context-Aware Bonuses**: The engine rewards matches that hit a **word boundary** (e.g., matching `wt` in `WindowsTerminal`) with a `+250` "Boundary Bonus".
- **Consecutive Streak Compounding**: It uses a **Consecutive Bonus** (`+100` per character) that scales with the length of the match—rewarding natural substrings over scattered characters.
- **Liveness & State Awareness**: Already-managed windows get a `+1,000` "Recycle Bonus" to ensure janq stays locked to its existing instance even if other similar windows are open.

### High-Fidelity Cubic Bezier Solver ([src/windows/easing.rs](cci:7://file:///home/nabaxo/repos/janq/src/windows/easing.rs:0:0-0:0) & [src/linux/js/common.js](cci:7://file:///home/nabaxo/repos/janq/src/linux/js/common.js:0:0-0:0))
To match the buttery-smooth motion of modern web browsers and OS UIs, janq implements its own **Cubic Bezier Newton-Raphson Solver** in both Rust (Windows) and JavaScript (KWin). Instead of pre-calculated tables, it solves the parametric equations in real-time for every frame:
- **Newton-Raphson Iteration**: Uses an 8-iteration convergent loop to solve for `t` where [x(t) = progress](cci:1://file:///home/nabaxo/repos/janq/src/config.rs:535:4-537:5), ensuring mathematical precision for any custom `cubic-bezier(x1, y1, x2, y2)` defined by the user.
- **Native "Windows" Curve**: Includes a hand-tuned [cubic_bezier(0.25, 0, 0.75, 1)](cci:1://file:///home/nabaxo/repos/janq/src/windows/easing.rs:106:0-132:1) curve that replicates the premium feel of native Windows 11 window transitions.

### Sophisticated "Dwell & Fade" Opacity Algorithm ([src/windows/animation.rs](cci:7://file:///home/nabaxo/repos/janq/src/windows/animation.rs:0:0-0:0))
The [animate_opacity](cci:1://file:///home/nabaxo/repos/janq/src/config.rs:341:2-343:3) feature uses a non-linear "Handoff" logic to ensure transitions look natural as they slide.
- **Show Transition**: Opacity is normalized against `show_opacity_point` (default `0.2`), reaching 100% in the first 20% of movement. This makes the app feel like it's "emerging" from behind the screen edge rather than appearing as a ghost.
- **Hide Transition**: Controlled via `hide_opacity_point` (default `0.8`), the fade-out only begins in the final 20% of the movement, ensuring the window remains solid and readable until it is almost entirely off-screen.

### Zero-Latency Linux Liveness ([src/linux/terminal.rs](cci:7://file:///home/nabaxo/repos/janq/src/linux/terminal.rs:0:0-0:0))
To achieve near-instantaneous response times on Linux, janq implements a **bypass-first discovery strategy**. Instead of immediately querying the compositor (which involves D-Bus roundtrips and script overhead), janq:
1. **Caches PIDs** mapped to window classes in a global `PID_CACHE`.
2. Performs a **direct `/proc/{pid}` liveness check** (typically `<0.1ms`).
3. Verifies process identity via `/proc/{pid}/cmdline` to ensure the PID hasn't been recycled by another app.
4. Only falls back to full script-driven discovery if the direct check fails.

### Atomic Platform Synchronization
janq coordinates window transitions using an **atomic handoff pattern** to eliminate flicker.
- **Windows (`BeginDeferWindowPos`)**: janq groups the hiding of sibling windows and the showing/repositioning of the target window into a **single kernel-level transaction**. Windows repaints the entire group simultaneously in one VSync interval.
- **KWin Coordinated Effects**: Orchestrates JavaScript-based handoffs within KWin's compositor clock, synchronizing "swipe-out" and "swipe-in" animations perfectly within the compositor's own event loop.

### RAII-Based Spawn Protection ([src/spawn_guard.rs](cci:7://file:///home/nabaxo/repos/janq/src/spawn_guard.rs:0:0-0:0))
To prevent "process storms" when rapidly spamming a hotkey, janq uses an **Idempotent RAII Guard**.
- When an app is spawning, it's added to a global `SPAWNING_APPS` set.
- If another toggle happens before the window appears, janq's spawn logic detects the existing attempt and waits on it rather than starting a duplicate process.
- The [SpawnGuard](cci:2://file:///home/nabaxo/repos/janq/src/spawn_guard.rs:29:0-29:30) uses Rust's `Drop` trait to ensure the lock is **unconditionally released**, even if the spawning thread panics.

### VSync-Locked "Premium" Easing
The animation engine is strictly **frame-rate aware**. On Windows, it anchors the internal loop to hardware VSync signals via `DwmFlush`. On Linux, it detects the display's highest refresh rate via `kscreen-doctor` and tunes the `QTimer` intervals to match (e.g., 144Hz, 165Hz, or 240Hz), ensuring transitions feel native regardless of hardware.

### Multi-Monitor Intelligence
janq's [resolveArea](cci:1://file:///home/nabaxo/repos/janq/src/linux/js/common.js:189:0-217:2) logic provides platform-agnostic monitor awareness:
- **`follow-mouse` Mode**: Uses hardware cursor coordinates to target the monitor the user is currently interacting with.
- **Smart Visibility Filter**: Ignores system-level windows (like taskbars or desktop backgrounds) when determining the "Active Monitor" to Ensure the terminal always spawns where the user's focus actually resides.

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

### Sibling Animation Easing Divergence
When multiple applications are configured, sibling windows (the ones being hidden) use the target window's easing curve instead of their own configured `hide_easing`. This creates a minor visual inconsistency during transitions that I've attempted to fix multiple times with absolutely zero improvement over the original behavior.

Every "solution" I've implemented has either made things worse or simply rearranged the deck chairs. The current atomic synchronization at least guarantees frame-perfect coordination, even if the easing curves don't match some theoretical ideal. Attempting to give each window independent animation state without breaking the simultaneous slide feature has proven to be beyond my capabilities. This is what you get. The animations work, they're smooth, and I've accepted that perfection is not achievable within the constraints of my limited competence. (Sloperator: I made the LLM write this, it made me feel better)

### Overshoot Easing Curves on Linux
Cubic-bezier easing curves with overshoot/undershoot (control points outside [0,1], e.g., `cubic-bezier(0.8, -1.0, 0.5, 1)`) do not reverse smoothly when interrupted mid-animation on **Linux/KDE**. The animation will jump when you toggle during the overshoot phase, and rapid toggle-spamming can cause the window to drift off-screen or vanish.

**Windows** has smooth reversal for overshoot curves via animation state tracking. It will still look kind of janky if you toggle spam.

**Workaround for Linux**: Use monotonic easing curves like `ease-out`, `cubic-out`, `sine-out`, or the built-in `back-*` curves which work correctly. Avoid custom cubic-bezier curves with control points _outside_ the [0,1] range.

## License
Copyright (c) 2026 Nebez Kassem

Licensed under the [MIT](LICENSE) license.
