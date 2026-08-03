# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [1.0.6] - 2026-08-03

### Added
- **GitHub Actions release workflow**: Builds Linux (nightly musl, `immediate-abort`) and Windows (MSVC via `cargo-xwin`) on tag push. Creates GitHub release with changelog body and build provenance attestations.

### Changed
- **Example configs moved**: `dist/*.example.toml` → `examples/`.
- **Install scripts**: Removed dead `dist/` fallback URLs; scripts now require a release asset or error cleanly.
- **Makefile**: Simplified `clean` target now that example TOMLs no longer live in `dist/`.

### Removed
- **Tracked binaries**: `dist/janq` and `dist/janq.exe` removed from repo; `dist/` added to `.gitignore`. Build output still goes to `dist/` locally.

## [1.0.5] - 2026-08-02

### Changed
- **Dependencies**: Updated all dependencies to latest compatible versions via `cargo update`.
- **`notify`**: Upgraded from 8.2 to 9.0.0-rc.4. Event paths on Windows now preserve watched path separator style; relative watch paths consistently produce relative event paths.
- **`tray-icon`**: Upgraded from 0.21 to 0.24. Adds optional GTK feature, Windows tooltip and visibility fixes.
- **`tokio`**: 1.49 → 1.53.
- **`zbus`**: 5.14 → 5.18.
- **MSRV**: Raised from 1.70 to 1.88 (required by notify 9).

## [1.0.4] - 2026-07-31

### Fixed
- **(Linux) KWin rules stopped matching after Flatpak WM_CLASS change**: Flatpak apps can silently change their `WM_CLASS` on update (e.g., Obsidian: `obsidian` → `md.Obsidian`). KWin rule regex was case-sensitive, so a case change broke window matching. Rules now use `(?i)` prefix for case-insensitive matching.
- **(Linux) Generic Wayland icon after WM_CLASS change**: `find_desktop_file_id` matched `StartupWMClass` only by exact equality. When the runtime class was a substring of the desktop file's `StartupWMClass` (or vice versa), lookup failed and the app showed a generic Wayland icon. Now falls back to case-insensitive substring matching.

### Changed
- **(Linux) Log message on KWin rule update**: Daemon now prints when KWin window rules are updated.

## [1.0.3] - 2026-07-30

### Fixed
- **(Windows) Hung app could disable toggling for every managed window**: `release_windows` sets the animation-cancel flag, restores each window, then clears it — but `restore_hwnd` used `SendMessageW`, which has no timeout and blocks until the target thread pumps its message queue. A wedged app left the flag latched at `true` permanently, aborting every subsequent animation for *all* managed apps, and consumed the Tokio worker running the config-watcher task. The `WM_SIZE` repaint nudges now use `SendMessageTimeoutW` with `SMTO_ABORTIFHUNG` and a 250 ms bound, so a slow-but-alive window is still fully restored and only misses its nudge. `release_windows` additionally skips `IsHungAppWindow` targets as a fast path, since `SetWindowPos` and `ShowWindow` also dispatch synchronously.
- **(Windows) Black frame at the start of the show animation when `show_opacity_point = 0.0`**: `0.0 / 0.0` produces `NaN`, `clamp()` propagates it, and the `u8` cast saturates to alpha 0 for one frame. The show path now guards the division; the hide path already did.
- **(Windows) 0×0 animation when `GetWindowRect` fails**: a failed call left `r_target` at `RECT::default()`, so apps relying on auto-sized dimensions animated to a zero-size window. Now bails when the measurement failed *and* a dimension is actually unset — configs with explicit `width`/`height` never read the rect and are unaffected.
- **(Windows) `WM_SETCURSOR` sent with a menu-mode LPARAM**: the high-order word carried `0`, which per MSDN means "window entering menu mode", instead of the mouse message that prompted the update. Now sends `WM_MOUSEMOVE`. `DefWindowProc` keys off the low word either way, so this is a correctness fix rather than a behavioral one.
- **(Linux) inotify watcher thread spawn failure silently ignored**: `.ok()` discarded the spawn result and returned a live receiver on a channel with no sender, leaving config reload permanently dead with no diagnostic. The failure is now reported and propagated.

### Changed
- **(Linux) inotify spawn failure is no longer non-fatal**: propagating the error routes it into the existing supervisor loop, which retries up to `MAX_RETRY_COUNT` and then exits with a GUI error. A persistent thread-spawn failure therefore terminates the daemon rather than degrading to "toggling works, config reload silently does not". This matches how every other initialization failure in the watcher is already handled.

