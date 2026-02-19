//! Terminal/process lifecycle management for Linux.
//!
//! ## Responsibilities
//!
//! 1. **Window Discovery** - Finds windows matching a `window_class` pattern
//! 2. **Process Spawning** - Starts applications via their `start_command`
//! 3. **Spawn Idempotency** - Prevents duplicate spawns during rapid toggles
//! 4. **PID Caching** - Tracks process IDs for fast liveness checks
//!
//! ## Window Discovery Flow
//!
//! 1. Check in-memory PID cache for known window
//! 2. If cache miss or stale, trigger KWin script to enumerate all windows
//! 3. KWin script reports back via D-Bus `ReportWindowMetadata` callback
//! 4. Parse response and fuzzy-match against target `window_class`
//!
//! ## Spawn Idempotency
//!
//! Uses a static `HashSet` lock to prevent multiple concurrent spawns
//! of the same app. The `SpawnGuard` RAII wrapper ensures cleanup on
//! success, error, or panic.

use rustc_hash::FxHashMap;
use std::{
  fs,
  process::{id, Stdio},
  sync::{
    atomic::{AtomicU64, Ordering},
    Mutex, OnceLock,
  },
  time::Duration,
};

use tokio::sync::oneshot;
use tokio::time::sleep;
use zbus::Connection;

use crate::linux::cache::{get_cache, get_cached_window, remove_from_cache, update_cache};
use crate::linux::kwin::{ensure_grabbed, restore_app, trigger_fetch_windows};
use janq::config::{AppConfig, Config, FoundWindow};
use janq::error::show_error;
use janq::matching::fuzzy_match_window;
use janq::process;
use janq::spawn_guard::{get_spawning_apps, SpawnGuard};

// =============================================================================
// Batch Metadata Fetcher (D-Bus callback infrastructure)
// =============================================================================

/// Batch response from KWin window enumeration script.
///
/// The `raw` field contains semicolon-separated window entries in format:
/// `id|class|pid|visible;id|class|pid|visible;...`
pub struct WindowMetadataBatch {
  pub raw: String,
}

static METADATA_WAITERS: OnceLock<Mutex<FxHashMap<u64, oneshot::Sender<WindowMetadataBatch>>>> =
  OnceLock::new();

fn get_metadata_waiters() -> &'static Mutex<FxHashMap<u64, oneshot::Sender<WindowMetadataBatch>>> {
  METADATA_WAITERS.get_or_init(|| Mutex::new(FxHashMap::default()))
}

pub async fn report_metadata(mut payload: String) {
  if let Some(pos) = payload.find(':') {
    let id_str = &payload[..pos];
    if let Ok(request_id) = id_str.parse::<u64>() {
      let mut waiters = get_metadata_waiters().lock().unwrap();
      if let Some(tx) = waiters.remove(&request_id) {
        // Use in-place drain to remove the "id:" prefix without a new allocation
        payload.drain(..pos + 1);
        let _ = tx.send(WindowMetadataBatch { raw: payload });
      }
    }
  }
}

fn get_proc_name_by_pid(pid: u32) -> Option<String> {
  if pid == 0 {
    return None;
  }
  let mut buf = [0u8; 64];
  let path = {
    use std::io::Write;
    let mut cursor = std::io::Cursor::new(&mut buf[..]);
    let _ = write!(cursor, "/proc/{}/cmdline", pid);
    let len = cursor.position() as usize;
    std::str::from_utf8(&buf[..len]).unwrap_or("")
  };

  if !path.is_empty() {
    if let Ok(cmdline) = fs::read(path) {
      if let Some(part) = cmdline.split(|&b| b == 0).next() {
        let s = String::from_utf8_lossy(part);
        if let Some(name) = s.split('/').next_back() {
          return Some(name.to_lowercase());
        }
      }
    }
  }
  None
}

// =============================================================================
// Terminal Management
// =============================================================================

/// Ensures the application's terminal/window is running.
///
/// Returns `true` if a new process was spawned, `false` if already running.
pub async fn ensure_terminal_running(
  app_name: &str,
  app_cfg: &AppConfig,
  config: &Config,
  conn: &Connection,
) -> bool {
  ensure_terminal_running_with_candidates(app_name, app_cfg, config, conn, None).await
}

