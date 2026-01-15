#![allow(dead_code)] // Traits serve as documentation, not runtime polymorphism
//! Shared traits defining platform-agnostic interfaces.
//!
//! These traits establish common contracts for window management and process control
//! that are implemented differently on Linux (KWin/D-Bus) and Windows (Win32 API).
//! They serve as documentation of the expected behavior rather than runtime dispatch,
//! since platform selection happens at compile time via `cfg` attributes.

use crate::config::{AppConfig, Config, FoundWindow};

// =============================================================================
// Window Discovery Interface
// =============================================================================

/// Interface for discovering and matching windows across the system.
///
/// On Linux, this queries KWin via D-Bus scripts that enumerate window metadata.
/// On Windows, this uses `EnumWindows` and process handle inspection.
///
/// The fuzzy matching algorithm uses a weighted scoring system:
/// - Exact match: 10000 points
/// - Substring match: 5000 points
/// - Subsequence match: 1000 base + boundary/consecutive bonuses
/// - Visibility bonus: 2000 points
/// - Already-managed bonus: 1000 points
pub trait WindowDiscovery {
  /// Fetches all visible application windows on the system.
  ///
  /// Filters out system windows (plasmashell, kwin, tool windows) and
  /// returns a list suitable for fuzzy matching against user config.
  fn fetch_all_windows(&self) -> Vec<FoundWindow>;

  /// Finds a window matching the given class name pattern.
  ///
  /// # Arguments
  /// * `class` - The window class to search for (e.g., "wezterm", "obsidian")
  /// * `candidates` - Optional pre-fetched window list to search within
  ///
  /// # Returns
  /// The best matching window's ID, or None if no suitable match found.
  fn find_by_class(&self, class: &str, candidates: Option<&[FoundWindow]>) -> Option<String>;
}

// =============================================================================
// Process Lifecycle Interface
// =============================================================================

/// Interface for managing terminal/application process lifecycle.
///
/// Handles spawning applications when they don't exist, detecting existing
/// processes, and managing the idempotency lock to prevent duplicate spawns.
pub trait ProcessManager {
  /// Ensures the terminal application for the given app is running.
  ///
  /// This is idempotent: calling it multiple times rapidly will only
  /// spawn one instance due to the internal spawn lock mechanism.
  ///
  /// # Returns
  /// `true` if a new process was spawned, `false` if already running.
  fn ensure_running(&self, app_name: &str, app_cfg: &AppConfig, config: &Config) -> bool;

  /// Checks if the process for the given app is still alive.
  ///
  /// On Linux: Checks `/proc/<pid>/` existence.
  /// On Windows: Calls `IsWindow()` on cached HWND.
  fn is_process_alive(&self, app_name: &str) -> bool;
}

// =============================================================================
// Animation/Toggle Interface
// =============================================================================

/// Interface for window visibility toggle and animation control.
///
/// Manages the show/hide animation, focus restoration, and sibling window
/// coordination (hiding other janq-managed windows when showing one).
pub trait VisibilityController {
  /// Toggles the visibility of the specified app's window.
  ///
  /// - If hidden → shows with slide-down animation, gains focus
  /// - If visible → hides with slide-up animation, restores previous focus
  ///
  /// # Returns
  /// `true` if toggle was initiated, `false` if window not found.
  fn toggle(&self, app_name: &str, config: &Config) -> bool;

  /// Restores a window to its normal state (undoes quake positioning).
  ///
  /// Called during graceful shutdown or when an app is removed from config.
  fn restore(&self, window_class: &str);
}
