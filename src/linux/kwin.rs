//! KWin script injection and window manipulation for Linux/KDE.
//!
//! ## Architecture
//!
//! janq controls windows on KDE by dynamically loading JavaScript scripts
//! into KWin via D-Bus. Scripts are:
//! 1. Written to temp files
//! 2. Loaded via `org.kde.kwin.Scripting.loadScript`
//! 3. Executed via `org.kde.kwin.Script.run`
//! 4. Unloaded after completion (or left resident for callbacks)
//!
//! ## Script Types
//!
//! - **Toggle Script** (`toggle_quake.js`) - Animates show/hide with easing
//! - **Grab Script** (`ensure_grabbed.js`) - Initial window capture and positioning
//! - **Restore Script** (`restore.js`) - Undoes quake positioning on exit
//! - **Fetch Windows** (`fetch_windows.js`) - Enumerates all windows for discovery
//! - **Get Active Window** (`get_active_window.js`) - Retrieves current focus for restoration
//!
//! ## State Management
//!
//! Global `KWinState` tracks:
//! - `visible_app` - Currently visible janq window (None if all hidden)
//! - `previous_window_id` - Window to restore focus to after hide
//! - `max_refresh_rate` - Detected system refresh rate, used when `framerate = "auto"`
use rustc_hash::FxHashMap;
use std::{
  env::temp_dir,
  fs,
  process::Command,
  sync::atomic::{AtomicU64, Ordering},
  sync::{Mutex as StdMutex, OnceLock},
};

use tokio::{
  process::Command as TokioCommand,
  sync::Mutex,
  time::{sleep, Duration},
};
use zbus::{names::BusName, names::InterfaceName, zvariant::ObjectPath, Connection};

use crate::linux::cache::{clear_cache, get_cached_window, remove_from_cache, update_cache};
use crate::linux::desktop::find_desktop_file_id;
use crate::linux::terminal::{
  check_window_exists, check_window_exists_with_candidates, ensure_terminal_running,
  fetch_system_windows, get_pid_for_app, is_window_valid,
};
use janq::config::{AppConfig, Config, Framerate, PositionOffset, SlideDirection};
use janq::error::Result;
use janq::format_error_boxed;

// =============================================================================
// Linux Animation Logic
// =============================================================================

/// Consolidated animation parameters for a specific application (Linux/JS specific).
#[derive(Clone, Debug)]
struct ResolvedAnimationParts {
  pub dir: &'static str,
  pub val: f64,
  pub is_pct: bool,
  pub is_neg: bool,
  pub is_center: bool,
  pub depth_val: f64,
  pub depth_is_pct: bool,
  pub depth_is_neg: bool,
  pub depth_is_center: bool,
  pub hide_titlebar: bool,
  pub animate_opacity: bool,
  pub no_borders: bool,
  pub hide_easing: String,
}

#[inline]
fn get_animation_parts(app_cfg: &AppConfig, global_window: &Config) -> ResolvedAnimationParts {
  let (dir, offset) = app_cfg.resolve_slide_config(&global_window.window);
  let dir_str = match dir {
    SlideDirection::Top => "top",
    SlideDirection::Bottom => "bottom",
    SlideDirection::Left => "left",
    SlideDirection::Right => "right",
  };
  let (val, is_pct, is_neg) = match offset {
    PositionOffset::Center | PositionOffset::HideTitlebar => (0.0, false, false),
    PositionOffset::Pixels(px) => (px.abs() as f64, false, px < 0),
    PositionOffset::Percent(pct) => (pct.abs() * 100.0, true, pct < 0.0),
  };
  let is_center = matches!(offset, PositionOffset::Center);

  let depth = app_cfg.resolve_depth_offset(&global_window.window);
  let (depth_val, depth_is_pct, depth_is_neg) = match depth {
    PositionOffset::Center | PositionOffset::HideTitlebar => (0.0, false, false),
    PositionOffset::Pixels(px) => (px.abs() as f64, false, px < 0),
    PositionOffset::Percent(pct) => (pct.abs() * 100.0, true, pct < 0.0),
  };
  let depth_is_center = matches!(depth, PositionOffset::Center);
  let hide_titlebar = app_cfg.resolve_hide_titlebar(&global_window.window);

  let animate_opacity = if matches!(global_window.animation.framerate, Framerate::Specific(0)) {
    false
  } else {
    app_cfg.get_animate_opacity(global_window.animation.animate_opacity)
  };
  let no_borders = app_cfg.get_no_borders(global_window.window.no_borders);

  ResolvedAnimationParts {
    dir: dir_str,
    val,
    is_pct,
    is_neg,
    is_center,
    depth_val,
    depth_is_pct,
    depth_is_neg,
    depth_is_center,
    hide_titlebar,
    animate_opacity,
    no_borders,
    hide_easing: global_window.animation.hide_easing.to_string(),
  }
}

