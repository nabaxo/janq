//! Shared in-memory cache for Linux window tracking and process liveness.
//!
//! Unifies identity (window_class) and liveness (PID) into a single source
//! of truth, used by both the hot-path (liveness checks) and KWin scripts.

use rustc_hash::FxHashMap;
use std::sync::{Mutex, OnceLock};

#[derive(Clone, Debug)]
pub struct CachedWindow {
  /// The KWin window ID (e.g., "{abc-123}")
  pub id: String,
  /// The process ID for liveness checks via /proc
  pub pid: u32,
}

static CACHE: OnceLock<Mutex<FxHashMap<String, CachedWindow>>> = OnceLock::new();

pub fn get_cache() -> &'static Mutex<FxHashMap<String, CachedWindow>> {
  CACHE.get_or_init(|| Mutex::new(FxHashMap::default()))
}

/// Updates or inserts a window into the cache.
pub fn update_cache(app_name: &str, window_id: String, pid: u32) {
  let mut cache = get_cache().lock().unwrap();
  cache.insert(app_name.to_string(), CachedWindow { id: window_id, pid });
}

/// Retrieves a cached window by app name.
pub fn get_cached_window(app_name: &str) -> Option<CachedWindow> {
  let cache = get_cache().lock().unwrap();
  cache.get(app_name).cloned()
}

/// Removes an app from the cache (e.g., if it crashed or was removed from config).
pub fn remove_from_cache(app_name: &str) {
  let mut cache = get_cache().lock().unwrap();
  cache.remove(app_name);
}
