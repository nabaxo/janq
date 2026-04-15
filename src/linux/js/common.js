const normalizeId = (id) => {
  if (!id) return "";
  return id.toString().replace(/[{}]/g, "");
};

const findTarget = (windowClass, targetWindowId, targetPid) => {
  const clients = workspace.windowList ? workspace.windowList() : workspace.clientList();
  const cleanTargetId = normalizeId(targetWindowId);
  const safeTargetPid = targetPid || 0;
  const lowerClass = (windowClass || "").toLowerCase();

  // 1. If we have a specific ID, try to find it first (Absolute Strictness)
  if (cleanTargetId !== "") {
    for (const c of clients) {
      if (c.internalId && normalizeId(c.internalId) === cleanTargetId) {
        return c;
      }
    }
  }

  // 2. Fallback to scoring if no ID match found (or no ID provided)
  let target = null;
  let bestScore = -1;

  for (const c of clients) {
    let score = 0;
    const cClass = (c.resourceClass || "").toLowerCase();
    const cName = (c.resourceName || "").toLowerCase();

    // If we have an ID but didn't find it in step 1, we still allow a PID fallback
    // but with much lower weight to prevent stealing visible instances of the same app.
    if (safeTargetPid > 0 && c.pid == safeTargetPid) {
      score += (cleanTargetId !== "" ? 100 : 500);
    }

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

  if (type === "impulse" || type === "windows") {
    return solveBezier(progress, 0.25, 0, 0, 1);
  }

  switch (type) {
    case "linear": return progress;
    case "ease-in": return progress ** 2;
    case "ease-out": return progress * (2 - progress);
    case "ease":
    case "ease-in-out":
      return progress < 0.5 ? 2 * progress ** 2 : -1 + (4 - 2 * progress) * progress;
    case "quart-in": case "ease-in-quart": case "in-quart": return progress ** 4;
    case "quart-out": case "ease-out-quart": case "out-quart": return 1 - (1 - progress) ** 4;
    case "quart":
    case "quart-in-out":
    case "ease-in-out-quart":
    case "in-out-quart":
      return progress < 0.5 ? 8 * progress ** 4 : 1 - (-2 * progress + 2) ** 4 / 2;
    case "cubic-in":
    case "ease-in-cubic":
    case "in-cubic":
      return progress ** 3;
    case "cubic-out": case "ease-out-cubic": case "out-cubic": return 1 - (1 - progress) ** 3;
    case "cubic":
    case "cubic-in-out":
    case "ease-in-out-cubic":
    case "in-out-cubic":
      return progress < 0.5 ? 4 * progress ** 3 : 1 - (-2 * progress + 2) ** 3 / 2;
    case "sine-in":
    case "ease-in-sine":
    case "in-sine":
      return 1 - Math.cos((progress * Math.PI) / 2);
    case "sine-out": case "ease-out-sine": case "out-sine": return Math.sin((progress * Math.PI) / 2);
    case "sine":
    case "sine-in-out":
    case "ease-in-out-sine":
    case "in-out-sine":
      return -(Math.cos(Math.PI * progress) - 1) / 2;
    case "back-in": case "ease-in-back": case "in-back": {
      const c1 = 1.70158; const c3 = c1 + 1;
      return c3 * progress ** 3 - c1 * progress ** 2;
    }
    case "back-out": case "ease-out-back": case "out-back": {
      const c1 = 1.70158; const c3 = c1 + 1;
      return 1 + c3 * (progress - 1) ** 3 + c1 * (progress - 1) ** 2;
    }
    case "back":
    case "back-in-out": case "ease-in-out-back": case "in-out-back": {
      const c1 = 1.70158; const c2 = c1 * 1.525;
      return progress < 0.5
        ? ((2 * progress) ** 2 * ((c2 + 1) * 2 * progress - c2)) / 2
        : ((2 * progress - 2) ** 2 * ((c2 + 1) * (progress * 2 - 2) + c2) + 2) / 2;
    }
    case "expo-in": case "ease-in-expo": case "in-expo":
      return progress === 0 ? 0 : 2 ** (10 * progress - 10);
    case "expo-out": case "ease-out-expo": case "out-expo":
      return progress === 1 ? 1 : 1 - 2 ** (-10 * progress);
    case "expo":
    case "expo-in-out": case "ease-in-out-expo": case "in-out-expo":
      if (progress === 0) return 0;
      if (progress === 1) return 1;
      return progress < 0.5
        ? 2 ** (20 * progress - 10) / 2
        : (2 - 2 ** (-20 * progress + 10)) / 2;
    default: return progress * (2 - progress);
  }
};

const setQuakeProperties = (target, keepAbove, noBorders, skipPager, isVisible, forcePriority, allDesktops) => {
  target.onAllDesktops = allDesktops !== undefined ? allDesktops : true;
  if (!target.onAllDesktops && isVisible) {
    if (target.desktops !== undefined) target.desktops = [workspace.currentDesktop];
    else target.desktop = workspace.currentDesktop;
  }
  target.keepAbove = keepAbove;
  target.noBorder = noBorders;
  target.skipTaskbar = true;
  target.skipPager = isVisible ? skipPager : true;
  if (target.skipSwitcher !== undefined) target.skipSwitcher = isVisible ? skipPager : true;
  if (forcePriority && !isVisible) target.fullScreen = true;
};

const resetQuakeProperties = (target) => {
  target.janqTag = undefined;
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

  // If we're restoring focus, ensure the target is on the current desktop to avoid jumping
  if (restoreOriginal && target) {
    const targetDesktops = target.desktops || [];
    const currentDesktop = workspace.currentDesktop;
    const isOnCurrentDesktop = target.onAllDesktops || targetDesktops.indexOf(currentDesktop) !== -1;

    if (!isOnCurrentDesktop) {
      console.log("janq_focus: Skipping focus restoration to window on different desktop.");
      return;
    }
  }

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

const resolveAreaContext = (target, displayMode, displayIndex, ignoreSticky) => {
  const screens = workspace.screens || [];
  let screen = workspace.activeScreen;

  // 1. Specific monitor overrides everything
  if (displayMode === "specific" && displayIndex >= 0 && displayIndex < screens.length) {
    screen = screens[displayIndex];
  } else {
    // 2. Sticky Logic: If the window is already visible and on a valid monitor, stay there.
    // This prevents diagonal jumps during hides/follows.
    const targetArea = workspace.clientArea(KWin.PlacementArea, target);
    const isVisible = (targetArea && target.opacity > 0.05 && target.frameGeometry.y + target.frameGeometry.height > targetArea.y + 5);

    if (isVisible && !ignoreSticky) {
      screen = target;
    } else if (displayMode === "active") {
      // 3. Active Window Logic (only if Janq isn't already visible)
      const activeWin = workspace.activeWindow !== undefined ? workspace.activeWindow : workspace.activeClient;
      if (activeWin && (normalizeId(activeWin.internalId) !== normalizeId(target.internalId))) {
        screen = activeWin;
      }
    } else {
      // 4. Follow-Mouse Logic (fallback)
      const cursorPos = workspace.cursorPos;
      for (const s of screens) {
        const geo = s.geometry;
        if (cursorPos.x >= geo.x && cursorPos.x < geo.x + geo.width &&
          cursorPos.y >= geo.y && cursorPos.y < geo.y + geo.height) {
          screen = s;
          break;
        }
      }
    }
  }

  // Handle clientArea's flexible argument types (Window vs Screen object)
  if (screen.geometry !== undefined) {
    // Screen object - needs currentDesktop
    return {
      work: workspace.clientArea(KWin.PlacementArea, screen, workspace.currentDesktop),
      full: workspace.clientArea(KWin.FullScreenArea, screen, workspace.currentDesktop)
    };
  } else {
    // Window object - KWin derives screen/desktop from it automatically
    return {
      work: workspace.clientArea(KWin.PlacementArea, screen),
      full: workspace.clientArea(KWin.FullScreenArea, screen)
    };
  }
};

const resolveDimensions = (width, isWidthPercent, height, isHeightPercent, area, target) => {
  const finalWidth = width > 0 ? (isWidthPercent ? area.width * width : width) : target.frameGeometry.width;
  const finalHeight = height > 0 ? (isHeightPercent ? area.height * height : height) : target.frameGeometry.height;
  return { width: finalWidth, height: finalHeight };
};

const computeSlidePosition = (direction, offsetVal, isPercent, isNegative, isCenter, workArea, fullArea, winW, winH, depthVal, depthIsPercent, depthIsNegative, depthIsCenter) => {
  let shownX, shownY, hiddenX, hiddenY;

  // Depth: distance shown window is pushed into screen from slide edge.
  // Resolved in pixels along the slide axis. Negative pushes window toward/past edge.
  const computeDepth = (span) => {
    if (depthIsCenter) {
      return Math.floor((span - (direction === "top" || direction === "bottom" ? winH : winW)) / 2);
    }
    const raw = depthIsPercent ? (span * (depthVal || 0) / 100) : (depthVal || 0);
    return depthIsNegative ? -raw : raw;
  };

  if (direction === "top" || direction === "bottom") {
    // Fixed axis: X (Lock to workArea center/offset)
    if (isCenter) {
      shownX = workArea.x + (workArea.width - winW) / 2;
    } else if (isPercent) {
      const pct = offsetVal / 100;
      shownX = isNegative
        ? workArea.x + workArea.width - winW - (workArea.width * pct)
        : workArea.x + (workArea.width * pct);
    } else {
      shownX = isNegative
        ? workArea.x + workArea.width - winW - offsetVal
        : workArea.x + offsetVal;
    }
    hiddenX = shownX;

    const depth = computeDepth(workArea.height);
    // Add 10px margin so parked windows can't be grabbed at screen edges
    if (direction === "top") {
      shownY = workArea.y + depth;
      hiddenY = fullArea.y - winH - 10;
    } else {
      shownY = workArea.y + workArea.height - winH - depth;
      hiddenY = fullArea.y + fullArea.height + 10;
    }
  } else {
    // Fixed axis: Y (Lock to workArea center/offset)
    if (isCenter) {
      shownY = workArea.y + (workArea.height - winH) / 2;
    } else if (isPercent) {
      const pct = offsetVal / 100;
      shownY = isNegative
        ? workArea.y + workArea.height - winH - (workArea.height * pct)
        : workArea.y + (workArea.height * pct);
    } else {
      shownY = isNegative
        ? workArea.y + workArea.height - winH - offsetVal
        : workArea.y + offsetVal;
    }
    hiddenY = shownY;

    const depth = computeDepth(workArea.width);
    // Add 10px margin so parked windows can't be grabbed at screen edges
    if (direction === "left") {
      shownX = workArea.x + depth;
      hiddenX = fullArea.x - winW - 10;
    } else {
      shownX = workArea.x + workArea.width - winW - depth;
      hiddenX = fullArea.x + fullArea.width + 10;
    }
  }

  return { shownX, shownY, hiddenX, hiddenY };
};
