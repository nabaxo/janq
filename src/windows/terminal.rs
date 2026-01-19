//! Terminal/process lifecycle management for Windows.
//!
//! ## Responsibilities
//!
//! 1. **Window Discovery** - Uses `EnumWindows` and process inspection
//! 2. **Process Spawning** - Creates detached processes via `DETACHED_PROCESS` flag
//! 3. **Spawn Idempotency** - Prevents duplicate spawns with static lock
//! 4. **Visibility Polling** - Waits for window to appear and become visible
//!
//! ## Key Differences from Linux
//!
//! - Synchronous (no async/await)
//! - Uses `IsWindow`/`IsWindowVisible` for liveness checks
//! - `DETACHED_PROCESS` flag prevents console window inheritance

use std::{
  os::windows::process::CommandExt,
  process::{Command, Stdio},
  time::Duration,
};

use windows::Win32::UI::WindowsAndMessaging::{IsWindow, IsWindowVisible};

use crate::config::{AppConfig, Config, FoundWindow};
use crate::spawn_guard::{get_spawning_apps, SpawnGuard};
use crate::windows::window::{find_window_by_process, get_hwnd_cache, park_window, SendHwnd};

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
        if IsWindow(hwnd.0).as_bool() {
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

      park_window(SendHwnd(hwnd), config, app_cfg);
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

  let _guard = SpawnGuard::new(app_name);

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

  println!("Starting terminal: {}", app_cfg.start_command);
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
      crate::windows::show_error(&format!("Failed to start managed app: {}", e));
      return false;
    }
  }

  // Wait for window to appear
  let mut found = false;
  for i in 0..80 {
    // Wait up to 8s (100ms * 80)
    std::thread::sleep(Duration::from_millis(100));
    let send_hwnd = {
      if let Some(hwnd) = find_window_by_process(&app_cfg.window_class, None) {
        if unsafe { IsWindowVisible(hwnd).as_bool() } {
          found = true;
          {
            let mut cache = get_hwnd_cache().write().unwrap();
            cache.insert(app_name.to_string(), SendHwnd(hwnd));
          }
          Some(SendHwnd(hwnd))
        } else {
          None
        }
      } else {
        None
      }
    };

    if let Some(sh) = send_hwnd {
      // Add a slight settling delay for the window to be ready for manipulation
      std::thread::sleep(Duration::from_millis(50));

      // AUTOMATIC GRAB: Park newly discovered window immediately
      park_window(sh, config, app_cfg);

      break;
    }

    if i > 0 && (i + 1) % 20 == 0 {
      println!(
        "janq: Still waiting for window '{}' to appear (attempt {}/80)...",
        app_name,
        i + 1
      );
    }
  }

  if !found {
    crate::windows::show_error(&format!(
      "janq: Failed to detect window for '{}' after spawning.",
      app_name
    ));
  }
  found
}
