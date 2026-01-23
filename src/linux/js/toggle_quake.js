(function (
  windowClass, displayMode, displayIndex, width, isWidthPercent, height, isHeightPercent,
  duration, easingType, shouldShow, keepAbove, noBorders, animateOpacity,
  showOpacityPoint, hideOpacityPoint, prevWindowId, targetWindowId, targetPid, siblingsToHideJson,
  forcePriority, refreshRate,
  slideFrom, offsetValue, offsetIsPercent, offsetIsNegative, offsetIsCenter,
  allSlideConfigs
) {
  /*{{COMMON_KWIN_JS}}*/

  // Compute position along edge based on slide direction and offset.
  // Uses workArea (workspace) for shown position and fixed axis alignment,
  // and fullArea (absolute screen) for the hidden position on the sliding axis.

  // Look up a sibling's own slide config, fallback to current toggle's config
  function getSiblingSlideConfig(sibClass) {
    const key = (sibClass || "").toLowerCase();
    if (allSlideConfigs && allSlideConfigs[key]) {
      const cfg = allSlideConfigs[key];
      return { dir: cfg.dir, val: cfg.val, pct: cfg.pct, neg: cfg.neg, ctr: cfg.ctr, easing: cfg.easing, animOp: cfg.animOp };
    }
    // Fallback to current app's config (shouldn't happen for managed apps)
    return { dir: slideFrom, val: offsetValue, pct: offsetIsPercent, neg: offsetIsNegative, ctr: offsetIsCenter, easing: easingType, animOp: animateOpacity };
  }

  // Precise Single-Pass Discovery
  const clients = workspace.windowList ? workspace.windowList() : workspace.clientList();
  const incomingSiblings = siblingsToHideJson || [];
  const cleanTargetId = normalizeId(targetWindowId);
  const safeTargetPid = targetPid || 0;

  let target = null;
  const siblingsToHide = [];
  const managedSibIds = new Set(incomingSiblings.map(s => normalizeId(s.id)));
  const managedSibPids = new Set(incomingSiblings.map(s => s.pid).filter(p => p > 0));

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
      let isMatch = false;
      if (cId !== "" && managedSibIds.has(cId)) isMatch = true;
      else if (cPid > 0 && managedSibPids.has(cPid)) isMatch = true;

      if (isMatch) {
        // Resolve sibling context for visibility check
        const sibCfg = getSiblingSlideConfig(c.resourceClass || c.resourceName);
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
  const context = resolveAreaContext(target, displayMode, displayIndex);
  const workArea = context.work;
  const fullArea = context.full;

  let startX = target.frameGeometry.x;
  let startY = target.frameGeometry.y;
  let startOpacity = target.opacity;

  let finalWidth = target.frameGeometry.width;
  let finalHeight = target.frameGeometry.height;

  // ... (rest of the repositioning logic) ...
  const isHorizontalSlide = (slideFrom === "left" || slideFrom === "right");
  let offscreenPos, onScreenThreshold;
  if (isHorizontalSlide) {
    if (slideFrom === "left") {
      offscreenPos = fullArea.x - target.frameGeometry.width;
      onScreenThreshold = workArea.x + 50;
    } else {
      offscreenPos = fullArea.x + fullArea.width;
      onScreenThreshold = workArea.x + workArea.width - target.frameGeometry.width - 50;
    }
  } else {
    if (slideFrom === "top") {
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
    ? (slideFrom === "left" ? startX < offscreenPos - 50 || startX > onScreenThreshold : startX > offscreenPos + 50 || startX < onScreenThreshold)
    : (slideFrom === "top" ? startY < offscreenPos - 50 || startY > onScreenThreshold : startY > offscreenPos + 50 || startY < onScreenThreshold));

  if (needsReposition) {
    const dims = resolveDimensions(width, isWidthPercent, height, isHeightPercent, workArea, target);
    finalWidth = dims.width;
    finalHeight = dims.height;
  }

  const slidePos = computeSlidePosition(slideFrom, offsetValue, offsetIsPercent, offsetIsNegative, offsetIsCenter, workArea, fullArea, finalWidth, finalHeight);
  const finalX = shouldShow ? slidePos.shownX : slidePos.hiddenX;
  const finalY = shouldShow ? slidePos.shownY : slidePos.hiddenY;

  // Prepare sibling animation data
  const siblingDatas = [];
  const targetTotalDist = Math.sqrt((finalX - startX) ** 2 + (finalY - startY) ** 2);
  let groupMaxDist = targetTotalDist;

  for (const sib of siblingsToHide) {
    const sibCfg = getSiblingSlideConfig(sib.resourceClass || sib.resourceName);
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
  const targetScaledDur = (groupMaxDist > 0) ? Math.min(duration, (duration * targetTotalDist) / groupMaxDist) : duration;

  for (const s of siblingDatas) {
    s.duration = (groupMaxDist > 0) ? Math.min(duration, (duration * s.dist) / groupMaxDist) : duration;
  }

  const startAnimation = () => {
    if (duration > 0) {
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
        const globalProgress = Math.min(elapsed / duration, 1.0);

        // Target's internal progress
        const targetProgress = targetScaledDur > 0 ? Math.min(elapsed / targetScaledDur, 1.0) : 1.0;
        const targetEase = getEasing(targetProgress, easingType);

        if (firstFrame) {
          if (shouldShow) {
            focusKick(target, false);
          }
          if (forcePriority) target.fullScreen = true;
          if (shouldShow) target.opacity = animateOpacity ? 0.0 : 1.0;
          firstFrame = false;
        }

        // Apply target window transformation
        if (shouldShow) {
          target.frameGeometry = { x: startX + diffX * targetEase, y: startY + diffY * targetEase, width: finalWidth, height: finalHeight };
          if (animateOpacity) {
            const rawProgress = Math.min(1.0, Math.max(0, globalProgress / (showOpacityPoint <= 0 ? 0.0001 : showOpacityPoint)));
            const opEase = Math.min(1.0, Math.max(0.0, getEasing(rawProgress, easingType)));
            target.opacity = opEase;
          } else {
            target.opacity = 1.0;
          }
        } else {
          target.frameGeometry = { x: startX + diffX * targetEase, y: startY + diffY * targetEase, width: finalWidth, height: finalHeight };
          if (animateOpacity) {
            const denom = 1.0 - hideOpacityPoint;
            const rawProgress = Math.min(1.0, Math.max(0, (globalProgress - hideOpacityPoint) / (denom <= 0 ? 0.0001 : denom)));
            const opEase = Math.min(1.0, Math.max(0.0, getEasing(rawProgress, easingType)));
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
            const denom = 1.0 - hideOpacityPoint;
            const rawProgress = Math.min(1.0, Math.max(0, (sOpProgress - hideOpacityPoint) / (denom <= 0 ? 0.0001 : denom)));
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
          if (shouldShow) {
            target.opacity = 1.0;
            target.frameGeometry = { x: finalX, y: finalY, width: finalWidth, height: finalHeight };
            focusKick(target, false);
          } else {
            target.opacity = 0.0;
            target.frameGeometry = { x: finalX, y: finalY, width: finalWidth, height: finalHeight };
            target.fullScreen = false;
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
                  if (c.normalWindow && c.opacity > 0 && (c.resourceClass || c.resourceName)) {
                    targetBehind = c;
                    break;
                  }
                }
              }

              if (targetBehind) {
                focusKick(targetBehind, false);
              } else if (prevWindowId && prevWindowId !== "") {
                const allClients = workspace.windowList ? workspace.windowList() : workspace.clientList();
                for (const c of allClients) {
                  if (c.internalId && normalizeId(c.internalId) === normalizeId(prevWindowId)) {
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
            data.client.frameGeometry = { x: data.endX, y: data.endY, width: data.client.frameGeometry.width, height: data.client.frameGeometry.height };
          }
          if (!shouldShow && KWin.callDBus) KWin.callDBus("org.kde.KWin", "/KWin", "org.kde.KWin", "reconfigure");
        }
      });
      setForceBlur(target, true);
      for (const data of siblingDatas) setForceBlur(data.client, true);
      timer.start();
    } else {
      // Instant transition
      target.opacity = shouldShow ? 1.0 : 0.0;
      target.frameGeometry = { x: finalX, y: finalY, width: finalWidth, height: finalHeight };
      for (const data of siblingDatas) {
        data.client.opacity = 0.0;
        data.client.frameGeometry = { x: data.endX, y: data.endY, width: data.client.frameGeometry.width, height: data.client.frameGeometry.height };
      }
    }
  };

  if (shouldShow) {
    setQuakeProperties(target, keepAbove, noBorders, true, forcePriority);
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
});