pub async fn ensure_terminal_running_with_candidates(
  app_name: &str,
  app_cfg: &AppConfig,
  config: &Config,
  conn: &Connection,
  candidates: Option<&[FoundWindow]>,
) -> bool {
  let window_class = &app_cfg.window_class;
  let start_command = &app_cfg.start_command;

  // 1. Check if window already exists
  if check_window_exists_with_candidates_and_managed(app_name, window_class, conn, candidates)
    .await
    .is_some()
  {
    return false;
  }

  // 2. Idempotency Lock
  loop {
    let already_spawning = {
      let spawning = get_spawning_apps().lock().unwrap();
      spawning.contains(window_class)
    };

    if !already_spawning {
      // Try to acquire the "lock" by adding to the set
      let mut spawning = get_spawning_apps().lock().unwrap();
      spawning.insert(window_class.to_string());
      break;
    }

    // Wait and re-check if the window exists (the other track might have finished)
    sleep(Duration::from_millis(200)).await;
    if check_window_exists(app_name, window_class, conn)
      .await
      .is_some()
    {
      return false;
    }
  }

  let _guard = SpawnGuard::new(window_class);

  // 3. Check if process is already running
  let process_running = check_process_running(app_name, window_class);

  if start_command.is_empty() {
    show_error(&format!(
      "janq: No start_command for app with class '{}'",
      window_class
    ));
    return false;
  }

  // 3. Process is running but no window found: Prioritize Release (un-quake)
  if process_running {
    let _ = restore_app(app_name, window_class, conn).await;
    sleep(Duration::from_millis(400)).await;

    // If release uncovered an existing window, reuse it immediately
    if let Some(id) =
      check_window_exists_with_candidates_and_managed(app_name, window_class, conn, None).await
    {
      println!(
        "janq: Recovering window {} for '{}' after release.",
        id, window_class
      );
      let _ = ensure_grabbed(app_cfg, config, conn).await;
      return true;
    }
  }

  let full_cmd = start_command.clone();

  println!(
    "janq: starting app with class '{}' (cmd: {})...",
    window_class, full_cmd
  );

  // Use tokio::process::Command to avoid blocking threads for reaping
  match tokio::process::Command::new("sh")
    .arg("-c")
    .arg(&full_cmd)
    .stdin(Stdio::null())
    .stdout(Stdio::null())
    .stderr(Stdio::null())
    .spawn()
  {
    Ok(mut child) => {
      tokio::spawn(async move {
        let _ = child.wait().await;
      });
    }
    Err(e) => {
      show_error(&format!("Failed to start managed app: {}", e));
      return false;
    }
  }

  // Wait for window to appear (more reliable than just process)
  for i in 0..20 {
    if let Some(_id) =
      check_window_exists_with_candidates_and_managed(app_name, window_class, conn, None).await
    {
      // Give it a moment to finalize
      tokio::time::sleep(Duration::from_millis(500)).await;
      // Call ensure_grabbed (async)
      let _ = ensure_grabbed(app_cfg, config, conn).await;
      return true;
    }
    if i % 5 == 0 && i > 0 {
      println!(
        "janq: Still waiting for window '{}' to appear (attempt {}/20)...",
        window_class, i
      );
    }
    tokio::time::sleep(Duration::from_millis(400)).await;
  }

  // Fallback: check if process is at least running
  if check_process_running(app_name, window_class) {
    println!(
      "janq: Process for '{}' is running, but no window appeared after 8 seconds. This might be a configuration issue. Retrying next toggle.",
      window_class
    );
    return false; // Return false so the next toggle can try to find/spawn it again properly
  }

  show_error(&format!(
    "janq: Failed to detect process or window for '{}' after spawning.\n\nPlease check if 'start_command' is correct and executable.",
    window_class
  ));
  false
}

// =============================================================================
// Window Discovery
// =============================================================================

pub async fn check_window_exists(
  app_name: &str,
  target_class: &str,
  conn: &Connection,
) -> Option<Box<str>> {
  check_window_exists_with_candidates(app_name, target_class, conn, None).await
}

pub async fn check_window_exists_with_candidates(
  app_name: &str,
  target_class: &str,
  conn: &Connection,
  candidates: Option<&[FoundWindow]>,
) -> Option<Box<str>> {
  check_window_exists_with_candidates_and_managed(app_name, target_class, conn, candidates).await
}

