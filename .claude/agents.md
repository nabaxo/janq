# janq — Architecture Context

## Project Overview
**janq** is a high-performance, lightweight Quake-style dropdown terminal/application manager. Toggle applications (terminals, VS Code, Obsidian) with a global hotkey, featuring smooth sliding animations and intelligent window discovery.

- **Primary Languages:** Rust (Core), JavaScript (KWin scripts for Linux), Shell (Cleanup utilities).
- **Supported Platforms:**
    - **Linux:** KDE Plasma 6 (Wayland via KWin scripts and D-Bus).
    - **Windows:** Windows 10/11 (Native Win32 API).

## Core Architecture
janq operates on a **Daemon-Client model**:
1. **Daemon:** Persistent background process — handles hotkeys, manages window states, runs animations, maintains system tray icon.
2. **Client:** Same binary with flags like `--app` — sends signal to running daemon via IPC and exits.

### Key Shared Components (`src/`)
- [src/main.rs](src/main.rs): Entry point. CLI argument parsing, branches into Daemon or Client mode.
- [src/lib.rs](src/lib.rs): Shared logic — lock file acquisition (preventing multiple daemons), app name resolution.
- [src/config.rs](src/config.rs): `janq.toml` handling. Config structs (`Config`, `AppConfig`, `WindowConfig`, `AnimationConfig`), enums (`SlideDirection`, `Dimension`, `Easing`, `Framerate`, `DisplayMode`, `PositionOffset`), validation.
- [src/matching.rs](src/matching.rs): **The "Brain."** Weighted fuzzy matching algorithm — finds best window for a configured app (class names, titles, etc.).
- [src/process.rs](src/process.rs): App running detection and spawning.
- [src/config_watcher.rs](src/config_watcher.rs): Shared config file watching. Linux: raw inotify syscalls (zero deps) via `inotify.rs`. Windows: `notify` crate.
- [src/inotify.rs](src/inotify.rs): *(Linux only)* Raw inotify config file watcher, zero external dependencies. Watches config file's parent directory, signals changes via tokio channel.
- [src/error.rs](src/error.rs): Centralized error handling, UI error dialogs (crucial for a tool that often runs without a terminal).
- [src/paths.rs](src/paths.rs): Cross-platform helpers for `home_dir`, `config_dir`, `data_local_dir`, `cache_dir` via environment variables.
- [src/validation.rs](src/validation.rs): Input validation for hotkey strings, easing curve names, cubic-bezier specifications.
- [src/spawn_guard.rs](src/spawn_guard.rs): RAII guard + global `FxHashSet` preventing duplicate process spawns during rapid toggles.
- [src/shutdown.rs](src/shutdown.rs): Shared shutdown messaging for consistent daemon termination output.

## Platform-Specific Implementation

### Linux (KDE Plasma 6) - [src/linux/](src/linux/)
- **IPC:** D-Bus (`zbus`) for client-daemon communication.
- **Window Management:** KWin's D-Bus scripting interface.
- **Daemon:** [src/linux/daemon.rs](src/linux/daemon.rs) — D-Bus daemon exposing `org.freedesktop.Application` for hotkey triggers and `org.janq.Daemon` for CLI commands.
- **KWin:** [src/linux/kwin.rs](src/linux/kwin.rs) — Core KWin script injection and window manipulation; coordinates all script loading/execution.
- **Scripts:** [src/linux/js/](src/linux/js/) — JavaScript snippets dynamically injected into KWin for animations and window queries. Includes `common.js` (shared window matching utilities), `toggle_quake.js`, `ensure_grabbed.js`, `fetch_windows.js`, `get_active_window.js`, `restore.js`.
- **Hotkeys:** [src/linux/hotkey.rs](src/linux/hotkey.rs) — Registers shortcuts with KDE's KGlobalAccel via D-Bus, converting hotkey strings to Qt keycodes.
- **Tray:** [src/linux/tray.rs](src/linux/tray.rs) — Hand-rolled `com.canonical.dbusmenu` implementation on existing `zbus` connection for StatusNotifierItem (KDE/Plasma tray) with right-click context menu.
- **Icon:** [src/linux/icon.rs](src/linux/icon.rs) — Theme detection for StatusNotifierItem tray icon; dark/light mode icon selection.
- **Cache:** [src/linux/cache.rs](src/linux/cache.rs) — In-memory `FxHashMap` mapping app names to KWin window IDs, PIDs, and lowercased process names.
- **Desktop:** [src/linux/desktop.rs](src/linux/desktop.rs) — Generates `.desktop` files, manages autostart symlinks, runs `kbuildsycoca6`.
- **Terminal:** [src/linux/terminal.rs](src/linux/terminal.rs) — Window discovery (via KWin script callbacks), process spawning, PID caching, spawn idempotency.

