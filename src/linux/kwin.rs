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

    // Optimized script using format! to minimize string allocations
    let script = format!(
        r#"
// Compatibility
var clients = workspace.windowList ? workspace.windowList() : workspace.clientList();
var target = null;
var windowClass = "{window_class}";

for (var i = 0; i < clients.length; i++) {{
  var c = clients[i];
  var match = false;
  if (c.resourceClass && c.resourceClass.toLowerCase() == windowClass.toLowerCase()) match = true;
  else if (c.resourceName && c.resourceName.toLowerCase() == windowClass.toLowerCase()) match = true;
  else if (c.caption && c.caption.toLowerCase().indexOf(windowClass.toLowerCase()) !== -1) match = true;
  if (match) {{ target = c; break; }}
}}

function getEasing(progress, type) {{
  switch (type) {{
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
      return (function(x, x1, y1, x2, y2) {{
        if (x <= 0) return 0; if (x >= 1) return 1;
        var t = x;
        for (var i = 0; i < 8; i++) {{
          var x_t = 3 * Math.pow(1 - t, 2) * t * x1 + 3 * (1 - t) * Math.pow(t, 2) * x2 + Math.pow(t, 3);
          var dx_t = 3 * (1 - 4 * t + 3 * t * t) * x1 + 3 * (2 * t - 3 * t * t) * x2 + 3 * t * t;
          if (Math.abs(dx_t) < 1e-6) break;
          t -= (x_t - x) / dx_t;
        }}
        return 3 * Math.pow(1 - t, 2) * t * y1 + 3 * (1 - t) * Math.pow(t, 2) * y2 + Math.pow(t, 3);
      }})(progress, 0.25, 0, 0, 1);
    default: return progress * (2 - progress);
  }}
}}

if (target) {{
  var displayMode = "{display_mode}";
  var displayIndex = {display_index};
  var widthPct = {width_percent} / 100.0;
  var heightPct = {height_percent} / 100.0;
  var widthCols = {width_cols};
  var heightRows = {height_rows};
  var duration = {duration};
  var easingType = "{easing}";
  var shouldShow = {should_show};
  var keepAbove = {keep_above};
  var animateOpacity = {animate_opacity};
  var opacityPoint = {opacity_point};
  var prevWindowId = "{prev_window_id}";

  var screens = workspace.screens;
  var targetArea = null;

  if (displayMode === "specific" && displayIndex >= 0 && displayIndex < screens.length) {{
    targetArea = screens[displayIndex].geometry;
  }} else if (displayMode === "active") {{
    var activeWin = workspace.activeWindow !== undefined ? workspace.activeWindow : workspace.activeClient;
    if (activeWin && activeWin !== target) {{
      targetArea = workspace.clientArea(KWin.PlacementArea, activeWin);
    }} else {{
      targetArea = workspace.activeScreen.geometry;
    }}
  }} else {{
    var cursorPos = workspace.cursorPos;
    for (var i = 0; i < screens.length; i++) {{
      var geo = screens[i].geometry;
      if (cursorPos.x >= geo.x && cursorPos.x < geo.x + geo.width &&
        cursorPos.y >= geo.y && cursorPos.y < geo.y + geo.height) {{
        targetArea = geo;
        break;
      }}
    }}
    if (!targetArea) targetArea = workspace.activeScreen.geometry;
  }}

  var currentArea = workspace.clientArea(KWin.PlacementArea, target);
  var isMostlyHidden = target.minimized || (target.frameGeometry.y + target.frameGeometry.height <= currentArea.y + 5);
  var area = shouldShow ? targetArea : currentArea;
  var needsReposition = shouldShow && (isMostlyHidden || currentArea.x != targetArea.x || currentArea.y != targetArea.y);

  var finalWidth = ( widthCols > 0 ) ? target.frameGeometry.width : area.width * widthPct;
  var finalHeight = ( heightRows > 0 ) ? target.frameGeometry.height : area.height * heightPct;
  var finalX = area.x + (area.width - finalWidth) / 2;
  var finalY = area.y;

  target.keepAbove = keepAbove;
  target.onAllDesktops = true;
  target.noBorder = true;
  target.skipTaskbar = true;
  target.skipPager = true;

  if (shouldShow) {{
    var startY = target.frameGeometry.y;
    var startOpacity = animateOpacity ? 0.0 : 1.0;

    if (needsReposition) {{
      startY = finalY - finalHeight;
      target.opacity = 0.0;
      target.frameGeometry = {{ x: finalX, y: startY, width: finalWidth, height: finalHeight }};
    }}

    if (target.minimized) target.minimized = false;
    if (workspace.activeWindow !== undefined) workspace.activeWindow = target;
    else workspace.activeClient = target;

    if (!animateOpacity) target.opacity = 1.0;

    if (duration > 0) {{
      var endY = finalY;
      var startTime = new Date().getTime();
      var diff = endY - startY;
      var timer = new QTimer();
      timer.interval = 16;
      timer.timeout.connect(function() {{
        var now = new Date().getTime();
        var elapsed = now - startTime;
        var progress = Math.min(elapsed / duration, 1.0);
        var ease = getEasing(progress, easingType);
        var currentY = startY + diff * ease;

        if (animateOpacity) {{
          var opacityProgress = Math.min(progress / opacityPoint, 1.0);
          target.opacity = startOpacity + (1.0 - startOpacity) * opacityProgress;
        }}

        target.frameGeometry = {{ x: finalX, y: currentY, width: finalWidth, height: finalHeight }};

        if (progress >= 1.0) {{
          timer.stop();
          target.opacity = 1.0;
          target.frameGeometry = {{ x: finalX, y: finalY, width: finalWidth, height: finalHeight }};
        }}
      }});
      timer.start();
    }} else {{
      target.opacity = 1.0;
      target.frameGeometry = {{ x: finalX, y: finalY, width: finalWidth, height: finalHeight }};
    }}

  }} else {{
    var startY = target.frameGeometry.y;
    var startX = target.frameGeometry.x;
    var startW = target.frameGeometry.width;
    var startH = target.frameGeometry.height;
    var endY = area.y - startH;

    if (duration > 0) {{
      var startTime = new Date().getTime();
      var diff = endY - startY;
      var timer = new QTimer();
      timer.interval = 16;
      timer.timeout.connect(function() {{
        var now = new Date().getTime();
        var elapsed = now - startTime;
        var progress = Math.min(elapsed / duration, 1.0);
        var ease = getEasing(progress, easingType);
        var currentY = startY + diff * ease;

        if (animateOpacity) {{
          var opacityProgress = Math.max((progress - opacityPoint) / (1.0 - opacityPoint), 0.0);
          target.opacity = 1.0 - opacityProgress;
        }}

        target.frameGeometry = {{ x: startX, y: currentY, width: startW, height: startH }};

        if (progress >= 1.0) {{
          timer.stop();
          target.opacity = 0.0;
          var hiddenX = targetArea.x + (targetArea.width - startW) / 2;
          var hiddenY = targetArea.y - startH;
          target.frameGeometry = {{ x: hiddenX, y: hiddenY, width: startW, height: startH }};

          if (prevWindowId && prevWindowId !== "") {{
            var allClients = workspace.windowList ? workspace.windowList() : workspace.clientList();
            for (var j = 0; j < allClients.length; j++) {{
              var c = allClients[j];
              if (c.internalId && c.internalId.toString() === prevWindowId) {{
                if (workspace.activeWindow !== undefined) workspace.activeWindow = c;
                else workspace.activeClient = c;
                break;
              }}
            }}
          }}
        }}
      }});
      timer.start();
    }} else {{
      var hiddenX = targetArea.x + (targetArea.width - startW) / 2;
      var hiddenY = targetArea.y - startH;
      target.frameGeometry = {{ x: hiddenX, y: hiddenY, width: startW, height: startH }};
      target.opacity = 0.0;

      if (prevWindowId && prevWindowId !== "") {{
        var allClients = workspace.windowList ? workspace.windowList() : workspace.clientList();
        for (var j = 0; j < allClients.length; j++) {{
          var c = allClients[j];
          if (c.internalId && c.internalId.toString() === prevWindowId) {{
            if (workspace.activeWindow !== undefined) workspace.activeWindow = c;
            else workspace.activeClient = c;
            break;
          }}
        }}
      }}
    }}
  }}
}}
"#,
        window_class = config.general.window_class,
        display_mode = config.window.display_mode,
        display_index = config.window.display_index,
        width_percent = config.window.width_percent,
        height_percent = config.window.height_percent,
        width_cols = config.window.width_cols,
        height_rows = config.window.height_rows,
        duration = duration,
        easing = easing,
        should_show = visible,
        keep_above = keep_above,
        animate_opacity = config.animation.animate_opacity,
        opacity_point = opacity_point,
        prev_window_id = prev_window_id_to_pass
    );

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

    let script = format!(
        r#"
var clients = workspace.windowList ? workspace.windowList() : workspace.clientList();
var target = null;
var windowClass = "{window_class}";

for (var i = 0; i < clients.length; i++) {{
  var c = clients[i];
  var match = false;
  if (c.resourceClass && c.resourceClass.toLowerCase() == windowClass.toLowerCase()) match = true;
  else if (c.resourceName && c.resourceName.toLowerCase() == windowClass.toLowerCase()) match = true;
  else if (c.caption && c.caption.toLowerCase().indexOf(windowClass.toLowerCase()) !== -1) match = true;
  if (match) {{ target = c; break; }}
}}

if (target) {{
  var displayMode = "{display_mode}";
  var displayIndex = {display_index};
  var widthPct = {width_percent} / 100.0;
  var heightPct = {height_percent} / 100.0;
  var widthCols = {width_cols};
  var heightRows = {height_rows};
  var keepAbove = {keep_above};

  var area = null;
  var screens = workspace.screens;
  if (displayMode === "specific" && displayIndex >= 0 && displayIndex < screens.length) {{
    area = screens[displayIndex].geometry;
  }} else if (displayMode === "active") {{
    var activeWin = workspace.activeWindow !== undefined ? workspace.activeWindow : workspace.activeClient;
    if (activeWin && activeWin !== target) {{
      area = workspace.clientArea(KWin.PlacementArea, activeWin);
    }} else {{
      area = workspace.activeScreen.geometry;
    }}
  }} else {{
    var cursorPos = workspace.cursorPos;
    for (var i = 0; i < screens.length; i++) {{
      var geo = screens[i].geometry;
      if (cursorPos.x >= geo.x && cursorPos.x < geo.x + geo.width &&
        cursorPos.y >= geo.y && cursorPos.y < geo.y + geo.height) {{
        area = geo;
        break;
      }}
    }}
    if (!area) area = workspace.activeScreen.geometry;
  }}

  var finalWidth = ( widthCols > 0 ) ? target.frameGeometry.width : area.width * widthPct;
  var finalHeight = ( heightRows > 0 ) ? target.frameGeometry.height : area.height * heightPct;
  var finalX = area.x + (area.width - finalWidth) / 2;
  var finalY = area.y;

  target.keepAbove = keepAbove;
  target.onAllDesktops = true;
  target.noBorder = true;
  target.skipTaskbar = true;

  target.frameGeometry = {{
    x: finalX,
    y: finalY - finalHeight,
    width: finalWidth,
    height: finalHeight
  }};
}}
"#,
        window_class = config.general.window_class,
        display_mode = config.window.display_mode,
        display_index = config.window.display_index,
        width_percent = config.window.width_percent,
        height_percent = config.window.height_percent,
        width_cols = config.window.width_cols,
        height_rows = config.window.height_rows,
        keep_above = config.window.keep_above
    );

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

    let script = format!(
        r#"
var clients = workspace.windowList ? workspace.windowList() : workspace.clientList();
var target = null;
var windowClass = "{window_class}";
for (var i = 0; i < clients.length; i++) {{
  var c = clients[i];
  if ((c.resourceClass && c.resourceClass.toLowerCase() == windowClass.toLowerCase()) ||
    (c.resourceName && c.resourceName.toLowerCase() == windowClass.toLowerCase())) {{
    target = c;
    break;
  }}
}}
if (target) {{
  target.minimized = false;
  target.keepAbove = false;
  target.onAllDesktops = false;
  target.noBorder = false;
  target.opacity = 1.0;

  var geo = target.frameGeometry;
  var area = workspace.clientArea(KWin.PlacementArea, target);

  if (geo.y + geo.height <= area.y + 50) {{
    target.frameGeometry = {{
      x: geo.x,
      y: area.y + 100,
      width: geo.width,
      height: geo.height
    }};
  }}
}}
"#,
        window_class = config.general.window_class
    );

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
