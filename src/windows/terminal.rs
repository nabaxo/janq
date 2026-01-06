use crate::config::{AppConfig, Config};
use crate::windows::window::{find_window_by_process, get_hwnd_cache, SendHwnd};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

// Global guard for preventing multiple spawns
static IS_SPAWNING: AtomicBool = AtomicBool::new(false);

pub async fn ensure_terminal_running(app_name: &str, app_cfg: &AppConfig, config: &Config) -> bool {
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
    // 1. Check if window already exists (e.g. user started it manually or it appeared while waiting for lock)
    if let Some(hwnd) = find_window_by_process(&app_cfg.window_class) {
      {
        let mut cache = get_hwnd_cache().write().unwrap();
        cache.insert(app_name.to_string(), SendHwnd(hwnd));
      }

      crate::windows::window::park_window(SendHwnd(hwnd), config, app_cfg).await;
      return false;
    }

    // 2. Try to take the spawn lock
    if IS_SPAWNING
      .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
      .is_ok()
    {
      break; // Got lock, proceed to spawn
    }

    tokio::time::sleep(Duration::from_millis(200)).await;
  }

  // -- CRITICAL SECTION --
  // We own the spawn lock.

  if app_cfg.start_command.is_empty() {
    IS_SPAWNING.store(false, Ordering::SeqCst);
    return false;
  }

  // On Windows, start_command might need cmd /C or just running executable
  // Split command
  let parts: Vec<&str> = app_cfg.start_command.split_whitespace().collect();
  if parts.is_empty() {
    IS_SPAWNING.store(false, Ordering::SeqCst);
    return false;
  }

  let cmd = parts[0];
  let final_args = &parts[1..];

  use std::os::windows::process::CommandExt;
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
      IS_SPAWNING.store(false, Ordering::SeqCst);
      return false;
    }
  }

  // Wait for window to appear
  let mut found = false;
  for _ in 0..40 {
    // Wait up to 8s (200ms * 40)
    tokio::time::sleep(Duration::from_millis(200)).await;
    let send_hwnd = {
      if let Some(hwnd) = find_window_by_process(&app_cfg.window_class) {
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
      tokio::time::sleep(Duration::from_millis(100)).await;

      // AUTOMATIC GRAB: Park newly discovered window immediately
      crate::windows::window::park_window(sh, config, app_cfg).await;

      break;
    }
  }

  IS_SPAWNING.store(false, Ordering::SeqCst);
  found
}