pub async fn check_window_exists_with_candidates_and_managed(
  app_name: &str,
  target_class: &str,
  conn: &Connection,
  candidates: Option<&[FoundWindow]>,
) -> Option<Box<str>> {
  // 1. If candidates are provided (batch search), use them immediately
  if let Some(list) = candidates {
    return fuzzy_match_window(target_class, list, Some(app_name)).map(|w| w.id.clone());
  }

  // 2. Hot path: Check cache and verify liveness
  {
    if let Some(cached) = get_cached_window(app_name) {
      if !cached.id.is_empty() && process::is_process_running(cached.pid, None) {
        // Process is alive, trust the cached window ID
        return Some(cached.id.clone());
      }
    }
  }

  // Dead cache entry will be cleaned up in the fallback path below

  // 3. Fallback: Full system fetch and fuzzy match
  let all_windows = fetch_system_windows_async(conn).await;

  if let Some(best) = fuzzy_match_window(target_class, &all_windows, Some(app_name)) {
    // Update cache
    update_cache(
      app_name,
      best.id.clone(),
      best.pid,
      best.proc_lowercase.clone(),
    );
    return Some(best.id.clone());
  }

  None
}

pub async fn fetch_system_windows(conn: &Connection) -> Vec<FoundWindow> {
  fetch_system_windows_async(conn).await
}

static REQUEST_ID_COUNTER: AtomicU64 = AtomicU64::new(0);

pub async fn fetch_system_windows_async(conn: &Connection) -> Vec<FoundWindow> {
  let mut windows = Vec::new();
  let request_id = REQUEST_ID_COUNTER.fetch_add(1, Ordering::SeqCst);
  let (tx, rx) = oneshot::channel();
  {
    let mut waiters = get_metadata_waiters().lock().unwrap();
    waiters.insert(request_id, tx);
  }

  // 2. Trigger KWin script (Use provided connection)
  if let Err(e) = trigger_fetch_windows(conn, request_id).await {
    show_error(&format!(
      "janq: Failed to trigger window fetch script: {}",
      e
    ));
    let mut waiters = get_metadata_waiters().lock().unwrap();
    waiters.remove(&request_id);
    return windows;
  }

  let batch = match tokio::time::timeout(Duration::from_millis(2000), rx).await {
    Ok(Ok(b)) => b,
    _ => {
      show_error(&format!(
        "janq: Timeout waiting for window metadata ID {} from KWin.",
        request_id
      ));
      let mut waiters = get_metadata_waiters().lock().unwrap();
      waiters.remove(&request_id);
      return windows;
    }
  };

  use std::sync::Arc;

  // 4. Transform into FoundWindow objects
  // Build a fast lookup map for managed windows from the cache.
  // We use owned strings for the keys since the cache lock is released immediately.
  let managed_lookup: FxHashMap<Box<str>, (Arc<str>, Box<str>)> = {
    let cache = get_cache().lock().unwrap();
    cache
      .iter()
      .map(|(name, window)| {
        (
          window.id.clone(),
          (Arc::clone(name), window.proc_lowercase.clone()),
        )
      })
      .collect()
  };

  for line in batch.raw.split(';') {
    if line.is_empty() {
      continue;
    }
    let mut parts = line.split('|');
    let id = match parts.next() {
      Some(s) => s,
      None => continue,
    };
    let class = match parts.next() {
      Some(s) => s,
      None => continue,
    };
    let pid_str = parts.next().unwrap_or("0");
    let pid = pid_str.parse::<u32>().unwrap_or(0);
    let is_visible = parts.next() == Some("1");

    if class.is_empty() || class == "plasmashell" || class == "kwin_wayland" {
      continue;
    }

    let mut managed_by = None;
    let mut proc_lowercase: Box<str> = "".into();

    if let Some((app_name, cached_proc)) = managed_lookup.get(id) {
      managed_by = Some(Arc::clone(app_name));
      proc_lowercase = cached_proc.clone();
    } else if pid > 0 {
      // Optimization: Only read /proc for windows with a valid PID and not in cache.
      if let Some(name) = get_proc_name_by_pid(pid) {
        proc_lowercase = name.into();
      }
    }

    // Optimization: Avoid to_lowercase() and double-allocation if the class is already lowercase.
    let class_lowercase: Box<str> = if class.as_bytes().iter().any(|b| b.is_ascii_uppercase()) {
      class.to_lowercase().into()
    } else {
      class.into()
    };

    windows.push(FoundWindow {
      id: id.into(),
      class_lowercase,
      proc_lowercase,
      pid,
      is_visible,
      is_managed: managed_by.is_some(),
      managed_by,
    });
  }

  windows
}