### Known Issues
- **(Windows) A hung window is not restored on config reload**: when `release_windows` skips an unresponsive window, the window stays parked offscreen — transparent, `WS_EX_TOOLWINDOW`, no taskbar button — and the caller has already dropped it from the cache, so nothing retries it. Recovery is restarting the affected app. This is a deliberate tradeoff over stalling the daemon indefinitely.

## [1.0.2] - 2026-07-30

### Fixed
- **(Windows) Invisible cursor and blank window after toggle**: Fixed rendering artifacts caused by `WS_EX_LAYERED` interaction with show animation — strips layered style after animation, adds `WM_SETCURSOR` nudge, and applies `SWP_FRAMECHANGED` to force repaint.
- **(Windows) Icon not embedded in MSVC builds**: `embed_resource::compile()` returns a `CompilationResult` marked `#[must_use]` — the previous `let _ =` silently dropped it, preventing the linker instruction from being emitted.

### Changed
- **Windows build target**: Default Makefile Windows target switched from MinGW static (`build-windows-static`) to MSVC (`build-windows-msvc`) via `cargo-xwin`. MSVC-compiled binaries avoid CrowdStrike EDR heuristic false positives triggered by MinGW cross-compilation.
- **CLI shorthands**: `-D` → `-d` (daemon), `-V` → `-v` (version). Arguments are now case-insensitive via `to_ascii_lowercase()` normalization before matching.
- **Build prerequisites**: Replaced `gcc-mingw-w64` with `llvm` and `cargo-xwin` for Windows cross-compilation.

## [1.0.1] - 2026-07-28

### Changed
- **Tray icon (Linux)**: Replaced `resvg` build-time SVG rasterization and runtime `IconPixmap` rendering with pure `IconName`-based icon serving. Plasma now resolves `janq-symbolic` and `janq-color` SVGs directly from the hicolor theme, handling CSS recoloring natively. Removes `resvg` build dependency and all ARGB pixmap infrastructure.
- **Symbolic SVG (Linux)**: Optimized and added KDE `id="current-color-scheme"` style block and `ColorScheme-Text` class to `icon-symbolic.svg` so Plasma's KIconEngine applies theme-aware recoloring.
- **Build script**: Removed stale `resvg`/IconPixmap references from `build.rs` documentation.

### Removed
- **`resvg` build dependency**: No longer needed — tray icon SVGs are resolved natively by Plasma instead of being pre-rendered to ARGB pixmaps at build time.

### Known Issues
- **Tray icon cache (Linux)**: First-time `mono_icon = true` may require a plasmashell restart (`kquitapp6 plasmashell && kstart plasmashell`) for Plasma's KIconLoader to discover the newly installed symbolic SVG. Subsequent theme changes and config reloads work correctly.

## [1.0.0] - 2026-04-15

### Added
- **Install script**: For easy installation and system integration on Linux.
- **Graceful quit command (`--quit` / `-q`)**: New CLI flag that sends an IPC quit signal to the running daemon, restoring all managed windows before exiting. Uses named pipes on Windows and D-Bus on Linux.
- **Typo-Tolerant Suggestions**: Integrated Levenshtein distance algorithm for command-line arguments, app names, and configuration values (modifiers, keys, and enums).
- **GUI Warning Pop-ups**: Warnings now display as GUI pop-ups in non-interactive sessions (Windows/Linux).
- **Improved CLI Error Handling**: Enhanced argument parsing with suggestions for unknown arguments. Now tolerates transpositions and small typos (e.g., `--hlep` suggests `--help`).
- **Automated KWin Rule Lifecycle**: Implemented a sophisticated rule synchronization engine that manages `kwinrulesrc` automatically. It groups managed apps by desktop ID, applies regex-based matching, and surgically prunes stale or redundant janq rules directly from the filesystem to bypass KConfig errors.
- **Self-Healing Recovery (Linux)**: Integrated D-Bus and system bus watchers for `org.kde.plasmashell`, `org.kde.KWin`, and `logind`'s `PrepareForSleep`. Managed windows now automatically re-park offscreen and restore their hidden properties (opacity, blur, skip-taskbar) 2 seconds after a shell restart, compositor replacement, or wake-from-sleep event.
- **Diagnostic Recovery Logging**: Added explicit terminal feedback when recovery triggers are detected.
- **`kde_window_rules` Setting**: New configuration option under `[window]` (defaulting to `true`) to control whether janq manages system window rules for taskbar icon fixes.
- **Linux Setup & Cleanup**: New `--setup` CLI flag to force regeneration of desktop files, icons, and D-Bus services, and a `--cleanup` flag to completely purge janq system integration (rules, desktop files, services).
- **D-Bus Reload Trigger**: Added automatic `org.freedesktop.DBus.ReloadConfig` calls (via `qdbus6` or `dbus-send`) when system services are installed, ensuring "activatable" services work without a logout.
- **KWin Icon Rule Automation**: Added internal `find_desktop_file_id` auto-discovery to support forced icon association via KWin Window Rules.
- **Auto-Discovery of Desktop Files**: `janq` now automatically searches for the correct `.desktop` association for your managed apps based on their `window_class`, fixing generic Wayland icons without any manual configuration required.
- **Unified Selection Engine**: Consolidated all window discovery and fuzzy matching logic into a shared platform-agnostic crate module. Windows and Linux now share a weighted scoring algorithm (Exact > Substring > Visible > Managed).
- **Name-Aware Process Liveness**: New `process` module for platform-agnostic PID verification.
  - **Linux**: Migrated to `/proc/{pid}/cmdline` parsing to support full binary names (e.g., `org.wezfurlong.wezterm`) and bypass 15-character kernel `comm` truncation.
  - **Windows**: Implemented `GetExitCodeProcess` verification to prevent "Zombie App" detection caused by PID recycling.
