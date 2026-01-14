use rustc_hash::FxHashMap;
use std::{env::temp_dir, fs, path::Path, process::Command, sync::OnceLock};

use tokio::{
  sync::Mutex,
  time::{sleep, Duration},
};
use zbus::{Connection, Proxy, Result};

use crate::config::{AppConfig, Config};
use crate::linux::terminal::{
  check_window_exists, check_window_exists_with_candidates, fetch_system_windows,
  get_pid_for_class, is_window_valid,
};

// =============================================================================
// KWin Script Runner
// =============================================================================

/// Helper to run a KWin script with common boilerplate:
/// unload old script, write to temp file, load, run, and optionally cleanup.
async fn run_kwin_script(
  conn: &Connection,
  script_name: &str,
  script_content: &str,
  delay_before_unload: Option<Duration>,
) -> Result<()> {
  let scripting_proxy =
    Proxy::new(conn, "org.kde.KWin", "/Scripting", "org.kde.kwin.Scripting").await?;
  let _ = scripting_proxy
    .call_method("unloadScript", &(script_name))
    .await;

  let tmp_path = temp_dir().join(format!("{}.js", script_name));
  fs::write(&tmp_path, script_content)
    .map_err(|e| zbus::Error::Failure(format!("Failed to write script: {}", e)))?;

  let tmp_path_str = tmp_path.to_string_lossy().to_string();
  let reply = scripting_proxy
    .call_method("loadScript", &(tmp_path_str, script_name))
    .await?;
  let script_id: i32 = reply.body().deserialize()?;

  if script_id >= 0 {
    let script_obj_path = format!("/Scripting/Script{}", script_id);
    let script_proxy =
      Proxy::new(conn, "org.kde.KWin", script_obj_path, "org.kde.kwin.Script").await?;
    script_proxy.call_method("run", &()).await?;

    if let Some(delay) = delay_before_unload {
      sleep(delay).await;
      let _ = scripting_proxy
        .call_method("unloadScript", &(script_name))
        .await;
    }
    let _ = fs::remove_file(tmp_path);
  }
  Ok(())
}

// =============================================================================
// Global State
// =============================================================================

struct KWinState {
  visible_app: Option<String>,
  previous_window_id: String, // Last window active before ANY quake window was shown
  max_refresh_rate: f64,
}

static STATE: Mutex<KWinState> = Mutex::const_new(KWinState {
  visible_app: None,
  previous_window_id: String::new(),
  max_refresh_rate: 60.0,
});

fn get_max_refresh_rate() -> f64 {
  let output = Command::new("kscreen-doctor")
    .arg("-o")
    .output()
    .ok()
    .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
    .unwrap_or_default();

  let max_hz = output
    .lines()
    .flat_map(|l| {
      l.split_whitespace()
        .filter(|w| w.contains('*'))
        .filter_map(|w| {
          w.split('@')
            .last()?
            .chars()
            .take_while(|c| c.is_digit(10) || *c == '.')
            .collect::<String>()
            .parse::<f64>()
            .ok()
        })
    })
    .fold(0.0, f64::max);

  let final_hz = if max_hz > 0.0 { max_hz.round() } else { 60.0 };
  println!("janq: Detected highest refresh rate: {}Hz", final_hz);
  final_hz
}

pub async fn init() {
  let hz = get_max_refresh_rate();
  let mut state = STATE.lock().await;
  state.max_refresh_rate = hz;
}

static WINDOW_CACHE: OnceLock<Mutex<FxHashMap<String, (String, u32)>>> = OnceLock::new();

fn get_window_cache() -> &'static Mutex<FxHashMap<String, (String, u32)>> {
  WINDOW_CACHE.get_or_init(|| Mutex::new(FxHashMap::default()))
}

/// Parameters for toggle script execution
struct ToggleParams<'a> {
  visible: bool,
  prev_id: &'a str,
  target_id: &'a str,
  target_pid: u32,
  janq_classes: &'a str,
}

// =============================================================================
// KWin Script Templates
// =============================================================================

