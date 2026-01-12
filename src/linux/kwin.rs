use std::env::temp_dir;
use std::fs;
use std::path::Path;
use std::process::Command;

use tokio::{
  sync::Mutex,
  time::{sleep, Duration},
};
use zbus::{Connection, Proxy, Result};

use crate::config::{AppConfig, Config};
use crate::linux::terminal::{
  check_window_exists, check_window_exists_with_candidates, fetch_system_windows, get_pid_for_class,
};

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

// Global state
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

use std::collections::HashMap;
use std::sync::OnceLock;

static WINDOW_CACHE: OnceLock<Mutex<HashMap<String, (String, u32)>>> = OnceLock::new();

fn get_window_cache() -> &'static Mutex<HashMap<String, (String, u32)>> {
  WINDOW_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Parameters for toggle script execution
struct ToggleParams<'a> {
  visible: bool,
  prev_id: &'a str,
  target_id: &'a str,
  target_pid: u32,
  janq_classes: &'a str,
}

const COMMON_KWIN_JS: &str = include_str!("js/common.js");

// Template bodies that take arguments in their IIFE
const TOGGLE_SCRIPT_TEMPLATE: &str = include_str!("js/toggle_quake.js");

const ENSURE_GRABBED_BATCH_TEMPLATE: &str = include_str!("js/ensure_grabbed.js");

const RESTORE_TEMPLATE: &str = include_str!("js/restore.js");

fn update_focus_state(state: &mut KWinState, janq_classes: &[String]) {
  let id_output = Command::new("kdotool").arg("getactivewindow").output();
  let current_id = match id_output {
    Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).trim().to_string(),
    _ => return,
  };
  if current_id.is_empty() {
    return;
  }

  let class_output = Command::new("kdotool")
    .args(["getwindowclassname", &current_id])
    .output();
  match class_output {
    Ok(o) if o.status.success() => {
      let class_name = String::from_utf8_lossy(&o.stdout).trim().to_string();
      let class_lower = class_name.to_lowercase();
      for managed_class in janq_classes {
        if class_lower.contains(&managed_class.to_lowercase()) {
          return;
        }
      }
      state.previous_window_id = current_id;
    }
    _ => {}
  }
}

fn get_window_id_and_pid(app_name: &str, class: &str) -> Option<(String, u32)> {
  // 1. Check Cache
  {
    if let Ok(cache) = get_window_cache().try_lock() {
      if let Some((id, pid)) = cache.get(app_name) {
        // Verify PID liveness via /proc (ultra-fast)
        if Path::new(&format!("/proc/{}", pid)).exists() {
          return Some((id.clone(), *pid));
        }
      }
    }
  }

  // 2. Fallback to Search
  if let Some(id) = check_window_exists(class) {
    let pid = get_pid_for_class(class).unwrap_or(0);
    // 3. Update Cache
    if let Ok(mut cache) = get_window_cache().try_lock() {
      cache.insert(app_name.to_string(), (id.clone(), pid));
    }
    return Some((id, pid));
  }
  None
}

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
  let should_show = !is_currently_visible;

  let janq_classes: Vec<String> = config
    .app
    .values()
    .map(|v| v.window_class.to_string())
    .collect();
  let classes_string = janq_classes.join(",");

  if should_show {
    let _ = crate::linux::terminal::ensure_terminal_running(app_cfg, config, conn).await;
    update_focus_state(&mut state, &janq_classes);
    let (target_id, target_pid) =
      get_window_id_and_pid(app_name, &app_cfg.window_class).unwrap_or((String::new(), 0));

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
    let (target_id, target_pid) =
      get_window_id_and_pid(app_name, &app_cfg.window_class).unwrap_or((String::new(), 0));

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

  let script_body = TOGGLE_SCRIPT_TEMPLATE.replace("/*{{COMMON_KWIN_JS}}*/", COMMON_KWIN_JS);
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
  println!("janq: Grabbing apps...");
  let all_windows = fetch_system_windows();
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
      check_window_exists_with_candidates(&app_cfg.window_class, Some(&all_windows))
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

  let script_body = ENSURE_GRABBED_BATCH_TEMPLATE.replace("/*{{COMMON_KWIN_JS}}*/", COMMON_KWIN_JS);
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
  let script_body = RESTORE_TEMPLATE.replace("/*{{COMMON_KWIN_JS}}*/", COMMON_KWIN_JS);
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