// =============================================================================
// KWin Script Runner
// =============================================================================

/// Runs a KWin script with common boilerplate:
/// 1. Unload any existing script with the same name
/// 2. Write script content to temp file
/// 3. Load and execute script
/// 4. Optionally wait and unload (for one-shot scripts)
async fn run_kwin_script(
  conn: &Connection,
  script_name: &str,
  script_content: &str,
  delay_before_unload: Option<Duration>,
) -> Result<()> {
  // 1. Unload existing script if any
  let _ = conn
    .call_method(
      Some(BusName::try_from("org.kde.KWin").expect("valid D-Bus bus name")),
      "/Scripting",
      Some(InterfaceName::try_from("org.kde.kwin.Scripting").expect("valid D-Bus interface name")),
      "unloadScript",
      &(script_name),
    )
    .await;

  // 2. Write content to shared memory (/dev/shm) to avoid SSD churn
  let shm_dir = std::path::PathBuf::from("/dev/shm");
  let tmp_path = if shm_dir.exists() {
    shm_dir.join(format!("{}.js", script_name))
  } else {
    temp_dir().join(format!("{}.js", script_name))
  };

  fs::write(&tmp_path, script_content)
    .map_err(|e| zbus::Error::Failure(format!("Failed to write script: {}", e)))?;

  let tmp_path_str = tmp_path.to_string_lossy().to_string();

  // 3. Load script
  let reply = conn
    .call_method(
      Some(BusName::try_from("org.kde.KWin").expect("valid D-Bus bus name")),
      "/Scripting",
      Some(InterfaceName::try_from("org.kde.kwin.Scripting").expect("valid D-Bus interface name")),
      "loadScript",
      &(tmp_path_str, script_name),
    )
    .await?;

  let script_id: i32 = reply.body().deserialize::<i32>()?;

  if script_id >= 0 {
    let script_obj_path = format!("/Scripting/Script{}", script_id);

    // 4. Run script
    conn
      .call_method(
        Some(BusName::try_from("org.kde.KWin").expect("valid D-Bus bus name")),
        ObjectPath::try_from(script_obj_path).expect("valid D-Bus object path"),
        Some(InterfaceName::try_from("org.kde.kwin.Script").expect("valid D-Bus interface name")),
        "run",
        &(),
      )
      .await?;

    if let Some(delay) = delay_before_unload {
      sleep(delay).await;
      let _ = conn
        .call_method(
          Some(BusName::try_from("org.kde.KWin").expect("valid D-Bus bus name")),
          "/Scripting",
          Some(
            InterfaceName::try_from("org.kde.kwin.Scripting").expect("valid D-Bus interface name"),
          ),
          "unloadScript",
          &(script_name),
        )
        .await;
    }
    let _ = fs::remove_file(tmp_path);
  }
  Ok(())
}

// =============================================================================
// Global State
// =============================================================================

/// Internal state tracking for the toggle engine.
struct KWinState {
  /// Currently visible janq app name, or None if all hidden.
  visible_app: Option<std::sync::Arc<str>>,
  /// Window ID that had focus before janq showed a window.
  /// Used to restore focus when hiding.
  previous_window_id: Option<Box<str>>,
  /// Maximum detected display refresh rate for smooth animation.
  max_refresh_rate: f64,
  /// Whether kscreen-doctor has been called yet.
  is_hz_initialized: bool,
}

static STATE: Mutex<KWinState> = Mutex::const_new(KWinState {
  visible_app: None,
  previous_window_id: None,
  max_refresh_rate: 60.0,
  is_hz_initialized: false,
});

