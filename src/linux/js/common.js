const normalizeId = (id) => {
  if (!id) return "";
  return id.toString().replace(/[{}]/g, "");
};

const findTarget = (windowClass, targetWindowId, targetPid) => {
  const clients = workspace.windowList ? workspace.windowList() : workspace.clientList();
  const cleanTargetId = normalizeId(targetWindowId);
  const safeTargetPid = targetPid || 0;
  const lowerClass = (windowClass || "").toLowerCase();

  let target = null;
  let bestScore = -1;

  for (const c of clients) {
    let score = 0;
    const cClass = (c.resourceClass || "").toLowerCase();
    const cName = (c.resourceName || "").toLowerCase();

    if (cleanTargetId !== "" && c.internalId && normalizeId(c.internalId) === cleanTargetId) score += 1000;
    if (safeTargetPid > 0 && c.pid == safeTargetPid) score += 500;

    // Modern .includes() for readability
    if (lowerClass) {
      if (cClass.includes(lowerClass) || cName.includes(lowerClass)) score += 100;
      if (c.desktopFileName && c.desktopFileName.toLowerCase().includes(lowerClass)) score += 50;
    }

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
};

const setForceBlur = (target, enabled) => {
  if (target?.setData) {
    target.setData(1, enabled ? true : null);
  }
};

/**
 * Optimized Bezier Solver (The rewrite you liked)
 */
const solveBezier = (progress, x1, y1, x2, y2) => {
  if (progress <= 0) return 0;
  if (progress >= 1) return 1;

  // Pre-calculate polynomial coefficients
  const cx = 3.0 * x1;
  const bx = 3.0 * (x2 - x1) - cx;
  const ax = 1.0 - cx - bx;

  const cy = 3.0 * y1;
  const by = 3.0 * (y2 - y1) - cy;
  const ay = 1.0 - cy - by;

  let t = progress;
  for (let i = 0; i < 8; i++) {
    // x(t) = ((ax * t + bx) * t + cx) * t
    const xt = ((ax * t + bx) * t + cx) * t;
    // dx/dt = (3at^2 + 2bt + c)
    const dxt = (3.0 * ax * t + 2.0 * bx) * t + cx;

    if (Math.abs(dxt) < 1e-6) break;
    t -= (xt - progress) / dxt;
  }

  // Return y(t) = ((ay * t + by) * t + cy) * t
  return ((ay * t + by) * t + cy) * t;
};

const getEasing = (progress, type) => {
  if (type.includes("(")) {
    let content = "";
    if (type.startsWith("cubic-bezier(")) content = type.slice(13, -1);
    else if (type.startsWith("bezier(")) content = type.slice(7, -1);
    else if (type.startsWith("(")) content = type.slice(1, -1);

    if (content) {
      const parts = content.split(",").map(p => parseFloat(p.trim()));
      if (parts.length === 4 && !parts.some(isNaN)) {
        return solveBezier(progress, ...parts);
      }
    }
  }

  if (type === "windows") {
    return solveBezier(progress, 0.25, 0, 0.75, 1);
  }

  switch (type) {
    case "linear": return progress;
    case "ease-in": return progress ** 2;
    case "ease-out": return progress * (2 - progress);
    case "ease":
    case "ease-in-out":
      return progress < 0.5 ? 2 * progress ** 2 : -1 + (4 - 2 * progress) * progress;
    case "quart-in": case "ease-in-quart": return progress ** 4;
    case "quart-out": case "ease-out-quart": return 1 - (1 - progress) ** 4;
    case "quart":
    case "quart-in-out":
    case "ease-in-out-quart":
      return progress < 0.5 ? 8 * progress ** 4 : 1 - (-2 * progress + 2) ** 4 / 2;
    case "cubic-in":
    case "ease-in-cubic":
      return progress ** 3;
    case "cubic-out": case "ease-out-cubic": return 1 - (1 - progress) ** 3;
    case "cubic":
    case "cubic-in-out":
    case "ease-in-out-cubic":
      return progress < 0.5 ? 4 * progress ** 3 : 1 - (-2 * progress + 2) ** 3 / 2;
    case "sine-in":
    case "ease-in-sine":
      return 1 - Math.cos((progress * Math.PI) / 2);
    case "sine-out": case "ease-out-sine": return Math.sin((progress * Math.PI) / 2);
    case "sine":
    case "sine-in-out":
    case "ease-in-out-sine":
      return -(Math.cos(Math.PI * progress) - 1) / 2;
    case "back-in": case "ease-in-back": {
      const c1 = 1.70158; const c3 = c1 + 1;
      return c3 * progress ** 3 - c1 * progress ** 2;
    }
    case "back-out": case "ease-out-back": {
      const c1 = 1.70158; const c3 = c1 + 1;
      return 1 + c3 * (progress - 1) ** 3 + c1 * (progress - 1) ** 2;
    }
    case "back":
    case "back-in-out": case "ease-in-out-back": {
      const c1 = 1.70158; const c2 = c1 * 1.525;
      return progress < 0.5
        ? ((2 * progress) ** 2 * ((c2 + 1) * 2 * progress - c2)) / 2
        : ((2 * progress - 2) ** 2 * ((c2 + 1) * (progress * 2 - 2) + c2) + 2) / 2;
    }
    case "expo-in": case "ease-in-expo":
      return progress === 0 ? 0 : 2 ** (10 * progress - 10);
    case "expo-out": case "ease-out-expo":
      return progress === 1 ? 1 : 1 - 2 ** (-10 * progress);
    case "expo":
    case "expo-in-out": case "ease-in-out-expo":
      if (progress === 0) return 0;
      if (progress === 1) return 1;
      return progress < 0.5
        ? 2 ** (20 * progress - 10) / 2
        : (2 - 2 ** (-20 * progress + 10)) / 2;
    default: return progress * (2 - progress);
  }
};

const setQuakeProperties = (target, keepAbove, isVisible, forcePriority) => {
  target.onAllDesktops = true;
  target.keepAbove = keepAbove;
  target.noBorder = true;
  target.skipTaskbar = true;
  target.skipPager = true;
  if (target.skipSwitcher !== undefined) target.skipSwitcher = true;
  if (forcePriority && !isVisible) target.fullScreen = true;
};

const resetQuakeProperties = (target) => {
  target.keepAbove = false;
  target.onAllDesktops = false;
  target.noBorder = false;
  target.skipTaskbar = false;
  target.skipPager = false;
  if (target.skipSwitcher !== undefined) target.skipSwitcher = false;
  target.fullScreen = false;
  target.opacity = 1.0;
};

const focusKick = (target, restoreOriginal) => {
  const activeWin = workspace.activeWindow !== undefined ? workspace.activeWindow : workspace.activeClient;
  if (workspace.activeWindow !== undefined) {
    workspace.activeWindow = null;
    workspace.activeWindow = target;
    if (restoreOriginal && activeWin && activeWin !== target) workspace.activeWindow = activeWin;
  } else {
    workspace.activeClient = null;
    workspace.activeClient = target;
    if (restoreOriginal && activeWin && activeWin !== target) workspace.activeClient = activeWin;
  }
};

const resolveArea = (target, displayMode, displayIndex, currentArea) => {
  const screens = workspace.screens || [];
  if (displayMode === "specific" && displayIndex >= 0 && displayIndex < screens.length) {
    return screens[displayIndex].geometry;
  }

  const activeWin = workspace.activeWindow !== undefined ? workspace.activeWindow : workspace.activeClient;
  const isTargetActive = (activeWin && activeWin.internalId && target.internalId && normalizeId(activeWin.internalId) === normalizeId(target.internalId));

  if (displayMode === "active") {
    if (activeWin && !isTargetActive) {
      return workspace.clientArea(KWin.FullScreenArea, activeWin);
    }
    if (currentArea && target.opacity > 0.05 && target.frameGeometry.y + target.frameGeometry.height > currentArea.y + 5) {
      return currentArea;
    }
    return workspace.clientArea(KWin.FullScreenArea, workspace.activeScreen, workspace.currentDesktop);
  }

  const cursorPos = workspace.cursorPos;
  for (const screen of screens) {
    const geo = screen.geometry;
    if (cursorPos.x >= geo.x && cursorPos.x < geo.x + geo.width &&
      cursorPos.y >= geo.y && cursorPos.y < geo.y + geo.height) {
      return geo;
    }
  }
  return (workspace.activeScreen ? workspace.activeScreen.geometry : null);
};

const resolveDimensions = (width, isWidthPercent, height, isHeightPercent, area, target) => {
  const finalWidth = width > 0 ? (isWidthPercent ? area.width * width : width) : target.frameGeometry.width;
  const finalHeight = height > 0 ? (isHeightPercent ? area.height * height : height) : target.frameGeometry.height;
  return { width: finalWidth, height: finalHeight };
};
