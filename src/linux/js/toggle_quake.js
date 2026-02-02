(function (config, incomingSiblings, refreshRate) {
  /*{{COMMON_KWIN_JS}}*/

  // Compute position along edge based on slide direction and offset.
  // Uses workArea (workspace) for shown position and fixed axis alignment,
  // and fullArea (absolute screen) for the hidden position on the sliding axis.

  // Precise Single-Pass Discovery
  const clients = workspace.windowList ? workspace.windowList() : workspace.clientList();
  const cleanTargetId = normalizeId(config.targetWindowId);
  const safeTargetPid = config.targetPid || 0;

  let target = null;
  const siblingsToHide = [];

  for (const c of clients) {
    const cId = normalizeId(c.internalId);
    const cPid = c.pid || 0;

    // 1. Identify Target
    if (!target) {
      if (cleanTargetId !== "" && cId === cleanTargetId) target = c;
      else if (safeTargetPid > 0 && cPid === safeTargetPid) target = c;
    }

    // 2. Identify Siblings (only if they aren't the target and are visible/alive)
    if (c !== target && c.opacity > 0.01) {
      let managedData = null;
      if (cId !== "") managedData = incomingSiblings.find(s => normalizeId(s.id) === cId);
      if (!managedData && cPid > 0) managedData = incomingSiblings.find(s => s.pid === cPid);

      if (managedData) {
        // Use pre-calculated data from Rust
        const sibCfg = managedData;
        const cArea = workspace.clientArea(KWin.PlacementArea, c);
        let isActuallyVisible = false;

        if (sibCfg.dir === "top") isActuallyVisible = (c.frameGeometry.y + c.frameGeometry.height > cArea.y + 1);
        else if (sibCfg.dir === "bottom") isActuallyVisible = (c.frameGeometry.y < cArea.y + cArea.height - 1);
        else if (sibCfg.dir === "left") isActuallyVisible = (c.frameGeometry.x + c.frameGeometry.width > cArea.x + 1);
        else if (sibCfg.dir === "right") isActuallyVisible = (c.frameGeometry.x < cArea.x + cArea.width - 1);

        if (isActuallyVisible) siblingsToHide.push(c);
      }
    }
  }

  if (!target) return;

  // Resolve areas for the target
  const context = resolveAreaContext(target, config.displayMode, config.displayIndex, config.shouldShow);
  const workArea = context.work;
  const fullArea = context.full;

  let startX = target.frameGeometry.x;
  let startY = target.frameGeometry.y;
  let startOpacity = target.opacity;

  let finalWidth = target.frameGeometry.width;
  let finalHeight = target.frameGeometry.height;

  const isHorizontalSlide = (config.slideFrom === "left" || config.slideFrom === "right");
  let offscreenPos, onScreenThreshold;
  if (isHorizontalSlide) {
    if (config.slideFrom === "left") {
      offscreenPos = fullArea.x - target.frameGeometry.width;
      onScreenThreshold = workArea.x + 50;
    } else {
      offscreenPos = fullArea.x + fullArea.width;
      onScreenThreshold = workArea.x + workArea.width - target.frameGeometry.width - 50;
    }
  } else {
    if (config.slideFrom === "top") {
      offscreenPos = fullArea.y - target.frameGeometry.height;
      onScreenThreshold = workArea.y + 50;
    } else {
      offscreenPos = fullArea.y + fullArea.height;
      onScreenThreshold = workArea.y + workArea.height - target.frameGeometry.height - 50;
    }
  }

  const onWrongMonitor = isHorizontalSlide
    ? (startY < workArea.y - 10) || (startY > workArea.y + workArea.height + 10)
    : (startX < workArea.x - 10) || (startX > workArea.x + workArea.width + 10);

  const needsReposition = onWrongMonitor || (isHorizontalSlide
    ? (config.slideFrom === "left" ? startX < offscreenPos - 50 || startX > onScreenThreshold : startX > offscreenPos + 50 || startX < onScreenThreshold)
    : (config.slideFrom === "top" ? startY < offscreenPos - 50 || startY > onScreenThreshold : startY > offscreenPos + 50 || startY < onScreenThreshold));

  if (needsReposition) {
    const dims = resolveDimensions(config.width, config.isWidthPercent, config.height, config.isHeightPercent, workArea, target);
    finalWidth = dims.width;
    finalHeight = dims.height;
  }

  const slidePos = computeSlidePosition(config.slideFrom, config.offsetValue, config.offsetIsPercent, config.offsetIsNegative, config.offsetIsCenter, workArea, fullArea, finalWidth, finalHeight);
  const finalX = config.shouldShow ? slidePos.shownX : slidePos.hiddenX;
  const finalY = config.shouldShow ? slidePos.shownY : slidePos.hiddenY;

  // Prepare sibling animation data
  const siblingDatas = [];
  const targetTotalDist = Math.sqrt((finalX - startX) ** 2 + (finalY - startY) ** 2);
  let groupMaxDist = targetTotalDist;

  for (const sib of siblingsToHide) {
    const sibCfg = incomingSiblings.find(s => normalizeId(s.id) === normalizeId(sib.internalId)) ||
      incomingSiblings.find(s => s.pid === sib.pid);
    if (!sibCfg) continue;

    const sibContext = resolveAreaContext(sib, "active", 0);
    const sibOffscreen = computeSlidePosition(sibCfg.dir, sibCfg.val, sibCfg.pct, sibCfg.neg, sibCfg.ctr, sibContext.work, sibContext.full, sib.frameGeometry.width, sib.frameGeometry.height);

    const sDiffX = sibOffscreen.hiddenX - sib.frameGeometry.x;
    const sDiffY = sibOffscreen.hiddenY - sib.frameGeometry.y;
    const sDist = Math.sqrt(sDiffX * sDiffX + sDiffY * sDiffY);
    if (sDist > groupMaxDist) groupMaxDist = sDist;

    siblingDatas.push({
      client: sib,
      startX: sib.frameGeometry.x,
      startY: sib.frameGeometry.y,
      startOpacity: sib.opacity,
      endX: sibOffscreen.hiddenX,
      endY: sibOffscreen.hiddenY,
      dist: sDist,
      easing: sibCfg.easing,
      animOp: sibCfg.animOp,
      blurActive: true
    });
  }

  // Calculate scaled durations relative to max distance
  const targetScaledDur = (groupMaxDist > 0) ? Math.min(config.duration, (config.duration * targetTotalDist) / groupMaxDist) : config.duration;

  for (const s of siblingDatas) {
    s.duration = (groupMaxDist > 0) ? Math.min(config.duration, (config.duration * s.dist) / groupMaxDist) : config.duration;
  }

  const connectFocusWatcher = () => {
    if (!config.autoHide || !config.shouldShow || !target) return;
    const tid = normalizeId(target.internalId), tpid = target.pid || 0, tcls = (target.resourceClass || "").toLowerCase();
    const checkSelf = (c) => {
      if (!c) return false;
      const cid = normalizeId(c.internalId), cpid = c.pid || 0, ccls = (c.resourceClass || "").toLowerCase();
      return (cid === tid || (tpid > 0 && cpid === tpid) || ccls === tcls);
    };
    const focusWatcher = function (c) {
      if (!checkSelf(c)) {
        const otherClass = (c && c.resourceClass) ? c.resourceClass : "unknown";
        workspace.windowActivated.disconnect(focusWatcher);
        callDBus("dev.nabaxo.janq", "/dev/nabaxo/janq", "dev.nabaxo.janq", "ToggleApp", config.appName.toString());
      }
    };
    workspace.windowActivated.connect(focusWatcher);
  };

  const startAnimation = () => {
    if (config.duration > 0) {
      const startTime = Date.now();
      const diffX = finalX - startX;
      const diffY = finalY - startY;

      let firstFrame = true;
      const wasActive = (workspace.activeWindow === target || workspace.activeClient === target);
      let targetBlurActive = true;

      const timer = new QTimer();
      timer.interval = Math.max(1, Math.floor(1000 / refreshRate));
      timer.timeout.connect(() => {
        const now = Date.now();
        const elapsed = now - startTime;

        // Timer runs for the full global duration to ensure all windows finish and the script unloads
        const globalProgress = Math.min(elapsed / config.duration, 1.0);

        // Target's internal progress
        const targetProgress = targetScaledDur > 0 ? Math.min(elapsed / targetScaledDur, 1.0) : 1.0;
        const targetEase = getEasing(targetProgress, config.easingType);

        if (firstFrame) {
          if (config.shouldShow) {
            focusKick(target, false);
          }
          if (config.forcePriority) target.fullScreen = true;
          if (config.shouldShow) target.opacity = config.animateOpacity ? 0.0 : 1.0;
          firstFrame = false;
        }

        // Apply target window transformation
        if (config.shouldShow) {
          target.frameGeometry = { x: startX + diffX * targetEase, y: startY + diffY * targetEase, width: finalWidth, height: finalHeight };
          if (config.animateOpacity) {
            const rawProgress = Math.min(1.0, Math.max(0, globalProgress / (config.showOpacityPoint <= 0 ? 0.0001 : config.showOpacityPoint)));
            const opEase = Math.min(1.0, Math.max(0.0, getEasing(rawProgress, config.easingType)));
            target.opacity = opEase;
          } else {
            target.opacity = 1.0;
          }
        } else {
          target.frameGeometry = { x: startX + diffX * targetEase, y: startY + diffY * targetEase, width: finalWidth, height: finalHeight };
          if (config.animateOpacity) {
            const denom = 1.0 - config.hideOpacityPoint;
            const rawProgress = Math.min(1.0, Math.max(0, (globalProgress - config.hideOpacityPoint) / (denom <= 0 ? 0.0001 : denom)));
            const opEase = Math.min(1.0, Math.max(0.0, getEasing(rawProgress, config.easingType)));
            target.opacity = 1.0 - opEase;
          } else {
            target.opacity = (globalProgress >= 1.0 ? 0.0 : 1.0);
          }
        }

        // Target Blur Cleanup (Early)
        if (targetProgress >= 1.0 && targetBlurActive) {
          setForceBlur(target, false);
          targetBlurActive = false;
        }

        for (const data of siblingDatas) {
          const sProgress = data.duration > 0 ? Math.min(elapsed / data.duration, 1.0) : 1.0;
          const sEase = getEasing(sProgress, data.easing);

          data.client.frameGeometry = { x: data.startX + (data.endX - data.startX) * sEase, y: data.startY + (data.endY - data.startY) * sEase, width: data.client.frameGeometry.width, height: data.client.frameGeometry.height };

          if (data.animOp) {
            // Sync opacity with sibling's own internal duration
            const sOpProgress = data.duration > 0 ? Math.min(elapsed / data.duration, 1.0) : 1.0;
            // Siblings are ALWAYS being hidden in this script logic
            const denom = 1.0 - config.hideOpacityPoint;
            const rawProgress = Math.min(1.0, Math.max(0, (sOpProgress - config.hideOpacityPoint) / (denom <= 0 ? 0.0001 : denom)));
            const opEase = Math.min(1.0, Math.max(0.0, getEasing(rawProgress, data.easing)));
            data.client.opacity = 1.0 - opEase;
          } else {
            data.client.opacity = (globalProgress >= 1.0 ? 0.0 : 1.0);
          }

          // Sibling Blur Cleanup (Early)
          if (sProgress >= 1.0 && data.blurActive) {
            setForceBlur(data.client, false);
            data.blurActive = false;
          }
        }

        if (globalProgress >= 1.0) {
          timer.stop();
          if (config.shouldShow) {
            target.opacity = 1.0;
            target.frameGeometry = { x: finalX, y: finalY, width: finalWidth, height: finalHeight };
            focusKick(target, false);
            connectFocusWatcher();
          } else {
            target.opacity = 0.0;
            target.frameGeometry = { x: finalX, y: finalY, width: finalWidth, height: finalHeight };
            target.fullScreen = false;
            target.skipPager = true;
            if (target.skipSwitcher !== undefined) target.skipSwitcher = true;

            const stillActive = (workspace.activeWindow === target || workspace.activeClient === target);
            if (wasActive && stillActive) {
              const stacking = workspace.stackingOrder;
              let targetBehind = null;
              let targetIndex = -1;
              for (let s = 0; s < stacking.length; s++) {
                if (stacking[s] === target) {
                  targetIndex = s;
                  break;
                }
              }
              if (targetIndex > 0) {
                for (let s = targetIndex - 1; s >= 0; s--) {
                  const c = stacking[s];
                  const isOnCurrentDesktop = c.onAllDesktops || (c.desktops && c.desktops.indexOf(workspace.currentDesktop) !== -1);
                  if (c.normalWindow && c.opacity > 0 && (c.resourceClass || c.resourceName) && isOnCurrentDesktop) {
                    targetBehind = c;
                    break;
                  }
                }
              }

              if (targetBehind) {
                focusKick(targetBehind, false);
              } else if (config.prevWindowId && config.prevWindowId !== "") {
                const allClients = workspace.windowList ? workspace.windowList() : workspace.clientList();
                for (const c of allClients) {
                  if (c.internalId && normalizeId(c.internalId) === normalizeId(config.prevWindowId)) {
                    focusKick(c, false);
                    break;
                  }
                }
              }
            }
          }
          if (targetBlurActive) setForceBlur(target, false);
          for (const data of siblingDatas) {
            if (data.blurActive) setForceBlur(data.client, false);
            data.client.opacity = 0.0;
            data.client.skipPager = true;
            if (data.client.skipSwitcher !== undefined) data.client.skipSwitcher = true;
            data.client.frameGeometry = { x: data.endX, y: data.endY, width: data.client.frameGeometry.width, height: data.client.frameGeometry.height };
          }
          // Removed: KWin.reconfigure() was resetting skipSwitcher on all windows
        }
      });
      setForceBlur(target, true);
      for (const data of siblingDatas) setForceBlur(data.client, true);
      timer.start();
    } else {
      // --- Instant Transition (duration = 0) ---
      if (config.shouldShow) {
        if (config.forcePriority) target.fullScreen = true;
        target.opacity = 1.0;
        target.frameGeometry = { x: finalX, y: finalY, width: finalWidth, height: finalHeight };
        focusKick(target, false);
        connectFocusWatcher();
      } else {
        target.opacity = 0.0;
        target.frameGeometry = { x: finalX, y: finalY, width: finalWidth, height: finalHeight };
        target.fullScreen = false;
        target.skipPager = true;
        if (target.skipSwitcher !== undefined) target.skipSwitcher = true;

        if (config.prevWindowId && config.prevWindowId !== "") {
          const allClients = workspace.windowList ? workspace.windowList() : workspace.clientList();
          for (const c of allClients) {
            if (c.internalId && normalizeId(c.internalId) === normalizeId(config.prevWindowId)) {
              focusKick(c, false);
              break;
            }
          }
        }
      }
      for (const data of siblingDatas) {
        data.client.opacity = 0.0;
        data.client.skipPager = true;
        if (data.client.skipSwitcher !== undefined) data.client.skipSwitcher = true;
        data.client.frameGeometry = { x: data.endX, y: data.endY, width: data.client.frameGeometry.width, height: data.client.frameGeometry.height };
      }
    }
  };

  if (config.shouldShow) {
    setQuakeProperties(target, config.keepAbove, config.noBorders, config.skipPager, true, config.forcePriority, config.allDesktops);
    if (needsReposition) {
      target.opacity = 0.0;
      target.fullScreen = false;
      target.frameGeometry = { x: slidePos.hiddenX, y: slidePos.hiddenY, width: finalWidth, height: finalHeight };
      startX = slidePos.hiddenX;
      startY = slidePos.hiddenY;
      const delayTimer = new QTimer();
      delayTimer.interval = 200;
      delayTimer.singleShot = true;
      delayTimer.timeout.connect(startAnimation);
      delayTimer.start();
    } else {
      startAnimation();
    }
  } else {
    startAnimation();
  }
}
);