### Windows - [src/windows/](src/windows/)
- **IPC:** Named Pipes for client-daemon communication.
- **Window Management:** Direct Win32 API calls (`SetWindowPos`, `ShowWindow`, etc.).
- **Animations:** Manual animation loops in Rust with easing (see [src/windows/easing.rs](src/windows/easing.rs) and [src/windows/animation.rs](src/windows/animation.rs)).
- **Hotkeys:** [src/windows/hotkey.rs](src/windows/hotkey.rs) — Parses hotkey strings into `global-hotkey` crate's `HotKey` structs. Includes fallback logic for EU keyboards (IntlBackslash → Backquote → Backslash).
- **Tray:** Uses `tray-icon` crate.
- **Bridge Window:** [src/windows/daemon.rs](src/windows/daemon.rs) — "Bridge window" to safely receive signals even when UI thread is stuck in modal loop (like open context menu).
- **Terminal:** [src/windows/terminal.rs](src/windows/terminal.rs) — Window discovery, process lifecycle management, spawn/toggle coordination.
- **Discovery:** [src/windows/discovery.rs](src/windows/discovery.rs) — `EnumWindows` callback collecting window class/process info for fuzzy matching.
- **Parking:** [src/windows/parking.rs](src/windows/parking.rs) — Parks windows offscreen (transparent, repositioned) when hidden; restores on daemon exit.
- **Window:** [src/windows/window.rs](src/windows/window.rs) — Core Win32 window management: HWND cache, toggle animation engine (`DwmFlush` vsync), multi-monitor detection, force-focus, focus hook.

## Window Discovery Algorithm (`matching.rs`)
Since apps like "wezterm" might have different internal class names or multiple windows, janq uses a scoring system:
1. **Exact Match:** 10,000 pts.
2. **Substring Match:** 5,000 pts.
3. **Subsequence Match:** 1,000 pts + bonuses for boundaries and consecutive chars.
4. **Visibility/Managed Bonus:** Prefers windows already visible or managed by janq.

## Important Workflows
1. **Toggling an App:**
    - Hotkey/CLI triggers `toggle`.
    - Daemon looks for cached window handle.
    - If not found or invalid, runs matching algorithm.
    - If still not found, spawns configured executable.
    - Once window found/spawned, triggers slide animation (Up/Down/Left/Right).

2. **Recovery (`--recover` / systray "Recover"):**
    - Purges stale KWin script name slots (`janq_toggle_engine`, `janq_init_script`, `janq_restore_script`).
    - Clears internal window/PID caches and resets visibility state.
    - Re-grabs all configured app windows via `ensure_grabbed.js`.
    - Triggered: automatically on daemon startup (stale script cleanup only), via `janq --recover` CLI, or via systray right-click → "Recover".
    - Fixes: stuck toggles from occupied script slots, invisible windows from prior crashed sessions, stale cache entries pointing to dead windows.

3. **Config Reload:**
    - File watcher detects change (raw inotify on Linux, `notify` crate on Windows).
    - Daemon parses new config.
    - If valid, updates internal state and re-registers hotkeys.
    - If invalid, shows error dialog and preserves previous working state.

