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
}

static STATE: Mutex<KWinState> = Mutex::const_new(KWinState {
  visible_app: None,
  previous_window_id: String::new(),
});

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

const COMMON_KWIN_JS: &str = r#"
    function normalizeId(id) {
        if (!id) return "";
        return id.toString().replace(/[{}]/g, "");
    }

    function findTarget(windowClass, targetWindowId, targetPid) {
        var clients = workspace.windowList ? workspace.windowList() : workspace.clientList();
        var target = null;
        var bestScore = -1;
        var cleanTargetId = normalizeId(targetWindowId);
        var safeTargetPid = targetPid || 0;
        var lowerClass = (windowClass || "").toLowerCase();

        for (var i = 0; i < clients.length; i++) {
            var c = clients[i];
            var score = 0;
            var cClass = (c.resourceClass || "").toLowerCase();
            var cName = (c.resourceName || "").toLowerCase();

            if (cleanTargetId !== "" && c.internalId && normalizeId(c.internalId) === cleanTargetId) score += 1000;
            if (safeTargetPid > 0 && c.pid == safeTargetPid) score += 500;
            if (lowerClass && (cClass.indexOf(lowerClass) !== -1 || cName.indexOf(lowerClass) !== -1)) score += 100;
            if (lowerClass && c.desktopFileName && c.desktopFileName.toLowerCase().indexOf(lowerClass) !== -1) score += 50;

            if (score > 0) {
                if (c.normalWindow) score += 2000;
                if (c.caption && c.caption.length > 0) score += 10;
                if (score > bestScore) {
                    bestScore = score;
                    target = c;
                }
            }
        }
        return target;
    }

    function getEasing(progress, type) {
      if (type.indexOf("(") !== -1) {
          var content = "";
          if (type.indexOf("cubic-bezier(") === 0) content = type.substring(13, type.length - 1);
          else if (type.indexOf("bezier(") === 0) content = type.substring(7, type.length - 1);
          else if (type.indexOf("(") === 0) content = type.substring(1, type.length - 1);

          if (content) {
              var parts = content.split(",").map(function(p) { return parseFloat(p.trim()); });
              if (parts.length === 4 && !parts.some(isNaN)) {
                  var t = progress;
                  for (var i = 0; i < 8; i++) {
                      var xt = 3 * (1 - t) * (1 - t) * t * parts[0] + 3 * (1 - t) * t * t * parts[2] + t * t * t;
                      var dxt = 3 * (1 - 4 * t + 3 * t * t) * parts[0] + 3 * (2 * t - 3 * t * t) * parts[2] + 3 * t * t;
                      if (Math.abs(dxt) < 1e-6) break;
                      t -= (xt - progress) / dxt;
                  }
                  return 3 * (1 - t) * (1 - t) * t * parts[1] + 3 * (1 - t) * t * t * parts[3] + t * t * t;
              }
          }
      }
      if (type === "windows") {
          var t = progress;
          for (var i = 0; i < 8; i++) {
              var xt = 3 * (1 - t) * (1 - t) * t * 0.25 + t * t * t;
              var dxt = 3 * (1 - 4 * t + 3 * t * t) * 0.25 + 3 * t * t;
              if (Math.abs(dxt) < 1e-6) break;
              t -= (xt - progress) / dxt;
          }
          return 3 * (1 - t) * t * t * 1 + t * t * t;
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

    function setQuakeProperties(target, keepAbove, isVisible, forcePriority) {
        target.onAllDesktops = true;
        target.keepAbove = keepAbove;
        target.noBorder = true;
        target.skipTaskbar = true;
        target.skipPager = true;
        if (target.skipSwitcher !== undefined) target.skipSwitcher = true;
        if (forcePriority && !isVisible) target.fullScreen = true;
    }

    function resetQuakeProperties(target) {
        target.keepAbove = false;
        target.onAllDesktops = false;
        target.noBorder = false;
        target.skipTaskbar = false;
        target.skipPager = false;
        if (target.skipSwitcher !== undefined) target.skipSwitcher = false;
        target.fullScreen = false;
        target.opacity = 1.0;
    }

    function focusKick(target, restoreOriginal) {
        var activeWin = (workspace.activeWindow !== undefined ? workspace.activeWindow : workspace.activeClient);
        if (workspace.activeWindow !== undefined) {
            workspace.activeWindow = null;
            workspace.activeWindow = target;
            if (restoreOriginal && activeWin && activeWin !== target) workspace.activeWindow = activeWin;
        } else {
            workspace.activeClient = null;
            workspace.activeClient = target;
            if (restoreOriginal && activeWin && activeWin !== target) workspace.activeClient = activeWin;
        }
    }

    function resolveArea(target, displayMode, displayIndex, currentArea) {
        var screens = workspace.screens || [];
        if (displayMode === "specific" && displayIndex >= 0 && displayIndex < screens.length) {
            return screens[displayIndex].geometry;
        }

        var activeWin = (workspace.activeWindow !== undefined ? workspace.activeWindow : workspace.activeClient);
        var isTargetActive = (activeWin && activeWin.internalId && target.internalId && normalizeId(activeWin.internalId) === normalizeId(target.internalId));

        if (displayMode === "active") {
            if (activeWin && !isTargetActive) {
                return workspace.clientArea(KWin.PlacementArea, activeWin);
            }
            if (currentArea && target.opacity > 0.05 && target.frameGeometry.y + target.frameGeometry.height > currentArea.y + 5) {
                return currentArea;
            }
            return workspace.clientArea(KWin.PlacementArea, workspace.activeScreen, workspace.currentDesktop);
        }

        // follow-mouse
        var cursorPos = workspace.cursorPos;
        for (var i = 0; i < screens.length; i++) {
            var geo = screens[i].geometry;
            if (cursorPos.x >= geo.x && cursorPos.x < geo.x + geo.width &&
                cursorPos.y >= geo.y && cursorPos.y < geo.y + geo.height) {
                return geo;
            }
        }
        return (workspace.activeScreen ? workspace.activeScreen.geometry : null);
    }

    function resolveDimensions(width, isWidthPercent, height, isHeightPercent, area, target) {
        var finalWidth = width > 0 ? (isWidthPercent ? area.width * width : width) : target.frameGeometry.width;
        var finalHeight = height > 0 ? (isHeightPercent ? area.height * height : height) : target.frameGeometry.height;
        return { width: finalWidth, height: finalHeight };
    }
"#;

// Template bodies that take arguments in their IIFE
const TOGGLE_SCRIPT_TEMPLATE: &str = r#"
(function(
    windowClass, displayMode, displayIndex, width, isWidthPercent, height, isHeightPercent,
    duration, easingType, shouldShow, keepAbove, animateOpacity,
    showOpacityPoint, hideOpacityPoint, prevWindowId, targetWindowId, targetPid, janqClasses,
    forcePriority
) {
    {{COMMON_KWIN_JS}}

    var target = findTarget(windowClass, targetWindowId, targetPid);
    if (!target) return;

    var currentArea = workspace.clientArea(KWin.PlacementArea, target);
    var area = shouldShow ? resolveArea(target, displayMode, displayIndex, currentArea) : currentArea;

    var startX = target.frameGeometry.x;
    var startY = target.frameGeometry.y;
    var startOpacity = target.opacity;
    var areaTop = area.y;
    var offscreenY = areaTop - target.frameGeometry.height;

    var onWrongMonitor = (startX < area.x - 10) || (startX > area.x + area.width + 10);
    var needsReposition = onWrongMonitor || (startY < offscreenY - 50) || (startY > areaTop + 50);

    var finalWidth = target.frameGeometry.width;
    var finalHeight = target.frameGeometry.height;

    if (needsReposition) {
        var dims = resolveDimensions(width, isWidthPercent, height, isHeightPercent, area, target);
        finalWidth = dims.width;
        finalHeight = dims.height;
    }

    var finalX = area.x + (area.width - finalWidth) / 2;
    var finalY = area.y;

    var clients = workspace.windowList ? workspace.windowList() : workspace.clientList();
    var rawClasses = (janqClasses || "").toLowerCase().split(",");
    var allClasses = [];
    for (var k = 0; k < rawClasses.length; k++) {
        var trimmed = rawClasses[k].replace(/^\s+|\s+$/g, "");
        if (trimmed) allClasses.push(trimmed);
    }

    var siblingsToHide = [];
    for (var i = 0; i < clients.length; i++) {
        var c = clients[i];
        if (c === target) continue;
        var cClass = (c.resourceClass || "").toLowerCase();
        var cName = (c.resourceName || "").toLowerCase();

        var isManaged = false;
        for (var j = 0; j < allClasses.length; j++) {
            var siblingClass = allClasses[j];
            if (cClass.indexOf(siblingClass) !== -1 || cName.indexOf(siblingClass) !== -1) {
                isManaged = true;
                break;
            }
        }

        if (isManaged) {
            var cArea = workspace.clientArea(KWin.PlacementArea, c);
            if (c.opacity > 0.01 && c.frameGeometry.y + c.frameGeometry.height > cArea.y + 1) {
                siblingsToHide.push(c);
            }
        }
    }

    if (shouldShow) {
        setQuakeProperties(target, keepAbove, true, forcePriority);
        focusKick(target, false);

        function startAnimation() {
            if (duration > 0) {
              var startTime = Date.now();
              var diff = finalY - startY;
              var siblingDatas = [];
              for (var s = 0; s < siblingsToHide.length; s++) {
                  var sib = siblingsToHide[s];
                  var sibArea = workspace.clientArea(KWin.PlacementArea, sib);
                  siblingDatas.push({
                      client: sib,
                      startY: sib.frameGeometry.y,
                      startOpacity: sib.opacity,
                      endY: sibArea.y - sib.frameGeometry.height
                  });
              }

              var firstFrame = true;
              var timer = new QTimer();
              timer.interval = 8;
              timer.timeout.connect(function() {
                var now = Date.now();
                var elapsed = now - startTime;
                var progress = Math.min(elapsed / duration, 1.0);
                var ease = getEasing(progress, easingType);

                if (firstFrame) {
                    focusKick(target, false);
                    if (forcePriority) target.fullScreen = true;
                    if (animateOpacity) target.opacity = 0.0;
                    else target.opacity = 1.0;
                    firstFrame = false;
                }

                var currentY = startY + diff * ease;
                if (animateOpacity) {
                    var opacityEase = Math.min(1.0, Math.max(0, ease / (showOpacityPoint <= 0 ? 0.0001 : showOpacityPoint)));
                    target.opacity = Math.max(target.opacity, startOpacity + (1.0 - startOpacity) * opacityEase);
                } else {
                    target.opacity = 1.0;
                }
                target.frameGeometry = { x: finalX, y: currentY, width: finalWidth, height: finalHeight };

                for (var d = 0; d < siblingDatas.length; d++) {
                    var data = siblingDatas[d];
                    var sibY = data.startY + (data.endY - data.startY) * ease;
                    data.client.frameGeometry = { x: data.client.frameGeometry.x, y: sibY, width: data.client.frameGeometry.width, height: data.client.frameGeometry.height };
                    if (animateOpacity) {
                        var denom = 1.0 - hideOpacityPoint;
                        var opacityEase = Math.min(1.0, Math.max(0, (ease - hideOpacityPoint) / (denom <= 0 ? 0.0001 : denom)));
                        data.client.opacity = Math.min(data.client.opacity, data.startOpacity * (1.0 - opacityEase));
                    }
                }

                if (progress >= 1.0) {
                  timer.stop();
                  target.opacity = 1.0;
                  target.frameGeometry = { x: finalX, y: finalY, width: finalWidth, height: finalHeight };
                  focusKick(target, false);
                  for (var d = 0; d < siblingDatas.length; d++) {
                      var data = siblingDatas[d];
                      data.client.opacity = 0.0;
                      var sibArea = workspace.clientArea(KWin.PlacementArea, data.client);
                      data.client.frameGeometry = { x: data.client.frameGeometry.x, y: sibArea.y - data.client.frameGeometry.height, width: data.client.frameGeometry.width, height: data.client.frameGeometry.height };
                  }
                }
              });
              timer.start();
            } else {
              target.opacity = 1.0;
              target.frameGeometry = { x: finalX, y: finalY, width: finalWidth, height: finalHeight };
            }
        }

        if (needsReposition) {
             target.opacity = 0.0;
             target.fullScreen = false;
             var jumpY = areaTop - finalHeight;
             target.frameGeometry = { x: finalX, y: jumpY, width: finalWidth, height: finalHeight };
             startX = finalX;
             startY = jumpY;
             if (animateOpacity) startOpacity = 0.0;
             else startOpacity = 1.0;
             var delayTimer = new QTimer();
             delayTimer.interval = 200;
             delayTimer.singleShot = true;
             delayTimer.timeout.connect(startAnimation);
             delayTimer.start();
        } else {
             startAnimation();
        }
    } else {
        var endY = area.y - finalHeight;
        var wasActive = (workspace.activeWindow === target || workspace.activeClient === target);

        if (duration > 0) {
          var startTime = Date.now();
          var diff = endY - startY;

          var siblingDatas = [];
          for (var s = 0; s < siblingsToHide.length; s++) {
              var sib = siblingsToHide[s];
              var sibArea = workspace.clientArea(KWin.PlacementArea, sib);
              siblingDatas.push({
                  client: sib,
                  startY: sib.frameGeometry.y,
                  startOpacity: sib.opacity,
                  endY: sibArea.y - sib.frameGeometry.height
              });
          }

          var timer = new QTimer();
          timer.interval = 8;
          timer.timeout.connect(function() {
            var now = Date.now();
            var elapsed = now - startTime;
            var progress = Math.min(elapsed / duration, 1.0);
            var ease = getEasing(progress, easingType);
            var currentY = startY + diff * ease;

            if (animateOpacity) {
               var denom = 1.0 - hideOpacityPoint;
               var opacityEase = Math.min(1.0, Math.max(0, (ease - hideOpacityPoint) / (denom <= 0 ? 0.0001 : denom)));
               target.opacity = Math.min(target.opacity, startOpacity * (1.0 - opacityEase));
            }

            target.frameGeometry = { x: finalX, y: currentY, width: finalWidth, height: finalHeight };

            for (var d = 0; d < siblingDatas.length; d++) {
                var data = siblingDatas[d];
                var sibY = data.startY + (data.endY - data.startY) * ease;
                data.client.frameGeometry = { x: data.client.frameGeometry.x, y: sibY, width: data.client.frameGeometry.width, height: data.client.frameGeometry.height };
                if (animateOpacity) {
                    var denom = 1.0 - hideOpacityPoint;
                    var opacityEase = Math.min(1.0, Math.max(0, (ease - hideOpacityPoint) / (denom <= 0 ? 0.0001 : denom)));
                    data.client.opacity = Math.min(data.client.opacity, data.startOpacity * (1.0 - opacityEase));
                }
            }

            if (progress >= 1.0) {
              timer.stop();
              target.opacity = 0.0;
              target.frameGeometry = { x: finalX, y: endY, width: finalWidth, height: finalHeight };
              target.fullScreen = false;
              if (target.skipSwitcher !== undefined) target.skipSwitcher = true;

              for (var d = 0; d < siblingDatas.length; d++) {
                  var data = siblingDatas[d];
                  data.client.opacity = 0.0;
                  var sibArea = workspace.clientArea(KWin.PlacementArea, data.client);
                  data.client.frameGeometry = { x: data.client.frameGeometry.x, y: sibArea.y - data.client.frameGeometry.height, width: data.client.frameGeometry.width, height: data.client.frameGeometry.height };
              }

              var stillActive = (workspace.activeWindow === target || workspace.activeClient === target);
              if (wasActive && stillActive) {
                var stacking = workspace.stackingOrder;
                var targetBehind = null;
                var targetIndex = -1;
                for (var s = 0; s < stacking.length; s++) {
                    if (stacking[s] === target) {
                        targetIndex = s;
                        break;
                    }
                }
                if (targetIndex > 0) {
                    for (var s = targetIndex - 1; s >= 0; s--) {
                        var c = stacking[s];
                        if (c.normalWindow && c.opacity > 0 && (c.resourceClass || c.resourceName)) {
                            targetBehind = c;
                            break;
                        }
                    }
                }

                if (targetBehind) {
                    focusKick(targetBehind, false);
                } else if (prevWindowId && prevWindowId !== "") {
                    var allClients = workspace.windowList ? workspace.windowList() : workspace.clientList();
                    for (var j = 0; j < allClients.length; j++) {
                      var c = allClients[j];
                      if (c.internalId && normalizeId(c.internalId) === normalizeId(prevWindowId)) {
                        focusKick(c, false);
                        break;
                      }
                    }
                }
              }
              if (KWin.callDBus) KWin.callDBus("org.kde.KWin", "/KWin", "org.kde.KWin", "reconfigure");
            }
          });
          timer.start();
        } else {
          target.opacity = 0.0;
          target.frameGeometry = { x: finalX, y: endY, width: finalWidth, height: finalHeight };
          target.fullScreen = false;
          if (target.skipSwitcher !== undefined) target.skipSwitcher = true;
          for (var i = 0; i < siblingsToHide.length; i++) {
              var sib = siblingsToHide[i];
              sib.opacity = 0.0;
              var sibArea = workspace.clientArea(KWin.PlacementArea, sib);
              sib.frameGeometry = { x: sib.frameGeometry.x, y: sibArea.y - sib.frameGeometry.height, width: sib.frameGeometry.width, height: sib.frameGeometry.height };
          }
          if (KWin.callDBus) KWin.callDBus("org.kde.KWin", "/KWin", "org.kde.KWin", "reconfigure");
        }
    }
})"#;

const ENSURE_GRABBED_BATCH_TEMPLATE: &str = r#"
(function(apps) {
    {{COMMON_KWIN_JS}}

    for (var a = 0; a < apps.length; a++) {
        var app = apps[a];
        var target = findTarget(app.windowClass, app.targetWindowId, app.targetPid);

        if (target) {
          console.log("janq_grab: Grabbing window for " + app.windowClass + " (id: " + target.internalId + ", pid: " + target.pid + ")");
          setQuakeProperties(target, app.keepAbove, app.isVisible, app.forcePriority);

          var area = resolveArea(target, app.displayMode, app.displayIndex, null);
          var dims = resolveDimensions(app.width, app.isWidthPercent, app.height, app.isHeightPercent, area, target);
          var finalX = area.x + (area.width - dims.width) / 2;

          if (!app.isVisible) {
              console.log("janq_grab: Parking " + app.windowClass + " offscreen.");
              target.opacity = 0.0;
              target.frameGeometry = { x: finalX, y: area.y - dims.height - 10, width: dims.width, height: dims.height };
          } else {
              console.log("janq_grab: Skipping position update for " + app.windowClass + " (already visible).");
          }
        } else {
          console.log("janq_grab: FAILED to find window for " + app.windowClass);
        }
    }
})"#;

const RESTORE_TEMPLATE: &str = r#"
(function(windowClass) {
    {{COMMON_KWIN_JS}}
    var clients = workspace.windowList ? workspace.windowList() : workspace.clientList();
    for (var i = 0; i < clients.length; i++) {
      var c = clients[i];
      var cClass = (c.resourceClass || "").toLowerCase();
      var cName = (c.resourceName || "").toLowerCase();
      if (cClass.indexOf(windowClass.toLowerCase()) !== -1 || cName.indexOf(windowClass.toLowerCase()) !== -1) {
          console.log("janq_restore: Restoring window " + cClass);
          var area = workspace.clientArea(KWin.PlacementArea, c);
          var geo = c.frameGeometry;
          var needsCenter = (geo.y + geo.height <= area.y + 50 || c.opacity < 0.1 || geo.y < area.y + 10);

          resetQuakeProperties(c);
          focusKick(c, true);

          if (needsCenter) {
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

  let script_body = TOGGLE_SCRIPT_TEMPLATE.replace("{{COMMON_KWIN_JS}}", COMMON_KWIN_JS);
  let script_content = format!(
    "{}(\n  \"{}\", \"{}\", {}, {}, {}, {}, {},\n  {}, \"{}\", {}, {}, {},\n  {}, {}, \"{}\", \"{}\", {}, \"{}\", {}\n);",
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
    config.window.force_priority
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

  let script_body = ENSURE_GRABBED_BATCH_TEMPLATE.replace("{{COMMON_KWIN_JS}}", COMMON_KWIN_JS);
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
  let script_body = RESTORE_TEMPLATE.replace("{{COMMON_KWIN_JS}}", COMMON_KWIN_JS);
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
