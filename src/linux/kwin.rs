use crate::config::Config;
use std::fs;
use std::process::Command;
use tokio::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use zbus::{Connection, Result};

// Global state
struct KWinState {
    target_visible: bool,
    previous_window_id: String, // Window UUID for precise targeting
    ruake_window_id: String,    // Cache Ruake's own UUID
}

static STATE: Mutex<KWinState> = Mutex::const_new(KWinState {
    target_visible: false,
    previous_window_id: String::new(),
    ruake_window_id: String::new(),
});

const TOGGLE_SCRIPT_BODY: &str = r#"
// Compatibility
var clients = workspace.windowList ? workspace.windowList() : workspace.clientList();
var target = null;

for (var i = 0; i < clients.length; i++) {
  var c = clients[i];
  var match = false;
  if (c.resourceClass && c.resourceClass.toLowerCase() == windowClass.toLowerCase()) match = true;
  else if (c.resourceName && c.resourceName.toLowerCase() == windowClass.toLowerCase()) match = true;
  else if (c.caption && c.caption.toLowerCase().indexOf(windowClass.toLowerCase()) !== -1) match = true;
  if (match) { target = c; break; }
}

function getEasing(progress, type) {
  switch (type) {
    case "linear": return progress;
    case "ease-in": return progress * progress;
    case "ease-out": return progress * (2 - progress);
    case "ease-in-out":
      return progress < .5 ? 2 * progress * progress : -1 + (4 - 2 * progress) * progress;
    case "sine-in": case "ease-in-sine": return 1 - Math.cos((progress * Math.PI) / 2);
    case "sine-out": case "ease-out-sine": return Math.sin((progress * Math.PI) / 2);
    case "sine-in-out": case "ease-in-out-sine": return -(Math.cos(Math.PI * progress) - 1) / 2;
    case "quart-in": case "ease-in-quart": return progress * progress * progress * progress;
    case "quart-out": case "ease-out-quart": return 1 - Math.pow(1 - progress, 4);
    case "quart-in-out": case "ease-in-out-quart": return progress < 0.5 ? 8 * Math.pow(progress, 4) : 1 - Math.pow(-2 * progress + 2, 4) / 2;
    case "cubic-in": case "ease-in-cubic": return progress * progress * progress;
    case "cubic-out": case "ease-out-cubic": return 1 - Math.pow(1 - progress, 3);
    case "cubic-in-out": case "ease-in-out-cubic": return progress < 0.5 ? 4 * Math.pow(progress, 3) : 1 - Math.pow(-2 * progress + 2, 3) / 2;
    case "back-in": case "ease-in-back": var c1 = 1.70158; var c3 = c1 + 1; return c3 * progress * progress * progress - c1 * progress * progress;
    case "back-out": case "ease-out-back": var c1 = 1.70158; var c3 = c1 + 1; return 1 + c3 * Math.pow(progress - 1, 3) + c1 * Math.pow(progress - 1, 2);
    case "back-in-out": case "ease-in-out-back": var c1 = 1.70158; var c2 = c1 * 1.525; return progress < 0.5 ? (Math.pow(2 * progress, 2) * ((c2 + 1) * 2 * progress - c2)) / 2 : (Math.pow(2 * progress - 2, 2) * ((c2 + 1) * (progress * 2 - 2) + c2) + 2) / 2;
    case "windows":
      return (function(x, x1, y1, x2, y2) {
        if (x <= 0) return 0; if (x >= 1) return 1;
        var t = x;
        for (var i = 0; i < 8; i++) {
          var x_t = 3 * Math.pow(1 - t, 2) * t * x1 + 3 * (1 - t) * Math.pow(t, 2) * x2 + Math.pow(t, 3);
          var dx_t = 3 * (1 - 4 * t + 3 * t * t) * x1 + 3 * (2 * t - 3 * t * t) * x2 + 3 * t * t;
          if (Math.abs(dx_t) < 1e-6) break;
          t -= (x_t - x) / dx_t;
        }
        return 3 * Math.pow(1 - t, 2) * t * y1 + 3 * (1 - t) * Math.pow(t, 2) * y2 + Math.pow(t, 3);
      })(progress, 0.25, 0, 0, 1);
    default: return progress * (2 - progress);
  }
}

