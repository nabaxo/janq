//! Shared config file watching infrastructure.
//!
//! Provides the debounce loop and path matching logic used by both Linux and
//! Windows daemons.
//!
//! - **Linux:** Raw `inotify` syscalls (zero dependencies).
//! - **Windows:** `notify` crate (`RecommendedWatcher`).

use std::{
  path::PathBuf,
  time::{Duration, Instant},
};

use std::future::Future;
#[cfg(windows)]
use tokio::sync::mpsc as tokio_mpsc;

// =============================================================================
// Public API (shared)
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

// =============================================================================
// Linux: Raw inotify (see linux/inotify.rs)
// =============================================================================

#[cfg(target_os = "linux")]
async fn run_watcher_loop_async<F, Fut>(config_path: Option<PathBuf>, on_change: F)
where
  F: Fn() -> Fut + Send + 'static,
  Fut: Future<Output = ()> + Send + 'static,
{
  loop {
    let mut rx = match crate::inotify::watch_config(config_path.clone()) {
      Some(rx) => rx,
      None => {
        eprintln!("Watcher: Failed to initialize inotify. Retrying in 5s...");
        tokio::time::sleep(Duration::from_secs(5)).await;
        continue;
      }
    };

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
            Some(()) => {
              last_event = Instant::now();
              pending = true;
            }
            None => {
              eprintln!("Watcher: inotify channel closed. Restarting monitor in 5s...");
              break; // Break inner loop to retry outer
            }
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
    tokio::time::sleep(Duration::from_secs(5)).await;
  }
}

// =============================================================================
// Windows: notify crate
// =============================================================================

#[cfg(windows)]
use notify::{
  Config as NotifyConfig, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher,
};

#[cfg(windows)]
async fn run_watcher_loop_async<F, Fut>(config_path: Option<PathBuf>, on_change: F)
where
  F: Fn() -> Fut + Send + 'static,
  Fut: Future<Output = ()> + Send + 'static,
{
  loop {
    let (tx, mut rx) = tokio_mpsc::unbounded_channel();
    let mut watcher = match RecommendedWatcher::new(
      move |res| {
        let _ = tx.send(res);
      },
      NotifyConfig::default(),
    ) {
      Ok(w) => w,
      Err(e) => {
        eprintln!(
          "Watcher: Failed to create RecommendedWatcher: {}. Retrying in 5s...",
          e
        );
        tokio::time::sleep(Duration::from_secs(5)).await;
        continue;
      }
    };

    if let Err(e) = setup_watch_path(&mut watcher, &config_path) {
      eprintln!(
        "Watcher: Failed to setup watch path: {}. Retrying in 5s...",
        e
      );
      tokio::time::sleep(Duration::from_secs(5)).await;
      continue;
    }

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
            Some(Err(e)) => eprintln!("Watcher: monitor error: {:?}. Restarting in 5s...", e),
            None => {
              eprintln!("Watcher: monitor channel closed. Restarting in 5s...");
              break;
            }
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
    tokio::time::sleep(Duration::from_secs(5)).await;
  }
}

#[cfg(windows)]
fn setup_watch_path(
  watcher: &mut RecommendedWatcher,
  config_path: &Option<PathBuf>,
) -> notify::Result<()> {
  if let Some(path) = config_path {
    if let Ok(abs_path) = path.canonicalize() {
      println!("Watcher: Monitoring config file: {:?}", abs_path);
      // Watch parent directory to catch file replacements (common with editors)
      if let Some(parent) = abs_path.parent() {
        watcher.watch(parent, RecursiveMode::NonRecursive)?;
      } else {
        watcher.watch(&abs_path, RecursiveMode::NonRecursive)?;
      }
    } else {
      watcher.watch(path, RecursiveMode::NonRecursive)?;
    }
  } else if let Some(home) = crate::paths::home_dir() {
    watcher.watch(&home, RecursiveMode::NonRecursive)?;
  }
  Ok(())
}

#[cfg(windows)]
fn is_interesting_event(event: &Event) -> bool {
  matches!(
    event.kind,
    EventKind::Modify(_) | EventKind::Create(_) | EventKind::Remove(_)
  )
}

#[cfg(windows)]
fn is_config_event(event_paths: &[PathBuf], config_path: Option<&PathBuf>) -> bool {
  let target = if let Some(p) = config_path {
    p.clone()
  } else {
    match crate::paths::home_dir().map(|h| h.join(".janq.toml")) {
      Some(h) => h,
      None => return false,
    }
  };

  let target_abs = target.canonicalize().ok();

  for p in event_paths {
    if *p == target {
      return true;
    }
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
