use rustc_hash::{FxHashMap, FxHashSet};
use std::{
  fmt::Write,
  fs,
  path::Path,
  process::{id, Command, Stdio},
  sync::{Mutex, OnceLock},
  time::Duration,
};

use tokio::time::sleep;
use zbus::Connection;

use crate::config::{fuzzy_match_window, AppConfig, Config, FoundWindow};

static SPAWNING_APPS: OnceLock<Mutex<FxHashSet<String>>> = OnceLock::new();

fn get_spawning_apps() -> &'static Mutex<FxHashSet<String>> {
  SPAWNING_APPS.get_or_init(|| Mutex::new(FxHashSet::default()))
}

pub async fn ensure_terminal_running(
  app_cfg: &AppConfig,
  config: &Config,
  conn: &Connection,
) -> bool {
  let window_class = &app_cfg.window_class;
  let start_command = &app_cfg.start_command;

  // 1. Check if window already exists
  if check_window_exists(window_class).is_some() {
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
    if check_window_exists(window_class).is_some() {
      return false;
    }
  }

  // Ensure we remove the app from the spawning set even on error/panic
  struct SpawnGuard(String);
  impl Drop for SpawnGuard {
    fn drop(&mut self) {
      let mut spawning = get_spawning_apps().lock().unwrap();
      spawning.remove(&self.0);
    }
  }
  let _guard = SpawnGuard(window_class.to_string());

  // 3. Check if process is already running
  let process_running = check_process_running(window_class);

  if start_command.is_empty() {
    eprintln!(
      "janq: No start_command for app with class '{}'",
      window_class
    );
    return false;
  }

  // If process is running but no window, we still want to try starting it
  // (e.g. for terminals that open new windows on command even if daemon is running)
  if process_running {
    println!(
      "janq: Process for '{}' exists but no window found. Attempting to start/reanimate...",
      window_class
    );
  }

  let full_cmd = start_command.clone();

  println!("Starting terminal: {}", full_cmd);

  // Use sh -c
  match Command::new("sh")
    .arg("-c")
    .arg(&full_cmd)
    .stdin(Stdio::null())
    .stdout(Stdio::null())
    .stderr(Stdio::null())
    .spawn()
  {
    Ok(_) => {}
    Err(e) => {
      println!("Failed to start managed app: {}", e);
      return false;
    }
  }

  // Wait for window to appear (more reliable than just process)
  for i in 0..20 {
    if let Some(_id) = check_window_exists(window_class) {
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
      "janq: Process for '{}' is running, but no window appeared after 8 seconds. This might be a configuration issue.",
      window_class
    );
    return true;
  }

  println!(
    "janq: Failed to detect process or window for '{}' after spawning.",
    window_class
  );
  false
}

pub fn check_window_exists(target_class: &str) -> Option<String> {
  check_window_exists_with_candidates(target_class, None)
}

pub fn check_window_exists_with_candidates(
  target_class: &str,
  candidates: Option<&[FoundWindow]>,
) -> Option<String> {
  // 1. If candidates are provided (batch search), use them immediately
  if let Some(list) = candidates {
    return fuzzy_match_window(target_class, list, &[]).map(|w| w.id);
  }

  // 2. Hot path: Check cache and verify liveness via /proc
  let mut cache = get_pid_cache().lock().unwrap();
  if let Some(cached) = cache.get(target_class) {
    if Path::new(&format!("/proc/{}", cached.pid)).exists() {
      // Light verification: window still belongs to the same class (cached ID check)
      // This is still fairly fast compared to a full scan.
      return Some(cached.id.clone());
    }
  }

  // 3. Fallback: Full system fetch and fuzzy match
  let all_windows = fetch_system_windows();
  let managed_ids: Vec<String> = cache.values().map(|c| c.id.clone()).collect();

  if let Some(best) = fuzzy_match_window(target_class, &all_windows, &managed_ids) {
    // Update cache
    cache.insert(
      target_class.to_string(),
      CachedWindow {
        id: best.id.clone(),
        pid: best.pid,
      },
    );
    return Some(best.id);
  }

  None
}

