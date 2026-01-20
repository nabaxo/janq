(function (apps) {
  /*{{COMMON_KWIN_JS}}*/

  // Compute position along edge based on slide direction and offset
  function computeSlidePosition(direction, offsetVal, isPercent, isNegative, isCenter, area, winW, winH) {
    let shownX, shownY, hiddenX, hiddenY;

    if (direction === "top" || direction === "bottom") {
      if (isCenter) {
        shownX = area.x + (area.width - winW) / 2;
      } else if (isPercent) {
        const pct = offsetVal / 100;
        shownX = isNegative
          ? area.x + area.width - winW - (area.width * pct)
          : area.x + (area.width * pct);
      } else {
        shownX = isNegative
          ? area.x + area.width - winW - offsetVal
          : area.x + offsetVal;
      }

      if (direction === "top") {
        shownY = area.y;
        hiddenY = area.y - winH;
      } else {
        shownY = area.y + area.height - winH;
        hiddenY = area.y + area.height;
      }
      hiddenX = shownX;
    } else {
      if (isCenter) {
        shownY = area.y + (area.height - winH) / 2;
      } else if (isPercent) {
        const pct = offsetVal / 100;
        shownY = isNegative
          ? area.y + area.height - winH - (area.height * pct)
          : area.y + (area.height * pct);
      } else {
        shownY = isNegative
          ? area.y + area.height - winH - offsetVal
          : area.y + offsetVal;
      }

      if (direction === "left") {
        shownX = area.x;
        hiddenX = area.x - winW;
      } else {
        shownX = area.x + area.width - winW;
        hiddenX = area.x + area.width;
      }
      hiddenY = shownY;
    }

    return { shownX, shownY, hiddenX, hiddenY };
  }

  for (const app of apps) {
    const target = findTarget(app.windowClass, app.targetWindowId, app.targetPid);

    if (target) {
      console.log(`janq_grab: Grabbing window for ${app.windowClass} (id: ${target.internalId}, pid: ${target.pid})`);
      setQuakeProperties(target, app.keepAbove, app.isVisible, app.forcePriority);

      const currentArea = workspace.clientArea(KWin.PlacementArea, target);
      const area = resolveArea(target, app.displayMode, app.displayIndex, currentArea);
      const dims = resolveDimensions(app.width, app.isWidthPercent, app.height, app.isHeightPercent, area, target);
      const slidePos = computeSlidePosition(
        app.slideFrom || "top",
        app.offsetValue || 0,
        app.offsetIsPercent || false,
        app.offsetIsNegative || false,
        app.offsetIsCenter !== false,
        area,
        dims.width,
        dims.height
      );

      if (!app.isVisible) {
        console.log(`janq_grab: Parking ${app.windowClass} offscreen (${app.slideFrom || "top"}).`);
        target.opacity = 0.0;
        target.frameGeometry = { x: slidePos.hiddenX, y: slidePos.hiddenY, width: dims.width, height: dims.height };
      } else {
        console.log(`janq_grab: Skipping position update for ${app.windowClass} (already visible).`);
      }
    } else {
      console.log(`janq_grab: FAILED to find window for ${app.windowClass}`);
    }
  }
});