- **Focus Inheritance (Linux)**: Implemented sticky focus restoration for rapid app switching. Toggling between multiple Janq-managed apps now correctly preserves and "inherits" the original external window focus target.
- **Momentum-Aware Animations**: Overhauled animation engines on both platforms to support "Handover" states. Toggling an app mid-animation now picks up from the current opacity/position instead of snapping back to the start.
- **Auto-Hide focus watcher**: New `auto_hide` option in the `[window]` block to automatically hide the window when it loses focus.
- **Systray menu on Linux**: Added tray functionality via `ksni` to Linux with full menu and shortcut display.
- **KWin Script Recovery**: Three-layer recovery system for stuck KWin script slots and invisible windows from crashed sessions. On daemon startup, stale scripts are automatically purged. `--recover` / `-r` CLI flag sends a recovery signal to the running daemon (purges scripts, clears caches, re-grabs all windows). Systray right-click menu includes a "Recover" entry for the same action without a terminal.
- **Animation Framerate Control**: New `framerate` option for the `[animation]` block.
  - Supports `"auto"` (VSync/Platform default), a specific number (e.g., `60`, `120`), or `0` to disable animations entirely (instant transitions).
  - Cross-platform implementation using `DwmFlush` on Windows and frequency-clamped timers on Linux.
- **Self-healing background tasks (Linux)**: Implemented supervisor loops for the system tray monitor and config watcher with a shared `MAX_RETRY_COUNT`. On persistent failure, `janq` now displays a GUI error and exits gracefully.
- **Refresh Rate Logging**: Optimized Linux backend to log the refresh rate only once per session or upon configuration reload, preventing terminal spam.
- **Strict Validation**: Configuration parsing now strictly enforces numeric framerates (no quoted numbers) and validates ranges (0-1000).
- **Framerate Display**: Added `Display` implementation for `Framerate` to provide clean logging of configured values.
- **`depth_offset` per-app/global setting**: New option on `[window]` and per-app that offsets the window into the screen along the slide axis. Accepts pixels (`"-30px"` to hide titlebar, `"100px"` to push central), percent (`"10%"`), or `"center"` (perfectly centered on slide axis). Applies to all four slide directions.
- **`hide_titlebar` option**: Auto-detects the server-side titlebar height and hides exactly that much of the window (only effective when `slide_from = "top"`). Also accessible via `depth_offset = "auto"` or `"titlebar"`. Works only for SSD apps — custom-chrome/CSD apps (Electron, VS Code, Chrome, GTK headerbar apps) are deliberately left untouched to avoid clipping real UI.
- **`mono_icon` / `mono_icon_light` / `mono_icon_dark` options**: Monochrome tray icon that adapts to the system theme. `mono_icon` forces mono in both modes; `mono_icon_light` / `mono_icon_dark` each restrict mono to a single system mode (e.g. colored in light, mono in dark). On Linux, the SVG is pre-rendered to ARGB pixmaps at build time and served directly via SNI `IconPixmap`; at runtime the symbolic variant is retinted to match `[Colors:Window] ForegroundNormal` from `kdeglobals`, and an inotify watcher re-emits `NewIcon` whenever the color scheme changes — emulating SVG's `currentColor` without a runtime SVG parser. On Windows, embeds separate black/white icons and swaps live when the system theme changes (light → black icon, dark → white icon).

