use crate::config::{Config, AppConfig};
use std::fs;
use std::process::Command;
use tokio::sync::Mutex;
use tokio::time::{sleep, Duration};

use zbus::{Connection, Result};

// Global state
struct KWinState {
    visible_app: Option<String>,
    previous_window_id: String, // Last window active before ANY quake window was shown
}

static STATE: Mutex<KWinState> = Mutex::const_new(KWinState {
    visible_app: None,
    previous_window_id: String::new(),
});

// Template bodies that take arguments in their IIFE
const TOGGLE_SCRIPT_TEMPLATE: &str = r#"
(function(
    windowClass, displayMode, displayIndex, width, height,
    duration, easingType, shouldShow, keepAbove, animateOpacity,
    opacityPoint, prevWindowId, targetWindowId, targetPid, ruakeClasses
) {
    var clients = workspace.windowList ? workspace.windowList() : workspace.clientList();
    var target = null;
    var bestScore = -1;
    var siblingsToHide = [];

    var safeTargetId = targetWindowId ? targetWindowId.toString() : "";
    var safeTargetPid = targetPid || 0;
    var rawClasses = (ruakeClasses || "").toLowerCase().split(",");
    var allClasses = [];
    for (var k = 0; k < rawClasses.length; k++) {
        var trimmed = rawClasses[k].replace(/^\s+|\s+$/g, "");
        if (trimmed) allClasses.push(trimmed);
    }

    function normalizeId(id) {
        if (!id) return "";
        return id.toString().replace(/[{}]/g, "");
    }

    var cleanTargetId = normalizeId(safeTargetId);


    // 1. Identification
    for (var i = 0; i < clients.length; i++) {
        var c = clients[i];
        var cClass = (c.resourceClass || "").toLowerCase();
        var cName = (c.resourceName || "").toLowerCase();

        // Match siblings for cross-sliding
        var isRuake = false;
        for (var j = 0; j < allClasses.length; j++) {
            var siblingClass = allClasses[j];
            if (cClass.indexOf(siblingClass) !== -1 || cName.indexOf(siblingClass) !== -1) {
                isRuake = true;
                break;
            }
        }

        // Identify Target
        var score = -1;
        if (cleanTargetId !== "" && c.internalId && normalizeId(c.internalId) === cleanTargetId) {
            score = 1000;
        } else if (safeTargetPid > 0 && c.pid == safeTargetPid) {
            score = 500;
        } else if (cClass.indexOf(windowClass.toLowerCase()) !== -1) {
            score = 100;
        }

        if (score > 0) {
            if (c.normalWindow) score += 2000;
            if (c.caption && c.caption.length > 0) score += 10;
            if (score > bestScore) {
                bestScore = score;
                target = c;
            }
        } else if (isRuake && shouldShow) {
            // It's a sibling that should slide UP while target slides DOWN
            var area = workspace.clientArea(KWin.PlacementArea, c);
            if (c.opacity > 0 && c.frameGeometry.y + c.frameGeometry.height > area.y) {
                siblingsToHide.push(c);
            }
        }
    }

    if (!target) return;

    function getEasing(progress, type) {
      if (type === "windows") {
          // Cubic Bezier solver for (0.25, 0, 0, 1)
          var t = progress;
          for (var i = 0; i < 5; i++) {
              var xt = 3 * (1 - t) * (1 - t) * t * 0.25 + t * t * t;
              var dxt = 3 * (1 - t) * (1 - t) * 0.25 + 6 * (1 - t) * t * (0.5 - 0.25) + 3 * t * t;
              t -= (xt - progress) / dxt;
          }
          return 3 * (1 - t) * (1 - t) * t * 0 + 3 * (1 - t) * t * t * 1 + t * t * t;
      }
      switch (type) {
        case "linear": return progress;
        case "ease-in": return progress * progress;
        case "ease-out": return progress * (2 - progress);
        case "ease":
        case "ease-in-out":
          return progress < .5 ? 2 * progress * progress : -1 + (4 - 2 * progress) * progress;
        case "quart-in": case "ease-in-quart": return Math.pow(progress, 4);
        case "quart-out": case "ease-out-quart": return 1 - Math.pow(1 - progress, 4);
        case "quart":
        case "quart-in-out":
        case "ease-in-out-quart":
          return progress < 0.5 ? 8 * Math.pow(progress, 4) : 1 - Math.pow(-2 * progress + 2, 4) / 2;
        case "cubic-in":
        case "ease-in-cubic":
          return Math.pow(progress, 3);
        case "cubic-out": case "ease-out-cubic": return 1 - Math.pow(1 - progress, 3);
        case "cubic":
        case "cubic-in-out":
        case "ease-in-out-cubic":
          return progress < 0.5 ? 4 * Math.pow(progress, 3) : 1 - Math.pow(-2 * progress + 2, 3) / 2;
        case "sine-in":
        case "ease-in-sine":
          return 1 - Math.cos((progress * Math.PI) / 2);
        case "sine-out": case "ease-out-sine": return Math.sin((progress * Math.PI) / 2);
        case "sine":
        case "sine-in-out":
        case "ease-in-out-sine":
          return -(Math.cos(Math.PI * progress) - 1) / 2;
        case "back-in": case "ease-in-back":
          var c1 = 1.70158; var c3 = c1 + 1;
          return c3 * progress * progress * progress - c1 * progress * progress;
        case "back-out": case "ease-out-back":
          var c1 = 1.70158; var c3 = c1 + 1;
          return 1 + c3 * Math.pow(progress - 1, 3) + c1 * Math.pow(progress - 1, 2);
        case "back":
        case "back-in-out": case "ease-in-out-back":
          var c1 = 1.70158; var c2 = c1 * 1.525;
          return progress < 0.5
            ? (Math.pow(2 * progress, 2) * ((c2 + 1) * 2 * progress - c2)) / 2
            : (Math.pow(2 * progress - 2, 2) * ((c2 + 1) * (progress * 2 - 2) + c2) + 2) / 2;
        default: return progress * (2 - progress);
      }
    }

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
    var area = shouldShow ? targetArea : currentArea;

    var startY = target.frameGeometry.y;
    var startOpacity = target.opacity;
    var areaTop = area.y;
    var isMostlyHidden = (startY + target.frameGeometry.height <= areaTop + 50) || (target.opacity < 0.1);
    var needsReposition = isMostlyHidden || (startY > areaTop + 50 || startY < areaTop - 50);

    var finalWidth = target.frameGeometry.width;
    var finalHeight = target.frameGeometry.height;

    if (needsReposition) {
        if (width > 0) {
            if (width <= 1.0) finalWidth = area.width * width;
            else finalWidth = width;
        }
        if (height > 0) {
            if (height <= 1.0) finalHeight = area.height * height;
            else finalHeight = height;
        }
    }

    var finalX = area.x + (area.width - finalWidth) / 2;
    var finalY = area.y;

    target.keepAbove = keepAbove;
    target.onAllDesktops = true;
    target.noBorder = true;
    target.skipTaskbar = true;
    target.skipPager = true;
    if (target.skipSwitcher !== undefined) target.skipSwitcher = true;

    if (shouldShow) {
        target.fullScreen = true;
        if (workspace.activeWindow !== undefined) workspace.activeWindow = target;
        else workspace.activeClient = target;

        if (needsReposition) {
             startY = areaTop - finalHeight;
             if (animateOpacity) startOpacity = 0.0;
             else startOpacity = 1.0;
             target.opacity = startOpacity;
             target.frameGeometry = { x: finalX, y: startY, width: finalWidth, height: finalHeight };
        }

        if (duration > 0) {
          var startTime = new Date().getTime();
          var diff = finalY - startY;
          var siblingDatas = [];
          for (var s = 0; s < siblingsToHide.length; s++) {
              var sib = siblingsToHide[s];
              siblingDatas.push({
                  client: sib,
                  startY: sib.frameGeometry.y,
                  startOpacity: sib.opacity,
                  endY: area.y - sib.frameGeometry.height
              });
          }

          var timer = new QTimer();
          timer.interval = 16;
          timer.timeout.connect(function() {
            var now = new Date().getTime();
            var elapsed = now - startTime;
            var progress = Math.min(elapsed / duration, 1.0);
            var ease = getEasing(progress, easingType);

            var currentY = startY + diff * ease;
            if (animateOpacity) {
                target.opacity = Math.max(target.opacity, startOpacity + (1.0 - startOpacity) * progress);
            } else {
                target.opacity = 1.0;
            }
            target.frameGeometry = { x: finalX, y: currentY, width: finalWidth, height: finalHeight };

            for (var d = 0; d < siblingDatas.length; d++) {
                var data = siblingDatas[d];
                var sibY = data.startY + (data.endY - data.startY) * ease;
                data.client.frameGeometry = { x: data.client.frameGeometry.x, y: sibY, width: data.client.frameGeometry.width, height: data.client.frameGeometry.height };
                if (animateOpacity) data.client.opacity = Math.max(0, data.startOpacity * (1.0 - progress));
            }

            if (progress >= 1.0) {
              timer.stop();
              target.opacity = 1.0;
              target.frameGeometry = { x: finalX, y: finalY, width: finalWidth, height: finalHeight };
              if (workspace.activeWindow !== undefined) workspace.activeWindow = target;
              else workspace.activeClient = target;
              for (var d = 0; d < siblingDatas.length; d++) {
                  var data = siblingDatas[d];
                  data.client.opacity = 0.0;
                  data.client.frameGeometry = { x: data.client.frameGeometry.x, y: area.y - data.client.frameGeometry.height, width: data.client.frameGeometry.width, height: data.client.frameGeometry.height };
              }
            }
          });
          timer.start();
        } else {
          target.opacity = 1.0;
          target.frameGeometry = { x: finalX, y: finalY, width: finalWidth, height: finalHeight };
        }
    } else {
        var endY = area.y - finalHeight;
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
               target.opacity = Math.min(target.opacity, startOpacity * (1.0 - progress));
            }

            target.frameGeometry = { x: finalX, y: currentY, width: finalWidth, height: finalHeight };

            if (progress >= 1.0) {
              timer.stop();
              target.opacity = 0.0;
              target.frameGeometry = { x: finalX, y: endY, width: finalWidth, height: finalHeight };
              target.fullScreen = false;
              target.keepAbove = keepAbove;
              target.skipTaskbar = true;
              target.skipPager = true;
              if (target.skipSwitcher !== undefined) target.skipSwitcher = true;

              if (prevWindowId && prevWindowId !== "") {
                var allClients = workspace.windowList ? workspace.windowList() : workspace.clientList();
                for (var j = 0; j < allClients.length; j++) {
                  var c = allClients[j];
                  if (c.internalId && normalizeId(c.internalId) === normalizeId(prevWindowId)) {
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
          target.opacity = 0.0;
          target.frameGeometry = { x: finalX, y: endY, width: finalWidth, height: finalHeight };
          target.fullScreen = false;
          target.keepAbove = keepAbove;
          target.skipTaskbar = true;
          target.skipPager = true;
          if (target.skipSwitcher !== undefined) target.skipSwitcher = true;
        }
    }
})"#;

const ENSURE_GRABBED_TEMPLATE: &str = r#"
(function(
    windowClass, displayMode, displayIndex, width, height,
    keepAbove, targetWindowId, targetPid
){
    var clients = workspace.windowList ? workspace.windowList() : workspace.clientList();
    var target = null;
    var bestScore = -1;

    function normalizeId(id) {
        if (!id) return "";
        return id.toString().replace(/[{}]/g, "");
    }
    var cleanTargetId = normalizeId(targetWindowId);

    for (var i = 0; i < clients.length; i++) {
        var c = clients[i];
        var score = 0;
        var cClass = (c.resourceClass || "").toLowerCase();
        var cName = (c.resourceName || "").toLowerCase();

        if (cleanTargetId !== "" && c.internalId && normalizeId(c.internalId) === cleanTargetId) score += 1000;
        if (targetPid > 0 && c.pid == targetPid) score += 500;
        if (cClass.indexOf(windowClass.toLowerCase()) !== -1 || cName.indexOf(windowClass.toLowerCase()) !== -1) score += 100;

        if (score > 0) {
            if (c.normalWindow) score += 2000;
            if (score > bestScore) {
                bestScore = score;
                target = c;
            }
        }
    }

    if (target) {
      target.onAllDesktops = true;
      target.keepAbove = keepAbove;
      target.noBorder = true;
      target.skipTaskbar = true;
      target.skipPager = true;
      if (target.skipSwitcher !== undefined) target.skipSwitcher = true;

      var screens = workspace.screens;
      var area = null;
      if (displayMode === "specific" && displayIndex >= 0 && displayIndex < screens.length) area = screens[displayIndex].geometry;
      else if (displayMode === "active") area = (workspace.activeWindow ? workspace.clientArea(KWin.PlacementArea, workspace.activeWindow) : workspace.activeScreen.geometry);
      else area = workspace.activeScreen.geometry;

      var finalWidth = width > 0 ? (width <= 1.0 ? area.width * width : width) : target.frameGeometry.width;
      var finalHeight = height > 0 ? (height <= 1.0 ? area.height * height : height) : target.frameGeometry.height;
      var finalX = area.x + (area.width - finalWidth) / 2;
      var finalY = area.y;

      var isHiddenOffscreen = (target.frameGeometry.y + target.frameGeometry.height <= area.y + 50);
      var isVisibleQuake = (target.frameGeometry.y >= area.y - 10 && target.frameGeometry.y <= area.y + 100);

      if (!isHiddenOffscreen && !isVisibleQuake) {
          target.opacity = 0.0;
          target.frameGeometry = { x: finalX, y: area.y - finalHeight, width: finalWidth, height: finalHeight };
      }
    }
})"#;

const RESTORE_TEMPLATE: &str = r#"
(function(windowClass) {
    var clients = workspace.windowList ? workspace.windowList() : workspace.clientList();
    for (var i = 0; i < clients.length; i++) {
      var c = clients[i];
      var cClass = (c.resourceClass || "").toLowerCase();
      if (cClass.indexOf(windowClass.toLowerCase()) !== -1) {
          c.keepAbove = false;
          c.onAllDesktops = false;
          c.noBorder = false;
          c.skipTaskbar = false;
          c.skipPager = false;
          c.opacity = 1.0;

          var area = workspace.clientArea(KWin.PlacementArea, c);
          var geo = c.frameGeometry;

          // If it was hidden offscreen, move it to center of screen
          if (geo.y + geo.height <= area.y + 50) {
            c.frameGeometry = {
              x: area.x + (area.width - geo.width) / 2,
              y: area.y + 100,
              width: geo.width,
              height: geo.height
            };
          }
      }
    }
})"#;



fn update_focus_state(state: &mut KWinState, ruake_classes: &[String]) {
    let id_output = Command::new("kdotool").arg("getactivewindow").output();
    let current_id = match id_output {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).trim().to_string(),
        _ => return,
    };
    if current_id.is_empty() { return; }

    let class_output = Command::new("kdotool").args(["getwindowclassname", &current_id]).output();
    match class_output {
        Ok(o) if o.status.success() => {
            let class_name = String::from_utf8_lossy(&o.stdout).trim().to_string();
            let class_lower = class_name.to_lowercase();
            for ruake_class in ruake_classes {
                if class_lower.contains(&ruake_class.to_lowercase()) { return; }
            }
            state.previous_window_id = current_id;
        },
        _ => {}
    }
}

fn get_window_id_and_pid(class: &str) -> Option<(String, u32)> {
    if let Some(id) = crate::linux::terminal::check_window_exists(class) {
        if let Ok(pid_out) = Command::new("kdotool").args(["getwindowpid", &id]).output() {
            if pid_out.status.success() {
                let pid_str = String::from_utf8_lossy(&pid_out.stdout).trim().to_string();
                if let Ok(pid) = pid_str.parse::<u32>() { return Some((id, pid)); }
            }
        }
        return Some((id, 0));
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
        let _ = crate::linux::terminal::ensure_terminal_running(app_cfg, config, conn).await;
        if state.visible_app.is_none() {
            update_focus_state(&mut state, &ruake_classes);
        }
        let (target_id, target_pid) = get_window_id_and_pid(&app_cfg.window_class).unwrap_or((String::new(), 0));

        run_toggle_script(app_cfg, config, conn, true, "", &target_id, target_pid, &classes_string).await?;
        state.visible_app = Some(app_name.to_string());
    } else {
        let (target_id, target_pid) = get_window_id_and_pid(&app_cfg.window_class).unwrap_or((String::new(), 0));

        let prev_id = state.previous_window_id.clone();
        run_toggle_script(app_cfg, config, conn, false, &prev_id, &target_id, target_pid, &classes_string).await?;
        state.visible_app = None;
    }
    Ok(())
}

async fn run_toggle_script(app_cfg: &AppConfig, config: &Config, conn: &Connection, visible: bool, prev_id: &str, target_id: &str, target_pid: u32, ruake_classes: &str) -> Result<()> {
    let scripting_proxy = zbus::Proxy::new(conn, "org.kde.KWin", "/Scripting", "org.kde.kwin.Scripting").await?;
    let script_name = "ruake_toggle_engine";
    let _ = scripting_proxy.call_method("unloadScript", &(script_name)).await;

    let duration = if visible { config.animation.show_duration } else { config.animation.hide_duration };
    let (width, height) = app_cfg.resolve_dimensions(&config.window);
    let animate_opacity = app_cfg.get_animate_opacity(config.animation.animate_opacity);

    let tmp_path = std::env::temp_dir().join(format!("{}.js", script_name));
    {
        use std::io::Write;
        let file = std::fs::File::create(&tmp_path).expect("Failed to create tmp script");
        let mut writer = std::io::BufWriter::new(file);
        writer.write_all(TOGGLE_SCRIPT_TEMPLATE.as_bytes()).unwrap();
        writeln!(writer, "(\n  \"{}\", \"{}\", {}, {}, {},", app_cfg.window_class, config.window.display_mode, config.window.display_index, width, height).unwrap();
        let easing = if visible { &config.animation.show_easing } else { &config.animation.hide_easing };
        writeln!(writer, "  {}, \"{}\", {}, {}, {},", duration, easing, visible, config.window.keep_above, animate_opacity).unwrap();
        let opacity_point = if visible { config.animation.show_opacity_point } else { config.animation.hide_opacity_point };
        writeln!(writer, "  {}, \"{}\", \"{}\", {}, \"{}\"\n);", opacity_point, prev_id, target_id, target_pid, ruake_classes).unwrap();
    }

    let tmp_path_str = tmp_path.to_string_lossy().to_string();
    let reply = scripting_proxy.call_method("loadScript", &(tmp_path_str, script_name)).await?;
    let script_id: i32 = reply.body().deserialize()?;
    if script_id >= 0 {
        let script_obj_path = format!("/Scripting/Script{}", script_id);
        let script_proxy = zbus::Proxy::new(conn, "org.kde.KWin", script_obj_path, "org.kde.kwin.Script").await?;
        script_proxy.call_method("run", &()).await?;
        let _ = fs::remove_file(tmp_path);
    }
    Ok(())
}

pub async fn ensure_grabbed(app_cfg: &AppConfig, config: &Config, conn: &Connection) -> Result<()> {
    let (target_id, target_pid) = get_window_id_and_pid(&app_cfg.window_class).unwrap_or((String::new(), 0));
    let scripting_proxy = zbus::Proxy::new(conn, "org.kde.KWin", "/Scripting", "org.kde.kwin.Scripting").await?;
    let script_name = "ruake_init_script";
    let _ = scripting_proxy.call_method("unloadScript", &(script_name)).await;

    let (width, height) = app_cfg.resolve_dimensions(&config.window);
    let tmp_path = std::env::temp_dir().join(format!("{}.js", script_name));
    {
        use std::io::Write;
        let file = std::fs::File::create(&tmp_path).expect("Failed to create init script");
        let mut writer = std::io::BufWriter::new(file);
        writer.write_all(ENSURE_GRABBED_TEMPLATE.as_bytes()).unwrap();
        writeln!(writer, "(\n  \"{}\", \"{}\", {}, {}, {},", app_cfg.window_class, config.window.display_mode, config.window.display_index, width, height).unwrap();
        writeln!(writer, "  {}, \"{}\", {}\n);", config.window.keep_above, target_id, target_pid).unwrap();
    }
    let tmp_path_str = tmp_path.to_string_lossy().to_string();
    let reply = scripting_proxy.call_method("loadScript", &(tmp_path_str, script_name)).await?;
    let script_id: i32 = reply.body().deserialize()?;
    if script_id >= 0 {
        let script_obj_path = format!("/Scripting/Script{}", script_id);
        let script_proxy = zbus::Proxy::new(conn, "org.kde.KWin", script_obj_path, "org.kde.kwin.Script").await?;
        script_proxy.call_method("run", &()).await?;
        let _ = scripting_proxy.call_method("unloadScript", &(script_name)).await;
        let _ = fs::remove_file(tmp_path);
    }
    Ok(())
}

pub async fn restore_app(_app_name: &str, window_class: &str, conn: &Connection) -> Result<()> {
    let scripting_proxy = zbus::Proxy::new(conn, "org.kde.KWin", "/Scripting", "org.kde.kwin.Scripting").await?;
    let script_name = "ruake_restore_script";
    let _ = scripting_proxy.call_method("unloadScript", &(script_name)).await;

    let tmp_path = std::env::temp_dir().join(format!("{}.js", script_name));
    {
        use std::io::Write;
        let file = std::fs::File::create(&tmp_path).expect("Failed to create restore script");
        let mut writer = std::io::BufWriter::new(file);
        writer.write_all(RESTORE_TEMPLATE.as_bytes()).unwrap();
        writeln!(writer, "(\"{}\");", window_class).unwrap();
    }
    let tmp_path_str = tmp_path.to_string_lossy().to_string();
    let reply = scripting_proxy.call_method("loadScript", &(tmp_path_str, script_name)).await?;
    let script_id: i32 = reply.body().deserialize()?;
    if script_id >= 0 {
        let script_obj_path = format!("/Scripting/Script{}", script_id);
        let script_proxy = zbus::Proxy::new(conn, "org.kde.KWin", script_obj_path, "org.kde.kwin.Script").await?;
        script_proxy.call_method("run", &()).await?;
        sleep(Duration::from_millis(300)).await;
        let _ = scripting_proxy.call_method("unloadScript", &(script_name)).await;
        let _ = fs::remove_file(tmp_path);
    }
    Ok(())
}

pub async fn restore_quake(config: &Config, conn: &Connection) -> Result<()> {
    for app_cfg in config.app.values() {
        let _ = restore_app("", &app_cfg.window_class, conn).await;
    }
    Ok(())
}

pub async fn reset_visibility() {
    let mut state = STATE.lock().await;
    state.visible_app = None;
}
