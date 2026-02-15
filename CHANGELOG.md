# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [1.0.0] - 2026-02-16

### Added
- **Install script**: For easy installation and system integration on Linux.

### Added
- **GUI Warning Pop-ups**: Warnings now display as GUI pop-ups in non-interactive sessions (Windows/Linux).
- **Improved CLI Error Handling**: Enhanced argument parsing with suggestions for unknown arguments using fuzzy matching.

### Fixed
- **Single-Instance Lock**: Fixed lock file mechanism to correctly handle platform-specific `fs4` behavior. On Linux (MUSL), the lock now properly detects `Ok(false)` return values. Lock acquisition now only occurs when starting a daemon process, not during client IPC operations.
- **Windows Error Visibility**: Lock file errors and other critical errors during daemon startup now properly invoke `show_error()` instead of silently propagating through the async runtime, ensuring users see error dialogs on Windows.
- **Error Display**: Converted silent `eprintln!` error messages to user-visible GUI notifications for non-terminal sessions.
- **Lock File Lifetime**: Lock file handle is now leaked using `Box::leak()` to ensure it persists for the entire process lifetime, preventing premature release during async yields.

### Changed
- **Lock File Storage**: Moved lock file from config directory to cache directory (`XDG_CACHE_HOME` on Linux, `LOCALAPPDATA` on Windows) to follow platform conventions.
- **Import Cleanup**: Refactored internal module imports across `main.rs` and daemon modules to use consolidated `janq::` import blocks instead of fully-qualified paths, improving code readability and maintainability.
- **Linux Error Handling**: Simplified Linux error and warning display by removing unused `kdialog` and `zenity` fallbacks and non-blocking terminal spawn logic.

### Added
- **Automated KWin Rule Lifecycle**: Implemented a sophisticated rule synchronization engine that manages `kwinrulesrc` automatically. It groups managed apps by desktop ID, applies regex-based matching, and surgically prunes stale or redundant janq rules directly from the filesystem to bypass KConfig errors.
- **`kde_window_rules` Setting**: New configuration option under `[window]` (defaulting to `true`) to control whether janq manages system window rules for taskbar icon fixes.
- **Linux Setup & Cleanup**: New `--setup` CLI flag to force regeneration of desktop files, icons, and D-Bus services, and a `--cleanup` flag to completely purge janq system integration (rules, desktop files, services).
- **D-Bus Reload Trigger**: Added automatic `org.freedesktop.DBus.ReloadConfig` calls (via `qdbus6` or `dbus-send`) when system services are installed, ensuring "activatable" services work without a logout.
- **KWin Icon Rule Automation**: Added `desktop_file_id` configuration field to support forced icon association via KWin Window Rules.
- **Auto-Discovery of Desktop Files**: `janq` now automatically searches for the correct `.desktop` association for your managed apps based on their `window_class`, fixing generic Wayland icons without any manual configuration required.

### Changed
- **Unified Rule Engine**: Refactored the Linux KWin rule management into a single execution path for both installation and purging, ensuring consistent cleanup of `kwinrulesrc` across all operations.
- **Normalized Error Handling**: Standardized result types and error macros across the Linux backend for better consistency with the core crate.

### Fixed
- **Linux First-Run Reliability**: Icons and D-Bus services now index correctly on the first run of the app on fresh KDE 6 installations.
- **Icon Cache Refresh**: Integrated `kbuildsycoca6 --noincremental` triggers into the installation flow to ensure the `janq` icon appears in the taskbar immediately.

## [1.0.0] - 2026-02-06

### Added
- **Unified Selection Engine (Task 2)**: Consolidated all window discovery and fuzzy matching logic into a shared platform-agnostic crate module. Windows and Linux now share a weighted scoring algorithm (Exact > Substring > Managed > Visible).
- **Name-Aware Process Liveness**: New `process` module for platform-agnostic PID verification.
  - **Linux**: Migrated to `/proc/{pid}/cmdline` parsing to support full binary names (e.g., `org.wezfurlong.wezterm`) and bypass 15-character kernel `comm` truncation.
  - **Windows**: Implemented `GetExitCodeProcess` verification to prevent "Zombie App" detection caused by PID recycling.
- **Focus Inheritance (Linux)**: Implemented sticky focus restoration for rapid app switching. Toggling between multiple Janq-managed apps now correctly preserves and "inherits" the original external window focus target.
- **Momentum-Aware Animations**: Overhauled animation engines on both platforms to support "Handover" states. Toggling an app mid-animation now picks up from the current opacity/position instead of snapping back to the start.

### Fixed
- **Windows Focus "Yo-Yo"**: Hardened focus hooks to prevent `auto_hide` from triggering incorrectly when a window is manually toggled or focus-forced.
- **Windows WezTerm Reopening**: Fixed a bug where closed windows stayed in the management cache, preventing re-spawning. Added aggressive cache pruning upon PID death detection.
- **Linux Focus Restore Lag**: Bypassed the metadata cache during toggle events to ensure focus restoration targets are captured with zero-latency D-Bus synchronization.
- **Type-Safety & Build Reliability**: Resolved numerous cross-platform type regressions and Win32 import conflicts introduced during the architectural deduplication.

### Changed
- **Zero-Allocation Discovery Loops**: Refactored core window discovery interfaces to use `&[FoundWindow]` slices. This eliminates thousands of heap allocations per minute during system polling and hotkey triggers.
- **Aggressive Cache Pruning**: Windows backend now proactively evicts handles if `IsWindow` or `is_process_running` fails, ensuring much faster recovery from application crashes.

## [1.0.0] - 2026-01-31

### Added
- **Auto-Hide focus watcher**: New `auto_hide` option in the `[window]` block to automatically hide the window when it loses focus.
- **Systray menu on Linux**: Add tray functionality via `ksni` to linux with full menu and shortcut display

### Fixed
- **Shortcut display in tray**: Windows now displays shortcut on context menu in systray.

### Changed
- **Change opt-level**: Change opt-level to `"s"` for even harder memory optimization.

### Added
- **Animation Framerate Control**: New `framerate` option for the `[animation]` block.
  - Supports `"auto"` (VSync/Platform default), a specific number (e.g., `60`, `120`), or `0` to disable animations entirely (instant transitions).
  - Cross-platform implementation using `DwmFlush` on Windows and frequency-clamped timers on Linux.
- **Strict Validation**: Configuration parsing now strictly enforces numeric framerates (no quoted numbers) and validates ranges (0-1000).

### Fixed
- **Instant Focus**: Resolved issue on both Windows and Linux where `framerate = 0` (instant mode) would fail to grab focus correctly.
- **Opacity Sync**: Opacity animations are now automatically bypassed if animations are disabled (`framerate = 0`), preventing windows from appearing invisible.
- **Config Watcher Loop**: Fixed an infinite reload loop on Linux caused by the file watcher triggering on "Access" events.
- **Windows Polish**: Fixed a race condition where Win32 focus calls could fail during near-instant transitions.

### Changed
- **Config Refactoring**: Internal simplification of `Dimension`, `PositionOffset`, and `DisplayMode` deserialization to reduce code duplication and improve maintainability.

## [1.0.0] - 2026-01-24

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
- **Weighted Matcher**: Tiered scoring system (exact > subtitle > visible > managed).

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
- **Dependency Jumps**: Updated `windows` (0.62), `notify` (8.2), `zbus` (5.0), `toml` (0.9), `tokio` (1.49), and `indexmap` (2.7).
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
