(function (
  windowClass, displayMode, displayIndex, width, isWidthPercent, height, isHeightPercent,
  duration, easingType, shouldShow, keepAbove, animateOpacity,
  showOpacityPoint, hideOpacityPoint, prevWindowId, targetWindowId, targetPid, janqClasses,
  forcePriority, refreshRate
) {
  /*{{COMMON_KWIN_JS}}*/

  const target = findTarget(windowClass, targetWindowId, targetPid);
  if (!target) return;

  const currentArea = workspace.clientArea(KWin.PlacementArea, target);
  const area = shouldShow ? resolveArea(target, displayMode, displayIndex, currentArea) : currentArea;

  let startX = target.frameGeometry.x;
  let startY = target.frameGeometry.y;
  let startOpacity = target.opacity;
  const areaTop = area.y;
  const offscreenY = areaTop - target.frameGeometry.height;

  const onWrongMonitor = (startX < area.x - 10) || (startX > area.x + area.width + 10);
  const needsReposition = onWrongMonitor || (startY < offscreenY - 50) || (startY > areaTop + 50);

  let finalWidth = target.frameGeometry.width;
  let finalHeight = target.frameGeometry.height;

  if (needsReposition) {
    const dims = resolveDimensions(width, isWidthPercent, height, isHeightPercent, area, target);
    finalWidth = dims.width;
    finalHeight = dims.height;
  }

  const finalX = area.x + (area.width - finalWidth) / 2;
  const finalY = area.y;

  const clients = workspace.windowList ? workspace.windowList() : workspace.clientList();
  const rawClasses = (janqClasses || "").toLowerCase().split(",");
  const allClasses = [];
  for (let k = 0; k < rawClasses.length; k++) {
    const trimmed = rawClasses[k].trim();
    if (trimmed) allClasses.push(trimmed);
  }

  const siblingsToHide = [];
  for (let i = 0; i < clients.length; i++) {
    const c = clients[i];
    if (c === target) continue;
    const cClass = (c.resourceClass || "").toLowerCase();
    const cName = (c.resourceName || "").toLowerCase();

    let isManaged = false;
    for (let j = 0; j < allClasses.length; j++) {
      const siblingClass = allClasses[j];
      if (cClass.includes(siblingClass) || cName.includes(siblingClass)) {
        isManaged = true;
        break;
      }
    }

    if (isManaged) {
      const cArea = workspace.clientArea(KWin.PlacementArea, c);
      if (c.opacity > 0.01 && c.frameGeometry.y + c.frameGeometry.height > cArea.y + 1) {
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
        const diff = finalY - startY;
        const siblingDatas = [];
        for (let s = 0; s < siblingsToHide.length; s++) {
          const sib = siblingsToHide[s];
          const sibArea = workspace.clientArea(KWin.PlacementArea, sib);
          siblingDatas.push({
            client: sib,
            startY: sib.frameGeometry.y,
            startOpacity: sib.opacity,
            endY: sibArea.y - sib.frameGeometry.height
          });
        }

        let firstFrame = true;
        const timer = new QTimer();
        timer.interval = Math.max(1, Math.floor(1000 / refreshRate));
        timer.timeout.connect(() => {
          const now = Date.now();
          const elapsed = now - startTime;
          const progress = Math.min(elapsed / duration, 1.0);
          const ease = getEasing(progress, easingType);

          if (firstFrame) {
            focusKick(target, false);
            if (forcePriority) target.fullScreen = true;
            target.opacity = animateOpacity ? 0.0 : 1.0;
            firstFrame = false;
          }

          const currentY = startY + diff * ease;
          if (animateOpacity) {
            const opacityEase = Math.min(1.0, Math.max(0, ease / (showOpacityPoint <= 0 ? 0.0001 : showOpacityPoint)));
            target.opacity = Math.max(target.opacity, startOpacity + (1.0 - startOpacity) * opacityEase);
          } else {
            target.opacity = 1.0;
          }
          target.frameGeometry = { x: finalX, y: currentY, width: finalWidth, height: finalHeight };

          for (let d = 0; d < siblingDatas.length; d++) {
            const data = siblingDatas[d];
            const sibY = data.startY + (data.endY - data.startY) * ease;
            data.client.frameGeometry = { x: data.client.frameGeometry.x, y: sibY, width: data.client.frameGeometry.width, height: data.client.frameGeometry.height };
            if (animateOpacity) {
              const denom = 1.0 - hideOpacityPoint;
              const opacityEase = Math.min(1.0, Math.max(0, (ease - hideOpacityPoint) / (denom <= 0 ? 0.0001 : denom)));
              data.client.opacity = Math.min(data.client.opacity, data.startOpacity * (1.0 - opacityEase));
            }
          }

          if (progress >= 1.0) {
            timer.stop();
            target.opacity = 1.0;
            target.frameGeometry = { x: finalX, y: finalY, width: finalWidth, height: finalHeight };
            focusKick(target, false);
            setForceBlur(target, false);
            for (let d = 0; d < siblingDatas.length; d++) {
              const data = siblingDatas[d];
              data.client.opacity = 0.0;
              const sibArea = workspace.clientArea(KWin.PlacementArea, data.client);
              data.client.frameGeometry = { x: data.client.frameGeometry.x, y: sibArea.y - data.client.frameGeometry.height, width: data.client.frameGeometry.width, height: data.client.frameGeometry.height };
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
      const jumpY = areaTop - finalHeight;
      target.frameGeometry = { x: finalX, y: jumpY, width: finalWidth, height: finalHeight };
      startX = finalX;
      startY = jumpY;
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
    const endY = area.y - finalHeight;
    const wasActive = (workspace.activeWindow === target || workspace.activeClient === target);

    if (duration > 0) {
      const startTime = Date.now();
      const diff = endY - startY;

      const siblingDatas = [];
      for (let s = 0; s < siblingsToHide.length; s++) {
        const sib = siblingsToHide[s];
        const sibArea = workspace.clientArea(KWin.PlacementArea, sib);
        siblingDatas.push({
          client: sib,
          startY: sib.frameGeometry.y,
          startOpacity: sib.opacity,
          endY: sibArea.y - sib.frameGeometry.height
        });
      }

      const timer = new QTimer();
      timer.interval = Math.max(1, Math.floor(1000 / refreshRate));
      timer.timeout.connect(() => {
        const now = Date.now();
        const elapsed = now - startTime;
        const progress = Math.min(elapsed / duration, 1.0);
        const ease = getEasing(progress, easingType);
        const currentY = startY + diff * ease;

        if (animateOpacity) {
          const denom = 1.0 - hideOpacityPoint;
          const opacityEase = Math.min(1.0, Math.max(0, (ease - hideOpacityPoint) / (denom <= 0 ? 0.0001 : denom)));
          target.opacity = Math.min(target.opacity, startOpacity * (1.0 - opacityEase));
        }

        target.frameGeometry = { x: finalX, y: currentY, width: finalWidth, height: finalHeight };

        for (let d = 0; d < siblingDatas.length; d++) {
          const data = siblingDatas[d];
          const sibY = data.startY + (data.endY - data.startY) * ease;
          data.client.frameGeometry = { x: data.client.frameGeometry.x, y: sibY, width: data.client.frameGeometry.width, height: data.client.frameGeometry.height };
          if (animateOpacity) {
            const denom = 1.0 - hideOpacityPoint;
            const opacityEase = Math.min(1.0, Math.max(0, (ease - hideOpacityPoint) / (denom <= 0 ? 0.0001 : denom)));
            data.client.opacity = Math.min(data.client.opacity, data.startOpacity * (1.0 - opacityEase));
          }
        }

        if (progress >= 1.0) {
          timer.stop();
          target.opacity = 0.0;
          target.frameGeometry = { x: finalX, y: endY, width: finalWidth, height: finalHeight };
          target.fullScreen = false;
          if (target.skipSwitcher !== undefined) target.skipSwitcher = true;
          setForceBlur(target, false);

          for (let d = 0; d < siblingDatas.length; d++) {
            const data = siblingDatas[d];
            data.client.opacity = 0.0;
            const sibArea = workspace.clientArea(KWin.PlacementArea, data.client);
            data.client.frameGeometry = { x: data.client.frameGeometry.x, y: sibArea.y - data.client.frameGeometry.height, width: data.client.frameGeometry.width, height: data.client.frameGeometry.height };
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
              for (let j = 0; j < allClients.length; j++) {
                const c = allClients[j];
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
      target.frameGeometry = { x: finalX, y: endY, width: finalWidth, height: finalHeight };
      target.fullScreen = false;
      if (target.skipSwitcher !== undefined) target.skipSwitcher = true;
      for (let i = 0; i < siblingsToHide.length; i++) {
        const sib = siblingsToHide[i];
        sib.opacity = 0.0;
        const sibArea = workspace.clientArea(KWin.PlacementArea, sib);
        sib.frameGeometry = { x: sib.frameGeometry.x, y: sibArea.y - sib.frameGeometry.height, width: sib.frameGeometry.width, height: sib.frameGeometry.height };
      }
      if (KWin.callDBus) KWin.callDBus("org.kde.KWin", "/KWin", "org.kde.KWin", "reconfigure");
    }
  }
});