pub async fn is_window_valid(app_name: &str, id: &str, conn: &Connection) -> bool {
  if id.is_empty() {
    return false;
  }

  // 1. Fast path: Check cache
  if let Some(cached) = get_cached_window(app_name) {
    if &*cached.id == id && process::is_process_running(cached.pid, None) {
      return true;
    }
  }

  // 2. Fallback: full system fetch
  let windows = fetch_system_windows_async(conn).await;
  windows.iter().any(|w| &*w.id == id)
}

pub fn get_pid_for_app(app_name: &str) -> Option<u32> {
  get_cached_window(app_name).map(|c| c.pid)
}

pub fn check_process_running(app_name: &str, target_class: &str) -> bool {
  // 1. Fast path: Check cached PID for this specific app
  if let Some(cached) = get_cached_window(app_name) {
    if process::is_process_running(cached.pid, None) {
      return true;
    }
  }
  // If we're here, cache was invalid or empty for this app
  remove_from_cache(app_name);

  // 2. Slow path: Iterate /proc
  let procs = match fs::read_dir("/proc") {
    Ok(p) => p,
    Err(_) => return false,
  };

  let my_pid = id();

  for entry in procs.flatten() {
    if let Ok(name) = entry.file_name().into_string() {
      if let Ok(pid) = name.parse::<u32>() {
        if pid == my_pid {
          continue;
        }

        if verify_pid_matches(pid, target_class) {
          // Identity Guard: Only claim this PID if it's not already owned by another app.
          // This prevents apps from "stealing" each other's processes if they share a class.
          let already_owned = {
            let cache = get_cache().lock().unwrap();
            cache
              .iter()
              .any(|(name, c)| c.pid == pid && &**name != app_name)
          };

          if !already_owned {
            let proc_name = get_proc_name_by_pid(pid).unwrap_or_default();
            update_cache(app_name, String::new().into(), pid, proc_name.into());
            return true;
          }
        }
      }
    }
  }
  false
}

fn verify_pid_matches(pid: u32, target_class: &str) -> bool {
  use std::io::Write;
  let mut buf = [0u8; 64];
  let path_len = {
    let mut cursor = std::io::Cursor::new(&mut buf[..]);
    let _ = write!(cursor, "/proc/{}/cmdline", pid);
    cursor.position() as usize
  };
  let path = match std::str::from_utf8(&buf[..path_len]) {
    Ok(p) => p,
    Err(_) => return false,
  };

  if let Ok(cmdline) = fs::read(path) {
    let mut iter = cmdline.split(|&b| b == 0);
    while let Some(part) = iter.next() {
      if let Ok(s) = std::str::from_utf8(part) {
        if s == "--class" {
          if let Some(next_part) = iter.next() {
            if let Ok(ns) = std::str::from_utf8(next_part) {
              if ns.eq_ignore_ascii_case(target_class) {
                return true;
              }
            }
          }
        } else if s.len() >= 8 && s[..8].eq_ignore_ascii_case("--class=") {
          if s[8..].eq_ignore_ascii_case(target_class) {
            return true;
          }
        }
      }
    }

    // Binary name match: Strict or known variations (like wezterm-gui)
    let mut exe_buf = [0u8; 64];
    let exe_len = {
      let mut exe_cursor = std::io::Cursor::new(&mut exe_buf[..]);
      let _ = write!(exe_cursor, "/proc/{}/exe", pid);
      exe_cursor.position() as usize
    };
    if let Ok(exe_path) = std::str::from_utf8(&exe_buf[..exe_len]) {
      if let Ok(exe) = fs::read_link(exe_path) {
        if let Some(name) = exe.file_name().and_then(|n| n.to_str()) {
          if name.eq_ignore_ascii_case(target_class)
            || (target_class.eq_ignore_ascii_case("wezterm")
              && name.eq_ignore_ascii_case("wezterm-gui"))
          {
            return true;
          }
        }
      }
    }
  }
  false
}
