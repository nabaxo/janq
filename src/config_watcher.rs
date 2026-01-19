//! Shared config file watching infrastructure.
//!
//! Provides the debounce loop and path matching logic used by both Linux and
//! Windows daemons. Platform-specific reload handlers are passed as callbacks.
//!
//! Note: Currently unused - daemons still use inline watchers due to complex
//! async/sync callback differences. This module provides shared infrastructure
//! for potential future consolidation.

#![allow(dead_code)]

use std::{
  path::PathBuf,
  sync::mpsc::{self, RecvTimeoutError},
  time::{Duration, Instant},
};

use notify::{Config as NotifyConfig, RecommendedWatcher, RecursiveMode, Watcher};

// =============================================================================
// Config Watcher
// =============================================================================

/// Result from the config watcher event loop for each debounced event.
pub enum WatcherEvent {
  /// Config file was modified (debounced). Payload is the config path.
  ConfigChanged,
  /// Watcher channel disconnected (shutdown).
  Disconnected,
}

/// Spawns a config watcher thread with debouncing.
///
/// Returns a thread handle. The `on_change` callback is called after debouncing
/// when the config file changes. The callback runs on the watcher thread.
pub fn spawn_config_watcher<F>(
  config_path: Option<PathBuf>,
  on_change: F,
) -> std::thread::JoinHandle<()>
where
  F: Fn() + Send + 'static,
{
  std::thread::Builder::new()
    .name("config-watcher".to_string())
    .stack_size(128 * 1024)
    .spawn(move || {
      let (tx, rx) = mpsc::channel();
      let mut watcher = match RecommendedWatcher::new(tx, NotifyConfig::default()) {
        Ok(w) => w,
        Err(e) => {
          crate::error::show_error(&format!("Failed to create config watcher: {}", e));
          return;
        }
      };

      // Setup watch path
      if let Some(path) = &config_path {
        if let Ok(abs_path) = path.canonicalize() {
          println!("Watcher: Monitoring config file: {:?}", abs_path);
          if let Some(parent) = abs_path.parent() {
            let _ = watcher.watch(parent, RecursiveMode::NonRecursive);
          } else {
            let _ = watcher.watch(&abs_path, RecursiveMode::NonRecursive);
          }
        } else {
          let _ = watcher.watch(path, RecursiveMode::NonRecursive);
        }
      } else if let Some(home) = dirs::home_dir() {
        let _ = watcher.watch(&home, RecursiveMode::NonRecursive);
      }

      // Debounce loop
      let debounce_duration = Duration::from_millis(500);
      let mut last_event = Instant::now();
      let mut pending = false;

      loop {
        let timeout = if pending {
          debounce_duration.saturating_sub(last_event.elapsed())
        } else {
          Duration::from_secs(60)
        };

        match rx.recv_timeout(timeout) {
          Ok(Ok(event)) => {
            // Check if this event is for our config file
            let is_config_file = is_config_event(&event.paths, &config_path);
            if is_config_file {
              last_event = Instant::now();
              pending = true;
            }
          }
          Ok(Err(e)) => crate::error::show_warning(&format!("Watcher error: {:?}", e)),
          Err(RecvTimeoutError::Timeout) => {
            if pending {
              pending = false;
              println!("Watcher: Debounced event triggered config reload...");
              on_change();
            }
          }
          Err(RecvTimeoutError::Disconnected) => break,
        }
      }
    })
    .expect("Failed to spawn config watcher thread")
}

/// Checks if any of the event paths match the config file.
fn is_config_event(event_paths: &[PathBuf], config_path: &Option<PathBuf>) -> bool {
  if let Some(target_path) = config_path {
    let target_abs = target_path.canonicalize().unwrap_or(target_path.clone());
    for p in event_paths {
      let p_abs = p.canonicalize().unwrap_or(p.clone());
      if p_abs == target_abs {
        return true;
      }
    }
    false
  } else if let Some(home) = dirs::home_dir() {
    // Default config location
    let target = home.join(".janq.toml");
    event_paths.iter().any(|p| p == &target)
  } else {
    false
  }
}
