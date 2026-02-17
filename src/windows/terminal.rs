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

use crate::windows::window::{
  find_window_by_process, get_app_cache, park_window, update_managed_hwnds_cache,
};
use janq::config::{AppConfig, Config, FoundWindow};
use janq::spawn_guard::{get_spawning_apps, SpawnGuard};

pub fn ensure_terminal_running(
  app_name: &str,
  app_cfg: &AppConfig,
  config: &Config,
  candidates: Option<&[FoundWindow]>,
) -> bool {
  // 0. Check cache first
  {
    let cache = get_app_cache().read().unwrap();
    if let Some(cw) = cache.get(app_name) {
      // Check window liveness via IsWindow
      unsafe {
        if IsWindow(Some(cw.hwnd)).as_bool() {
          return false; // Already managed and running
        }
      }
    }
  }

  // 0.5. Idempotency Lock Part 1: Early Exit
  {
    let spawning = get_spawning_apps().lock().unwrap();
    if spawning.contains(app_name) {
      return false;
    }
  }

  // 1. Check if window already exists
  if let Some(cw) = find_window_by_process(&app_cfg.window_class, candidates, Some(app_name)) {
    {
      let mut cache = get_app_cache().write().unwrap();
      cache.insert(std::sync::Arc::from(app_name), cw);
    }
    update_managed_hwnds_cache();

    park_window(cw, config, app_cfg);
    return false;
  }

  // 2. Idempotency Lock Part 2: Acquisition
  {
    let mut spawning = get_spawning_apps().lock().unwrap();
    if spawning.contains(app_name) {
      return false; // Rare race condition win
    }
    spawning.insert(app_name.to_string());
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

  println!(
    "janq: starting app '{}' (cmd: {})...",
    app_name, app_cfg.start_command
  );
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
  let mut current_delay = 100;

  for i in 0..50 {
    // Poll for window with linear backoff to save CPU/Battery
    std::thread::sleep(Duration::from_millis(current_delay));
    let cw = {
      if let Some(found_cw) = find_window_by_process(&app_cfg.window_class, None, Some(app_name)) {
        if unsafe { IsWindowVisible(found_cw.hwnd).as_bool() } {
          found = true;
          {
            let mut cache = get_app_cache().write().unwrap();
            cache.insert(std::sync::Arc::from(app_name), found_cw);
          }
          update_managed_hwnds_cache();
          Some(found_cw)
        } else {
          None
        }
      } else {
        None
      }
    };

    if let Some(sh) = cw {
      // Add a slight settling delay for the window to be ready for manipulation
      std::thread::sleep(Duration::from_millis(50));

      // AUTOMATIC GRAB: Park newly discovered window immediately
      park_window(sh, config, app_cfg);

      break;
    }

    if i > 0 && (i + 1) % 10 == 0 {
      println!(
        "janq: Still waiting for window '{}' to appear (attempt {}/50)...",
        app_name,
        i + 1
      );
    }

    // Backoff: Starts at 100ms, increases by 50ms every loop after 5 attempts, capped at 1s.
    if i >= 5 {
      current_delay = (current_delay + 50).min(1000);
    }
  }

  if !found {
    crate::windows::show_error(&format!(
      "janq: Failed to detect window for '{}' after spawning.\n\nPlease check if 'start_command' is correct and executable.",
      app_name
    ));
  }
  found
}