if (target) {
  var screens = workspace.screens;
  var targetArea = null;

  if (displayMode === "specific" && displayIndex >= 0 && displayIndex < screens.length) {
    targetArea = screens[displayIndex].geometry;
  } else if (displayMode === "active") {
    var activeWin = workspace.activeWindow !== undefined ? workspace.activeWindow : workspace.activeClient;
    if (activeWin && activeWin !== target) {
      targetArea = workspace.clientArea(KWin.PlacementArea, activeWin);
    } else {
      targetArea = workspace.activeScreen.geometry;
    }
  } else {
    var cursorPos = workspace.cursorPos;
    for (var i = 0; i < screens.length; i++) {
      var geo = screens[i].geometry;
      if (cursorPos.x >= geo.x && cursorPos.x < geo.x + geo.width &&
        cursorPos.y >= geo.y && cursorPos.y < geo.y + geo.height) {
        targetArea = geo;
        break;
      }
    }
    if (!targetArea) targetArea = workspace.activeScreen.geometry;
  }

  var currentArea = workspace.clientArea(KWin.PlacementArea, target);
  var isMostlyHidden = target.minimized || (target.frameGeometry.y + target.frameGeometry.height <= currentArea.y + 10);
  var area = shouldShow ? targetArea : currentArea;

  // Only snap to start position if the window is truly hidden/inactive or on a different screen
  var sameScreen = Math.abs(target.frameGeometry.x - (area.x + (area.width - target.frameGeometry.width)/2)) < 500;
  var needsReposition = shouldShow && (isMostlyHidden || !sameScreen);

  var finalWidth = ( widthPct > 0 ) ? area.width * widthPct : target.frameGeometry.width;
  var finalHeight = ( heightPct > 0 ) ? area.height * heightPct : target.frameGeometry.height;
  var finalX = area.x + (area.width - finalWidth) / 2;
  var finalY = area.y;

  target.keepAbove = keepAbove;
  target.onAllDesktops = true;
  target.noBorder = true;
  target.skipTaskbar = true;
  target.skipPager = true;

  if (shouldShow) {
    var startY = target.frameGeometry.y;
    var startOpacity = target.opacity;

    if (needsReposition) {
      startY = finalY - finalHeight;
      if (animateOpacity) target.opacity = 0.0;
      else target.opacity = 1.0;
      target.frameGeometry = { x: finalX, y: startY, width: finalWidth, height: finalHeight };
      startOpacity = target.opacity;
    }

    if (target.minimized) target.minimized = false;
    if (workspace.activeWindow !== undefined) workspace.activeWindow = target;
    else workspace.activeClient = target;

    if (duration > 0) {
      var endY = finalY;
      var startTime = new Date().getTime();
      var diff = endY - startY;
      var timer = new QTimer();
      timer.interval = 16;
      timer.timeout.connect(function() {
        var now = new Date().getTime();
        var elapsed = now - startTime;
        var progress = Math.min(elapsed / duration, 1.0);
        var ease = getEasing(progress, easingType);
        var currentY = startY + diff * ease;

        if (animateOpacity) {
          var opacityProgress = 0.0;
          if (opacityPoint > 0.0) {
            opacityProgress = Math.min(progress / opacityPoint, 1.0);
          } else {
            opacityProgress = 1.0;
          }
          target.opacity = Math.max(target.opacity, startOpacity + (1.0 - startOpacity) * opacityProgress);
        }

        target.frameGeometry = { x: finalX, y: currentY, width: finalWidth, height: finalHeight };

        if (progress >= 1.0) {
          timer.stop();
          target.opacity = 1.0;
          target.frameGeometry = { x: finalX, y: finalY, width: finalWidth, height: finalHeight };
        }
      });
      timer.start();
    } else {
      target.opacity = 1.0;
      target.frameGeometry = { x: finalX, y: finalY, width: finalWidth, height: finalHeight };
    }

  } else {
    var startY = target.frameGeometry.y;
    var startX = target.frameGeometry.x;
    var startW = target.frameGeometry.width;
    var startH = target.frameGeometry.height;
    var startOpacity = target.opacity;
    var endY = area.y - startH;

    if (duration > 0) {
      var startTime = new Date().getTime();
      var diff = endY - startY;
      var timer = new QTimer();
      timer.interval = 16;
      timer.timeout.connect(function() {
        var now = new Date().getTime();
        var elapsed = now - startTime;
        var progress = Math.min(elapsed / duration, 1.0);
        var ease = getEasing(progress, easingType);
        var currentY = startY + diff * ease;

        if (animateOpacity) {
          var fadeStart = 1.0 - opacityPoint;
          var opacityProgress = 0.0;
          if (progress <= fadeStart) {
            opacityProgress = 0.0;
          } else if (opacityPoint > 0.0) {
            opacityProgress = Math.min((progress - fadeStart) / opacityPoint, 1.0);
          } else {
            opacityProgress = 1.0;
          }
          target.opacity = Math.min(target.opacity, startOpacity * (1.0 - opacityProgress));
        }

        target.frameGeometry = { x: startX, y: currentY, width: startW, height: startH };

        if (progress >= 1.0) {
          timer.stop();
          if (animateOpacity) target.opacity = 0.0;
          else target.opacity = 1.0;
          var hiddenX = targetArea.x + (targetArea.width - startW) / 2;
          var hiddenY = targetArea.y - startH;
          target.frameGeometry = { x: hiddenX, y: hiddenY, width: startW, height: startH };

          if (prevWindowId && prevWindowId !== "") {
            var allClients = workspace.windowList ? workspace.windowList() : workspace.clientList();
            for (var j = 0; j < allClients.length; j++) {
              var c = allClients[j];
              if (c.internalId && c.internalId.toString() === prevWindowId) {
                if (workspace.activeWindow !== undefined) workspace.activeWindow = c;
                else workspace.activeClient = c;
                break;
              }
            }
          }
        }
      });
      timer.start();
    } else {
      var hiddenX = targetArea.x + (targetArea.width - startW) / 2;
      var hiddenY = targetArea.y - startH;
      target.frameGeometry = { x: hiddenX, y: hiddenY, width: startW, height: startH };
      target.opacity = 0.0;

      if (prevWindowId && prevWindowId !== "") {
        var allClients = workspace.windowList ? workspace.windowList() : workspace.clientList();
        for (var j = 0; j < allClients.length; j++) {
          var c = allClients[j];
          if (c.internalId && c.internalId.toString() === prevWindowId) {
            if (workspace.activeWindow !== undefined) workspace.activeWindow = c;
            else workspace.activeClient = c;
            break;
          }
        }
      }
    }
  }
}
"#;