### Changed
- **Nightly Linux Build (default)**: Default Makefile target now uses `cargo +nightly -Zbuild-std=std,panic_abort` with `RUSTFLAGS="-Zunstable-options -Cpanic=immediate-abort"`, reducing binary size by ~494 KiB and RSS by ~452 KiB (2360 → 1908 KiB, a 19% reduction). Stable build remains available via `make build-linux-musl`.
- **Hand-rolled dbusmenu (Linux)**: Replaced the `ksni` crate with a minimal `com.canonical.dbusmenu` implementation served on the existing `zbus` connection. This eliminates `ksni` and its separate D-Bus stack, saving an additional ~248 KiB RSS (1908 → 1660 KiB) while preserving full menu functionality (per-app toggle, shortcut display, hot-reload, separator, quit). The `systray` feature flag has been removed; the menu is now always available.
- **Raw inotify config watcher (Linux)**: Replaced the `notify` crate with direct inotify syscalls for config file watching on Linux, eliminating a transitive dependency tree. Saves ~68 KiB binary size; `notify` is now Windows-only.
- **Dependency pruning**: Disabled default features on the `toml` crate, selecting only `std`, `serde`, `parse`, and `preserve_order`. Removes unused `display`/formatting code from the binary.
- **Error dialog UX**: "Press Enter to exit..." changed to "Press any key to exit..." using `read -rsn1`. All Linux error dialog terminal entries now use `bash -c` instead of `sh -c` for consistent behavior.
- **Lock File Storage**: Moved lock file from config directory to cache directory (`XDG_CACHE_HOME` on Linux, `LOCALAPPDATA` on Windows) to follow platform conventions.
- **Import Cleanup**: Refactored internal module imports across `main.rs` and daemon modules to use consolidated `janq::` import blocks instead of fully-qualified paths, improving code readability and maintainability.
- **Linux Error Handling**: Simplified Linux error and warning display by removing unused `kdialog` and `zenity` fallbacks and non-blocking terminal spawn logic.
- **Unified Rule Engine**: Refactored the Linux KWin rule management into a single execution path for both installation and purging, ensuring consistent cleanup of `kwinrulesrc` across all operations.
- **Normalized Error Handling**: Standardized result types and error macros across the Linux backend for better consistency with the core crate.
- **Zero-Allocation Discovery Loops**: Refactored core window discovery interfaces to use `&[FoundWindow]` slices. This eliminates thousands of heap allocations per minute during system polling and hotkey triggers.
- **Aggressive Cache Pruning**: Windows backend now proactively evicts handles if `IsWindow` or `is_process_running` fails, ensuring much faster recovery from application crashes.
- **Refresh Rate Logic**: Optimized Linux backend to skip `kscreen-doctor` execution when a fixed `framerate` is provided in the configuration.
- **Config Refactoring**: Internal simplification of `Dimension`, `PositionOffset`, and `DisplayMode` deserialization to reduce code duplication and improve maintainability.
- **Zero-Dependency Signal Iteration**: Leveraged `zbus` internal re-exports instead of `futures-lite` for event-driven monitoring, minimizing binary bloat and memory footprint.
- **Guard Ordering (Linux)**: `start_command` existence check now runs before process liveness check in the spawn path, short-circuiting earlier for apps without a configured command.
- **Lock Scope Reduction (Linux)**: `SCAN_CACHE` clone extracted outside the Mutex guard to minimize lock contention during window discovery.
- **D-Bus Panic Context (Linux)**: Bare `.unwrap()` on D-Bus name parsing in daemon and KWin modules replaced with `.expect()` with descriptive messages.
- **Tray Quit Grace Period (Linux)**: Added 100ms sleep before `exit(0)` in the tray quit handler to allow pending D-Bus responses to flush, matching the signal handler's existing grace period.
- **Hash Collection Consistency (Linux)**: Replaced `std::collections::HashSet` with `rustc_hash::FxHashSet` in hotkey registration, aligning with the project's `FxHash` convention.
- **(Windows) PID Cache Simplification**: Removed unreachable dead branch in PID cache lookup.
- **(Windows) Dead code removal**: Removed unused `WAKE_EVENT` / `SyncHandle` / `MsgWaitForMultipleObjects` infrastructure that was planned but never completed. The bridge window `WM_USER+4` handler now covers the modal-loop exit path this code was intended to address.
- **(Linux) Window stuck translucent at wrong position after sleep/wake**: `reset_state()` unconditionally cleared `visible_app`, causing the re-grab to park all windows offscreen — but KWin's session restore raced and left the window at a stale position with partial opacity. Sleep handler now preserves visibility state, and `ensure_grabbed.js` reapplies shown position and full opacity for visible windows instead of skipping them.

