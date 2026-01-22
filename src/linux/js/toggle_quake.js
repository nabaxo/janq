(function (
  windowClass, displayMode, displayIndex, width, isWidthPercent, height, isHeightPercent,
  duration, easingType, shouldShow, keepAbove, animateOpacity,
  showOpacityPoint, hideOpacityPoint, prevWindowId, targetWindowId, targetPid, janqClasses,
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
      return { dir: cfg.dir, val: cfg.val, pct: cfg.pct, neg: cfg.neg, ctr: cfg.ctr, easing: cfg.easing };
    }
    // Fallback to current app's config (shouldn't happen for managed apps)
    return { dir: slideFrom, val: offsetValue, pct: offsetIsPercent, neg: offsetIsNegative, ctr: offsetIsCenter, easing: easingType };
  }

  const target = findTarget(windowClass, targetWindowId, targetPid);
  if (!target) return;

  // Resolve the monitor and areas together for perfect stability.
  // The internal 'sticky' logic in resolveAreaContext handles hide-locking automatically.
  const context = resolveAreaContext(target, displayMode, displayIndex);
  const workArea = context.work;
  const fullArea = context.full;

  let startX = target.frameGeometry.x;
  let startY = target.frameGeometry.y;
  let startOpacity = target.opacity;

  let finalWidth = target.frameGeometry.width;
  let finalHeight = target.frameGeometry.height;

  // Determine if window needs repositioning based on slide direction
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
  const finalX = slidePos.shownX;
  const finalY = slidePos.shownY;
  const offscreenX = slidePos.hiddenX;
  const offscreenY = slidePos.hiddenY;

  const clients = workspace.windowList ? workspace.windowList() : workspace.clientList();
  const rawClasses = (janqClasses || "").toLowerCase().split(",");
  const allClasses = [];
  for (const rawClass of rawClasses) {
    const trimmed = rawClass.trim();
    if (trimmed) allClasses.push(trimmed);
  }

  const siblingsToHide = [];
  for (const c of clients) {
    if (c === target) continue;
    const cClass = (c.resourceClass || "").toLowerCase();
    const cName = (c.resourceName || "").toLowerCase();

    let isManaged = false;
    for (const siblingClass of allClasses) {
      if (cClass.includes(siblingClass) || cName.includes(siblingClass)) {
        isManaged = true;
        break;
      }
    }

    if (isManaged) {
      const cArea = workspace.clientArea(KWin.PlacementArea, c);
      const sibCfg = getSiblingSlideConfig(c.resourceClass || c.resourceName);
      let isActuallyVisible = false;

      // Direction-aware visibility detection
      if (sibCfg.dir === "top") {
        isActuallyVisible = (c.frameGeometry.y + c.frameGeometry.height > cArea.y + 1);
      } else if (sibCfg.dir === "bottom") {
        isActuallyVisible = (c.frameGeometry.y < cArea.y + cArea.height - 1);
      } else if (sibCfg.dir === "left") {
        isActuallyVisible = (c.frameGeometry.x + c.frameGeometry.width > cArea.x + 1);
      } else if (sibCfg.dir === "right") {
        isActuallyVisible = (c.frameGeometry.x < cArea.x + cArea.width - 1);
      }

      if (c.opacity > 0.01 && isActuallyVisible) {
        siblingsToHide.push(c);
      }
    }
  }

  if (shouldShow) {
    setQuakeProperties(target, keepAbove, true, forcePriority);
    focusKick(target, false);

    const startAnimation = () => {
      if (duration > 0) {
        const startTime = Date.now();
        const diffX = finalX - startX;
        const diffY = finalY - startY;

        // Implementation of duration scaling (Velocity matching)
        let scaledDuration = duration;
        if (duration > 0) {
          const totalDist = isHorizontalSlide ? finalWidth : finalHeight;
          const currentDist = Math.sqrt(diffX * diffX + diffY * diffY);
          if (totalDist > 0) {
            scaledDuration = Math.min(duration, (duration * currentDist) / totalDist);
          }
        }

        const siblingDatas = [];
        for (const sib of siblingsToHide) {
          const sibContext = resolveAreaContext(sib, "active", 0); // Active mode for siblings is effectively current monitor
          const sibWorkArea = sibContext.work;
          const sibFullArea = sibContext.full;
          const sibCfg = getSiblingSlideConfig(sib.resourceClass || sib.resourceName);
          const sibOffscreen = computeSlidePosition(sibCfg.dir, sibCfg.val, sibCfg.pct, sibCfg.neg, sibCfg.ctr, sibWorkArea, sibFullArea, sib.frameGeometry.width, sib.frameGeometry.height);
          siblingDatas.push({
            client: sib,
            startX: sib.frameGeometry.x,
            startY: sib.frameGeometry.y,
            startOpacity: sib.opacity,
            endX: sibOffscreen.hiddenX,
            endY: sibOffscreen.hiddenY,
            easing: sibCfg.easing
          });
        }

        let firstFrame = true;
        const timer = new QTimer();
        timer.interval = Math.max(1, Math.floor(1000 / refreshRate));
        timer.timeout.connect(() => {
          const now = Date.now();
          const elapsed = now - startTime;
          const progress = scaledDuration > 0 ? Math.min(elapsed / scaledDuration, 1.0) : 1.0;
          const ease = getEasing(progress, easingType);

          if (firstFrame) {
            focusKick(target, false);
            if (forcePriority) target.fullScreen = true;
            target.opacity = animateOpacity ? 0.0 : 1.0;
            firstFrame = false;
          }

          const currentX = startX + diffX * ease;
          const currentY = startY + diffY * ease;
          if (animateOpacity) {
            const opacityEase = Math.min(1.0, Math.max(0, ease / (showOpacityPoint <= 0 ? 0.0001 : showOpacityPoint)));
            target.opacity = Math.max(target.opacity, startOpacity + (1.0 - startOpacity) * opacityEase);
          } else {
            target.opacity = 1.0;
          }
          target.frameGeometry = { x: currentX, y: currentY, width: finalWidth, height: finalHeight };

          for (const data of siblingDatas) {
            const sibEase = getEasing(progress, data.easing);
            const sibX = data.startX + (data.endX - data.startX) * sibEase;
            const sibY = data.startY + (data.endY - data.startY) * sibEase;
            data.client.frameGeometry = { x: sibX, y: sibY, width: data.client.frameGeometry.width, height: data.client.frameGeometry.height };
            if (animateOpacity) {
              const denom = 1.0 - hideOpacityPoint;
              const opacityEase = Math.min(1.0, Math.max(0, (sibEase - hideOpacityPoint) / (denom <= 0 ? 0.0001 : denom)));
              data.client.opacity = Math.min(data.client.opacity, data.startOpacity * (1.0 - opacityEase));
            }
          }

          if (progress >= 1.0) {
            timer.stop();
            target.opacity = 1.0;
            target.frameGeometry = { x: finalX, y: finalY, width: finalWidth, height: finalHeight };
            focusKick(target, false);
            setForceBlur(target, false);
            for (const data of siblingDatas) {
              data.client.opacity = 0.0;
              data.client.frameGeometry = { x: data.endX, y: data.endY, width: data.client.frameGeometry.width, height: data.client.frameGeometry.height };
            }
          }
        });
        setForceBlur(target, true);
        timer.start();
      } else {
        target.opacity = 1.0;
        target.frameGeometry = { x: finalX, y: finalY, width: finalWidth, height: finalHeight };
      }
    };

    if (needsReposition) {
      target.opacity = 0.0;
      target.fullScreen = false;
      target.frameGeometry = { x: offscreenX, y: offscreenY, width: finalWidth, height: finalHeight };
      startX = offscreenX;
      startY = offscreenY;
      startOpacity = animateOpacity ? 0.0 : 1.0;
      const delayTimer = new QTimer();
      delayTimer.interval = 200;
      delayTimer.singleShot = true;
      delayTimer.timeout.connect(startAnimation);
      delayTimer.start();
    } else {
      startAnimation();
    }
  } else {
    const endX = offscreenX;
    const endY = offscreenY;
    const wasActive = (workspace.activeWindow === target || workspace.activeClient === target);

    if (duration > 0) {
      const startTime = Date.now();
      const diffX = endX - startX;
      const diffY = endY - startY;

      // Implementation of duration scaling (Velocity matching)
      let scaledDuration = duration;
      if (duration > 0) {
        const totalDist = isHorizontalSlide ? finalWidth : finalHeight;
        const currentDist = Math.sqrt(diffX * diffX + diffY * diffY);
        if (totalDist > 0) {
          scaledDuration = Math.min(duration, (duration * currentDist) / totalDist);
        }
      }

      const siblingDatas = [];
      for (const sib of siblingsToHide) {
        const sibContext = resolveAreaContext(sib, "active", 0);
        const sibWorkArea = sibContext.work;
        const sibFullArea = sibContext.full;
        const sibCfg = getSiblingSlideConfig(sib.resourceClass || sib.resourceName);
        const sibOffscreen = computeSlidePosition(sibCfg.dir, sibCfg.val, sibCfg.pct, sibCfg.neg, sibCfg.ctr, sibWorkArea, sibFullArea, sib.frameGeometry.width, sib.frameGeometry.height);
        siblingDatas.push({
          client: sib,
          startX: sib.frameGeometry.x,
          startY: sib.frameGeometry.y,
          startOpacity: sib.opacity,
          endX: sibOffscreen.hiddenX,
          endY: sibOffscreen.hiddenY,
          easing: sibCfg.easing
        });
      }

      const timer = new QTimer();
      timer.interval = Math.max(1, Math.floor(1000 / refreshRate));
      timer.timeout.connect(() => {
        const now = Date.now();
        const elapsed = now - startTime;
        const progress = scaledDuration > 0 ? Math.min(elapsed / scaledDuration, 1.0) : 1.0;
        const ease = getEasing(progress, easingType);
        const currentX = startX + diffX * ease;
        const currentY = startY + diffY * ease;

        if (animateOpacity) {
          const denom = 1.0 - hideOpacityPoint;
          const opacityEase = Math.min(1.0, Math.max(0, (ease - hideOpacityPoint) / (denom <= 0 ? 0.0001 : denom)));
          target.opacity = Math.min(target.opacity, startOpacity * (1.0 - opacityEase));
        }

        target.frameGeometry = { x: currentX, y: currentY, width: finalWidth, height: finalHeight };

        for (const data of siblingDatas) {
          const sibEase = getEasing(progress, data.easing);
          const sibX = data.startX + (data.endX - data.startX) * sibEase;
          const sibY = data.startY + (data.endY - data.startY) * sibEase;
          data.client.frameGeometry = { x: sibX, y: sibY, width: data.client.frameGeometry.width, height: data.client.frameGeometry.height };
          if (animateOpacity) {
            const denom = 1.0 - hideOpacityPoint;
            const opacityEase = Math.min(1.0, Math.max(0, (sibEase - hideOpacityPoint) / (denom <= 0 ? 0.0001 : denom)));
            data.client.opacity = Math.min(data.client.opacity, data.startOpacity * (1.0 - opacityEase));
          }
        }

        if (progress >= 1.0) {
          timer.stop();
          target.opacity = 0.0;
          target.frameGeometry = { x: endX, y: endY, width: finalWidth, height: finalHeight };
          target.fullScreen = false;
          if (target.skipSwitcher !== undefined) target.skipSwitcher = true;
          setForceBlur(target, false);

          for (const data of siblingDatas) {
            data.client.opacity = 0.0;
            data.client.frameGeometry = { x: data.endX, y: data.endY, width: data.client.frameGeometry.width, height: data.client.frameGeometry.height };
          }

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
          if (KWin.callDBus) KWin.callDBus("org.kde.KWin", "/KWin", "org.kde.KWin", "reconfigure");
        }
      });
      setForceBlur(target, true);
      timer.start();
    } else {
      target.opacity = 0.0;
      target.frameGeometry = { x: endX, y: endY, width: finalWidth, height: finalHeight };
      target.fullScreen = false;
      if (target.skipSwitcher !== undefined) target.skipSwitcher = true;
      for (const sib of siblingsToHide) {
        sib.opacity = 0.0;
        const sibArea = workspace.clientArea(KWin.PlacementArea, sib);
        const sibCfg = getSiblingSlideConfig(sib.resourceClass || sib.resourceName);
        const sibOffscreen = computeSlidePosition(sibCfg.dir, sibCfg.val, sibCfg.pct, sibCfg.neg, sibCfg.ctr, sibArea, sib.frameGeometry.width, sib.frameGeometry.height);
        sib.frameGeometry = { x: sibOffscreen.hiddenX, y: sibOffscreen.hiddenY, width: sib.frameGeometry.width, height: sib.frameGeometry.height };
      }
      if (KWin.callDBus) KWin.callDBus("org.kde.KWin", "/KWin", "org.kde.KWin", "reconfigure");
    }
  }
});