const COMMON_KWIN_JS: &str = include_str!("js/common.js");
const TOGGLE_SCRIPT_TEMPLATE: &str = include_str!("js/toggle_quake.js");

const ENSURE_GRABBED_BATCH_TEMPLATE: &str = include_str!("js/ensure_grabbed.js");

const RESTORE_TEMPLATE: &str = include_str!("js/restore.js");

const FETCH_WINDOWS_SCRIPT: &str = include_str!("js/fetch_windows.js");

const GET_ACTIVE_WINDOW_SCRIPT: &str = include_str!("js/get_active_window.js");

pub async fn trigger_fetch_windows(conn: &Connection, request_id: u64) -> Result<()> {
  let script_body_raw = FETCH_WINDOWS_SCRIPT.replace("/*{{COMMON_KWIN_JS}}*/", COMMON_KWIN_JS);
  let script_body_trimmed = script_body_raw.trim();
  let script_body = script_body_trimmed
    .strip_suffix(';')
    .unwrap_or(script_body_trimmed);
  let script_content = format!("{}(\"{}\");", script_body, request_id);

  let script_name = format!("janq_fetch_{}", request_id);
  run_kwin_script(
    conn,
    &script_name,
    &script_content,
    Some(Duration::from_millis(100)),
  )
  .await
}

// =============================================================================
// Active Window Fetcher (D-Bus callback infrastructure)
// =============================================================================

use std::sync::Mutex as StdMutex;
use tokio::sync::oneshot;

struct ActiveWindowInfo {
  id: String,
  class: String,
}

static ACTIVE_WINDOW_WAITERS: OnceLock<
  StdMutex<FxHashMap<u64, oneshot::Sender<ActiveWindowInfo>>>,
> = OnceLock::new();

fn get_active_window_waiters(
) -> &'static StdMutex<FxHashMap<u64, oneshot::Sender<ActiveWindowInfo>>> {
  ACTIVE_WINDOW_WAITERS.get_or_init(|| StdMutex::new(FxHashMap::default()))
}

pub async fn report_active_window(payload: String) {
  let parts: Vec<&str> = payload.splitn(3, ':').collect();
  if parts.len() < 3 {
    return;
  }
  if let Ok(request_id) = parts[0].parse::<u64>() {
    let info = ActiveWindowInfo {
      id: parts[1].to_string(),
      class: parts[2].to_string(),
    };
    let mut waiters = get_active_window_waiters().lock().unwrap();
    if let Some(tx) = waiters.remove(&request_id) {
      let _ = tx.send(info);
    }
  }
}

// Short-term cache for active window (debounces rapid toggles)
struct CachedActiveWindow {
  id: String,
  class: String,
  fetched_at: std::time::Instant,
}

static ACTIVE_WINDOW_CACHE: OnceLock<StdMutex<Option<CachedActiveWindow>>> = OnceLock::new();

fn get_active_window_cache() -> &'static StdMutex<Option<CachedActiveWindow>> {
  ACTIVE_WINDOW_CACHE.get_or_init(|| StdMutex::new(None))
}

async fn fetch_active_window_cached(conn: &Connection) -> Option<(String, String)> {
  // DEBOUNCE CONFIGURATION:
  // - ENABLE_ACTIVE_WINDOW_CACHE: Set to false to always fetch fresh (disables caching)
  // - CACHE_TTL_MS: How long (in ms) to reuse cached active window info
  //
  // Purpose: Prevents KWin script flooding during rapid toggling. When you spam
  // the hotkey, multiple show-toggles would each trigger a KWin script to get
  // the active window. With caching, subsequent calls within TTL reuse the result.
  //
  // Trade-off: Higher TTL = fewer scripts but potentially stale focus restoration
  // if user Alt+Tabs during the window. 100ms is fast enough for human perception.
  const ENABLE_ACTIVE_WINDOW_CACHE: bool = true;
  const CACHE_TTL_MS: u128 = 100;

  if !ENABLE_ACTIVE_WINDOW_CACHE {
    return fetch_active_window(conn).await;
  }

  // Check cache first
  {
    let cache = get_active_window_cache().lock().unwrap();
    if let Some(ref cached) = *cache {
      if cached.fetched_at.elapsed().as_millis() < CACHE_TTL_MS {
        return Some((cached.id.clone(), cached.class.clone()));
      }
    }
  }

  // Cache miss or stale, fetch fresh
  let result = fetch_active_window(conn).await;

  // Update cache
  if let Some((ref id, ref class)) = result {
    let mut cache = get_active_window_cache().lock().unwrap();
    *cache = Some(CachedActiveWindow {
      id: id.clone(),
      class: class.clone(),
      fetched_at: std::time::Instant::now(),
    });
  }

  result
}