### Fixed
- **(Windows) Daemon unresponsive after tray menu stuck open**: When `TrackPopupMenu` entered a permanent modal loop (e.g. due to a foreground-window race), the main `GetMessageW` loop was suspended indefinitely. All daemon events — hotkeys, quit, Ctrl+C signals — queued in the channel but were never processed. The bridge window now handles `WM_USER+4` as a direct exit message dispatched even inside modal loops, and the Ctrl+C/Break/Close signal handler posts to it instead of relying on channel drain.
- **(Windows) Shutdown hangs on unresponsive managed windows**: `restore_window_visibility()` sends Win32 messages (`SetWindowPos`, `ShowWindow`) to each managed window during shutdown. If a window was hung (e.g. GPU-stalled WezTerm), these calls blocked indefinitely. Now checks `IsHungAppWindow` before each restore and skips hung windows.
- **(Windows) Taskbar icon not hiding for some apps**: Apps with `WS_EX_APPWINDOW` extended style (e.g. Basitune) would force a taskbar button even when janq set a hidden owner window. janq now strips `WS_EX_APPWINDOW` when managing a window and restores it on daemon exit.
- **(Windows) Alt-Tab**: Resolved focus void after Alt-Tab and hardened focus logic.
- **Single-Instance Lock**: Fixed lock file mechanism to correctly handle platform-specific `fs4` behavior. On Linux (MUSL), the lock now properly detects `Ok(false)` return values. On Windows, lock contention (`Err` with OS error `0x21`) is now distinguished from genuine I/O failures. Lock acquisition now only occurs when starting a daemon process, not during client IPC operations.
- **Windows Error Visibility**: Lock file errors and other critical errors during daemon startup now properly invoke `show_error()` instead of silently propagating through the async runtime, ensuring users see error dialogs on Windows.
- **Error Display**: Converted silent `eprintln!` error messages to user-visible GUI notifications for non-terminal sessions.
- **Lock File Lifetime**: Lock file handle is now leaked using `Box::leak()` to ensure it persists for the entire process lifetime, preventing premature release during async yields.
- **Linux First-Run Reliability**: Icons and D-Bus services now index correctly on the first run of the app on fresh KDE 6 installations.
- **Icon Cache Refresh**: Integrated `kbuildsycoca6 --noincremental` triggers into the installation flow to ensure the `janq` icon appears in the taskbar immediately.
- **Windows Focus "Yo-Yo"**: Hardened focus hooks to prevent `auto_hide` from triggering incorrectly when a window is manually toggled or focus-forced.
- **Windows WezTerm Reopening**: Fixed a bug where closed windows stayed in the management cache, preventing re-spawning. Added aggressive cache pruning upon PID death detection.
- **Linux Focus Restore Lag**: Bypassed the metadata cache during toggle events to ensure focus restoration targets are captured with zero-latency D-Bus synchronization.
- **Systray Icon Persistence**: Resolved an issue where the tray icon would disappear after a compositor or panel (StatusNotifierWatcher) restart on KDE Plasma.
- **Plasma Ghost Windows (Linux)**: Fixed "ghost" window artifacts (persistent blur or reset opacity) appearing after a Plasma/KWin restart or wake-from-sleep by implementing a robust state re-synchronization engine.
- **Type-Safety & Build Reliability**: Resolved numerous cross-platform type regressions and Win32 import conflicts introduced during the architectural deduplication.
- **Shortcut display in tray**: Windows now displays shortcut on context menu in systray.
- **Instant Focus**: Resolved issue on both Windows and Linux where `framerate = 0` (instant mode) would fail to grab focus correctly.
- **Opacity Sync**: Opacity animations are now automatically bypassed if animations are disabled (`framerate = 0`), preventing windows from appearing invisible.
- **Config Watcher Loop**: Fixed an infinite reload loop on Linux caused by the file watcher triggering on "Access" events.
- **Windows Polish**: Fixed a race condition where Win32 focus calls could fail during near-instant transitions.
- **(Windows) Hotkeys not restored after sleep/wake**: Hotkey registrations can be silently invalidated by Windows after sleep/hibernate resume. The signature cache (`LAST_SYNC_SIG`) previously suppressed re-registration unless the config changed. The bridge window now handles `WM_POWERBROADCAST` / `PBT_APMRESUMEAUTOMATIC`, clearing the cache and forcing hotkey re-registration on wake.
- **(Windows) Border restoration repaint**: Windows managed with `no_borders = true` failed to repaint their client area when borders were restored on daemon exit. A 1px size nudge now forces full client-area invalidation on the restore path.
- **ANSI sequence stripping**: `strip_ansi` only terminated CSI sequences on the `m` character (SGR). Non-SGR sequences (cursor movement, erase) leaked through and corrupted error messages in GUI dialogs. Now terminates on any ASCII letter per ECMA-48.
- **(Windows) WM_SIZE parameter overflow**: Clamped `WM_SIZE` width/height parameters to `u16::MAX` to prevent silent truncation on large client areas.
- **Spawn guard key mismatch (Linux)**: The spawn idempotency guard used `window_class` as the deduplication key while the rest of the spawn path used `app_name`, allowing a bypass when the two differed.
- **Spawn guard TOCTOU**: Collapsed check-then-insert race in spawn idempotency guards into an atomic `HashSet::insert` return value on both platforms.

