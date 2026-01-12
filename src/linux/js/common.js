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
      var parts = content.split(",").map(function (p) { return parseFloat(p.trim()); });
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