async fn get_max_refresh_rate() -> f64 {
  println!("janq: Detecting display refresh rate...");
  let output = TokioCommand::new("kscreen-doctor")
    .arg("-o")
    .output()
    .await
    .ok()
    .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
    .unwrap_or_default();

  let max_hz = output
    .lines()
    .flat_map(|l| {
      l.split_whitespace()
        .filter(|w| w.contains('*'))
        .filter_map(|w| {
          let s = w.split('@').next_back()?;
          // Efficiently parse the number prefix (e.g. "144.02*")
          let end = s
            .find(|c: char| !c.is_ascii_digit() && c != '.')
            .unwrap_or(s.len());
          s[..end].parse::<f64>().ok()
        })
    })
    .fold(0.0, f64::max);

  let final_hz = if max_hz > 0.0 { max_hz.round() } else { 60.0 };
  println!("janq: Detected highest refresh rate: {}Hz", final_hz);
  final_hz
}

pub async fn reset_refresh_rate_logging() {
  let mut state = STATE.lock().await;
  state.is_hz_initialized = false;
}

/// Resets all KWin-side state after a compositor restart.
/// Clears the visible app tracker and forces refresh rate re-detection.
pub async fn reset_state() {
  let mut state = STATE.lock().await;
  state.visible_app = None;
  state.is_hz_initialized = false;
}

pub async fn init() {
  reset_refresh_rate_logging().await;
}

// Window cache is now managed in src/linux/cache.rs

/// Parameters for toggle script execution
struct ToggleParams<'a> {
  app_name: &'a str,
  visible: bool,
  auto_hide: bool,
  prev_id: &'a str,
  target_id: &'a str,
  target_pid: u32,
  siblings_json: String,
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
  let mut script_content =
    String::with_capacity(FETCH_WINDOWS_SCRIPT.len() + COMMON_KWIN_JS.len() + 32);
  let body = FETCH_WINDOWS_SCRIPT
    .trim()
    .strip_suffix(';')
    .unwrap_or(FETCH_WINDOWS_SCRIPT.trim());

  if let Some(pos) = body.find("/*{{COMMON_KWIN_JS}}*/") {
    script_content.push_str(&body[..pos]);
    script_content.push_str(COMMON_KWIN_JS);
    script_content.push_str(&body[pos + 22..]);
  } else {
    script_content.push_str(body);
  }

  use std::fmt::Write;
  let _ = write!(script_content, "(\"{}\");", request_id);

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

// StdMutex already imported at the top
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
  let mut parts = payload.splitn(3, ':');
  let request_id_str = match parts.next() {
    Some(s) => s,
    None => return,
  };
  let id = match parts.next() {
    Some(s) => s.to_string(),
    None => return,
  };
  let class = match parts.next() {
    Some(s) => s.to_string(),
    None => return,
  };

  if let Ok(request_id) = request_id_str.parse::<u64>() {
    let info = ActiveWindowInfo { id, class };
    let mut waiters = get_active_window_waiters().lock().unwrap();
    if let Some(tx) = waiters.remove(&request_id) {
      let _ = tx.send(info);
    }
  }
}

static REQUEST_ID_COUNTER: AtomicU64 = AtomicU64::new(0);

async fn fetch_active_window(conn: &Connection) -> Option<(String, String)> {
  let request_id = REQUEST_ID_COUNTER.fetch_add(1, Ordering::SeqCst);

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
  // Always fetch fresh when toggling - D-Bus focus events are asynchronous and can be laggy.
  // Using the 100ms cache during a toggle event leads to stale restoration targets.
  let (current_id, class_name) = match fetch_active_window(conn).await {
    Some(info) => info,
    None => return,
  };

  if current_id.is_empty() {
    return;
  }

  // Optimize search: check if current class matches any managed class (case-insensitive)
  // If focus is already on a Janq window, we inherit the existing previous_window_id.
  for managed_class in janq_classes {
    if class_name.eq_ignore_ascii_case(managed_class)
      || class_name
        .to_lowercase()
        .contains(&managed_class.to_lowercase())
    {
      return;
    }
  }

  // Focus is on an external window, save it as the restoration target
  state.previous_window_id = Some(current_id.into());
}

async fn get_window_id_and_pid(
  app_name: &str,
  class: &str,
  conn: &Connection,
) -> Option<(Box<str>, u32)> {
  // 1. Check Cache
  if let Some(cached) = get_cached_window(app_name) {
    // Verify PID liveness via /proc
    if std::path::Path::new(&format!("/proc/{}", cached.pid)).exists() {
      return Some((cached.id.clone(), cached.pid));
    }
  }

  // 2. Fallback to Search
  if let Some(id) = check_window_exists(app_name, class, conn).await {
    let pid = get_pid_for_app(app_name).unwrap_or(0);
    return Some((id, pid));
  }
  None
}