### The Inaugural Release
This is janq 1.0.0. It manages windows, handles hotkeys, and hopefully justifies its existence on your system.

### Core Features
#### Platform Support
- **Full feature parity**: Linux (KDE Plasma 6) and Windows 10/11.
- **Native integrations**: Win32 on Windows; D-Bus and KWin injection on Linux.
- **Symmetric Configuration**: Identical TOML behavior on both platforms.

#### Window Management
- **Dimension flexible**: Support for `px` and `%` units.
- **Display modes**: `follow-mouse`, `active`, and `specific`.
- **Z-Order control**: `keep_above` and Linux-specific `force_priority` (Fullscreen role).
- **Border control**: `no_borders` option to remove window chrome. Supports both global default and per-app overrides.
- **Pager control**: `skip_pager` option to hide managed windows from task managers, pagers, and the task switcher on Linux (defaults to `false`).
- **Workspace control**: `all_desktops` option (Linux only) to allow windows to follow you across virtual desktops (defaults to `true`).
- **Positioning**: Multi-axis slide directions and configurable edge offsets.

#### Multi-App Logic
- **Ordered configuration**: Config order determines menu and activation priority.
- **Atomic transitions**: Synchronized animations where outgoing windows clear the way for incoming ones.

#### Animation Engine
- **Velocity-based scaling**: Durations scale based on distance, ensuring constant pixels-per-second regardless of window position.
- **Unified Easing**: Support for 15+ curves and custom `cubic-bezier`.
- **Opacity fades**: Configurable fade-in and fade-out points.

#### Hotkey & Matching
- **Zero-config registration**: Native sync with KDE and Win32.
- **Weighted Matcher**: Tiered scoring system (exact > substring > visible > managed).