const ENSURE_GRABBED_SCRIPT_BODY: &str = r#"
var clients = workspace.windowList ? workspace.windowList() : workspace.clientList();
var target = null;

for (var i = 0; i < clients.length; i++) {
  var c = clients[i];
  var match = false;
  if (c.resourceClass && c.resourceClass.toLowerCase() == windowClass.toLowerCase()) match = true;
  else if (c.resourceName && c.resourceName.toLowerCase() == windowClass.toLowerCase()) match = true;
  else if (c.caption && c.caption.toLowerCase().indexOf(windowClass.toLowerCase()) !== -1) match = true;
  if (match) { target = c; break; }
}

if (target) {
  var area = null;
  var screens = workspace.screens;
  if (displayMode === "specific" && displayIndex >= 0 && displayIndex < screens.length) {
    area = screens[displayIndex].geometry;
  } else if (displayMode === "active") {
    var activeWin = workspace.activeWindow !== undefined ? workspace.activeWindow : workspace.activeClient;
    if (activeWin && activeWin !== target) {
      area = workspace.clientArea(KWin.PlacementArea, activeWin);
    } else {
      area = workspace.activeScreen.geometry;
    }
  } else {
    var cursorPos = workspace.cursorPos;
    for (var i = 0; i < screens.length; i++) {
      var geo = screens[i].geometry;
      if (cursorPos.x >= geo.x && cursorPos.x < geo.x + geo.width &&
        cursorPos.y >= geo.y && cursorPos.y < geo.y + geo.height) {
        area = geo;
        break;
      }
    }
    if (!area) area = workspace.activeScreen.geometry;
  }

  var finalWidth = ( widthPct > 0 ) ? area.width * widthPct : target.frameGeometry.width;
  var finalHeight = ( heightPct > 0 ) ? area.height * heightPct : target.frameGeometry.height;
  var finalX = area.x + (area.width - finalWidth) / 2;
  var finalY = area.y;

  target.keepAbove = keepAbove;
  target.onAllDesktops = true;
  target.noBorder = true;
  target.skipTaskbar = true;

  target.frameGeometry = {
    x: finalX,
    y: finalY - finalHeight,
    width: finalWidth,
    height: finalHeight
  };
}
"#;

const RESTORE_SCRIPT_BODY: &str = r#"
var clients = workspace.windowList ? workspace.windowList() : workspace.clientList();
var target = null;

for (var i = 0; i < clients.length; i++) {
  var c = clients[i];
  if ((c.resourceClass && c.resourceClass.toLowerCase() == windowClass.toLowerCase()) ||
    (c.resourceName && c.resourceName.toLowerCase() == windowClass.toLowerCase())) {
    target = c;
    break;
  }
}
if (target) {
  target.minimized = false;
  target.keepAbove = false;
  target.onAllDesktops = false;
  target.noBorder = false;
  target.opacity = 1.0;

  var geo = target.frameGeometry;
  var area = workspace.clientArea(KWin.PlacementArea, target);

  if (geo.y + geo.height <= area.y + 50) {
    target.frameGeometry = {
      x: geo.x,
      y: area.y + 100,
      width: geo.width,
      height: geo.height
    };
  }
}
"#;

