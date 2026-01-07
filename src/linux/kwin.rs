use crate::config::{AppConfig, Config};
use std::fs;
use tokio::sync::Mutex;
use tokio::time::{sleep, Duration};

use zbus::{Connection, Result};

/// Helper to run a KWin script with common boilerplate:
/// unload old script, write to temp file, load, run, and optionally cleanup.
async fn run_kwin_script(
  conn: &Connection,
  script_name: &str,
  script_content: &str,
  delay_before_unload: Option<Duration>,
) -> Result<()> {
  let scripting_proxy = zbus::Proxy::new(conn, "org.kde.KWin", "/Scripting", "org.kde.kwin.Scripting").await?;
  let _ = scripting_proxy.call_method("unloadScript", &(script_name)).await;

  let tmp_path = std::env::temp_dir().join(format!("{}.js", script_name));
  fs::write(&tmp_path, script_content).map_err(|e| zbus::Error::Failure(format!("Failed to write script: {}", e)))?;

  let tmp_path_str = tmp_path.to_string_lossy().to_string();
  let reply = scripting_proxy
    .call_method("loadScript", &(tmp_path_str, script_name))
    .await?;
  let script_id: i32 = reply.body().deserialize()?;

  if script_id >= 0 {
    let script_obj_path = format!("/Scripting/Script{}", script_id);
    let script_proxy = zbus::Proxy::new(conn, "org.kde.KWin", script_obj_path, "org.kde.kwin.Script").await?;
    script_proxy.call_method("run", &()).await?;

    if let Some(delay) = delay_before_unload {
      sleep(delay).await;
      let _ = scripting_proxy.call_method("unloadScript", &(script_name)).await;
    }
    let _ = fs::remove_file(tmp_path);
  }
  Ok(())
}

// Global state
struct KWinState {
  visible_app: Option<String>,
}

static STATE: Mutex<KWinState> = Mutex::const_new(KWinState { visible_app: None });

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
  ruake_classes: &'a str,
}

// Template bodies that take arguments in their IIFE
const TOGGLE_SCRIPT_TEMPLATE: &str = r#"
(function(
    windowClass, displayMode, displayIndex, width, isWidthPercent, height, isHeightPercent,
    duration, easingType, shouldShow, keepAbove, animateOpacity,
    opacityPoint, prevWindowId, targetWindowId, targetPid, ruakeClasses,
    forcePriority
) {
    var clients = workspace.windowList();
    var target = null;

    for (var i = 0; i < clients.length; i++) {
        var c = clients[i];
        if (targetWindowId && c.internalId && c.internalId.toString().includes(targetWindowId)) { target = c; break; }
        if (targetPid > 0 && c.pid == targetPid) { target = c; break; }
        if (c.resourceClass && c.resourceClass.toLowerCase().includes(windowClass.toLowerCase())) { target = c; break; }
    }

    if (!target) return;

    if (shouldShow) {
        if (workspace.activeWindow && workspace.activeWindow !== target) {
            target.ruakePrevWindowId = workspace.activeWindow.internalId.toString();
        }
        target.onAllDesktops = true;
        target.keepAbove = keepAbove;
        target.noBorder = true;
        target.skipTaskbar = true;
        target.skipPager = true;
        target.skipSwitcher = true;
        if (forcePriority) target.fullScreen = true;
    }

    var currentArea = workspace.clientArea(KWin.PlacementArea, target);
    var targetArea = shouldShow ? ((displayMode === "specific" && displayIndex >= 0 && displayIndex < workspace.screens.length)
        ? workspace.screens[displayIndex].geometry : workspace.activeScreen.geometry) : currentArea;

    var finalWidth = width > 0 ? (isWidthPercent ? targetArea.width * width : width) : target.frameGeometry.width;
    var finalHeight = height > 0 ? (isHeightPercent ? targetArea.height * height : height) : target.frameGeometry.height;
    var finalX = shouldShow ? (targetArea.x + (targetArea.width - finalWidth) / 2) : target.frameGeometry.x;

    var startY = target.frameGeometry.y;
    var endY = shouldShow ? targetArea.y : targetArea.y - finalHeight - 10;
    var startOpacity = target.opacity;
    var endOpacity = shouldShow ? 1.0 : 0.0;

    if (shouldShow) {
        target.minimized = false;
        var onWrongMonitor = (Math.abs(target.frameGeometry.x - finalX) > 100);
        var isHidden = (target.opacity < 0.05 || target.frameGeometry.y + target.frameGeometry.height <= targetArea.y + 10);

        if (onWrongMonitor || isHidden) {
            target.opacity = 0;
            target.frameGeometry = { x: finalX, y: targetArea.y - finalHeight - 10, width: finalWidth, height: finalHeight };
            startY = target.frameGeometry.y;
            startOpacity = 0;
        }
    }

    var actualDuration = (finalHeight > 0) ? (duration * (Math.abs(endY - startY) / finalHeight)) : duration;

    var startTime = Date.now();
    var timer = new QTimer();
    timer.interval = 16;
    timer.repeat = true;
    timer.timeout.connect(function() {
        var elapsed = Date.now() - startTime;
        var progress = Math.min(elapsed / (actualDuration || 1), 1.0);
        var ease = (easingType === "linear") ? progress : (progress < 0.5 ? 2 * progress * progress : 1 - Math.pow(-2 * progress + 2, 2) / 2);

        target.frameGeometry = { x: finalX, y: startY + (endY - startY) * ease, width: finalWidth, height: finalHeight };

        if (animateOpacity) {
            var denom = 1.0 - opacityPoint;
            var opProg = Math.max(0, Math.min(1, (ease - opacityPoint) / (denom || 0.001)));
            target.opacity = startOpacity + (endOpacity - startOpacity) * opProg;
        } else {
            if (shouldShow) target.opacity = 1.0;
        }

        if (progress >= 1.0) {
            timer.stop();
            if (!shouldShow) {
                target.opacity = 0;
                if (target.ruakePrevWindowId) {
                    var allC = workspace.windowList();
                    for (var f = 0; f < allC.length; f++) {
                        if (allC[f].internalId.toString() === target.ruakePrevWindowId) {
                            workspace.activeWindow = allC[f];
                            break;
                        }
                    }
                }
            } else workspace.activeWindow = target;
        }
    });
    timer.start();
    target.ruakeTimer = timer;
})"#;

