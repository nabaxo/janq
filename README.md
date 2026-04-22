# janq - A Fairly Solid Quake-Style Terminal/App Manager

> [!WARNING]
> Note that Github is only a mirror of my own private forge. Anyway, [fuck apartheid](https://bdsmovement.net/news/sign-pledge-boycott-microsoft%E2%80%99s-xbox), this repo is only here out of necessity. ¯\\\_(ツ)\_/¯

### _janq is 100%, unadulterated vibe coded slop. User discretion is advised._

<img src="icon.svg" width="190" height="190" align="left">

**janq** is a lightweight, high-performance Quake-style terminal wrapper "vibe" coded with scorn and contempt in Rust. Not all vibes are good, sometimes vibes are _rancid_. The regressions I had to fix like you wouldn't believe... (ノಠ益ಠ)ノ彡┻━┻
unreadMentionsBarunreadMentionsBar
But in the end I managed to wrangle the Wondrous Machine enough so that janq, on _**cold start** (where you don't have any of the apps to be managed open already),_ uses like ~2.5 MB RAM on Windows and ~1.7 MB on my Fedora KDE system. On not a cold start, it idles at ~2MB RAM on Windows and ~1.7MB on Linux.

It manages your favorite terminal emulator (WezTerm, Windows Terminal, etc.) or whatever app you feel like, allowing you to toggle it with a global hotkey, featuring smooth animations and multi-monitor support.

> [!WARNING]
> Seriously, I don't know how to program in Rust. I have no idea if the code is good or not. On a quick glance, it doesn't appear to do stupid shit; however I have no idea about the specifics of Rust, I'm just a very mid webdev.

> [!CAUTION]
> I have only tested this on like four machines, and I've tried getting it as [bug free](#known-issues-sloperator-that-will-probably-never-be-fixed-and-other-notes) as possible, but your mileage may vary and all that.

_Here's an example with wezterm opening from the top with `show_easing = "quart-out"` and `hide_easing = "expo-in"` (see [configuration](#configuration) for more information):_

<video controls src="https://nabaxo.dev/janq.mp4" width="100%"></video>

[(Click here in case the embed doesn't work)](https://nabaxo.dev/janq.mp4)

## Table of Contents

| Section                                                                                                    | Description                                                 |
| :--------------------------------------------------------------------------------------------------------- | :---------------------------------------------------------- |
| [Supported Platforms](#supported-platforms)                                                                | Linux KDE Plasma 6 & Windows 10/11                          |
| [Key Features](#key-features)                                                                              | Atomic switching, zero-config hotkeys, auto-hide, and more  |
| [Installation](#installation)                                                                              | Download binaries or run the install script                 |
| [Usage](#usage)                                                                                            | Smart startup & toggling behavior                           |
| [Command-Line Arguments](#command-line-arguments)                                                          | CLI flags for daemon control, app toggling, and setup       |
| [Linux (KDE)](#linux-kde)                                                                                  | Desktop integration, hotkeys, autostart, and cleanup        |
| [Windows](#windows)                                                                                        | Native hotkeys, startup configuration, and path formatting  |
| [Systray Behavior](#systray-behavior)                                                                      | Tray icon actions, menu order, and hot reloading            |
| [Configuration](#configuration)                                                                            | Config file location, setup, and all available options      |
| - [Setup](#setup)                                                                            | Config file location and setup      |
| - [Default Values](#default-values)                                                                            | Table of all the settings and their default values       |
| [Building](#building)                                                                                      | Prerequisites, build targets, and cross-compilation         |
| [Related Projects](#related-projects)                                                                      | Libraries and APIs used by janq                             |
| [Technical Implementation](#technical-implementation)                                                      | Performance details, animation physics, and window matching |
| [Troubleshooting & Recovery](#troubleshooting--recovery)                                                   | Linux cleanup scripts for when things go wrong              |
| [Known Issues and other notes](#known-issues-sloperator-that-will-probably-never-be-fixed-and-other-notes) | Animation quirks, platform limitations, and workarounds     |
| [License](#license)                                                                                        | MIT                                                         |

## Supported Platforms

- **Linux**: KDE Plasma 6 (Wayland via KWin scripts, D-Bus activation, and StatusNotifierItem)
- **Windows**: Windows 10\*/11 (Native WinAPI)

(Sloperator: The slopmachine says it runs on Windows 10, I have no idea if it actually runs on Window 10. ¯\\\_(ツ)\_/¯)

## Key Features

- **Atomic Switching**: Coordinated "swipe" animations—the outgoing app slides UP while the new one slides DOWN in perfect sync on both Linux and Windows. (See [Sibling Animation Duration Divergence](#sibling-animation-duration-divergence))
- **Zero-Config Hotkeys**: Automatically registers global hotkeys. On Windows, it's native; on Linux (KDE), it syncs your configuration via D-Bus.
- **Intelligent App Resolution**: Smart fallback logic for single-app setups and typo-tolerant validation (via Levenshtein distance) for flags, app names, and configuration values in multi-app setups.
- **Ordered Configuration**: The order of `[app]` sections in your config file determines their display order in the systray menu.
- **Auto-Hide**: If configured, automatically hides the managed window when it loses focus for a seamless experience.
- **Self-Healing Design (Linux)**: Background monitors for D-Bus and configuration changes are automatically supervised and restarted up to 3 additional times upon failure, providing a GUI error and exiting gracefully if the issue persists.
- **Zero-Polling Restoration**: Reactively restores the system tray icon if the desktop panel restarts, using event-driven D-Bus signal monitoring for zero idle overhead.
- **Robust Identification**: Advanced weighted scoring system to reliably target the main window of complex apps like Obsidian, VS Code, and Zed.
- **High-Performance**: Ultra-fast response with hardware-accelerated sliding animations and focus restoration.
- **CLI Power**: Control your setup via the [terminal](#command-line-arguments).

## Installation

1. - **Windows:**
     Download the binary from [releases](/nabaxo/janq/releases).
   - **Linux:**
     Run the install-script like so:

```sh
curl -f https://git.nabaxo.dev/nabaxo/janq/raw/branch/main/install.sh | sh -s -- --help
```

Check [Command Line Arguhments](#command-line-arguments) for more flags to use. Note that it's not recommened to run the `--setup` argument directly, unless you already have a [`~/.config/janq/janq.toml`](#setup) ready to go, since that forces a refresh of kwin rules etc based on your config.

> [!IMPORTANT]
> Both distributed binaries are for x86; I'm afraid you have to [build](#building) it yourself for that fancy ARM CPU of yours.

2. [Create a janq.toml](#configuration) with [your config](#setup)
3. [Run](#usage)
4. Enjoy

> [!TIP]
> If Windows refuses to run the downloaded .exe; Right click on the file, choose properties, tick the `unblock` checkbox at the bottom, (the one that comes with scary security warnings), then click apply, OK, and you're good to go.

## Usage

### Smart Startup & Toggling

- Start via your desktop or see the [Command-Line Arguments](#command-line-arguments) for manual daemon control and app toggling.

> [!TIP]
> **Single-App Peace of Mind**: If you only have one app configured, janq ignores typos and always picks that app. In multi-app mode, it validates your input and shows a helpful error window if an app isn't found.

## Command-Line Arguments

| Argument                     | Shorthand | Description                                          |
| :--------------------------- | :-------- | :--------------------------------------------------- |
| `--daemon`                   | `-D`      | Run as a persistent process (Server Mode).           |
| `--app [NAME]`               | `-a`      | Toggle a specific application from your config.      |
| `--quit`                     | `-q`      | Gracefully stop the running daemon.                  |
| `--help`                     | `-h`      | Print help information.                              |
| `--version`                  | `-V`      | Print version information.                           |
| **Linux Specific Arguments** |
| `--setup`                    | `-i`      | Force refresh of system/desktop/D-Bus/Rules.\*       |
| `--cleanup`                  | `-u`      | Remove all janq system integration.                  |
| `--enable-autostart`         | —         | Enable autostart (creates symlink to .desktop-file). |
| `--disable-autostart`        | —         | Disable autostart (removes symlink).                 |

\* This creates a `.desktop` file, adds whatever is in your config as kwin-rules for nice icon integration.

### Linux (KDE)

janq generates a `.desktop` file and syncs your hotkeys to **KDE System Settings** automatically. Just run the daemon, and your shortcuts (e.g., `Meta+Grave`) will work instantly.

If you are on a fresh installation and the icon is missing or hotkeys say "name not activatable", you can force a full system integration refresh using the `--setup` [flag](#command-line-arguments).

```bash
./janq --setup
```

This command reinstalls the icon, regenerates the `.desktop` and D-Bus `.service` files, and forces KDE 6 to rebuild its configuration cache and reload the D-Bus session bus. **It also automatically discovers and applies icon fixes for your managed applications (like WezTerm, Obsidian, or VS Code) using KWin Window Rules.**

To completely remove janq's system integration (desktop files, services, icons, and window rules), use the `--cleanup` [flag](#command-line-arguments).

```bash
./janq --cleanup
```

_Note: This does not remove your configuration file._

**Linux Startup (Automatic)**

To make janq start automatically on login (or to disable it), use the `--enable-autostart` and `--disable-autostart` [flags](#command-line-arguments).

These flags create/remove a symlink in `~/.config/autostart/` pointing to the application's desktop file.

### Windows

janq handles hotkeys natively as defined in your config. Right-click the tray icon to switch apps or quit.

#### Add janq to Windows Startup/Autostart (Manual)

To make janq start automatically when you log in:

1.  Press `Win + R`, type `shell:startup`, and press Enter.
2.  Right-click in the folder and select **New > Shortcut**.
3.  Browse to your `janq.exe` location.
4.  **Important**: To start in server mode, right-click the new shortcut, select **Properties**, and add the `--daemon` [flag](#command-line-arguments) to the end of the **Target** field.

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

## Systray Behavior

janq lives in your system tray (notification area) and provides a context menu for quick actions.

- **Left Click**: Toggles the **first** application defined in your configuration file.
- **Right Click**: Opens the menu to toggle specific applications, reload the configuration, or exit.
- (Linux) **Middle Click**: Instantly exits the daemon.
- (Windows) **Shift+Left Click**: Instantly exits the daemon.
- **Menu Order**: The application list in the tray menu follows the exact order of the `[app]` blocks in your `janq.toml`.
- **Shortcut Display**: If multiple `hotkeys` are configured for an application, only the first one is displayed in the menu (all remain functional).

> [!TIP]
> **Hot Reloading**: You don't need to restart janq when you change your config. Simply save your `janq.toml`, and the daemon will instantly reload your settings and update the systray menu.

## Configuration

### Search Priority for configuration file

janq searches for a configuration file _(janq.toml or .janq.toml)_ in the following order:

1.  **Binary Directory** (Portable Mode):
    - Same folder as the `janq` executable.
2.  **XDG Config Directory**:
    - `~/.config/janq/` or `~/.config/janq/`
    - _On Windows_: `%AppData%\Roaming\janq\`
3.  **User Configuration**:
    - `~/`
    - _On Windows_: `%UserProfile%\`

(Sloperator's note: Just put it next to the binary, unless you have dotfile repo, then use option 2. Option 3, the AI told me is stupid, since the crate we're using checks _any changes_ to containing _folder_. I only left it for completeness).

> [!NOTE]
> **Platform Validation**: janq validates your configuration against your current operating system. If you attempt to use Linux-specific settings (like `all_desktops` or `force_priority`) on Windows, janq will block startup with an error and explain which settings are incompatible.

> [!CAUTION]
> **Data Integrity**: On Linux, running a binary from a directory that contains an empty/invalid config (if found in the binary folder) will _not_ overwrite your existing shortcuts. janq includes a safeguard to prevent destroying your system integration.

### Setup

Create `janq.toml` in `~/.config/janq/` or next to the binary, in `/path/to/janq`.

#### Global settings

```toml
[window]
display_mode = "active"  # follow-mouse, specific, active
width = "50%"            # Supports %, px, "0" or "unset" to disable resizing.
height = "600px"
auto_show = false        # Show window on daemon startup
kde_window_rules = true  # (Linux) Manage KWin rule-based icon fixes

[animation]
duration = 350           # Sets both show and hide duration
easing = "impulse"       # Sets both show and hide easing
animate_opacity = true
framerate = "auto"       # "auto", 0 (disable), or 1-1000 FPS
```

#### Single App configuration

```toml
# You can omit the ".wezquake" in single app configs
[app.wezquake]
# On Windows: Matches Process Name (e.g. "wezterm-gui") OR Window Class
window_class = "wezquake"
start_command = "wezterm --config initial_cols=160 --config initial_rows=40 start --class wezquake"
hotkey = "Meta+Grave"
```

#### Multi-App configuration

```toml
[app.wezquake]
window_class = "wezquake"
start_command = "wezterm --config initial_cols=160 --config initial_rows=40 start --class wezquake"
hotkey = ["Meta+Grave", "Ctrl+Grave"]

[app.zed]
window_class = "zed"
start_command = "zed"
hotkey = "Meta+Z"
```

> [!WARNING]
> (Sloperator: Configuring a multiwindow app will act supremely janky. Do not do it. Or do. I'm not your mom. ¯\\\_(ツ)\_/¯. Do give her my regards though, you should call her more often).

#### Electron & Flatpak Apps

Electron apps (Discord, Obsidian, Slack, VS Code, etc.) aggressively throttle their renderer and timers when their window loses focus or is occluded. Since janq hides windows offscreen, this can cause a stale/frozen frame when toggling back. For apps with real-time activity (voice chat, notifications), add these Chromium flags:

```toml
[app.discord]
window_class = "discord"
start_command = "flatpak run com.discordapp.Discord -- --disable-renderer-backgrounding --disable-background-timer-throttling"
hotkey = "Meta+1"
```

| Flag                                       | What It Does                                                                |
| :----------------------------------------- | :-------------------------------------------------------------------------- |
| `--disable-renderer-backgrounding`         | Prevents deprioritizing rendering when the window loses focus.              |
| `--disable-background-timer-throttling`    | Prevents throttling JS timers to 1Hz in background, keeping features live.  |
| `--disable-backgrounding-occluded-windows` | Same as above but specifically for windows hidden behind other windows.     |

> [!TIP]
> For **Flatpak** apps, the `--` separator tells Flatpak to pass all subsequent flags to the app itself. For native Electron installs, pass the flags directly: `start_command = "discord --disable-renderer-backgrounding"`.

> [!NOTE]
> These flags are most useful for apps with real-time background activity (Discord, Slack, Teams). For apps like Obsidian that are mostly idle when hidden, a plain `start_command = "flatpak run md.obsidian.Obsidian"` is fine. Add `--disable-renderer-backgrounding` only if you notice a visual flash when toggling back.

### Default Values

| Section       | Option               | Default          | Description                                                                          | Per_App |
| :------------ | :------------------- | :--------------- | :----------------------------------------------------------------------------------- | :-----: |
| `[app]`       | `window_class`       | **Required**     | Window class/name to match for toggling                                              |    —    |
|               | `start_command`      | **Required**     | Command to launch the application                                                    |    —    |
|               | `hotkey`             | `"Meta+Grave"`   | Global hotkey(s) to toggle the app                                                   |    —    |
| `[window]`    | `display_mode`       | `"follow-mouse"` | Monitor selection: `follow-mouse`, `active`, or `specific`                           |   ❌    |
|               | `display_index`      | `0`              | Monitor index when `display_mode = "specific"`                                       |   ❌    |
|               | `width`              | —                | Window width (`%` or `px`)                                                           |   ✔️    |
|               | `height`             | —                | Window height (`%` or `px`)                                                          |   ✔️    |
|               | `slide_from`         | `"top"`          | Direction to slide in: `top`, `bottom`, `left`, `right`                              |   ✔️    |
|               | `offset`             | `"center"`       | Position along edge: `center`, `50%`, `-10%`, `100px`, `-50px`                       |   ✔️    |
|               | `depth_offset`       | `"0"`            | Offset into screen on slide axis: `center`, `50%`, `-30px` (hides titlebar), `100px`, `auto` / `titlebar` (auto-hide titlebar) |   ✔️    |
|               | `hide_titlebar`      | `false`          | Auto-hide server-side titlebar (only when `slide_from = "top"`; SSD apps only)       |   ✔️    |
|               | `keep_above`         | `false`          | Keep window above all others                                                         |   ❌    |
|               | `no_borders`         | `false`          | Remove window borders/chrome for managed windows                                     |   ✔️    |
|               | `skip_pager`         | `false`          | Hide window from task manager, pager, and switcher (Linux: also hides from Meta+Tab) |   ❌    |
|               | `all_desktops`       | `true`           | (Linux) Window follows you across virtual desktops                                   |   ❌    |
|               | `force_priority`     | `false`          | (Linux) Use KWin Fullscreen state to sit on top of other fullscreen apps             |   ❌    |
|               | `auto_show`          | `false`          | Show window on daemon startup                                                        |   ❌    |
|               | `auto_hide`          | `false`          | Hide window when it loses focus                                                      |   ❌    |
|               | `kde_window_rules`   | `true`           | (Linux) Automatically manage KWin window rules for icon fixes                        |   ❌    |
| `[animation]` | `duration`\*         | —                | Set both show/hide duration at once                                                  |   ❌    |
|               | `show_duration`      | `350` (ms)       | Duration of the show animation                                                       |   ❌    |
|               | `hide_duration`      | `350` (ms)       | Duration of the hide animation                                                       |   ❌    |
|               | `easing`\*           | —                | Set both show/hide easing at once                                                    |   ❌    |
|               | `show_easing`        | `"impulse"`      | Easing curve for showing                                                             |   ❌    |
|               | `hide_easing`        | `"impulse"`      | Easing curve for hiding                                                              |   ❌    |
|               | `animate_opacity`    | `false`          | Fade opacity during animations                                                       |   ✔️    |
|               | `show_opacity_point` | `0.2`            | Animation progress (0-1) by which the window becomes fully opaque                    |   ❌    |
|               | `hide_opacity_point` | `0.8`            | Animation progress (0-1) when fade-out starts                                        |   ❌    |
|               | `framerate`\*\*      | `"auto"`         | Animation framerate: `auto`, `0` (disable), or `1-1000` (FPS)                        |   ❌    |

\*`duration` and `easing` serve as global defaults for both directions. Specific fields (e.g. `show_duration`, `hide_easing`) always take absolute priority when defined. **Note: Durations are scaled based on distance to ensure a constant movement velocity.**

(Sloperator: For your own sanity, just use the single `duration` and `easing` keys, check [here](#sibling-animation-duration-divergence)).

\*\*(Sloperator: I don't know why I even put this in, I guess if you wanna go lower framerate than your actual framerate for performance reasons. Anyway just omitting this or setting it to `auto`, janq detects the display with the highest framerate and uses that; This is also the smoothest on windows due to some technical bullshit.)
_**Note**: Providing a fixed framerate on Linux skips the `kscreen-doctor` detection call entirely, saving a bit of CPU/RAM during use._.

#### Slide Direction

The `slide_from` option controls which edge of the screen the window animates from:

| Value    | Description                                                               |
| :------- | :------------------------------------------------------------------------ |
| `top`    | (**Default**) Window slides down from the top edge (classic Quake style). |
| `bottom` | Window slides up from the bottom edge.                                    |
| `left`   | Window slides in from the left edge.                                      |
| `right`  | Window slides in from the right edge.                                     |

#### Position Offset

The `offset` option controls where along the edge the window is positioned:

| Value           | Description                                                   |
| :-------------- | :------------------------------------------------------------ |
| `center` or `0` | (**Default**) Centered on the edge.                           |
| `50%`           | 50% from left/top of edge.                                    |
| `-10%`          | 10% from right/bottom of edge (negative = from opposite end). |
| `100px`         | 100 pixels from left/top of edge.                             |
| `-50px`         | 50 pixels from right/bottom of edge.                          |

> [!TIP]
> Combine these settings for creative layouts: `slide_from = "right"` with `offset = "0px"` creates a sidebar that slides in from the right at the top corner.

#### Depth Offset

The `depth_offset` option shifts the shown window along the slide axis (perpendicular to the edge):

| Value     | Description                                                                |
| :-------- | :------------------------------------------------------------------------- |
| `0`       | (**Default**) Flush with edge.                                             |
| `center`  | Centered on the slide axis (e.g. vertically centered for top/bottom).      |
| `100px`   | 100 pixels into the screen (pushes window more central).                   |
| `-30px`   | 30 pixels past the edge (negative = hide titlebar / spill offscreen).      |
| `10%`     | 10% of the work area along the slide axis.                                 |
| `auto` / `titlebar` | Auto-detect the titlebar height and hide exactly that much (equivalent to `hide_titlebar = true`, only effective with `slide_from = "top"`). |

#### Hide Titlebar

`hide_titlebar = true` (or `depth_offset = "auto"` / `"titlebar"`) pulls the window up by exactly its server-side titlebar height so only the content area is visible.

> [!NOTE]
> - Only effective when `slide_from = "top"` — ignored for other directions.
> - Works only for apps using **server-side decorations** (SSD): native Qt/KDE apps on Linux, classic Win32 apps on Windows.
> - **Custom-chrome / CSD apps** (Electron, VS Code, Chrome, GTK apps with headerbars, modern Windows apps with custom titlebars) own their full client area and often put menus/tabs in what looks like a titlebar — janq deliberately will **not** hide these, since it would clip real UI.

#### Easing Modes

| Mode           | Description                                               |
| :------------- | :-------------------------------------------------------- |
| `impulse`      | Cubic-bezier curve matching modern Windows 11 animations. |
| `linear`       | Direct, constant movement.                                |
| `ease`\*       | Smooth acceleration and deceleration.                     |
| `sine`\*       | Subtler sine-wave curve.                                  |
| `cubic`\*      | Sharper deceleration.                                     |
| `quart`\*      | Very sharp deceleration (popular for UI).                 |
| `expo`\*       | Exponential curve for a snappier, "high-speed" feeling.   |
| `back`\*       | Overshoots slightly before settling.                      |
| `cubic-bezier` | Custom CSS-style curve: `cubic-bezier(x1, y1, x2, y2)`.   |

\* Supports `ease-in`, `ease-out`, and `ease-in-out` variants (and short-hands like `in-` and `out-`, e.g., `ease-in-sine`, `in-sine`). The base name (e.g., `sine`) defaults to `in-out`.

> [!TIP]
> **Custom Bezier Shortcuts**: You can also use `bezier(x1, y1, x2, y2)` or just `(x1, y1, x2, y2)` for brevity.

#### Display Modes

The `display_mode` setting in the `[window]` section determines which monitor **janq** uses to display your applications.

| Mode           | Description                                                                                  |
| :------------- | :------------------------------------------------------------------------------------------- |
| `follow-mouse` | (**Default**) The window appears on the monitor where the mouse cursor is currently located. |
| `active`       | The window appears on the monitor that currently has keyboard focus (the active window).     |
| `specific`     | The window always appears on a specific monitor, defined by `display_index`.                 |

> [!NOTE]
> When using `display_mode = "specific"`, you must also set `display_index` (0-indexed) to the desired monitor.

### Animation Framerate & VSync (Windows)

For the smoothest experience on Windows, we recommend setting `framerate = "auto"`. This synchronizes janq's animation loop with the **Desktop Window Manager (DWM)** and your monitor's actual refresh rate via `DwmFlush`.

> [!IMPORTANT]
> **High Refresh Rates vs. Fixed FPS**: If you manually set a high framerate (e.g., `120`) on a monitor with a lower native refresh rate (e.g., `60Hz`), you may encounter visual "beat patterns" or choppiness. This is because the DWM only samples window updates once per hardware refresh. Setting a fixed framerate higher than your monitor's refresh rate increases CPU overhead without improving smoothness.

When using a numeric framerate (e.g., `60`), janq automatically engages high-resolution system timers (`timeBeginPeriod(1)`) for the duration of the animation to ensure timing accuracy, returning the system to its power-saving default immediately afterward.

#### Keycodes

janq supports a wide range of keycodes for defining hotkeys. Keys are case-insensitive.

**Modifiers:** `Ctrl`, `Alt`, `Shift`, `Meta` (Super/Windows/Cmd).

- **Aliases:** `Control`, `Super`, `Win`, `Cmd`.
  Multiple modifiers can be combined (e.g., `Meta+Shift+F`, `Ctrl+Alt+T`, or `ctrl+alt+shift+meta+z`, whatever floats your boat).

> [!NOTE]
> **Multi-Hotkey Support**: `janq` supports up to **four hotkeys** per application on both Windows and Linux.

> [!TIP]
> **Single Hotkey Support**: You can define hotkeys without any modifiers (e.g., `hotkey = "F1"` or `hotkey = "PageUp"`).
>
> [!IMPORTANT]
> **Global Hijacking**: If you use a single character key (like `hotkey = "s"`) as a global shortcut, it will act globally across your system while the daemon is running. Pressing that key will toggle your application instead of typing the character. We recommend using **Function keys (`F1`-`F12`)** or **Navigation keys** for single-key hotkeys.

**Standard Keys:**

- **Alphanumeric:** `a`-`z`, `0`-`9` (Case-insensitive)
- **Function:** `f1`-`f12`
- **Navigation:** `up` (`arrowup`), `down` (`arrowdown`), `left` (`arrowleft`), `right` (`arrowright`), `home`, `end`, `pgup` (`pageup`), `pgdn` (`pagedown`)
- **Editing:** `insert`, `delete` (`del`), `backspace`, `tab`, `enter` (`return`), `space`, `esc` (`escape`), `capslock` (`caps_lock`)

**Punctuation & Symbols:**

- `grave` / `backtick` / `` ` ``
- `minus` ( `-` ), `equal` ( `=` )
- `bracketleft` ( `[` ), `bracketright` ( `]` )
- `backslash` ( `\` ), `slash` ( `/` )
- `semicolon` ( `;` ), `quote` ( `'` )
- `comma` ( `,` ), `period` ( `.` )

**International / Special:**

- `section` ( `§` ) - _Mapped to `IntlBackslash` (the key below Esc on some ISO keyboards)_
- `plusminus` / `±`
- `dead_grave`

## Building

### Prerequisites

- **Rust** (stable). Install via [rustup](https://rustup.rs/):
  ```sh
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
  ```
- **KDE Plasma 6** (Linux) or **Windows 10/11**.

#### Optional: Cross-Compilation Targets & Nightly

For cross-compiling or building the optimized nightly binary, add the relevant toolchains and targets:

```sh
# Nightly toolchain (for build-linux-nightly)
rustup toolchain install nightly

# Cross-compilation targets
rustup target add x86_64-unknown-linux-musl
rustup target add x86_64-pc-windows-gnu
rustup target add x86_64-unknown-linux-musl --toolchain nightly

# rustfmt (for format/lint)
rustup component add rustfmt
rustup component add rustfmt --toolchain nightly
```

You'll also need a C cross-compiler on Linux:

```sh
# Debian/Ubuntu
sudo apt install musl-tools gcc-mingw-w64-x86-64 clang make

# Fedora/RHEL
sudo dnf install musl-gcc mingw64-gcc clang make

# Arch
sudo pacman -S musl mingw-w64-gcc clang make
```

### Build

```bash
# Default: nightly optimized Linux build (smallest binary + lowest RSS)
make build                     # Builds nightly Linux + stable Windows

# Individual targets
make build-linux-nightly       # Binary: ./dist/janq (nightly, ~1.6 MiB RSS)
make build-linux-musl          # Binary: ./dist/janq-stable (stable, ~2.4 MiB RSS)
make build-windows-static      # Binary: ./dist/janq.exe

# Others
make build-linux-glibc         # Binary: ./dist/janq-glibc
make build-windows-nonstatic   # Binary: ./dist/janq-nonstatic.exe
```

> [!NOTE]
> **Nightly Linux Build**: The default `make build` uses Rust nightly with `-Zbuild-std` and `panic=immediate-abort` to recompile the standard library with size optimizations and strip all panic formatting code. This produces a significantly smaller binary with ~20% lower RSS. Panics will silently abort instead of printing a message. Requires:
>
> ```bash
> rustup toolchain install nightly
> rustup component add rust-src --toolchain nightly
> rustup target add x86_64-unknown-linux-musl --toolchain nightly
> ```
>
> If nightly is unavailable, use `make build-linux-musl` instead for a stable toolchain build.

> [!TIP]
> Look in `Makefile` for all the options.

## Related Projects

(Sloperator: Shit that the AI used to cobble together this mess).

- **[tokio](https://tokio.rs/)**: The asynchronous runtime powering the unified event loop.
- **[zbus](https://github.com/dbus2/zbus)**: Facilitating D-Bus communication.
- **[KWin Scripting API](https://develop.kde.org/docs/plasma/kwin/)**: Direct integration for Wayland window management on Linux.
- **[windows](https://github.com/microsoft/windows-rs)**: Native Windows API bindings for window management and positioning.
- **[tray-icon](https://github.com/tauri-apps/tray-icon)** & **[global-hotkey](https://github.com/tauri-apps/global-hotkey)**: Managing the system tray and global shortcuts on Windows.

- **[notify](https://github.com/notify-rs/notify)**: Powering the configuration hot-reloading feature on Windows (Linux uses raw inotify syscalls).

## Technical Implementation

### (Sloperator: Features the slopmachine is particularly proud about)

### Performance & Platform backend

- **Platform Parity**: janq achieves cross-platform parity by utilizing native APIs. On Windows, it uses the Win32 API and `BeginDeferWindowPos` for atomic, flicker-free multi-window transitions. On Linux (KDE Plasma 6), it injects JavaScript directly into KWin's scripting engine via D-Bus.
- **Memory Footprint**: janq, on _**cold start** (where you don't have any of the apps to be managed open already),_ uses like ~2.5 MB RAM on Windows and ~1.7 MB on my Fedora KDE system. On not a cold start, it idles at ~2MB RAM on Windows and ~1.7MB on Linux. It manages animations at 144Hz+.
- **Velocity-Style Animations**: Both platforms use "Velocity-Style" animations where duration scales based on travel distance, ensuring constant movement speed regardless of window position. This coordinated "swipe" ensures the outgoing app slides UP while the new one slides DOWN in perfect sync.
- **Unified Async Architecture**: Uses a cross-platform Tokio-based async runtime with a single unified event loop for IPC, animations, and heartbeats.
- **Self-Healing Supervision**: Background tasks (D-Bus monitoring, config watching) are wrapped in supervisor loops that automatically attempt reconnection or re-initialization every 5 seconds upon failure.
- **Zero-IPC Liveness Checks**: On Linux, janq performs direct `/proc/{pid}` checks (<0.1ms) instead of querying KWin, ensuring instant response.

### Physics & Window Matching

- **Bezier Solver**: Both platforms implement identical Newton-Raphson cubic bezier solvers for smooth, hardware-accelerated transitions. (15+ curves including the "premium" `impulse` curve).
- **Advanced Window Matching**: A weighted fuzzy scoring system (Exact > Substring > Boundary > Subsequence) ensures reliable targeting of complex applications using `APP_CACHE` on Windows and PID caching on Linux. This includes ultra-fast <0.1ms verification.
- **Complex App Support**: Specialized matching ensures apps like **Windows Terminal** (via `CASCADIA_HOSTING_WINDOW_CLASS`), Obsidian, VS Code, and Zed are caught reliably even during complex startup sequences or when minimized.
- **Spawn Protection**: RAII-based `SpawnGuard` ensures rapid hotkey presses don't result in duplicate process spawns.
- **Animation Handover**: When toggling between two different apps rapidly (while one is mid-animation), the new animation picks up from the window's current physical position and velocity-scales the remaining duration. On Windows, an atomic generation counter instantly cancels the old animation loop; on Linux, a fresh KWin script injection reads the live `frameGeometry` and takes over. The result is continuous, smooth motion with no snap or freeze — just a proportionally shorter travel time for the remaining distance.

## Troubleshooting & Recovery

### Linux Recovery Utilities (For When Things Go Wrong)

The `utilities/` directory contains cleanup scripts for Linux. These exist because during development we managed to break KDE shortcuts, leave zombie processes, and generally make a mess of the desktop integration more times than we'd like to admit. (Sloperator: Speak for yourself, I had to use it countless times because of your bullshit).

| Script                    | Description                                                           |
| ------------------------- | --------------------------------------------------------------------- |
| `full_cleanup.sh`         | Nuclear option. Removes all janq/legacy janq traces from your system. |
| `hard_reset_kwin.sh`      | Aggressive recovery from KWin state corruption or script hangs.       |
| `cleanup_shortcuts.sh`    | Fixes KDE global shortcuts when they inevitably get stuck.            |
| `cleanup_desktop.sh`      | Removes desktop entries and icons.                                    |
| `cleanup_processes.sh`    | Kills any lingering daemon processes.                                 |
| `cleanup_kwin.sh`         | Removes KWin scripts.                                                 |
| `cleanup_kwin_rules.sh`   | Removes janq-managed KWin window rules from `kwinrulesrc`.            |
| `cleanup_janq_scripts.sh` | Removes janq KWin script files from shared memory and disk.           |
| `cleanup_metadata.sh`     | Clears cached window IDs and metadata.                                |
| `cleanup_errors.sh`       | Removes janq error temp files from /tmp.                              |
| `fix_kwin_zombies.sh`     | Kills zombie KWin script processes that survived daemon shutdown.     |

If janq stops responding to hotkeys or you want a completely clean slate, these will save you. We know this because we've used them. A lot.

_(Sloperator note: Just use `full_cleanup.sh`)._

## Known Issues (Sloperator: that will probably never be fixed) and other notes

### Linux: Hotkey registration delay

On KDE Plasma, there's a small intentional delay (~500ms) when registering or updating hotkeys. This is a workaround for a race condition in KWin's `GlobalShortcutsRegistry` that can cause crashes with rapid D-Bus operations. The delay only affects startup and config reloads, not toggle performance.

### Windows: Middle click tray icon

(Sloperator: There's some bug/feature in Windows 11 that messes up middle clicking tray icons, hence shift-left click on tray icon to close on windows).

### App Compatibility: Opacity Animations

(Sloperator: This mostly effects Linux, opacity seems to work fine on Windows, even on electron apps).

Some applications (especially Electron-based ones or XWayland clients) may experience unreliable transparency or "stutter" during motion on Linux.

**KWin Technical Limitation**: Due to the asynchronous nature of Wayland property updates vs. buffer commits, a window's `opacity` and its `frameGeometry` (position) may occasionally arrive in different compositor frames. This can cause a "flicker" where the window appears at the new position but with the old opacity for a single frame. (Sloperator: More like that it doesn't animate opacity all the way when opening sibling).

**The Fix**: janq uses the "Fullscreen role" (`force_priority = true`) or `ForceBlur` to trick the compositor into prioritizing these updates. However, for some apps, disabling `animate_opacity` is still the most stable choice. (Sloperator: It's the best I could make the AI do ¯\\\_(ツ)\_/¯).

### Linux: Animation Artifacts (Ghosting / Jitter)

**If you experience intense jittering or "fighting" animations**, you likely have a third-party KWin effect active (like "Geometry Change") that is competing with janq to animate the window. To fix this:

- Open your KWin effect settings.
- Find the conflicting effect (e.g., "Geometry Change").
- Add the window classes managed by janq (e.g., `wezterm`, `obsidian`) to the effect's **Exclusion List**.

While **janq** is optimized for high-refresh displays and uses `ForceBlur` to stabilize transitions, some degree of content lag is currently an inherent platform limitation for these types of apps.

(Sloperator: I haven't really noticed an animation smoothness issues on Linux, aside from me having Geometry Change kwin effect. But that got solved by blacklisting the janq managed app in the effects settings).

### Sibling Animation Duration Divergence

(Sloperator: I made the LLM write this, it made me feel better)

When multiple applications are configured, sibling windows (the ones being hidden) use the target window's duration instead of their own configured `hide_duration`. This creates a minor visual inconsistency during transitions that I've attempted to fix multiple times with absolutely zero improvement over the original behavior.

Every "solution" I've implemented has either made things worse or simply rearranged the deck chairs. The current atomic synchronization at least guarantees frame-perfect coordination, even if the durations don't match some theoretical ideal. Attempting to give each window independent animation state without breaking the simultaneous slide feature has proven to be beyond my capabilities. This is what you get. The animations work, they're smooth, and I've accepted that perfection is not achievable within the constraints of my limited competence.

### Overshoot Easing Curves on Linux

Cubic-bezier easing curves with overshoot/undershoot (control points outside [0,1], e.g., `cubic-bezier(0.8, -1.0, 0.5, 1)`) will be super janky when interrupted mid-animation on **Linux/KDE**. The animation will jump when you toggle during the overshoot phase, and rapid toggle-spamming can cause the window to drift off-screen or vanish.

**Windows** has smooth reversal for overshoot curves via animation state tracking. It will still look kind of janky if you toggle spam.

**Workaround for Linux**: Use monotonic easing curves like `ease-out`, `cubic-out`, `sine-out`, or the built-in `back-*` curves which work correctly. Avoid custom cubic-bezier curves with control points _outside_ the [0,1] range.

***(Sloperator: The next two are issues only on system boot and user has set janq to run on start)***
### Windows: Force-killing janq leaves managed windows in a broken state

If janq is force-killed (Task Manager, `taskkill`, crash, etc.) instead of being shut down gracefully (`--quit`, tray quit, Ctrl+C), managed windows are left in janq's internal state — offscreen, transparent, with a hidden owner window, and stripped of `WS_EX_APPWINDOW`. Since the cleanup path (`restore_window_visibility`) never runs, these windows may become unresponsive, invisible, or unable to receive input. GPU-composited apps (WezTerm, Electron apps, Windows Terminal) can cascade into a DWM compositor stall where none of them accept input even if brought on-screen.

**Recovery**: Force-kill all managed apps and restart them. Simply restarting janq is not sufficient — the affected app processes themselves need to be terminated.

**Prevention**: Hope janq doesn't crash (it shouldn't) or use the tray quit option, or `./janq.exe --quit` in a terminal to shut down cleanly.

### Linux: On Boot, Window Size Not Remembered for Apps Without Explicit Dimensions

Apps that rely on their own saved window geometry (e.g., Obsidian, other Electron apps) may lose their remembered size when managed by janq and no `width`/`height` is set in the config. This happens because KWin's `frameGeometry` API requires setting position and dimensions together — when janq parks the window offscreen at boot, it reads and writes back the window's current size, which may still be the default before the app has restored its saved geometry. Setting explicit `width` and `height` in the app's config is the current workaround.

### Linux: On Boot, Slow-Starting Apps May Not Be Grabbed

Some Electron/Flatpak apps (e.g., Discord) may not have their window ready when janq's initial grab runs at boot. If the app's window doesn't exist yet during the startup pass, janq won't be able to manage it until the next toggle or config reload. Toggling the app's hotkey after it has fully started will grab it normally.

### Apps That Minimize to Tray

If a managed app is closed to the system tray rather than fully quit, janq will silently do nothing when you next press its hotkey. This is a platform limitation: neither Linux nor Windows exposes a "minimized to tray" signal — the window is simply destroyed or hidden by the app itself, with no OS-level way to distinguish it from a normal close. The process remains alive, so janq won't attempt to respawn it either.

**Workaround**: Restore the app from its tray icon manually before using the janq hotkey, or configure the app to not minimize to tray.

(Sloperator: I.e. Janq can't do anything if your dumb ass closes Discord to tray and forgets, then wonders why janq isn't working).

## License

Copyright © 2026 Nebez Kassem

Licensed under the [MIT](LICENSE) license.
