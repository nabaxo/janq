use crate::config::Config;
use std::fs;
use tokio::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use zbus::{Connection, Result};

// Global state
struct KWinState {
    target_visible: bool,
    last_script_id: String,
}

static STATE: Mutex<KWinState> = Mutex::const_new(KWinState {
    target_visible: false,
    last_script_id: String::new(),
});

// Templates (Using placeholders to avoid format!/brace issues)
const TOGGLE_SCRIPT_TEMPLATE: &str = r#"
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
    // New Easings
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

  var cursorPos = workspace.cursorPos;
  var mouseArea = null;
  var screens = workspace.screens;
  for (var i = 0; i < screens.length; i++) {
    var geo = screens[i].geometry;
    if (cursorPos.x >= geo.x && cursorPos.x < geo.x + geo.width &&
      cursorPos.y >= geo.y && cursorPos.y < geo.y + geo.height) {
      mouseArea = geo;
      break;
    }
  }
  if (!mouseArea) {
    mouseArea = workspace.activeScreen.geometry;
  }

  var currentArea = workspace.clientArea(KWin.PlacementArea, target);
  var isMostlyHidden = target.minimized || (target.frameGeometry.y + target.frameGeometry.height <= currentArea.y + 5);
  var area = shouldShow ? mouseArea : currentArea;
  var needsReposition = shouldShow && (isMostlyHidden || currentArea.x != mouseArea.x || currentArea.y != mouseArea.y);

  var finalWidth = ( widthCols > 0 ) ? target.frameGeometry.width : area.width * widthPct;
  var finalHeight = ( heightRows > 0 ) ? target.frameGeometry.height : area.height * heightPct;
  var finalX = area.x + (area.width - finalWidth) / 2;
  var finalY = area.y;

  target.keepAbove = keepAbove;
  target.onAllDesktops = true;
  target.noBorder = true;
  target.skipTaskbar = true;
  target.skipPager = true;

  if (shouldShow) {
    var startY = target.frameGeometry.y;
    var startOpacity = animateOpacity ? 0.0 : 1.0;

    if (needsReposition) {
      startY = finalY - finalHeight;
      target.opacity = 0.0;
      target.frameGeometry = { x: finalX, y: startY, width: finalWidth, height: finalHeight };
    }

    if (target.minimized) target.minimized = false;
    if (workspace.activeWindow !== undefined) workspace.activeWindow = target;
    else workspace.activeClient = target;

    if (!animateOpacity) target.opacity = 1.0;

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
          var opacityProgress = Math.min(progress / opacityPoint, 1.0);
          var currentOpacity = startOpacity + (1.0 - startOpacity) * opacityProgress;
          target.opacity = currentOpacity;
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
    var currentGeo = target.frameGeometry;
    var startY = currentGeo.y;
    var startX = currentGeo.x;
    var startW = currentGeo.width;
    var startH = currentGeo.height;
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
          var opacityProgress = Math.max((progress - opacityPoint) / (1.0 - opacityPoint), 0.0);
          var currentOpacity = 1.0 - opacityProgress;
          target.opacity = currentOpacity;
        }

        target.frameGeometry = { x: startX, y: currentY, width: startW, height: startH };

        if (progress >= 1.0) {
          timer.stop();
          target.opacity = 0.0;
          var hiddenX = mouseArea.x + (mouseArea.width - startW) / 2;
          var hiddenY = mouseArea.y - startH;
          target.frameGeometry = { x: hiddenX, y: hiddenY, width: startW, height: startH };
        }
      });
      timer.start();
    } else {
      var hiddenX = mouseArea.x + (mouseArea.width - startW) / 2;
      var hiddenY = mouseArea.y - startH;
      target.frameGeometry = { x: hiddenX, y: hiddenY, width: startW, height: startH };
      target.opacity = 0.0;
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
  if (match) { target = c; break; }
}