const ENSURE_GRABBED_BATCH_TEMPLATE: &str = r#"
(function(apps) {
    var clients = workspace.windowList();
    for (var a = 0; a < apps.length; a++) {
        var app = apps[a];
        var target = null;
        for (var i = 0; i < clients.length; i++) {
            var c = clients[i];
            if (app.targetWindowId && c.internalId && c.internalId.toString().includes(app.targetWindowId)) { target = c; break; }
            if (app.targetPid > 0 && c.pid == app.targetPid) { target = c; break; }
            if (c.resourceClass && c.resourceClass.toLowerCase().includes(app.windowClass.toLowerCase())) { target = c; break; }
        }

        if (target) {
          target.onAllDesktops = true;
          target.keepAbove = app.keepAbove;
          target.noBorder = true;
          target.skipTaskbar = true;
          target.skipPager = true;
          target.skipSwitcher = true;
          if (app.forcePriority) target.fullScreen = true;

          var area = (app.displayMode === "specific" && app.displayIndex >= 0 && app.displayIndex < workspace.screens.length)
                     ? workspace.screens[app.displayIndex].geometry : workspace.activeScreen.geometry;

          var finalWidth = app.width > 0 ? (app.isWidthPercent ? area.width * app.width : app.width) : target.frameGeometry.width;
          var finalHeight = app.height > 0 ? (app.isHeightPercent ? area.height * app.height : app.height) : target.frameGeometry.height;
          var finalX = area.x + (area.width - finalWidth) / 2;

          if (!app.isVisible) {
              target.opacity = 0.0;
              target.frameGeometry = { x: finalX, y: area.y - finalHeight - 10, width: finalWidth, height: finalHeight };
          }
        }
    }
})"#;

const RESTORE_TEMPLATE: &str = r#"
(function(windowClass) {
    var clients = workspace.windowList();
    for (var i = 0; i < clients.length; i++) {
        var c = clients[i];
        if (c.resourceClass && c.resourceClass.toLowerCase().includes(windowClass.toLowerCase())) {
            var area = workspace.clientArea(KWin.PlacementArea, c);
            c.keepAbove = false;
            c.onAllDesktops = false;
            c.noBorder = false;
            c.skipTaskbar = false;
            c.skipPager = false;
            c.skipSwitcher = false;
            c.fullScreen = false;
            c.opacity = 1.0;
            if (c.frameGeometry.y + c.frameGeometry.height <= area.y + 10) {
                c.frameGeometry = { x: area.x + (area.width - c.frameGeometry.width) / 2, y: area.y + 50, width: c.frameGeometry.width, height: c.frameGeometry.height };
            }
        }
    }
})"#;

// Focus state is now tracked internally by the KWin script for zero latency.

fn get_window_id_and_pid(app_name: &str, class: &str) -> Option<(String, u32)> {
  // 1. Check Cache
  {
    if let Ok(cache) = get_window_cache().try_lock() {
      if let Some((id, pid)) = cache.get(app_name) {
        // We no longer verify with kdotool here to avoid latency.
        // The KWin script will fall back to class search if the ID is invalid.
        return Some((id.clone(), *pid));
      }
    }
  }

  // 2. Fallback to Search
  if let Some(id) = crate::linux::terminal::check_window_exists(class) {
    let pid = 0;
    // 3. Update Cache
    if let Ok(mut cache) = get_window_cache().try_lock() {
      cache.insert(app_name.to_string(), (id.clone(), pid));
    }
    return Some((id, pid));
  }
  None
}