pub fn fetch_system_windows() -> Vec<FoundWindow> {
  let mut windows = Vec::new();

  // 1. Get all IDs
  let all_ids = run_kdotool_cmd(&["search", "--class", ""]);
  if all_ids.is_empty() {
    return windows;
  }

  // 2. Get visible IDs for visibility boost
  let visible_ids: FxHashSet<String> = run_kdotool_cmd(&["search", "--onlyvisible", "--class", ""])
    .into_iter()
    .collect();

  for id in all_ids {
    let id = id.trim();
    if id.is_empty() {
      continue;
    }

    // Optimization: Skip obviously non-app windows if possible, but for now we follow the old logic
    // but with real visibility.
    let class = Command::new("kdotool")
      .args(["getwindowclassname", id])
      .output()
      .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_lowercase())
      .unwrap_or_default();

    if class.is_empty() || class == "plasmashell" || class == "kwin_x11" || class == "kwin_wayland"
    {
      continue;
    }

    let pid = Command::new("kdotool")
      .args(["getwindowpid", id])
      .output()
      .map(|o| {
        String::from_utf8_lossy(&o.stdout)
          .trim()
          .parse::<u32>()
          .unwrap_or(0)
      })
      .unwrap_or(0);

    let mut proc_name = String::new();
    if pid > 0 {
      if let Ok(cmdline) = fs::read(format!("/proc/{}/cmdline", pid)) {
        if let Some(part) = cmdline.split(|&b| b == 0).next() {
          proc_name = String::from_utf8_lossy(part)
            .split('/')
            .last()
            .unwrap_or_default()
            .to_lowercase();
        }
      }
    }

    windows.push(FoundWindow {
      id: id.to_string(),
      class_name: class,
      proc_name,
      pid,
      is_visible: visible_ids.contains(id),
    });
  }
  windows
}

fn run_kdotool_cmd(args: &[&str]) -> Vec<String> {
  let output = Command::new("kdotool").args(args).output();
  if let Ok(out) = output {
    if out.status.success() {
      return String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    }
  }
  Vec::new()
}

struct CachedWindow {
  id: String,
  pid: u32,
}

static PID_CACHE: OnceLock<Mutex<FxHashMap<String, CachedWindow>>> = OnceLock::new();

fn get_pid_cache() -> &'static Mutex<FxHashMap<String, CachedWindow>> {
  PID_CACHE.get_or_init(|| Mutex::new(FxHashMap::default()))
}

pub fn invalidate_search_cache() {
  if let Some(cache) = PID_CACHE.get() {
    let mut lock = cache.lock().unwrap();
    lock.clear();
  }
}

pub fn get_pid_for_class(target_class: &str) -> Option<u32> {
  let cache = get_pid_cache().lock().unwrap();
  cache.get(target_class).map(|c| c.pid)
}

pub fn check_process_running(target_class: &str) -> bool {
  let mut cache = get_pid_cache().lock().unwrap();

  // 1. Fast path: Check cached PID for this specific class
  if let Some(cached) = cache.get(target_class) {
    // Fast liveness check: just check if the directory exists
    // This is much faster than reading cmdline every time.
    if Path::new(&format!("/proc/{}", cached.pid)).exists() {
      return true;
    }
  }
  // If we're here, cache was invalid or empty for this class
  cache.remove(target_class);

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
          cache.insert(
            target_class.to_string(),
            CachedWindow {
              id: String::new(),
              pid,
            },
          );
          return true;
        }
      }
    }
  }
  false
}

fn verify_pid_matches(pid: u32, target_class: &str) -> bool {
  // Pre-compute lowercase target once
  let target_lower = target_class.to_lowercase();
  let target_dash_prefix = format!("{}-", target_lower);
  let target_dash_suffix = format!("-{}", target_lower);

  let mut path_buf = String::with_capacity(32);
  let _ = write!(path_buf, "/proc/{}/cmdline", pid);

  if let Ok(cmdline) = fs::read(&path_buf) {
    // Split by null byte
    let parts: Vec<&[u8]> = cmdline.split(|&b| b == 0).collect();

    for (i, part) in parts.iter().enumerate() {
      let s = String::from_utf8_lossy(part);
      // Match exact --class arg
      if s == "--class" && i + 1 < parts.len() {
        let next = String::from_utf8_lossy(parts[i + 1]);
        if next.eq_ignore_ascii_case(target_class) {
          return true;
        }
      }
      // Match --class=foo
      if s.to_lowercase().starts_with("--class=") && s[8..].eq_ignore_ascii_case(target_class) {
        return true;
      }
    }

    // Fallback logic
    let full_cmd_binding = cmdline
      .iter()
      .map(|&b| if b == 0 { 32 } else { b })
      .collect::<Vec<u8>>();
    let full_cmd = String::from_utf8_lossy(&full_cmd_binding);

    if full_cmd.to_lowercase().contains(&target_lower) {
      // Check exe
      path_buf.clear();
      let _ = write!(path_buf, "/proc/{}/exe", pid);
      if let Ok(exe) = fs::read_link(&path_buf) {
        let exe_str = exe.to_string_lossy().to_lowercase();

        // 1. General check: match filename against target class (Prefix/Suffix/Exact)
        let exe_name = Path::new(&exe_str)
          .file_name()
          .and_then(|n| n.to_str())
          .unwrap_or(&exe_str)
          .to_lowercase();

        if exe_name == target_lower
          || exe_name.starts_with(&target_dash_prefix)
          || exe_name.starts_with(&target_lower)
          || exe_name.ends_with(&target_dash_suffix)
        {
          return true;
        }
        // 2. Flatpak/Wrapper check
        if (exe_str.contains("flatpak") || exe_str.contains("bwrap") || exe_str.contains("snap"))
          && !exe_str.contains("steam")
        {
          return true;
        }
      }
    }
  }
  false
}
