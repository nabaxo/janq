//! Shared config file watching infrastructure.
//!
//! Provides the debounce loop and path matching logic used by both Linux and
//! Windows daemons. Platform-specific reload handlers are passed as callbacks.

use std::{
  path::PathBuf,
  time::{Duration, Instant},
};

use notify::{
  Config as NotifyConfig, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher,
};
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

  let settle_delay = Duration::from_millis(100);
  let mut last_event = Instant::now();
  let mut pending = false;

  loop {
    let timeout = if pending {
      settle_delay.saturating_sub(last_event.elapsed())
    } else {
      Duration::from_secs(60)
    };

    tokio::select! {
      res = rx.recv() => {
        match res {
          Some(Ok(event)) => {
            if is_interesting_event(&event) && is_config_event(&event.paths, config_path.as_ref()) {
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

/// Consolidated helper to reload config and update shared state.
/// Returns the OLD configuration if reload was successful.
pub fn reload_shared_config(
  path: Option<PathBuf>,
  shared_config: &std::sync::RwLock<crate::config::Config>,
) -> Option<crate::config::Config> {
  match crate::config::load_config(path) {
    Ok((new_cfg, _)) => {
      let mut w = shared_config.write().unwrap();
      let old = w.clone();
      *w = new_cfg;
      Some(old)
    }
    Err(e) => {
      crate::error::show_error(&format!(
        "Config reload failed: {}\nStaying with last known good configuration.",
        e
      ));
      None
    }
  }
}

/// Checks if the event is a modification we care about (ignores access/metadata)
fn is_interesting_event(event: &Event) -> bool {
  match event.kind {
    EventKind::Modify(_) | EventKind::Create(_) | EventKind::Remove(_) => true,
    _ => false,
  }
}

/// Checks if any of the event paths match the config file.
fn is_config_event(event_paths: &[PathBuf], config_path: Option<&PathBuf>) -> bool {
  let target = if let Some(p) = config_path {
    p.clone()
  } else {
    match crate::paths::home_dir().map(|h| h.join(".janq.toml")) {
      Some(h) => h,
      None => return false,
    }
  };

  // Pre-calculate canonical target for faster comparison
  let target_abs = target.canonicalize().ok();

  for p in event_paths {
    // 1. Exact path match (handles cases where canonicalization fails, e.g. deletions)
    if *p == target {
      return true;
    }

    // 2. Canonical match (handles symlinks and relative path variations)
    if let Ok(p_abs) = p.canonicalize() {
      if let Some(ref t_abs) = target_abs {
        if p_abs == *t_abs {
          return true;
        }
      }
    }
  }
  false
}