/// Update focus state (capture previous window if not Ruake)
/// Returns true if focus was updated
fn update_focus_state(state: &mut KWinState, ruake_class: &str) {
    // 1. Get current active window ID
    let id_output = Command::new("kdotool")
        .arg("getactivewindow")
        .output();

    let current_id = match id_output {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).trim().to_string(),
        _ => return, // No active window or error
    };

    if current_id.is_empty() { return; }

    // 2. Optimization: If ID matches cached Ruake ID, we are focusing Ruake -> Don't capture
    if !state.ruake_window_id.is_empty() && current_id == state.ruake_window_id {
        return;
    }

    // 3. Slow path: Check class name to see if it IS Ruake (and we need to update cache)
    // or if it's another window we should capture.
    let class_output = Command::new("kdotool")
        .args(["getwindowclassname", &current_id])
        .output();

    match class_output {
        Ok(o) if o.status.success() => {
            let class_name = String::from_utf8_lossy(&o.stdout).trim().to_string();
            if class_name.eq_ignore_ascii_case(ruake_class) {
                // It IS Ruake, update cache
                state.ruake_window_id = current_id;
            } else {
                // It is NOT Ruake, capture it
                state.previous_window_id = current_id;
            }
        },
        _ => {
            // Failed to get class, safe to assume it's valid target?
            // Better to ignore to avoid capturing something weird.
        }
    }
}

pub async fn toggle_quake(config: &Config, conn: &Connection) -> Result<()> {
    // Acquire lock (ASYNC)
    let mut state = STATE.lock().await;

    // 0. Ensure Terminal
    if crate::terminal::ensure_terminal_running(config, conn).await {
        state.target_visible = false;
    }

    // let conn = Connection::session().await?; // Reusing passed connection
    let proxy = zbus::Proxy::new(conn, "org.kde.KWin", "/Scripting", "org.kde.kwin.Scripting").await?;

    let script_name = "ruake_toggle";
    // 1. Unload previous script if exists
    let _ = proxy.call_method("unloadScript", &(script_name)).await;

    // 2. Toggle state
    state.target_visible = !state.target_visible;
    let visible = state.target_visible;
    let keep_above = config.window.keep_above;

    // Choose params based on state
    let duration = if visible { config.animation.show_duration } else { config.animation.hide_duration };
    let easing = if visible { &config.animation.show_easing } else { &config.animation.hide_easing };
    let opacity_point = if visible { config.animation.show_opacity_point } else { config.animation.hide_opacity_point };

    // Capture the current active window ID (if it's not Ruake/Quake)
    // This handles both initial show (active=Previous) AND focus changes while visible (active=NewWindow)
    update_focus_state(&mut state, &config.general.window_class);

    let prev_window_id_to_pass = if visible {
        String::new() // Don't restore focus when showing
    } else {
        state.previous_window_id.clone() // Pass the captured window for focus restoration
    };

    // Create temp file
    let tmp_path = std::env::temp_dir().join(format!("{}.js", script_name));
    {
        use std::io::Write;
        let file = std::fs::File::create(&tmp_path).expect("Failed to create tmp script");
        let mut writer = std::io::BufWriter::new(file);

        // Write Config Variables
        writeln!(writer, "var windowClass = \"{}\";", config.general.window_class).unwrap();
        writeln!(writer, "var displayMode = \"{}\";", config.window.display_mode).unwrap();
        writeln!(writer, "var displayIndex = {};", config.window.display_index).unwrap();
        writeln!(writer, "var widthPct = {} / 100.0;", config.window.width_percent).unwrap();
        writeln!(writer, "var heightPct = {} / 100.0;", config.window.height_percent).unwrap();
        writeln!(writer, "var duration = {};", duration).unwrap();
        writeln!(writer, "var easingType = \"{}\";", easing).unwrap();
        writeln!(writer, "var shouldShow = {};", if visible { "true" } else { "false" }).unwrap();
        writeln!(writer, "var keepAbove = {};", if keep_above { "true" } else { "false" }).unwrap();
        writeln!(writer, "var animateOpacity = {};", if config.animation.animate_opacity { "true" } else { "false" }).unwrap();
        writeln!(writer, "var opacityPoint = {};", opacity_point).unwrap();
        writeln!(writer, "var prevWindowId = \"{}\";", prev_window_id_to_pass).unwrap();

        // Write Static Body
        writer.write_all(TOGGLE_SCRIPT_BODY.as_bytes()).unwrap();
    }

    let tmp_path_str = tmp_path.to_string_lossy().to_string();

    let reply = proxy.call_method("loadScript", &(tmp_path_str, script_name)).await?;
    let script_id: i32 = reply.body().deserialize()?;

    if script_id >= 0 {
        let script_obj_path = format!("/Scripting/Script{}", script_id);
        let script_proxy = zbus::Proxy::new(conn, "org.kde.KWin", script_obj_path, "org.kde.kwin.Script").await?;
        script_proxy.call_method("run", &()).await?;

        // Spawn async task to remove temp file, but no longer unloads (we unload at start of next toggle)
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(duration as u64 + 500)).await;
            let _ = fs::remove_file(tmp_path);
        });
    }

    Ok(())
}

