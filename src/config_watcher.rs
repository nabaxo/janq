//! Shared config file watching infrastructure.
//!
//! Provides the debounce loop and path matching logic used by both Linux and
//! Windows daemons. Platform-specific reload handlers are passed as callbacks.

use std::{
  path::PathBuf,
  sync::mpsc::{self, RecvTimeoutError},
  time::{Duration, Instant},
};

use notify::{Config as NotifyConfig, RecommendedWatcher, RecursiveMode, Watcher};

// =============================================================================
// Config Watcher
// =============================================================================

/// Spawns a config watcher thread with debouncing.
///
/// Returns a thread handle. The `on_change` callback is called after debouncing
/// when the config file changes. The callback runs on the watcher thread.
///
/// # Arguments
/// * `config_path` - Path to watch, or None to watch `~/.janq.toml`
/// * `on_change` - Callback invoked on debounced config change (runs on watcher thread)
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
      run_watcher_loop(config_path, on_change);
    })
    .expect("Failed to spawn config watcher thread")
}

/// Core watcher loop with debouncing. Extracted for testability.
fn run_watcher_loop<F>(config_path: Option<PathBuf>, on_change: F)
where
  F: Fn(),
{
  let (tx, rx) = mpsc::channel();
  let mut watcher = match RecommendedWatcher::new(tx, NotifyConfig::default()) {
    Ok(w) => w,
    Err(e) => {
      crate::error::show_error(&format!("Failed to create config watcher: {}", e));
      return;
    }
  };

  // Setup watch path
  setup_watch_path(&mut watcher, &config_path);

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
        if is_config_event(&event.paths, &config_path) {
          last_event = Instant::now();
          pending = true;
        }
      }
      Ok(Err(e)) => crate::error::show_warning(&format!("Watcher error: {:?}", e)),
      Err(RecvTimeoutError::Timeout) => {
        if pending {
          pending = false;
          println!("Config change detected, reloading...");
          on_change();
        }
      }
      Err(RecvTimeoutError::Disconnected) => break,
    }
  }
}

/// Sets up the file watcher on the appropriate path.
fn setup_watch_path(watcher: &mut RecommendedWatcher, config_path: &Option<PathBuf>) {
  if let Some(path) = config_path {
    if let Ok(abs_path) = path.canonicalize() {
      println!("Watcher: Monitoring config file: {:?}", abs_path);
      // Watch parent directory to catch file replacements (common with editors)
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
