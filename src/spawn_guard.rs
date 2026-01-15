//! Shared spawn idempotency infrastructure.
//!
//! Prevents duplicate process spawns during rapid toggles by maintaining
//! a global set of "currently spawning" app names. The `SpawnGuard` RAII
//! wrapper ensures cleanup on success, error, or panic.

use rustc_hash::FxHashSet;
use std::sync::{Mutex, OnceLock};

// =============================================================================
// Spawn Idempotency Lock
// =============================================================================

/// Static set tracking apps currently being spawned.
/// Prevents duplicate spawn attempts during rapid toggles.
static SPAWNING_APPS: OnceLock<Mutex<FxHashSet<String>>> = OnceLock::new();

/// Returns the global spawning apps set.
pub fn get_spawning_apps() -> &'static Mutex<FxHashSet<String>> {
  SPAWNING_APPS.get_or_init(|| Mutex::new(FxHashSet::default()))
}

// =============================================================================
// SpawnGuard RAII
// =============================================================================

/// RAII guard that removes an app from the spawning set on drop.
///
/// Ensures idempotency lock is released even on error or panic.
pub struct SpawnGuard(String);

impl SpawnGuard {
  /// Creates a new spawn guard for the given app name.
  pub fn new(app_name: impl Into<String>) -> Self {
    Self(app_name.into())
  }
}

impl Drop for SpawnGuard {
  fn drop(&mut self) {
    let mut spawning = get_spawning_apps().lock().unwrap();
    spawning.remove(&self.0);
  }
}