pub async fn ensure_grabbed(config: &Config, conn: &Connection) -> Result<()> {
    // let conn = Connection::session().await?;
    let proxy = zbus::Proxy::new(conn, "org.kde.KWin", "/Scripting", "org.kde.kwin.Scripting").await?;

    let unique_name = "quake_init";
    let tmp_path = std::env::temp_dir().join("quake_init.js");
    {
        use std::io::Write;
        let file = std::fs::File::create(&tmp_path).expect("Failed to create init script");
        let mut writer = std::io::BufWriter::new(file);

        // Write config variables
        writeln!(writer, "var windowClass = \"{}\";", config.general.window_class).unwrap();
        writeln!(writer, "var displayMode = \"{}\";", config.window.display_mode).unwrap();
        writeln!(writer, "var displayIndex = {};", config.window.display_index).unwrap();
        writeln!(writer, "var widthPct = {} / 100.0;", config.window.width_percent).unwrap();
        writeln!(writer, "var heightPct = {} / 100.0;", config.window.height_percent).unwrap();
        writeln!(writer, "var keepAbove = {};", config.window.keep_above).unwrap();

        // Write static script body
        writer.write_all(ENSURE_GRABBED_SCRIPT_BODY.as_bytes()).unwrap();
    }
    let tmp_path_str = tmp_path.to_string_lossy().to_string();

    let reply = proxy.call_method("loadScript", &(tmp_path_str, unique_name)).await?;
    let script_id: i32 = reply.body().deserialize()?;

    if script_id >= 0 {
        let script_obj_path = format!("/Scripting/Script{}", script_id);
        let script_proxy = zbus::Proxy::new(conn, "org.kde.KWin", script_obj_path, "org.kde.kwin.Script").await?;
        script_proxy.call_method("run", &()).await?;

        tokio::time::sleep(Duration::from_millis(100)).await;
        let _ = script_proxy.call_method("stop", &()).await;
        let _ = proxy.call_method("unloadScript", &(unique_name)).await;
        let _ = fs::remove_file(tmp_path);
    }
    Ok(())
}

pub async fn restore_quake(config: &Config, conn: &Connection) -> Result<()> {
    // let conn = Connection::session().await?;
    let proxy = zbus::Proxy::new(conn, "org.kde.KWin", "/Scripting", "org.kde.kwin.Scripting").await?;

    let unique_name = format!("goake_restore_{}", SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos());
    let tmp_path = std::env::temp_dir().join(format!("{}.js", unique_name));
    {
        use std::io::Write;
        let file = std::fs::File::create(&tmp_path).expect("Failed to create restore script");
        let mut writer = std::io::BufWriter::new(file);

        // Write config variable
        writeln!(writer, "var windowClass = \"{}\";", config.general.window_class).unwrap();

        // Write static script body
        writer.write_all(RESTORE_SCRIPT_BODY.as_bytes()).unwrap();
    }
    let tmp_path_str = tmp_path.to_string_lossy().to_string();

    let reply = proxy.call_method("loadScript", &(tmp_path_str, unique_name.as_str())).await?;
    let script_id: i32 = reply.body().deserialize()?;

    if script_id >= 0 {
        let script_obj_path = format!("/Scripting/Script{}", script_id);
        let script_proxy = zbus::Proxy::new(conn, "org.kde.KWin", script_obj_path, "org.kde.kwin.Script").await?;
        script_proxy.call_method("run", &()).await?;

        tokio::time::sleep(Duration::from_millis(300)).await;
        let _ = script_proxy.call_method("stop", &()).await;
        let _ = proxy.call_method("unloadScript", &(unique_name.as_str())).await;
        let _ = fs::remove_file(tmp_path);
    }
    Ok(())
}

// Reset state
pub async fn reset_visibility() {
    let mut state = STATE.lock().await;
    state.target_visible = false;
}
