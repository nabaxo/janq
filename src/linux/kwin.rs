use crate::config::Config;
use std::fs;
use std::process::Command;
use tokio::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use zbus::{Connection, Result};

// Global state
struct KWinState {
    target_visible: bool,
    last_script_id: String,
    previous_window_id: String, // Window UUID for precise targeting
}

static STATE: Mutex<KWinState> = Mutex::const_new(KWinState {
    target_visible: false,
    last_script_id: String::new(),
    previous_window_id: String::new(),
});

// KWin script template with all animation logic and easings
const KWIN_SCRIPT_TEMPLATE: &str = r#"
// Compatibility
var clients = workspace.windowList ? workspace.windowList() : workspace.clientList();
var target = null;
var windowClass = "__WINDOW_CLASS__";

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
    // New Easings (and aliases)
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
    default: return progress * (2 - progress); // ease-out default
  }
}

if (target) {
  // Config Parameters
  var displayMode = "__DISPLAY_MODE__";
  var displayIndex = __DISPLAY_INDEX__;
  var widthPct = __WIDTH_PERCENT__ / 100.0;
  var heightPct = __HEIGHT_PERCENT__ / 100.0;
  var widthCols = __WIDTH_COLS__;
  var heightRows = __HEIGHT_ROWS__;
  var duration = __DURATION__;
  var easingType = "__EASING__";
  var shouldShow = __SHOULD_SHOW__;
  var keepAbove = __KEEP_ABOVE__;
  var animateOpacity = __ANIMATE_OPACITY__;
  var opacityPoint = __OPACITY_POINT__;
  var prevWindowId = "__PREV_WINDOW_ID__";

  var screens = workspace.screens;
  var targetArea = null;

  // Select target screen based on display_mode
  if (displayMode === "specific" && displayIndex >= 0 && displayIndex < screens.length) {
    targetArea = screens[displayIndex].geometry;
  } else if (displayMode === "active") {
    // Use the screen of the currently active/focused window
    var activeWin = workspace.activeWindow !== undefined ? workspace.activeWindow : workspace.activeClient;
    if (activeWin && activeWin !== target) {
      targetArea = workspace.clientArea(KWin.PlacementArea, activeWin);
    } else {
      // Fallback if no other active window
      targetArea = workspace.activeScreen.geometry;
    }
  } else {
    // "follow-mouse" - find screen containing cursor
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

  // We consider it mostly hidden if it's minimized OR if less than 5px is visible.
  var isMostlyHidden = target.minimized || (target.frameGeometry.y + target.frameGeometry.height <= currentArea.y + 5);

  // When SHOWING: use target screen based on display_mode
  // When HIDING: use the window's current screen so it hides upward from where it is
  var area = shouldShow ? targetArea : currentArea;

  // Track if we need to reposition (showing on a different screen than current)
  var needsReposition = shouldShow && (isMostlyHidden || currentArea.x != targetArea.x || currentArea.y != targetArea.y);

  // Target Geometry
  var finalWidth = ( widthCols > 0 ) ? target.frameGeometry.width : area.width * widthPct;
  var finalHeight = ( heightRows > 0 ) ? target.frameGeometry.height : area.height * heightPct;
  var finalX = area.x + (area.width - finalWidth) / 2;
  var finalY = area.y;

  // Properties
  target.keepAbove = keepAbove;
  target.onAllDesktops = true;
  target.noBorder = true;
  target.skipTaskbar = true;
  target.skipPager = true;

  if (shouldShow) {
    // SHOWING

    // Animation Start Point
    var startY = target.frameGeometry.y;
    var startOpacity = animateOpacity ? 0.0 : 1.0;

    // If we need to reposition (was hidden or on different screen), snap to new screen BEFORE unminimizing
    if (needsReposition) {
      // Reposition while still invisible
      startY = finalY - finalHeight;
      target.opacity = 0.0;
      target.frameGeometry = {
        x: finalX,
        y: startY,
        width: finalWidth,
        height: finalHeight
      };
    }

    // NOW unminimize after repositioning
    if (target.minimized) {
      target.minimized = false;
    }

    if (workspace.activeWindow !== undefined) workspace.activeWindow = target;
    else workspace.activeClient = target;

    // If not animating opacity, make visible immediately (after reposition is complete)
    if (!animateOpacity) {
      target.opacity = 1.0;
    }

    // Setup timer
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
          // Opacity completes at opacityPoint of animation (faster fade-in)
          var opacityProgress = Math.min(progress / opacityPoint, 1.0);
          var currentOpacity = startOpacity + (1.0 - startOpacity) * opacityProgress;
          target.opacity = currentOpacity;
        }

        target.frameGeometry = {
          x: finalX,
          y: currentY,
          width: finalWidth,
          height: finalHeight
        };

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
    // HIDING
    var currentGeo = target.frameGeometry;
    var startY = currentGeo.y;
    var startX = currentGeo.x;
    var startW = currentGeo.width;
    var startH = currentGeo.height;

    // Goal: Move up until completely off screen
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
          // Opacity starts fading at opacityPoint of animation (delayed fade-out)
          var opacityProgress = Math.max((progress - opacityPoint) / (1.0 - opacityPoint), 0.0);
          var currentOpacity = 1.0 - opacityProgress;
          target.opacity = currentOpacity;
        }

        target.frameGeometry = {
          x: startX,
          y: currentY,
          width: startW,
          height: startH
        };

        if (progress >= 1.0) {
          timer.stop();
          target.opacity = 0.0;

          // Reposition to hide above the target screen (based on display_mode)
          // This ensures next show slides down from the correct display
          var hiddenX = targetArea.x + (targetArea.width - startW) / 2;
          var hiddenY = targetArea.y - startH;
          target.frameGeometry = { x: hiddenX, y: hiddenY, width: startW, height: startH };

          // Restore focus to the previous window by ID
          if (prevWindowId && prevWindowId !== "") {
            var allClients = workspace.windowList ? workspace.windowList() : workspace.clientList();
            for (var j = 0; j < allClients.length; j++) {
              var c = allClients[j];
              // Match by internalId (UUID) for precise window targeting
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
      // Instant hide - position above target screen
      var hiddenX = targetArea.x + (targetArea.width - startW) / 2;
      var hiddenY = targetArea.y - startH;
      target.frameGeometry = { x: hiddenX, y: hiddenY, width: startW, height: startH };
      target.opacity = 0.0;

      // Restore focus to the previous window by ID
      if (prevWindowId && prevWindowId !== "") {
        var allClients = workspace.windowList ? workspace.windowList() : workspace.clientList();
        for (var j = 0; j < allClients.length; j++) {
          var c = allClients[j];
          // Match by internalId (UUID) for precise window targeting
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

const INIT_SCRIPT_TEMPLATE: &str = r#"
var clients = workspace.windowList ? workspace.windowList() : workspace.clientList();
var target = null;
var windowClass = "__WINDOW_CLASS__";

for (var i = 0; i < clients.length; i++) {
  var c = clients[i];
  var match = false;
  if (c.resourceClass && c.resourceClass.toLowerCase() == windowClass.toLowerCase()) match = true;
  else if (c.resourceName && c.resourceName.toLowerCase() == windowClass.toLowerCase()) match = true;
  else if (c.caption && c.caption.toLowerCase().indexOf(windowClass.toLowerCase()) !== -1) match = true;

  if (match) {
    target = c;
    break;
  }
}

if (target) {
  var displayMode = "__DISPLAY_MODE__";
  var displayIndex = __DISPLAY_INDEX__;
  var widthPct = __WIDTH_PERCENT__ / 100.0;
  var heightPct = __HEIGHT_PERCENT__ / 100.0;
  var widthCols = __WIDTH_COLS__;
  var heightRows = __HEIGHT_ROWS__;
  var keepAbove = __KEEP_ABOVE__;

  // Select screen based on display_mode
  var area = null;
  var screens = workspace.screens;
  if (displayMode === "specific" && displayIndex >= 0 && displayIndex < screens.length) {
    area = screens[displayIndex].geometry;
  } else if (displayMode === "active") {
    // Use the screen of the currently active/focused window
    var activeWin = workspace.activeWindow !== undefined ? workspace.activeWindow : workspace.activeClient;
    if (activeWin && activeWin !== target) {
      area = workspace.clientArea(KWin.PlacementArea, activeWin);
    } else {
      area = workspace.activeScreen.geometry;
    }
  } else {
    // "follow-mouse" - find screen containing cursor
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

  var finalWidth = ( widthCols > 0 ) ? target.frameGeometry.width : area.width * widthPct;
  var finalHeight = ( heightRows > 0 ) ? target.frameGeometry.height : area.height * heightPct;
  var finalX = area.x + (area.width - finalWidth) / 2;
  var finalY = area.y;

  target.keepAbove = keepAbove;
  target.onAllDesktops = true;
  target.noBorder = true;
  target.skipTaskbar = true;

  // Force off-screen
  target.frameGeometry = {
    x: finalX,
    y: finalY - finalHeight,
    width: finalWidth,
    height: finalHeight
  };
}
"#;

const RESTORE_SCRIPT: &str = r#"
var clients = workspace.windowList ? workspace.windowList() : workspace.clientList();
var target = null;
var windowClass = "__WINDOW_CLASS__";
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

  // If window is mostly hidden (offscreen), snap it back into the visible area
  if (geo.y + geo.height <= area.y + 50) {
    target.frameGeometry = {
      x: geo.x,
      y: area.y + 100, // Move to a visible top position
      width: geo.width,
      height: geo.height
    };
  }
}
"#;

/// Get the current active window's UUID using kdotool
fn get_active_window_id(exclude_class: &str) -> String {
    // First check if active window is the quake terminal - if so, don't capture it
    let class_output = Command::new("kdotool")
        .args(["getactivewindow", "getwindowclassname"])
        .output();

    if let Ok(output) = class_output {
        if output.status.success() {
            let class_name = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if class_name.eq_ignore_ascii_case(exclude_class) {
                return String::new();
            }
        }
    }

    // Get the window ID (UUID)
    let id_output = Command::new("kdotool")
        .arg("getactivewindow")
        .output();

    match id_output {
        Ok(output) if output.status.success() => {
            String::from_utf8_lossy(&output.stdout).trim().to_string()
        }
        _ => String::new(),
    }
}

pub async fn toggle_quake(config: &Config) -> Result<()> {
    // Acquire lock (ASYNC)
    let mut state = STATE.lock().await;

    // 0. Ensure Terminal
    if crate::terminal::ensure_terminal_running(config).await {
        state.target_visible = false;
    }

    let conn = Connection::session().await?;
    let proxy = zbus::Proxy::new(&conn, "org.kde.KWin", "/Scripting", "org.kde.kwin.Scripting").await?;

    // 1. Unload previous script if exists
    if !state.last_script_id.is_empty() {
         let _ = proxy.call_method("unloadScript", &(state.last_script_id.as_str())).await;
    }

    // 2. Toggle state
    state.target_visible = !state.target_visible;
    let visible = state.target_visible;
    let keep_above = config.window.keep_above;

    // Choose params based on state
    let duration = if visible { config.animation.show_duration } else { config.animation.hide_duration };
    let easing = if visible { &config.animation.show_easing } else { &config.animation.hide_easing };
    let opacity_point = if visible { config.animation.show_opacity_point } else { config.animation.hide_opacity_point };

    let prev_window_id_to_pass: String;
    if visible {
        // Capture the current active window ID before we show the terminal
        state.previous_window_id = get_active_window_id(&config.general.window_class);
        prev_window_id_to_pass = String::new(); // Don't restore focus when showing
    } else {
        prev_window_id_to_pass = state.previous_window_id.clone(); // Pass the captured window for focus restoration
    }

    let script = KWIN_SCRIPT_TEMPLATE
        .replace("__WINDOW_CLASS__", &config.general.window_class)
        .replace("__DISPLAY_MODE__", &config.window.display_mode)
        .replace("__DISPLAY_INDEX__", &config.window.display_index.to_string())
        .replace("__WIDTH_PERCENT__", &config.window.width_percent.to_string())
        .replace("__HEIGHT_PERCENT__", &config.window.height_percent.to_string())
        .replace("__WIDTH_COLS__", &config.window.width_cols.to_string())
        .replace("__HEIGHT_ROWS__", &config.window.height_rows.to_string())
        .replace("__DURATION__", &duration.to_string())
        .replace("__EASING__", easing)
        .replace("__SHOULD_SHOW__", &visible.to_string())
        .replace("__KEEP_ABOVE__", &keep_above.to_string())
        .replace("__ANIMATE_OPACITY__", &config.animation.animate_opacity.to_string())
        .replace("__OPACITY_POINT__", &opacity_point.to_string())
        .replace("__PREV_WINDOW_ID__", &prev_window_id_to_pass);

    let unique_name = format!("goake_toggle_{}", SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos());

    state.last_script_id = unique_name.clone();

    // Create temp file
    let tmp_path = std::env::temp_dir().join(format!("{}.js", unique_name));
    fs::write(&tmp_path, script).expect("Failed to write tmp script");

    let tmp_path_str = tmp_path.to_string_lossy().to_string();

    let reply = proxy.call_method("loadScript", &(tmp_path_str, unique_name.as_str())).await?;
    let script_id: i32 = reply.body().deserialize()?;

    if script_id >= 0 {
        let script_obj_path = format!("/Scripting/Script{}", script_id);
        let script_proxy = zbus::Proxy::new(&conn, "org.kde.KWin", script_obj_path, "org.kde.kwin.Script").await?;
        script_proxy.call_method("run", &()).await?;

        // Spawn async task to unload later
        let duration_ms = duration as u64;
        let name_clone = unique_name.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(duration_ms + 100)).await;
            if let Ok(conn2) = Connection::session().await {
                 if let Ok(proxy2) = zbus::Proxy::new(&conn2, "org.kde.KWin", "/Scripting", "org.kde.kwin.Scripting").await {
                      let _ = proxy2.call_method("unloadScript", &(name_clone)).await;
                 }
            }
            let _ = fs::remove_file(tmp_path);
        });
    }

    Ok(())
}

pub async fn ensure_grabbed(config: &Config) -> Result<()> {
    let conn = Connection::session().await?;
    let proxy = zbus::Proxy::new(&conn, "org.kde.KWin", "/Scripting", "org.kde.kwin.Scripting").await?;

    let script = INIT_SCRIPT_TEMPLATE
        .replace("__WINDOW_CLASS__", &config.general.window_class)
        .replace("__DISPLAY_MODE__", &config.window.display_mode)
        .replace("__DISPLAY_INDEX__", &config.window.display_index.to_string())
        .replace("__WIDTH_PERCENT__", &config.window.width_percent.to_string())
        .replace("__HEIGHT_PERCENT__", &config.window.height_percent.to_string())
        .replace("__WIDTH_COLS__", &config.window.width_cols.to_string())
        .replace("__HEIGHT_ROWS__", &config.window.height_rows.to_string())
        .replace("__KEEP_ABOVE__", &config.window.keep_above.to_string());

    let unique_name = "quake_init";
    let tmp_path = std::env::temp_dir().join("quake_init.js");
    fs::write(&tmp_path, script).expect("Failed to write init script");
    let tmp_path_str = tmp_path.to_string_lossy().to_string();

    let reply = proxy.call_method("loadScript", &(tmp_path_str, unique_name)).await?;
    let script_id: i32 = reply.body().deserialize()?;

    if script_id >= 0 {
        let script_obj_path = format!("/Scripting/Script{}", script_id);
        let script_proxy = zbus::Proxy::new(&conn, "org.kde.KWin", script_obj_path, "org.kde.kwin.Script").await?;
        script_proxy.call_method("run", &()).await?;

        tokio::time::sleep(Duration::from_millis(100)).await;
        let _ = script_proxy.call_method("stop", &()).await;
        let _ = proxy.call_method("unloadScript", &(unique_name)).await;
        let _ = fs::remove_file(tmp_path);
    }
    Ok(())
}

pub async fn restore_quake(config: &Config) -> Result<()> {
    let conn = Connection::session().await?;
    let proxy = zbus::Proxy::new(&conn, "org.kde.KWin", "/Scripting", "org.kde.kwin.Scripting").await?;

    let script = RESTORE_SCRIPT.replace("__WINDOW_CLASS__", &config.general.window_class);
    let unique_name = format!("goake_restore_{}", SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos());
    let tmp_path = std::env::temp_dir().join(format!("{}.js", unique_name));
    fs::write(&tmp_path, script).expect("Failed to write restore script");
    let tmp_path_str = tmp_path.to_string_lossy().to_string();

    let reply = proxy.call_method("loadScript", &(tmp_path_str, unique_name.as_str())).await?;
    let script_id: i32 = reply.body().deserialize()?;

    if script_id >= 0 {
        let script_obj_path = format!("/Scripting/Script{}", script_id);
        let script_proxy = zbus::Proxy::new(&conn, "org.kde.KWin", script_obj_path, "org.kde.kwin.Script").await?;
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
