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
  fmt::Write,
  fs,
  path::Path,
  process::{id, Stdio},
  sync::{Mutex, OnceLock},
  time::Duration,
};

use tokio::sync::oneshot;
use tokio::time::sleep;
use zbus::Connection;

use crate::linux::cache::{get_cache, get_cached_window, remove_from_cache, update_cache};
use janq::config::{fuzzy_match_window, AppConfig, Config, FoundWindow};
use janq::error::show_error;
use janq::spawn_guard::{get_spawning_apps, SpawnGuard};

// =============================================================================
// D-Bus Connection Cache (shared for window discovery)
// =============================================================================

/// Cached D-Bus connection for window discovery operations.
/// Reusing the connection avoids repeated handshake overhead.
static DISCOVERY_CONN: OnceLock<Connection> = OnceLock::new();

async fn get_discovery_conn() -> Option<Connection> {
  if let Some(conn) = DISCOVERY_CONN.get() {
    return Some(conn.clone());
  }
  if let Ok(conn) = Connection::session().await {
    let _ = DISCOVERY_CONN.set(conn.clone());
    return Some(conn);
  }
  None
}

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

pub async fn report_metadata(payload: String) {
  if let Some((id_str, raw)) = payload.split_once(':') {
    if let Ok(request_id) = id_str.parse::<u64>() {
      let mut waiters = get_metadata_waiters().lock().unwrap();
      if let Some(tx) = waiters.remove(&request_id) {
        let _ = tx.send(WindowMetadataBatch {
          raw: raw.to_string(),
        });
      }
    }
  }
}

// =============================================================================
// Terminal Management
// =============================================================================

/// Ensures the application's terminal/window is running.
///
/// Returns `true` if a new process was spawned, `false` if already running.
pub async fn ensure_terminal_running(
  app_cfg: &AppConfig,
  config: &Config,
  conn: &Connection,
) -> bool {
  ensure_terminal_running_with_candidates(app_cfg, config, conn, None).await
}

pub async fn ensure_terminal_running_with_candidates(
  app_cfg: &AppConfig,
  config: &Config,
  conn: &Connection,
  candidates: Option<&[FoundWindow]>,
) -> bool {
  let window_class = &app_cfg.window_class;
  let start_command = &app_cfg.start_command;

  // 1. Check if window already exists
  if check_window_exists_with_candidates(window_class, candidates)
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
    if check_window_exists(window_class).await.is_some() {
      return false;
    }
  }

  let _guard = SpawnGuard::new(window_class);

  // 3. Check if process is already running
  let process_running = check_process_running(window_class);

  if start_command.is_empty() {
    show_error(&format!(
      "janq: No start_command for app with class '{}'",
      window_class
    ));
    return false;
  }

  // 3. Process is running but no window found: Prioritize Release (un-quake)
  if process_running {
    let _ = crate::linux::kwin::restore_app("", window_class, conn).await;
    sleep(Duration::from_millis(400)).await;

    // If release uncovered an existing window, reuse it immediately
    if let Some(id) = check_window_exists(window_class).await {
      println!(
        "janq: Recovering window {} for '{}' after release.",
        id, window_class
      );
      let _ = crate::linux::kwin::ensure_grabbed(app_cfg, config, conn).await;
      return true;
    }
  }

  let full_cmd = start_command.clone();

  println!("Starting terminal: {}", full_cmd);

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
    if let Some(_id) = check_window_exists(window_class).await {
      // Give it a moment to finalize
      tokio::time::sleep(Duration::from_millis(500)).await;
      // Call ensure_grabbed (async)
      let _ = crate::linux::kwin::ensure_grabbed(app_cfg, config, conn).await;
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
  if check_process_running(window_class) {
    println!(
      "janq: Process for '{}' is running, but no window appeared after 8 seconds. This might be a configuration issue. Retrying next toggle.",
      window_class
    );
    return false; // Return false so the next toggle can try to find/spawn it again properly
  }

  eprintln!(
    "janq: Failed to detect process or window for '{}' after spawning.",
    window_class
  );
  false
}

// =============================================================================
// Window Discovery
// =============================================================================

pub async fn check_window_exists(target_class: &str) -> Option<String> {
  check_window_exists_with_candidates(target_class, None).await
}

pub async fn check_window_exists_with_candidates(
  target_class: &str,
  candidates: Option<&[FoundWindow]>,
) -> Option<String> {
  // 1. If candidates are provided (batch search), use them immediately
  if let Some(list) = candidates {
    return fuzzy_match_window(target_class, list, &[]).map(|w| w.id);
  }

  // 2. Hot path: Check cache and verify liveness via /proc (no expensive script call)
  {
    if let Some(cached) = get_cached_window(target_class) {
      if !cached.id.is_empty() && Path::new(&format!("/proc/{}", cached.pid)).exists() {
        // Process is alive, trust the cached window ID
        return Some(cached.id.clone());
      }
    }
  }

  // Dead cache entry will be cleaned up in the fallback path below

  // 3. Fallback: Full system fetch and fuzzy match
  let all_windows = fetch_system_windows_async().await;
  let managed_ids: Vec<String> = {
    let cache = get_cache().lock().unwrap();
    cache.values().map(|c| c.id.clone()).collect()
  };

  if let Some(best) = fuzzy_match_window(target_class, &all_windows, &managed_ids) {
    // Update cache
    update_cache(target_class, best.id.clone(), best.pid);
    return Some(best.id);
  }

  None
}

