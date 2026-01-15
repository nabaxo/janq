# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [1.0.3] - 2026-01-15

### Added
- **Windows**: Graceful signal handling for Ctrl+C, Ctrl+Break, and console close events
- **Windows**: Logging parity with Linux (config watcher path, grabbing apps, hotkey activation)

### Changed
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