## Configuration Structure (`janq.toml`)
Config searched in order: `./janq.toml` → `~/.config/janq/janq.toml` → `~/janq.toml`. (`.janq.toml` variants also checked at each location for legacy support.)

```toml
[app.terminal]           # App name (key in ordered map)
window_class = "wezterm" # Required, min 3 chars. Used for fuzzy window matching.
start_command = "wezterm" # Required. Executable to launch if not already running.
hotkey = "Meta+Grave"    # Up to 4 hotkeys per app. Also supports array: ["Meta+1", "Meta+2"]
width = "100%"           # Optional. Percent or pixels (e.g., "800px") or "unset".
height = "40%"           # Optional.
slide_from = "top"       # Optional per-app override. top|bottom|left|right.
offset = "center"        # Optional per-app override (alias: position_offset).
depth_offset = "0"       # Optional per-app override. Offset into screen on slide axis. "auto"/"titlebar" = hide titlebar.
hide_titlebar = false    # Optional per-app override. Auto-hide SSD titlebar (slide_from=top only; SSD apps only).
animate_opacity = true   # Optional per-app override.
no_borders = true        # Optional per-app override.

[window]                 # Global window defaults
display_mode = "follow-mouse" # follow-mouse|active|specific
slide_from = "top"       # Default slide direction for all apps.
offset = "center"        # center, pixels, or percent (alias: position_offset).
depth_offset = "0"       # Offset into screen on slide axis (neg hides titlebar, pos pushes central, center centers, "auto"/"titlebar" = hide titlebar).
hide_titlebar = false    # Auto-hide SSD titlebar (slide_from=top only; no effect on custom-chrome/CSD apps).
mono_icon = false        # Force monochrome tray icon (both light and dark modes; adapts to theme).
mono_icon_light = false  # Use monochrome tray icon only when the system is in light mode.
mono_icon_dark = false   # Use monochrome tray icon only when the system is in dark mode.
keep_above = false
no_borders = false
skip_pager = false
auto_show = false
auto_hide = false
all_desktops = false     # Linux only.
force_priority = false   # Linux only.
kde_window_rules = false # Linux only.

[animation]
duration = 350           # Shorthand for both show/hide (ms)
show_duration = 350      # Override show specifically
hide_duration = 350      # Override hide specifically
easing = "impulse"       # Shorthand. Named curves or cubic-bezier(x1,y1,x2,y2).
show_easing = "impulse"  # Override show specifically
hide_easing = "impulse"  # Override hide specifically
animate_opacity = false
show_opacity_point = 0.2 # Opacity threshold for show animation start.
hide_opacity_point = 0.8 # Opacity threshold for hide animation start.
framerate = "auto"       # "auto" (vsync) or 0-1000 (0 = no animation)
```

## Build & Repo-Level Files
- [build.rs](build.rs): On Windows targets, embeds the application icon (`icon.ico`) into the executable via [assets/janq.rc](assets/janq.rc).
- [Makefile](Makefile): Build/dev tooling.
- [install.sh](install.sh): Installation script.
- [.cargo/config.toml](.cargo/config.toml): Cargo build configuration.
- [.github/workflows/release.yml](.github/workflows/release.yml): CI/release workflow.
- [examples/](examples/): Example configs (`janq.single.example.toml`, `janq.multi.example.toml`).
- [assets/](assets/): Icons (`icon.ico`, `icon.svg`, `icon_full.svg`, `icon-symbolic.svg`, `icon-b.ico`, `icon-w.ico`) and `janq.rc`.

## Maintenance Utilities
The [utilities/](utilities/) folder contains cleanup scripts for crashed daemons or dirty dev environments: `cleanup_desktop.sh`, `cleanup_errors.sh`, `cleanup_janq_scripts.sh`, `cleanup_kwin_rules.sh`, `cleanup_kwin.sh`, `cleanup_metadata.sh`, `cleanup_processes.sh`, `cleanup_shortcuts.sh`, `fix_kwin_zombies.sh`, `full_cleanup.sh`, `hard_reset_kwin.sh`.