pub async fn fetch_system_windows() -> Vec<FoundWindow> {
  fetch_system_windows_async().await
}

pub async fn fetch_system_windows_async() -> Vec<FoundWindow> {
  let mut windows = Vec::new();

  // 1. Setup waiter with unique ID (timestamp based)
  let request_id = std::time::SystemTime::now()
    .duration_since(std::time::UNIX_EPOCH)
    .unwrap_or_default()
    .as_millis() as u64;
  let (tx, rx) = oneshot::channel();
  {
    let mut waiters = get_metadata_waiters().lock().unwrap();
    waiters.insert(request_id, tx);
  }

  // 2. Trigger KWin script (Reuse shared connection)
  let conn = match get_discovery_conn().await {
    Some(c) => c,
    None => return windows,
  };

  if let Err(e) = crate::linux::kwin::trigger_fetch_windows(&conn, request_id).await {
    eprintln!("janq: Failed to trigger window fetch script: {}", e);
    let mut waiters = get_metadata_waiters().lock().unwrap();
    waiters.remove(&request_id);
    return windows;
  }

  let batch = match tokio::time::timeout(Duration::from_millis(2000), rx).await {
    Ok(Ok(b)) => b,
    _ => {
      eprintln!(
        "janq: Timeout waiting for window metadata ID {} from KWin.",
        request_id
      );
      let mut waiters = get_metadata_waiters().lock().unwrap();
      waiters.remove(&request_id);
      return windows;
    }
  };

  // 4. Transform into FoundWindow objects
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
      Some(s) => s.to_lowercase(),
      None => continue,
    };
    let pid_str = parts.next().unwrap_or("0");
    let pid = pid_str.parse::<u32>().unwrap_or(0);
    let is_visible = parts.next() == Some("1");

    if class.is_empty() || class == "plasmashell" || class == "kwin_x11" || class == "kwin_wayland"
    {
      continue;
    }

    let mut proc_lowercase = String::new();
    if pid > 0 {
      if let Ok(cmdline) = fs::read(format!("/proc/{}/cmdline", pid)) {
        if let Some(part) = cmdline.split(|&b| b == 0).next() {
          proc_lowercase = String::from_utf8_lossy(part)
            .split('/')
            .last()
            .unwrap_or_default()
            .to_lowercase();
        }
      }
    }

    windows.push(FoundWindow {
      id: id.to_string(),
      class_lowercase: class,
      proc_lowercase,
      pid,
      is_visible,
    });
  }

  windows
}

pub async fn is_window_valid(window_class: &str, id: &str) -> bool {
  if id.is_empty() {
    return false;
  }

  // 1. Fast path: Check cache
  if let Some(cached) = get_cached_window(window_class) {
    if cached.id == id && Path::new(&format!("/proc/{}", cached.pid)).exists() {
      return true;
    }
  }

  // 2. Fallback: full system fetch
  let windows = fetch_system_windows_async().await;
  windows.iter().any(|w| w.id == id)
}

pub fn get_pid_for_class(target_class: &str) -> Option<u32> {
  get_cached_window(target_class).map(|c| c.pid)
}

pub fn check_process_running(target_class: &str) -> bool {
  // 1. Fast path: Check cached PID for this specific class
  if let Some(cached) = get_cached_window(target_class) {
    // Fast liveness check: just check if the directory exists
    // This is much faster than reading cmdline every time.
    if Path::new(&format!("/proc/{}", cached.pid)).exists() {
      return true;
    }
  }
  // If we're here, cache was invalid or empty for this class
  remove_from_cache(target_class);

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
          update_cache(target_class, String::new(), pid);
          return true;
        }
      }
    }
  }
  false
}

fn verify_pid_matches(pid: u32, target_class: &str) -> bool {
  let target_lower = target_class.to_lowercase();
  let mut path_buf = String::with_capacity(32);
  let _ = write!(path_buf, "/proc/{}/cmdline", pid);

  if let Ok(cmdline) = fs::read(&path_buf) {
    let parts: Vec<&[u8]> = cmdline.split(|&b| b == 0).collect();
    for (i, part) in parts.iter().enumerate() {
      let s = String::from_utf8_lossy(part);
      if s == "--class" && i + 1 < parts.len() {
        if String::from_utf8_lossy(parts[i + 1]).eq_ignore_ascii_case(target_class) {
          return true;
        }
      }
      if s.to_lowercase().starts_with("--class=") && s[8..].eq_ignore_ascii_case(target_class) {
        return true;
      }
    }

    // Binary name match: Strict or known variations (like wezterm-gui)
    path_buf.clear();
    let _ = write!(path_buf, "/proc/{}/exe", pid);
    if let Ok(exe) = fs::read_link(&path_buf) {
      if let Some(name) = exe.file_name().and_then(|n| n.to_str()) {
        let n_lower = name.to_lowercase();
        if n_lower == target_lower || (target_lower == "wezterm" && n_lower == "wezterm-gui") {
          return true;
        }
      }
    }
  }
  false
}