if (target) {
  var displayMode = "__DISPLAY_MODE__";
  var displayIndex = __DISPLAY_INDEX__;
  var widthPct = __WIDTH_PERCENT__ / 100.0;
  var heightPct = __HEIGHT_PERCENT__ / 100.0;
  var widthCols = __WIDTH_COLS__;
  var heightRows = __HEIGHT_ROWS__;
  var keepAbove = __KEEP_ABOVE__;

  var area = workspace.activeScreen.geometry;
  var finalWidth = ( widthCols > 0 ) ? target.frameGeometry.width : area.width * widthPct;
  var finalHeight = ( heightRows > 0 ) ? target.frameGeometry.height : area.height * heightPct;
  var finalX = area.x + (area.width - finalWidth) / 2;
  var finalY = area.y;

  target.keepAbove = keepAbove;
  target.onAllDesktops = true;
  target.noBorder = true;
  target.skipTaskbar = true;
  target.frameGeometry = { x: finalX, y: finalY - finalHeight, width: finalWidth, height: finalHeight };
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
  if (geo.y + geo.height <= area.y + 50) {
    target.frameGeometry = { x: geo.x, y: area.y + 100, width: geo.width, height: geo.height };
  }
}
"#;

pub async fn toggle_quake(config: &Config) -> Result<()> {
    // Acquire lock (ASYNC)
    let mut state = STATE.lock().await;

    // 0. Ensure Terminal
    if crate::terminal::ensure_terminal_running(config).await {
        state.target_visible = false;
    }

    let conn = Connection::session().await?;
    let proxy = zbus::Proxy::new(&conn, "org.kde.KWin", "/Scripting", "org.kde.kwin.Scripting").await?;

    // Unload previous
    if !state.last_script_id.is_empty() {
         let _ = proxy.call_method("unloadScript", &(state.last_script_id.as_str())).await;
    }

    state.target_visible = !state.target_visible;
    let visible = state.target_visible;
    let keep_above = config.keep_above;

    // We hold the lock during D-Bus calls, which is fine for serialization

    let duration = if visible { config.show_duration } else { config.hide_duration };
    let easing = if visible { &config.show_easing } else { &config.hide_easing };
    let opacity_point = if visible { config.show_opacity_point } else { config.hide_opacity_point };

    let script = TOGGLE_SCRIPT_TEMPLATE
        .replace("__WINDOW_CLASS__", &config.window_class)
        .replace("__WIDTH_PERCENT__", &config.width_percent.to_string())
        .replace("__HEIGHT_PERCENT__", &config.height_percent.to_string())
        .replace("__WIDTH_COLS__", &config.width_cols.to_string())
        .replace("__HEIGHT_ROWS__", &config.height_rows.to_string())
        .replace("__DURATION__", &duration.to_string())
        .replace("__EASING__", easing)
        .replace("__SHOULD_SHOW__", &visible.to_string())
        .replace("__KEEP_ABOVE__", &keep_above.to_string())
        .replace("__ANIMATE_OPACITY__", &config.animate_opacity.to_string())
        .replace("__OPACITY_POINT__", &opacity_point.to_string());

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
        .replace("__WINDOW_CLASS__", &config.window_class)
        .replace("__DISPLAY_MODE__", &config.display_mode)
        .replace("__DISPLAY_INDEX__", &config.display_index.to_string())
        .replace("__WIDTH_PERCENT__", &config.width_percent.to_string())
        .replace("__HEIGHT_PERCENT__", &config.height_percent.to_string())
        .replace("__WIDTH_COLS__", &config.width_cols.to_string())
        .replace("__HEIGHT_ROWS__", &config.height_rows.to_string())
        .replace("__KEEP_ABOVE__", &config.keep_above.to_string());

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
        // Unload
        let _ = proxy.call_method("unloadScript", &(unique_name)).await;
        let _ = fs::remove_file(tmp_path);
    }
    Ok(())
}

pub async fn restore_quake(config: &Config) -> Result<()> {
    let conn = Connection::session().await?;
    let proxy = zbus::Proxy::new(&conn, "org.kde.KWin", "/Scripting", "org.kde.kwin.Scripting").await?;

    let script = RESTORE_SCRIPT.replace("__WINDOW_CLASS__", &config.window_class);
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
