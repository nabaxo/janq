use std::{
  collections::HashSet,
  os::windows::process::CommandExt,
  process::{Command, Stdio},
  sync::{Mutex, OnceLock},
  time::Duration,
};

use crate::config::{AppConfig, Config, FoundWindow};
use crate::windows::window::{find_window_by_process, get_hwnd_cache, SendHwnd};

static SPAWNING_APPS: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();

pub fn get_spawning_apps() -> &'static Mutex<HashSet<String>> {
  SPAWNING_APPS.get_or_init(|| Mutex::new(HashSet::new()))
}

pub fn ensure_terminal_running(
  app_name: &str,
  app_cfg: &AppConfig,
  config: &Config,
  candidates: Option<&[FoundWindow]>,
) -> bool {
  // 0. Check cache first
  {
    let cache = get_hwnd_cache().read().unwrap();
    if let Some(hwnd) = cache.get(app_name) {
      unsafe {
        if windows::Win32::UI::WindowsAndMessaging::IsWindow(hwnd.0).as_bool() {
          return false; // Already managed and running
        }
      }
    }
  }

  // Loop to acquire lock or check existing window
  loop {
    // 1. Check if window already exists
    // We only use candidates on the first pass of the loop if provided
    let list_to_check = if candidates.is_some() {
      candidates
    } else {
      None
    };
    if let Some(hwnd) = find_window_by_process(&app_cfg.window_class, list_to_check) {
      {
        let mut cache = get_hwnd_cache().write().unwrap();
        cache.insert(app_name.to_string(), SendHwnd(hwnd));
      }

      crate::windows::window::park_window(SendHwnd(hwnd), config, app_cfg);
      return false;
    }

    // 2. Idempotency Lock
    let already_spawning = {
      let spawning = get_spawning_apps().lock().unwrap();
      spawning.contains(app_name)
    };

    if !already_spawning {
      // Try to acquire the "lock" by adding to the set
      let mut spawning = get_spawning_apps().lock().unwrap();
      spawning.insert(app_name.to_string());
      break;
    }

    std::thread::sleep(Duration::from_millis(100));
  }

  // Ensure we remove the app from the spawning set even on error/panic
  struct SpawnGuard(String);
  impl Drop for SpawnGuard {
    fn drop(&mut self) {
      let mut spawning = get_spawning_apps().lock().unwrap();
      spawning.remove(&self.0);
    }
  }
  let _guard = SpawnGuard(app_name.to_string());

  // -- CRITICAL SECTION --
  // We own the spawn lock for this specific app.

  if app_cfg.start_command.is_empty() {
    return false;
  }

  // On Windows, start_command might need cmd /C or just running executable
  let parts: Vec<&str> = app_cfg.start_command.split_whitespace().collect();
  if parts.is_empty() {
    return false;
  }

  let cmd = parts[0];
  let final_args = &parts[1..];

  const DETACHED_PROCESS: u32 = 0x00000008;

  let spawn_result = Command::new(cmd)
    .args(final_args)
    .creation_flags(DETACHED_PROCESS)
    .stdin(Stdio::null())
    .stdout(Stdio::null())
    .stderr(Stdio::null())
    .spawn();

  match spawn_result {
    Ok(_) => {}
    Err(e) => {
      println!("Failed to start managed app: {}", e);
      return false;
    }
  }

  // Wait for window to appear
  let mut found = false;
  for _ in 0..80 {
    // Wait up to 8s (100ms * 80)
    std::thread::sleep(Duration::from_millis(100));
    let send_hwnd = {
      if let Some(hwnd) = find_window_by_process(&app_cfg.window_class, None) {
        found = true;

        {
          let mut cache = get_hwnd_cache().write().unwrap();
          cache.insert(app_name.to_string(), SendHwnd(hwnd));
        }

        Some(SendHwnd(hwnd))
      } else {
        None
      }
    };

    if let Some(sh) = send_hwnd {
      // Add a slight settling delay for the window to be ready for manipulation
      std::thread::sleep(Duration::from_millis(50));

      // AUTOMATIC GRAB: Park newly discovered window immediately
      crate::windows::window::park_window(sh, config, app_cfg);

      break;
    }
  }
  found
}