### Performance & Quality
- **Unified Async Architecture**: Migrated both platforms to a shared Tokio-based async event loop for all IPC, file watching, and animation logic.
- **Library Split**: Extracted core logic (config, matching, lifecycle) into a dedicated library crate (`src/lib.rs`).
- **Minimalist Argument Parsing**: Removed `clap` dependency and implemented a minimal manual argument parser, reducing binary size and idle RAM usage by ~0.15MB.
- **Dependency Reduction**: Removed `anyhow` and `dirs` dependencies, replacing them with a custom `Result` type, platform-agnostic error macros, and a local `paths` module for significant binary size reduction and improved compile times.
- **Improved Windows Path Discovery**: Refactored Windows configuration path resolution to be more robust and provide platform-aware error messages (suggesting `%APPDATA%\janq\janq.toml` when no config is found).
- **Unified Cache Architecture**: Consolidated handle tracking for both platforms.
- **Liveness checks**: Efficient `/proc/{pid}` validation on Linux and direct `IsWindow` handle validation on Windows for sub-millisecond response.
- **Zero-Polling**: Event-driven architecture with instant loop wakeups.
- **Zero-Scan KWin Toggles**: Refactored Linux KWin scripts to perform a single-pass discovery using cached IDs and PIDs, eliminating redundant system-wide window scans.
- **Precise Restoration**: Optimized window focus restoration on Linux by targeting cached PIDs directly instead of scanning the window list.
- **Autonomous Sibling Logic**: Sibling windows now operate as independent physical entities with their own individual easing curves and directions. While they share the primary window's base duration, their progress is independently velocity-scaled for synchronized transitions.
- **JSON Argument Consolidation**: Refactored Linux KWin script invocation to use consolidated JSON objects, replacing 26+ positional arguments and eliminating redundant configuration maps for better performance.
- **Pre-calculated Animation Geometry**: Both platforms now fully pre-compute sibling trajectories and durations before entering the high-frequency animation loop, minimizing overhead during rendering.
- **Visual Polish**: Integrated frame-synchronized opacity transitions that respect configured motion easing curves, strictly clamped for stability.
- **Cross-Platform Parity**: Aligned Windows and Linux animation engines; Windows now uses correctly eased progress mapping and respects per-app sibling configurations.
- **Per-Window Blur Management**: Implemented granular `ForceBlur` lifecycle tracking on Linux, ensures compositor blur effects are disabled for each window the millisecond its personal animation ends.
- **Robust Hot-Reloading**: Daemon gracefully discards invalid configs and remains running on the last good state.
- **Desktop-Aware Focus**: Optimized window focus restoration on Linux to be virtual-desktop aware. janq now avoids "snapping" you back to your previous desktop if you've moved desktops while the managed app was open.
- **Platform-Specific Validation**: Added strict configuration validation that blocks startup with an error if Linux-only settings (`all_desktops`, `force_priority`) are present on Windows, regardless of their value.
- **Major Ecosystem Jump (0.62 Windows/8.2 Notify)**: Systematically refactored the entire Windows backend to comply with the strict handle requirements of `windows` 0.62. Migrated `config_watcher.rs` to the `notify` 8.2 API.
- **Dependency Jumps**: Updated `windows` (0.62), `notify` (8.2), `zbus` (5.0), `toml` (1.0), `tokio` (1.49), and `indexmap` (2.13).
- **Architecture Efficiency**: Switched to `FxHash` via `rustc-hash` for performance, replaced `ctrlc` with native `tokio::signal`, and migrated from `fs2` to `fs4`.
- **Flattened internals**: Eliminated proxy modules; consolidated the 1,200-line Win32 monolith into focused sub-modules.

### Utilities
- **KWin Reset Utility**: Added `hard_reset_kwin.sh` for aggressive recovery from KWin state corruption or script hangs.

## [0.1.6] - 2026-01-20

### Fixed
- **Linux**: Comprehensive taskbar respect for all slide directions. Implemented "Dual-Area" logic to anchor shown positions to the workspace while depth hiding to absolute monitor bounds.
- **Linux**: Eliminated diagonal drift during horizontal slides by locking the fixed-axis coordinate to a stable monitor context throughout the animation.
- **Linux**: Resolved "monitor jump" bug where `follow-mouse` mode would shift the coordinate system mid-animation if the cursor moved.
- **Linux**: Fixed follow-mouse hide "jump" and initial parking teleportation.
- **Windows**: Improved hide-animation focus restoration robustness using thread-input attachment.
- **Windows**: Synchronized 10px shadow buffer offsets between animation and parking logic for visual consistency.

### Added
- **Linux**: Independent sibling easing. Sibling windows now use their own individual `hide_easing` curves during primary window toggles.
- **Documentation**: Added priority hierarchy note for animation `duration` and `easing` configurations.

### Changed
- **Windows**: Optimized sibling discovery by utilizing `APP_CACHE`, eliminating expensive system-wide process enumeration during animations.
- **Linux**: Refactored KWin area resolution into a robust `resolveAreaContext` for bulletproof coordinate stability.

## [0.1.5] - 2026-01-20

### Changed
- **Icons**: Simplified and optimized `icon.svg` and `icon.ico` for efficiency.
- **Documentation**: Updated README with responsive icon sizing and refreshed descriptive text.
- **Code Quality**: Extracted shared ANSI stripping and position calculation logic to reduce duplication.
- **Code Quality**: Unified config watcher logic across platform daemons.
- **Code Quality**: Improved error handling and reduced aggressive GUI popups for non-critical issues.

## [0.1.4] - 2026-01-19