pub async fn toggle_quake(app_name: &str, config: &Config, conn: &Connection) -> Result<()> {
  let mut state = STATE.lock().await;
  let app_cfg = match config.app.get(app_name) {
    Some(c) => c,
    None => return Ok(()),
  };

  let is_currently_visible = state.visible_app.as_deref() == Some(app_name);
  let should_show = !is_currently_visible;

  let ruake_classes: Vec<String> = config.app.values().map(|v| v.window_class.to_string()).collect();
  let classes_string = ruake_classes.join(",");

  if should_show {
    // 1. Fast ID lookup (Checks cache first, no shell-out)
    let (mut target_id, mut target_pid) =
      get_window_id_and_pid(app_name, &app_cfg.window_class).unwrap_or((String::new(), 0));

    // 2. Only check/start terminal if we don't have a cached ID
    if target_id.is_empty() {
      let _ = crate::linux::terminal::ensure_terminal_running(app_cfg, config, conn).await;
      // Re-fetch after spawn attempt
      let res = get_window_id_and_pid(app_name, &app_cfg.window_class).unwrap_or((String::new(), 0));
      target_id = res.0;
      target_pid = res.1;
    }

    run_toggle_script(
      app_cfg,
      config,
      conn,
      ToggleParams {
        visible: true,
        prev_id: "", // Script manages focus restoration internally
        target_id: &target_id,
        target_pid,
        ruake_classes: &classes_string,
      },
    )
    .await?;
    state.visible_app = Some(app_name.to_string());
  } else {
    let (target_id, target_pid) = get_window_id_and_pid(app_name, &app_cfg.window_class).unwrap_or((String::new(), 0));

    run_toggle_script(
      app_cfg,
      config,
      conn,
      ToggleParams {
        visible: false,
        prev_id: "",
        target_id: &target_id,
        target_pid,
        ruake_classes: &classes_string,
      },
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
) -> Result<()> {
  let duration = if params.visible {
    config.animation.show_duration
  } else {
    config.animation.hide_duration
  };
  let ((width, is_width_percent), (height, is_height_percent)) = app_cfg.resolve_dimensions(&config.window);
  let animate_opacity = app_cfg.get_animate_opacity(config.animation.animate_opacity);
  let easing = if params.visible {
    &config.animation.show_easing
  } else {
    &config.animation.hide_easing
  };
  let opacity_point = if params.visible {
    config.animation.show_opacity_point
  } else {
    config.animation.hide_opacity_point
  };

  let script_content = format!(
    "{}(\n  \"{}\", \"{}\", {}, {}, {}, {}, {},\n  {}, \"{}\", {}, {}, {},\n  {}, \"{}\", \"{}\", {}, \"{}\", {}\n);",
    TOGGLE_SCRIPT_TEMPLATE,
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
    opacity_point,
    params.prev_id,
    params.target_id,
    params.target_pid,
    params.ruake_classes,
    config.window.force_priority
  );

  run_kwin_script(conn, "ruake_toggle_engine", &script_content, None).await
}

pub async fn ensure_grabbed(app_cfg: &AppConfig, config: &Config, conn: &Connection) -> Result<()> {
  grab_apps(&[(app_cfg.clone(), config.clone())], conn).await
}

pub async fn grab_apps(apps: &[(AppConfig, Config)], conn: &Connection) -> Result<()> {
  if apps.is_empty() {
    return Ok(());
  }
  let state = STATE.lock().await;

  let mut apps_json = Vec::new();
  for (app_cfg, config) in apps {
    let app_name = config
      .app
      .iter()
      .find(|(_, cfg)| cfg.window_class == app_cfg.window_class)
      .map(|(name, _)| name.as_str())
      .unwrap_or("");

    let (target_id, target_pid) = get_window_id_and_pid(app_name, &app_cfg.window_class).unwrap_or((String::new(), 0));
    let ((width, is_width_percent), (height, is_height_percent)) = app_cfg.resolve_dimensions(&config.window);
    let is_visible = state.visible_app.as_deref() == Some(app_name);
    apps_json.push(format!(
            "{{ windowClass: \"{}\", displayMode: \"{}\", displayIndex: {}, width: {}, isWidthPercent: {}, height: {}, isHeightPercent: {}, keepAbove: {}, targetWindowId: \"{}\", targetPid: {}, isVisible: {}, forcePriority: {} }}",
            app_cfg.window_class, config.window.display_mode, config.window.display_index, width, is_width_percent, height, is_height_percent,
            config.window.keep_above, target_id, target_pid, is_visible, config.window.force_priority
        ));
  }

  let script_content = format!(
    "{}([\n  {}\n]);",
    ENSURE_GRABBED_BATCH_TEMPLATE,
    apps_json.join(",\n  ")
  );

  run_kwin_script(conn, "ruake_init_script", &script_content, Some(Duration::ZERO)).await
}

pub async fn restore_app(window_class: &str, conn: &Connection) -> Result<()> {
  let script_content = format!("{}(\"{}\");", RESTORE_TEMPLATE, window_class);
  run_kwin_script(
    conn,
    "ruake_restore_script",
    &script_content,
    Some(Duration::from_millis(300)),
  )
  .await
}

pub async fn restore_quake(config: &Config, conn: &Connection) -> Result<()> {
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