async fn fetch_active_window(conn: &Connection) -> Option<(String, String)> {
  let request_id = std::time::SystemTime::now()
    .duration_since(std::time::UNIX_EPOCH)
    .unwrap_or_default()
    .as_nanos() as u64;

  let (tx, rx) = oneshot::channel();
  {
    let mut waiters = get_active_window_waiters().lock().unwrap();
    waiters.insert(request_id, tx);
  }

  let script_body_raw = GET_ACTIVE_WINDOW_SCRIPT.trim();
  let script_body = script_body_raw.strip_suffix(';').unwrap_or(script_body_raw);
  let script_content = format!("{}(\"{}\");", script_body, request_id);
  let script_name = format!("janq_active_{}", request_id);

  if run_kwin_script(
    conn,
    &script_name,
    &script_content,
    Some(Duration::from_millis(100)),
  )
  .await
  .is_err()
  {
    let mut waiters = get_active_window_waiters().lock().unwrap();
    waiters.remove(&request_id);
    return None;
  }

  match tokio::time::timeout(Duration::from_millis(500), rx).await {
    Ok(Ok(info)) if !info.id.is_empty() => Some((info.id, info.class)),
    _ => {
      let mut waiters = get_active_window_waiters().lock().unwrap();
      waiters.remove(&request_id);
      None
    }
  }
}

async fn update_focus_state(state: &mut KWinState, janq_classes: &[String], conn: &Connection) {
  let (current_id, class_name) = match fetch_active_window_cached(conn).await {
    Some(info) => info,
    None => return,
  };

  if current_id.is_empty() {
    return;
  }

  let class_lower = class_name.to_lowercase();
  for managed_class in janq_classes {
    if class_lower.contains(&managed_class.to_lowercase()) {
      return;
    }
  }
  state.previous_window_id = current_id;
}

async fn get_window_id_and_pid(app_name: &str, class: &str) -> Option<(String, u32)> {
  // 1. Check Cache
  {
    if let Ok(cache) = get_window_cache().try_lock() {
      if let Some((id, pid)) = cache.get(app_name) {
        // Verify PID liveness via /proc
        if Path::new(&format!("/proc/{}", pid)).exists() {
          return Some((id.clone(), *pid));
        }
      }
    }
  }

  // 2. Fallback to Search
  if let Some(id) = check_window_exists(class).await {
    let pid = get_pid_for_class(class).unwrap_or(0);
    // 3. Update Cache
    if let Ok(mut cache) = get_window_cache().try_lock() {
      cache.insert(app_name.to_string(), (id.clone(), pid));
    }
    return Some((id, pid));
  }
  None
}

// =============================================================================
// Toggle Logic
// =============================================================================

