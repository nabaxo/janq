pub mod config;
pub mod config_watcher;
pub mod error;
pub mod matching;
pub mod paths;
pub mod process;
pub mod shutdown;
pub mod spawn_guard;
pub mod validation;

use crate::config::Config;
use fs4::fs_std::FileExt;
use std::env::temp_dir;
use std::fs::File;

pub fn acquire_lock_file() -> crate::error::Result<File> {
  let lock_path = temp_dir().join("janq.lock");
  let lock_file = File::create(&lock_path)?;
  if lock_file.try_lock_exclusive().is_err() {
    return Err(crate::format_error_boxed!(
      "janq is already running (lock file active)."
    ));
  }
  Ok(lock_file)
}

pub fn resolve_target_app(msg: &str, cfg: &Config) -> Option<String> {
  let app_name = if msg.starts_with("toggle:") {
    msg.strip_prefix("toggle:").unwrap().trim().to_string()
  } else if msg == "toggle" {
    cfg.app.keys().next().cloned().unwrap_or_default()
  } else {
    msg.trim().to_string()
  };

  if cfg.app.len() == 1 {
    cfg.app.keys().next().cloned()
  } else if cfg.app.contains_key(&app_name) {
    Some(app_name)
  } else {
    None
  }
}