### Added
- **Error Display**: Colorized error messages with Rust-style formatting (red errors, blue arrows, cyan values, yellow app names)
- **Error Display**: Visual line pointer showing exact location of config errors
- **Error Display**: GUI error popup when running without terminal (hotkey/service) - spawns terminal on Linux, MessageBox on Windows
- **Utilities**: Added `cleanup_errors.sh` to full cleanup script

### Fixed
- **Error Display**: Fixed regression where TOML syntax errors pointed to wrong line numbers
- **Error Display**: Fixed "unknown field" errors pointing to incorrect lines when field name appeared elsewhere in config
- **Error Display**: GUI error popup now only appears when running without a terminal, avoiding double-display when running from terminal
- **Error Display**: Consistent error behavior on Windows and Linux

## [0.1.3] - 2026-01-19

### Added
- **Configuration**: Support for catch-all `duration` and `easing` keys in the `[animation]` section. These can be used to set both show and hide values simultaneously.

## [0.1.2] - 2026-01-16

### Fixed
- **Windows**: Smooth reversal for cubic-bezier easing curves with overshoot when toggle-spamming during animation. Animation now continues from the symmetric point in the curve rather than restarting.

## [0.1.1] - 2026-01-16

### Added
- **Shortcut**: `offset = "0"` is now an alias for `"center"` in window positioning.

### Fixed
- **Linux**: Enforced `FullScreenArea` for all animations, ensuring 100% stability on all monitor configurations and bypassing taskbar-shrunk calculations.

## [0.1.0] - 2026-01-16

### Added
- **New Feature**: `slide_from` option to control animation direction (`top`, `bottom`, `left`, `right`)
- **New Feature**: `offset` option to control window positioning along the edge (`center`, `50%`, `-10%`, `100px`, `-50px`)
- Both options available globally under `[window]` and per-app for individual overrides
- Defaults preserve existing behavior (slide from top, centered horizontally)

### Fixed
- **Windows**: Sibling windows now correctly use their own `slide_from` and `offset` when hiding (previously used the active window's config)
- **Windows**: Hot-reload now teleports window to new position before animating (matches Linux behavior)
- **Linux**: Fixed ~10-20px Y-offset drift on left/right slides with `center` offset

### Changed
- **Windows**: Refactored `window.rs` (1,188 lines) into 4 focused modules for maintainability:
  - `animation.rs` - Animation engine (~600 lines)
  - `discovery.rs` - Window enumeration and fuzzy matching (~120 lines)
  - `parking.rs` - Park/restore functions (~160 lines)
  - `window.rs` - Core toggle logic and state management (~280 lines)

## [0.0.3] - 2026-01-15

### Added
- **Windows**: Graceful signal handling for Ctrl+C, Ctrl+Break, and console close events
- **Windows**: Logging parity with Linux (config watcher path, grabbing apps, hotkey activation)

### Fixed
- **Windows**: Icon now properly embeds in executable and scales correctly in Windows Explorer

### Changed
- **Build**: Replaced manual `windres` invocation with `embed-resource` crate for reliable icon embedding on GNU/MinGW toolchain
- **Code Quality**: Extracted `spawn_guard.rs` for shared spawn idempotency (SpawnGuard RAII, SPAWNING_APPS static)
- **Code Quality**: Extracted `shutdown.rs` for consistent shutdown messaging across platforms
- **Code Quality**: Extracted `config_watcher.rs` infrastructure for potential future consolidation

## [0.0.2] - 2026-01-15

### Added
- Comprehensive module-level documentation across all Rust files
- New `src/traits.rs` defining shared platform contracts
- Unit tests for fuzzy matching, hotkey validation, and KDE shortcut mapping

### Changed
- **Code Quality**: Extracted `matching.rs` module for fuzzy window matching logic with named scoring constants
- **Code Quality**: Extracted `validation.rs` module for hotkey, easing, and bezier validation
- **Code Quality**: Improved case-insensitive matching (now lowercases both target and window class)
- Removed duplicate tests across modules

### Fixed
- **Linux**: Icon updates now detected by content comparison, ensuring new icons are installed when changed

## [0.0.1] - 2026-01-14

### Fixed
- **Windows**: Resolved issue where Electron apps (like Obsidian) were not "grabbed" (hidden) correctly on startup. Enforced a visibility check during the application polling loop to skip background/hidden windows.
- **Windows**: Fixed missing console output when running from a terminal. Implemented `AttachConsole` to bridge stdout while maintaining a detached GUI process for desktop launches.
- **Windows**: Synchronized daemon logging verbosity with the Linux version, providing better feedback during application startup and window discovery.