pub async fn toggle_quake(
  app_name: &str,
  config: &Config,
  conn: &Connection,
) -> anyhow::Result<()> {
  let mut state = STATE.lock().await;
  let app_cfg = match config.app.get(app_name) {
    Some(c) => c,
    None => return Ok(()),
  };

  let is_currently_visible = state.visible_app.as_deref() == Some(app_name);

  // If we think it's visible, verify the window still exists
  if is_currently_visible {
    let (target_id, _) = get_window_id_and_pid(app_name, &app_cfg.window_class)
      .await
      .unwrap_or((String::new(), 0));
    if target_id.is_empty() || !is_window_valid(&target_id).await {
      state.visible_app = None;
      return Ok(()); // Just reset state, don't immediately try to show/spawn.
    }
  }

  let should_show = !is_currently_visible;

  let janq_classes: Vec<String> = config
    .app
    .values()
    .map(|v| v.window_class.to_string())
    .collect();
  let classes_string = janq_classes.join(",");

  if should_show {
    let _ = crate::linux::terminal::ensure_terminal_running(app_cfg, config, conn).await;
    update_focus_state(&mut state, &janq_classes, conn).await;
    let (target_id, target_pid) = get_window_id_and_pid(app_name, &app_cfg.window_class)
      .await
      .unwrap_or((String::new(), 0));

    run_toggle_script(
      app_cfg,
      config,
      conn,
      ToggleParams {
        visible: true,
        prev_id: "",
        target_id: &target_id,
        target_pid,
        janq_classes: &classes_string,
      },
      state.max_refresh_rate,
    )
    .await?;
    state.visible_app = Some(app_name.to_string());
  } else {
    let (target_id, target_pid) = get_window_id_and_pid(app_name, &app_cfg.window_class)
      .await
      .unwrap_or((String::new(), 0));

    let prev_id = state.previous_window_id.clone();
    run_toggle_script(
      app_cfg,
      config,
      conn,
      ToggleParams {
        visible: false,
        prev_id: &prev_id,
        target_id: &target_id,
        target_pid,
        janq_classes: &classes_string,
      },
      state.max_refresh_rate,
    )
    .await?;
    state.visible_app = None;
  }
  Ok(())
}

async fn run_toggle_script(
  app_cfg: &AppConfig,
  config: &Config,
  conn: &Connection,
  params: ToggleParams<'_>,
  refresh_rate: f64,
) -> anyhow::Result<()> {
  let duration = if params.visible {
    config.animation.show_duration
  } else {
    config.animation.hide_duration
  };
  let ((width, is_width_percent), (height, is_height_percent)) =
    app_cfg.resolve_dimensions(&config.window);
  let animate_opacity = app_cfg.get_animate_opacity(config.animation.animate_opacity);
  let easing = if params.visible {
    &config.animation.show_easing
  } else {
    &config.animation.hide_easing
  };
  let show_opacity_point = config.animation.show_opacity_point.clamp(0.0, 1.0);
  let hide_opacity_point = config.animation.hide_opacity_point.clamp(0.0, 1.0);

  let script_body_raw = TOGGLE_SCRIPT_TEMPLATE.replace("/*{{COMMON_KWIN_JS}}*/", COMMON_KWIN_JS);
  let script_body_trimmed = script_body_raw.trim();
  let script_body = script_body_trimmed
    .strip_suffix(';')
    .unwrap_or(script_body_trimmed);
  let script_content = format!(
    "{}(\n  \"{}\", \"{}\", {}, {}, {}, {}, {},\n  {}, \"{}\", {}, {}, {},\n  {}, {}, \"{}\", \"{}\", {}, \"{}\", {}, {}\n);",
    script_body,
    app_cfg.window_class,
    config.window.display_mode,
    config.window.display_index,
    width,
    is_width_percent,
    height,
    is_height_percent,
    duration,
    easing,
    params.visible,
    config.window.keep_above,
    animate_opacity,
    show_opacity_point,
    hide_opacity_point,
    params.prev_id,
    params.target_id,
    params.target_pid,
    params.janq_classes,
    config.window.force_priority,
    refresh_rate
  );

  run_kwin_script(conn, "janq_toggle_engine", &script_content, None)
    .await
    .map_err(anyhow::Error::from)
}

pub async fn ensure_grabbed(
  app_cfg: &AppConfig,
  config: &Config,
  conn: &Connection,
) -> anyhow::Result<()> {
  grab_apps(&[(app_cfg.clone(), config.clone())], conn).await
}