// =============================================================================
// Toggle Logic
// =============================================================================

pub async fn get_visible_app() -> Option<std::sync::Arc<str>> {
  STATE.lock().await.visible_app.clone()
}

pub async fn toggle_quake(app_name: &str, config: &Config, conn: &Connection) -> Result<()> {
  {
    let mut state = STATE.lock().await;
    if !state.is_hz_initialized {
      if matches!(config.animation.framerate, Framerate::Auto) {
        state.max_refresh_rate = get_max_refresh_rate().await;
      } else {
        println!(
          "janq: Display refresh rate is read from config: {}Hz",
          config.animation.framerate
        );
      }
      state.is_hz_initialized = true;
    }
  }
  let mut state = STATE.lock().await;

  let app_cfg = match config.app.get(app_name) {
    Some(c) => c,
    None => return Ok(()),
  };

  let mut is_currently_visible = state.visible_app.as_deref() == Some(app_name);

  // If we think it's visible, verify the window still exists
  if is_currently_visible {
    let (target_id, _) = get_window_id_and_pid(app_name, &app_cfg.window_class, conn)
      .await
      .unwrap_or_else(|| ("".into(), 0));
    if target_id.is_empty() || !is_window_valid(app_name, &target_id, conn).await {
      state.visible_app = None;
      is_currently_visible = false;
    }
  }

  let should_show = !is_currently_visible;

  // Calculate explicit siblings to hide based on cache and managed apps
  let mut siblings_json_parts = Vec::new();
  for (name, other_app) in &config.app {
    if name == app_name || other_app.window_class == app_cfg.window_class {
      continue;
    }

    // Check cache for this sibling's window ID
    if let Some(cached) = get_cached_window(name) {
      if std::path::Path::new(&format!("/proc/{}", cached.pid)).exists() {
        let anim_parts = get_animation_parts(other_app, config);

        siblings_json_parts.push(format!(
          "{{ id: \"{}\", pid: {}, dir: \"{}\", val: {}, pct: {}, neg: {}, ctr: {}, depthVal: {}, depthPct: {}, depthNeg: {}, depthCtr: {}, easing: \"{}\", animOp: {}, noBrd: {} }}",
          cached.id, cached.pid, anim_parts.dir, anim_parts.val, anim_parts.is_pct, anim_parts.is_neg, anim_parts.is_center, anim_parts.depth_val, anim_parts.depth_is_pct, anim_parts.depth_is_neg, anim_parts.depth_is_center, anim_parts.hide_easing, anim_parts.animate_opacity, anim_parts.no_borders
        ));
      }
    }
  }
  let siblings_json = format!("[{}]", siblings_json_parts.join(", "));

  let janq_classes: Vec<String> = config
    .app
    .values()
    .map(|v| v.window_class.to_string())
    .collect();

  if should_show {
    let _ = ensure_terminal_running(app_name, app_cfg, config, conn).await;
    update_focus_state(&mut state, &janq_classes, conn).await;
    let (target_id, target_pid) = get_window_id_and_pid(app_name, &app_cfg.window_class, conn)
      .await
      .unwrap_or(("".into(), 0));

    let effective_hz = match config.animation.framerate {
      Framerate::Auto => state.max_refresh_rate,
      Framerate::Specific(fps) => fps as f64,
    };

    run_toggle_script(
      app_cfg,
      config,
      conn,
      ToggleParams {
        app_name,
        visible: true,
        auto_hide: config.window.auto_hide,
        prev_id: "",
        target_id: &target_id,
        target_pid,
        siblings_json: siblings_json.clone(),
      },
      effective_hz,
    )
    .await?;
    state.visible_app = Some(app_name.into());
  } else {
    let (target_id, target_pid) = get_window_id_and_pid(app_name, &app_cfg.window_class, conn)
      .await
      .unwrap_or(("".into(), 0));

    let prev_id = state
      .previous_window_id
      .as_deref()
      .unwrap_or("")
      .to_string();

    let effective_hz = match config.animation.framerate {
      Framerate::Auto => state.max_refresh_rate,
      Framerate::Specific(fps) => fps as f64,
    };

    run_toggle_script(
      app_cfg,
      config,
      conn,
      ToggleParams {
        app_name,
        visible: false,
        auto_hide: false,
        prev_id: &prev_id,
        target_id: &target_id,
        target_pid,
        siblings_json,
      },
      effective_hz,
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
) -> Result<()> {
  let duration = if matches!(config.animation.framerate, Framerate::Specific(0)) {
    0
  } else if params.visible {
    config.animation.show_duration
  } else {
    config.animation.hide_duration
  };
  let (width_res, height_res) = app_cfg.resolve_dimensions(&config.window);
  let (width, is_width_percent, height, is_height_percent) = (
    width_res.val,
    width_res.is_percent,
    height_res.val,
    height_res.is_percent,
  );
  let easing = if params.visible {
    &config.animation.show_easing
  } else {
    &config.animation.hide_easing
  };
  let show_opacity_point = config.animation.show_opacity_point.clamp(0.0, 1.0);
  let hide_opacity_point = config.animation.hide_opacity_point.clamp(0.0, 1.0);

  // Resolve slide direction and position offset for the target app
  let anim_parts = get_animation_parts(app_cfg, config);

  let mut script_content =
    String::with_capacity(TOGGLE_SCRIPT_TEMPLATE.len() + COMMON_KWIN_JS.len() + 1024);
  let body = TOGGLE_SCRIPT_TEMPLATE
    .trim()
    .strip_suffix(';')
    .unwrap_or(TOGGLE_SCRIPT_TEMPLATE.trim());

  if let Some(pos) = body.find("/*{{COMMON_KWIN_JS}}*/") {
    script_content.push_str(&body[..pos]);
    script_content.push_str(COMMON_KWIN_JS);
    script_content.push_str(&body[pos + 22..]);
  } else {
    script_content.push_str(body);
  }

  use std::fmt::Write;
  let _ = write!(
    script_content,
    "(\n  {{ appName: \"{}\", windowClass: \"{}\", displayMode: \"{}\", displayIndex: {}, width: {}, isWidthPercent: {}, height: {}, isHeightPercent: {}, duration: {}, easingType: \"{}\", shouldShow: {}, autoHide: {}, keepAbove: {}, noBorders: {}, skipPager: {}, allDesktops: {}, animateOpacity: {}, showOpacityPoint: {}, hideOpacityPoint: {}, prevWindowId: \"{}\", targetWindowId: \"{}\", targetPid: {}, forcePriority: {}, slideFrom: \"{}\", offsetValue: {}, offsetIsPercent: {}, offsetIsNegative: {}, offsetIsCenter: {}, depthValue: {}, depthIsPercent: {}, depthIsNegative: {}, depthIsCenter: {}, hideTitlebar: {} }},\n  {},\n  {}\n);",
    params.app_name, app_cfg.window_class, config.window.display_mode, config.window.display_index, width, is_width_percent, height, is_height_percent,
    duration, easing, params.visible, params.auto_hide, config.window.keep_above, anim_parts.no_borders, config.window.skip_pager, config.window.all_desktops.unwrap_or(true), anim_parts.animate_opacity, show_opacity_point, hide_opacity_point,
    params.prev_id, params.target_id, params.target_pid, config.window.force_priority.unwrap_or(false),
    anim_parts.dir, anim_parts.val, anim_parts.is_pct, anim_parts.is_neg, anim_parts.is_center,
    anim_parts.depth_val, anim_parts.depth_is_pct, anim_parts.depth_is_neg, anim_parts.depth_is_center, anim_parts.hide_titlebar,
    params.siblings_json, refresh_rate
  );

  run_kwin_script(conn, "janq_toggle_engine", &script_content, None)
    .await
    .map_err(|e| format_error_boxed!("{}", e))
}

pub async fn ensure_grabbed(app_cfg: &AppConfig, config: &Config, conn: &Connection) -> Result<()> {
  grab_apps(&[(app_cfg, config)], conn).await
}

pub async fn grab_apps(apps: &[(&AppConfig, &Config)], conn: &Connection) -> Result<()> {
  println!("janq: Yoinking apps...");
  let all_windows = fetch_system_windows(conn).await;
  let state = STATE.lock().await;

  let mut apps_json = Vec::new();
  for (app_cfg, config) in apps {
    let app_name = config
      .app
      .iter()
      .find(|(_, cfg)| cfg.window_class == app_cfg.window_class)
      .map(|(name, _)| name.as_str())
      .unwrap_or("");

    let (target_id, target_pid) = if let Some(id) = check_window_exists_with_candidates(
      app_name,
      &app_cfg.window_class,
      conn,
      Some(&all_windows[..]),
    )
    .await
    {
      let pid = get_pid_for_app(app_name).unwrap_or(0);
      if !app_name.is_empty() {
        let proc_name = all_windows
          .iter()
          .find(|w| w.id == id)
          .map(|w| w.proc_lowercase.clone())
          .unwrap_or_else(|| "".into());
        update_cache(app_name, id.clone(), pid, proc_name);
      }
      (id, pid)
    } else {
      ("".into(), 0)
    };

    let (width_res, height_res) = app_cfg.resolve_dimensions(&config.window);
    let (width, is_width_percent, height, is_height_percent) = (
      width_res.val,
      width_res.is_percent,
      height_res.val,
      height_res.is_percent,
    );
    let is_visible = state.visible_app.as_deref() == Some(app_name);

    // Resolve slide config for initial parking
    let anim_parts = get_animation_parts(app_cfg, config);

    apps_json.push(format!(
            "{{ windowClass: \"{}\", displayMode: \"{}\", displayIndex: {}, width: {}, isWidthPercent: {}, height: {}, isHeightPercent: {}, keepAbove: {}, noBorders: {}, skipPager: {}, allDesktops: {}, targetWindowId: \"{}\", targetPid: {}, isVisible: {}, forcePriority: {}, slideFrom: \"{}\", offsetValue: {}, offsetIsPercent: {}, offsetIsNegative: {}, offsetIsCenter: {}, depthValue: {}, depthIsPercent: {}, depthIsNegative: {}, depthIsCenter: {}, hideTitlebar: {} }}",
            app_cfg.window_class, config.window.display_mode, config.window.display_index, width, is_width_percent, height, is_height_percent,
            config.window.keep_above, anim_parts.no_borders, config.window.skip_pager, config.window.all_desktops.unwrap_or(true), target_id, target_pid, is_visible, config.window.force_priority.unwrap_or(false),
            anim_parts.dir, anim_parts.val, anim_parts.is_pct, anim_parts.is_neg, anim_parts.is_center,
            anim_parts.depth_val, anim_parts.depth_is_pct, anim_parts.depth_is_neg, anim_parts.depth_is_center, anim_parts.hide_titlebar
        ));
  }

  let mut script_content =
    String::with_capacity(ENSURE_GRABBED_BATCH_TEMPLATE.len() + COMMON_KWIN_JS.len() + 2048);
  let body = ENSURE_GRABBED_BATCH_TEMPLATE
    .trim()
    .strip_suffix(';')
    .unwrap_or(ENSURE_GRABBED_BATCH_TEMPLATE.trim());

  if let Some(pos) = body.find("/*{{COMMON_KWIN_JS}}*/") {
    script_content.push_str(&body[..pos]);
    script_content.push_str(COMMON_KWIN_JS);
    script_content.push_str(&body[pos + 22..]);
  } else {
    script_content.push_str(body);
  }

  use std::fmt::Write;
  let _ = write!(script_content, "([\n  {}\n]);", apps_json.join(",\n  "));

  run_kwin_script(
    conn,
    "janq_init_script",
    &script_content,
    Some(Duration::ZERO),
  )
  .await
  .map_err(|e| format_error_boxed!("{}", e))
}

pub async fn restore_app(app_name: &str, window_class: &str, conn: &Connection) -> Result<()> {
  let (id, pid) = get_cached_window(app_name)
    .map(|c| (c.id, c.pid))
    .unwrap_or_else(|| ("".into(), 0));

  let mut script_content =
    String::with_capacity(RESTORE_TEMPLATE.len() + COMMON_KWIN_JS.len() + 128);
  let body = RESTORE_TEMPLATE
    .trim()
    .strip_suffix(';')
    .unwrap_or(RESTORE_TEMPLATE.trim());

  if let Some(pos) = body.find("/*{{COMMON_KWIN_JS}}*/") {
    script_content.push_str(&body[..pos]);
    script_content.push_str(COMMON_KWIN_JS);
    script_content.push_str(&body[pos + 22..]);
  } else {
    script_content.push_str(body);
  }

  use std::fmt::Write;
  let _ = write!(
    script_content,
    "(\"{}\", \"{}\", {});",
    window_class, id, pid
  );

  run_kwin_script(
    conn,
    "janq_restore_script",
    &script_content,
    Some(Duration::from_millis(300)),
  )
  .await
  .map_err(|e| janq::format_error_boxed!("{}", e))
}

pub async fn restore_quake(config: &Config, conn: &Connection) -> Result<()> {
  for (name, app_cfg) in &config.app {
    let _ = restore_app(name, &app_cfg.window_class, conn).await;
  }
  Ok(())
}

/// Unloads all known KWin script name slots to clear stale scripts from prior sessions.
pub async fn purge_stale_scripts(conn: &Connection) {
  const STALE_NAMES: &[&str] = &[
    "janq_toggle_engine",
    "janq_init_script",
    "janq_restore_script",
  ];
  for name in STALE_NAMES {
    let _ = conn
      .call_method(
        Some(BusName::try_from("org.kde.KWin").expect("valid D-Bus bus name")),
        "/Scripting",
        Some(
          InterfaceName::try_from("org.kde.kwin.Scripting").expect("valid D-Bus interface name"),
        ),
        "unloadScript",
        &(*name),
      )
      .await;
  }
}

/// Full recovery: purge stale scripts, clear caches, reset state, re-grab all windows.
pub async fn recover_all(config: &Config, conn: &Connection) {
  println!("janq: Recovery started — purging scripts, clearing caches, re-grabbing windows...");
  purge_stale_scripts(conn).await;
  reset_state().await;
  clear_cache();

  let mut apps_for_grabbing = Vec::new();
  for app_cfg in config.app.values() {
    apps_for_grabbing.push((app_cfg, config));
  }
  let _ = grab_apps(&apps_for_grabbing, conn).await;
  println!("janq: Recovery complete.");
}

pub async fn reset_visibility(config: &Config) {
  let mut state = STATE.lock().await;
  if let Some(app) = &state.visible_app {
    if !config.app.contains_key(&**app) {
      println!(
        "Visibility: Currently visible app '{}' removed from config, resetting state.",
        app
      );
      state.visible_app = None;
    }
  }
}

pub fn clear_removed_apps_from_cache(old_config: &Config, new_config: &Config) {
  for name in old_config.app.keys() {
    if !new_config.app.contains_key(name) {
      remove_from_cache(name);
    }
  }
}

pub fn sync_kwin_rules(config: &Config) -> Result<()> {
  sync_kwin_rules_impl(Some(config))
}

pub fn purge_kwin_rules() -> Result<()> {
  sync_kwin_rules_impl(None)
}

static RULE_SYNC_MUTEX: StdMutex<()> = StdMutex::new(());

fn sync_kwin_rules_impl(config: Option<&Config>) -> Result<()> {
  let _lock = RULE_SYNC_MUTEX.lock().unwrap();
  let mut applied_any = false;

  // Group windows by their target desktop file to minimize the number of rules
  let mut group_map: FxHashMap<String, Vec<String>> = FxHashMap::default();

  if let Some(config) = config {
    if config.window.kde_window_rules.unwrap_or(true) {
      let default_id = "dev.nabaxo.janq".to_string();
      for app_cfg in config.app.values() {
        let id = find_desktop_file_id(&app_cfg.window_class).unwrap_or_else(|| default_id.clone());
        group_map
          .entry(id)
          .or_default()
          .push(app_cfg.window_class.clone());
      }
    }
  }

  // Identify ALL existing janq rules in the file (even if not in General.rules)
  let mut janq_rules = FxHashMap::default();
  let kwinrulesrc_path = std::env::var_os("HOME")
    .map(std::path::PathBuf::from)
    .unwrap_or_default()
    .join(".config/kwinrulesrc");

  if let Ok(content) = std::fs::read_to_string(&kwinrulesrc_path) {
    let mut current_group = String::new();
    for line in content.lines() {
      let line = line.trim();
      if line.starts_with('[') && line.ends_with(']') {
        current_group = line[1..line.len() - 1].to_string();
      } else if line.starts_with("Description=janq automated icon fix") {
        let desc = line.trim_start_matches("Description=").trim().to_string();
        janq_rules.insert(current_group.clone(), desc);
      }
    }
  }

  // Read the current rules list to maintain order/cleanup
  let current_rules_str = Command::new("kreadconfig6")
    .args([
      "--file",
      "kwinrulesrc",
      "--group",
      "General",
      "--key",
      "rules",
    ])
    .output()
    .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
    .unwrap_or_default();

  let mut rule_ids: Vec<String> = if current_rules_str.is_empty() {
    Vec::new()
  } else {
    current_rules_str
      .split(',')
      .map(|s| s.to_string())
      .collect()
  };

  let mut next_id = rule_ids
    .iter()
    .filter_map(|s| s.parse::<u32>().ok())
    .chain(janq_rules.keys().filter_map(|k| k.parse::<u32>().ok()))
    .max()
    .unwrap_or(0)
    + 1;

  let mut kept_ids = Vec::new();

  for (desktop_id, classes) in group_map {
    let target_description = format!("janq automated icon fix for {}", desktop_id);
    let target_id = janq_rules
      .iter()
      .find(|(_, d)| *d == &target_description)
      .map(|(k, _)| k.clone());

    let id = target_id.unwrap_or_else(|| {
      let id_str = next_id.to_string();
      next_id += 1;
      id_str
    });

    if !rule_ids.contains(&id) {
      rule_ids.push(id.clone());
    }
    kept_ids.push(id.clone());
    applied_any = true;

    let regex = format!("(?i){}", classes.join("|"));
    let kv = [
      ("Description", &target_description),
      ("enabled", &"true".to_string()),
      ("resourceClass", &regex),
      ("resourceClassMatch", &"3".to_string()),
      ("wmclass", &regex),
      ("wmclassmatch", &"3".to_string()),
      ("desktopfile", &desktop_id),
      ("desktopfilerule", &"2".to_string()),
    ];

    for (key, val) in kv {
      let _ = Command::new("kwriteconfig6")
        .args(["--file", "kwinrulesrc", "--group", &id, "--key", key, val])
        .status();
    }
  }

  // Remove stale or orphaned janq rules
  let mut rules_changed = false;
  let mut stale_to_purge = Vec::new();
  for stale_id in janq_rules.keys() {
    if !kept_ids.contains(stale_id) {
      if config.is_none() {
        println!("  - Removing rule group: {}", stale_id);
      }
      rule_ids.retain(|x| x != stale_id);
      stale_to_purge.push(stale_id.clone());
      rules_changed = true;
    }
  }

  // If we have stale rules, we perform a manual "sledgehammer" deletion
  // because kwriteconfig6 --delete often fails with "cannot mark groups as deleted"
  if !stale_to_purge.is_empty() {
    if let Ok(content) = std::fs::read_to_string(&kwinrulesrc_path) {
      let mut new_lines = Vec::new();
      let mut skip_until_next_group = false;

      for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
          let group_name = &trimmed[1..trimmed.len() - 1];
          if stale_to_purge.iter().any(|s| s == group_name) {
            skip_until_next_group = true;
            continue;
          } else {
            skip_until_next_group = false;
          }
        }

        if !skip_until_next_group {
          new_lines.push(line);
        }
      }

      let _ = std::fs::write(&kwinrulesrc_path, new_lines.join("\n"));
    }
  }

  if applied_any || rules_changed {
    // Sync the master list of rules and the count
    let _ = Command::new("kwriteconfig6")
      .args([
        "--file",
        "kwinrulesrc",
        "--group",
        "General",
        "--key",
        "rules",
        &rule_ids.join(","),
      ])
      .status();
    let _ = Command::new("kwriteconfig6")
      .args([
        "--file",
        "kwinrulesrc",
        "--group",
        "General",
        "--key",
        "count",
        &rule_ids.len().to_string(),
      ])
      .status();

    // Ask KWin to reload its rules immediately
    run_kwin_reconfigure();
    println!("janq: KWin window rules updated.");
  }

  Ok(())
}

fn run_kwin_reconfigure() {
  if Command::new("qdbus6")
    .args(["org.kde.KWin", "/KWin", "org.kde.KWin.reconfigure"])
    .status()
    .is_err()
  {
    let _ = Command::new("qdbus")
      .args(["org.kde.KWin", "/KWin", "org.kde.KWin.reconfigure"])
      .status();
  }
}
