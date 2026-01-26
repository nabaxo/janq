//! Shared config file watching infrastructure.
//!
//! Provides the debounce loop and path matching logic used by both Linux and
//! Windows daemons. Platform-specific reload handlers are passed as callbacks.

use std::{
  path::PathBuf,
  time::{Duration, Instant},
};

use notify::{Config as NotifyConfig, RecommendedWatcher, RecursiveMode, Watcher};
use std::future::Future;
use tokio::sync::mpsc as tokio_mpsc;

// =============================================================================
// Config Watcher
// =============================================================================

/// Spawns a config watcher as an async task with debouncing.
pub fn spawn_config_watcher<F, Fut>(config_path: Option<PathBuf>, on_change: F)
where
  F: Fn() -> Fut + Send + 'static,
  Fut: Future<Output = ()> + Send + 'static,
{
  tokio::spawn(async move {
    run_watcher_loop_async(config_path, on_change).await;
  });
}

/// Async watcher loop with debouncing.
async fn run_watcher_loop_async<F, Fut>(config_path: Option<PathBuf>, on_change: F)
where
  F: Fn() -> Fut + Send + 'static,
  Fut: Future<Output = ()> + Send + 'static,
{
  let (tx, mut rx) = tokio_mpsc::unbounded_channel();
  let mut watcher = match RecommendedWatcher::new(
    move |res| {
      let _ = tx.send(res);
    },
    NotifyConfig::default(),
  ) {
    Ok(w) => w,
    Err(e) => {
      crate::error::show_error(&format!("Failed to create config watcher: {}", e));
      return;
    }
  };

  setup_watch_path(&mut watcher, &config_path);

  let debounce_duration = Duration::from_millis(500);
  let mut last_event = Instant::now();
  let mut pending = false;

  loop {
    let timeout = if pending {
      debounce_duration.saturating_sub(last_event.elapsed())
    } else {
      Duration::from_secs(60)
    };

    tokio::select! {
      res = rx.recv() => {
        match res {
          Some(Ok(event)) => {
            if is_config_event(&event.paths, &config_path) {
              last_event = Instant::now();
              pending = true;
            }
          }
          Some(Err(e)) => crate::error::show_warning(&format!("Watcher error: {:?}", e)),
          None => break,
        }
      }
      _ = tokio::time::sleep(timeout) => {
        if pending {
          pending = false;
          println!("Config change detected, reloading...");
          on_change().await;
        }
      }
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
  } else if let Some(home) = crate::paths::home_dir() {
    let _ = watcher.watch(&home, RecursiveMode::NonRecursive);
  }
}

/// Checks if any of the event paths match the config file.
fn is_config_event(event_paths: &[PathBuf], config_path: &Option<PathBuf>) -> bool {
  let target_abs = if let Some(target_path) = config_path {
    target_path.canonicalize().ok()
  } else {
    crate::paths::home_dir()
      .map(|h| h.join(".janq.toml"))
      .and_then(|p| p.canonicalize().ok())
  };

  let target_abs = match target_abs {
    Some(t) => t,
    None => return false,
  };

  for p in event_paths {
    if let Ok(p_abs) = p.canonicalize() {
      if p_abs == target_abs {
        return true;
      }
    }
  }
  false
}