pub async fn grab_apps(apps: &[(AppConfig, Config)], conn: &Connection) -> anyhow::Result<()> {
  println!("janq: Yoinking apps...");
  let all_windows = fetch_system_windows().await;
  let state = STATE.lock().await;

  let mut apps_json = Vec::new();
  for (app_cfg, config) in apps {
    let app_name = config
      .app
      .iter()
      .find(|(_, cfg)| cfg.window_class == app_cfg.window_class)
      .map(|(name, _)| name.as_str())
      .unwrap_or("");

    let (target_id, target_pid) = if let Some(id) =
      check_window_exists_with_candidates(&app_cfg.window_class, Some(&all_windows)).await
    {
      let pid = get_pid_for_class(&app_cfg.window_class).unwrap_or(0);
      if !app_name.is_empty() {
        if let Ok(mut cache) = get_window_cache().try_lock() {
          cache.insert(app_name.to_string(), (id.clone(), pid));
        }
      }
      (id, pid)
    } else {
      (String::new(), 0)
    };

    let ((width, is_width_percent), (height, is_height_percent)) =
      app_cfg.resolve_dimensions(&config.window);
    let is_visible = state.visible_app.as_deref() == Some(app_name);
    apps_json.push(format!(
            "{{ windowClass: \"{}\", displayMode: \"{}\", displayIndex: {}, width: {}, isWidthPercent: {}, height: {}, isHeightPercent: {}, keepAbove: {}, targetWindowId: \"{}\", targetPid: {}, isVisible: {}, forcePriority: {} }}",
            app_cfg.window_class, config.window.display_mode, config.window.display_index, width, is_width_percent, height, is_height_percent,
            config.window.keep_above, target_id, target_pid, is_visible, config.window.force_priority
        ));
  }

  let script_body_raw =
    ENSURE_GRABBED_BATCH_TEMPLATE.replace("/*{{COMMON_KWIN_JS}}*/", COMMON_KWIN_JS);
  let script_body_trimmed = script_body_raw.trim();
  let script_body = script_body_trimmed
    .strip_suffix(';')
    .unwrap_or(script_body_trimmed);
  let script_content = format!("{}([\n  {}\n]);", script_body, apps_json.join(",\n  "));

  run_kwin_script(
    conn,
    "janq_init_script",
    &script_content,
    Some(Duration::ZERO),
  )
  .await
  .map_err(anyhow::Error::from)
}

pub async fn restore_app(window_class: &str, conn: &Connection) -> anyhow::Result<()> {
  let script_body_raw = RESTORE_TEMPLATE.replace("/*{{COMMON_KWIN_JS}}*/", COMMON_KWIN_JS);
  let script_body_trimmed = script_body_raw.trim();
  let script_body = script_body_trimmed
    .strip_suffix(';')
    .unwrap_or(script_body_trimmed);
  let script_content = format!("{}(\"{}\");", script_body, window_class);
  run_kwin_script(
    conn,
    "janq_restore_script",
    &script_content,
    Some(Duration::from_millis(300)),
  )
  .await
  .map_err(anyhow::Error::from)
}

pub async fn restore_quake(config: &Config, conn: &Connection) -> anyhow::Result<()> {
  for app_cfg in config.app.values() {
    let _ = restore_app(&app_cfg.window_class, conn).await;
  }
  Ok(())
}

pub async fn reset_visibility(config: &Config) {
  let mut state = STATE.lock().await;
  if let Some(app) = &state.visible_app {
    if !config.app.contains_key(app) {
      println!(
        "Visibility: Currently visible app '{}' removed from config, resetting state.",
        app
      );
      state.visible_app = None;
    }
  }
}

pub fn clear_removed_apps_from_cache(old_config: &Config, new_config: &Config) {
  let removed_apps: Vec<_> = old_config
    .app
    .keys()
    .filter(|name| !new_config.app.contains_key(*name))
    .cloned()
    .collect();

  if !removed_apps.is_empty() {
    if let Ok(mut cache) = get_window_cache().try_lock() {
      for name in removed_apps {
        cache.remove(&name);
      }
    }
  }
}
