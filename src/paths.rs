use std::env;
use std::path::PathBuf;

/// Returns the home directory of the current user.
pub fn home_dir() -> Option<PathBuf> {
  #[cfg(target_os = "windows")]
  {
    env::var_os("USERPROFILE").map(PathBuf::from)
  }
  #[cfg(target_os = "linux")]
  {
    env::var_os("HOME").map(PathBuf::from)
  }
}

/// Returns the config directory of the current user.
pub fn config_dir() -> Option<PathBuf> {
  #[cfg(target_os = "windows")]
  {
    env::var_os("APPDATA").map(PathBuf::from)
  }
  #[cfg(target_os = "linux")]
  {
    env::var_os("XDG_CONFIG_HOME")
      .map(PathBuf::from)
      .or_else(|| home_dir().map(|h| h.join(".config")))
  }
}

/// Returns the local data directory of the current user.
pub fn data_local_dir() -> Option<PathBuf> {
  #[cfg(target_os = "windows")]
  {
    env::var_os("LOCALAPPDATA").map(PathBuf::from)
  }
  #[cfg(target_os = "linux")]
  {
    env::var_os("XDG_DATA_HOME")
      .map(PathBuf::from)
      .or_else(|| home_dir().map(|h| h.join(".local/share")))
  }
}

/// Returns the per-user runtime directory (`$XDG_RUNTIME_DIR`): private (0700)
/// and RAM-backed, unlike the world-writable `/dev/shm` and `/tmp`.
#[cfg(target_os = "linux")]
pub fn runtime_dir() -> Option<PathBuf> {
  env::var_os("XDG_RUNTIME_DIR")
    .filter(|dir| !dir.is_empty())
    .map(PathBuf::from)
}

/// Returns the cache directory of the current user.
pub fn cache_dir() -> Option<PathBuf> {
  #[cfg(target_os = "windows")]
  {
    env::var_os("LOCALAPPDATA").map(PathBuf::from)
  }
  #[cfg(target_os = "linux")]
  {
    env::var_os("XDG_CACHE_HOME")
      .map(PathBuf::from)
      .or_else(|| home_dir().map(|h| h.join(".cache")))
  }
}
