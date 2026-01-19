# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [1.1.3] - 2026-01-19

### Added
- **Configuration**: Support for catch-all `duration` and `easing` keys in the `[animation]` section. These can be used to set both show and hide values simultaneously.

## [1.1.2] - 2026-01-16

### Fixed
- **Windows**: Smooth reversal for cubic-bezier easing curves with overshoot when toggle-spamming during animation. Animation now continues from the symmetric point in the curve rather than restarting.

## [1.1.1] - 2026-01-16

### Added
- **Shortcut**: `offset = "0"` is now an alias for `"center"` in window positioning.

### Fixed
- **Linux**: Enforced `FullScreenArea` for all animations, ensuring 100% stability on all monitor configurations and bypassing taskbar-shrunk calculations.

## [1.1.0] - 2026-01-16

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

## [1.0.3] - 2026-01-15

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

## [1.0.2] - 2026-01-15

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

## [1.0.1] - 2026-01-14

### Fixed
- **Windows**: Resolved issue where Electron apps (like Obsidian) were not "grabbed" (hidden) correctly on startup. Enforced a visibility check during the application polling loop to skip background/hidden windows.
- **Windows**: Fixed missing console output when running from a terminal. Implemented `AttachConsole` to bridge stdout while maintaining a detached GUI process for desktop launches.
- **Windows**: Synchronized daemon logging verbosity with the Linux version, providing better feedback during application startup and window discovery.
