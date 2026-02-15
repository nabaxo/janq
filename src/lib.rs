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

pub fn acquire_lock_file() -> crate::error::Result<()> {
  let lock_dir = crate::paths::cache_dir()
    .ok_or_else(|| crate::format_error_boxed!("Could not determine cache directory"))?
    .join("janq");

  std::fs::create_dir_all(&lock_dir)?;

  let lock_path = lock_dir.join("janq.lock");
  let lock_file = std::fs::OpenOptions::new()
    .read(true)
    .write(true)
    .create(true)
    .truncate(false)
    .open(&lock_path)?;

  let lock_res = lock_file.try_lock_exclusive();

  if lock_res.is_err() || lock_res.ok() == Some(false) {
    return Err(crate::format_error_boxed!(
      "janq is already running (lock file active)."
    ));
  }

  // Leak the file handle to ensure it lives as long as the process.
  // This prevents the Rust compiler from optimizing out the variable
  // during async yields, which was previously releasing the lock.
  Box::leak(Box::new(lock_file));
  Ok(())
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
