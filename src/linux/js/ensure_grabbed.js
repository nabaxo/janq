(function (apps) {
  /*{{COMMON_KWIN_JS}}*/

  for (const app of apps) {
    const target = findTarget(app.windowClass, app.targetWindowId, app.targetPid);

    if (target) {
      console.log(`janq_grab: Grabbing window for ${app.windowClass} (id: ${target.internalId}, pid: ${target.pid})`);
      setQuakeProperties(target, app.keepAbove, app.noBorders, app.skipPager, app.isVisible, app.forcePriority, app.allDesktops);

      const context = resolveAreaContext(target, app.displayMode, app.displayIndex);
      const area = context.work;
      const dims = resolveDimensions(app.width, app.isWidthPercent, app.height, app.isHeightPercent, area, target);
      const slidePos = computeSlidePosition(
        app.slideFrom || "top",
        app.offsetValue || 0,
        app.offsetIsPercent || false,
        app.offsetIsNegative || false,
        app.offsetIsCenter !== false,
        area,
        context.full,
        dims.width,
        dims.height,
        app.depthValue || 0,
        app.depthIsPercent || false,
        app.depthIsNegative || false,
        app.depthIsCenter || false
      );

      if (app.hideTitlebar && (app.slideFrom || "top") === "top" && target.clientGeometry) {
        const tb = Math.max(0, target.clientGeometry.y - target.frameGeometry.y);
        slidePos.shownY -= tb;
      }

      if (!app.isVisible) {
        console.log(`janq_grab: Parking ${app.windowClass} offscreen (${app.slideFrom || "top"}).`);
        target.opacity = 0.0;
        target.frameGeometry = { x: slidePos.hiddenX, y: slidePos.hiddenY, width: dims.width, height: dims.height };
      } else {
        console.log(`janq_grab: Restoring ${app.windowClass} to shown position.`);
        target.opacity = 1.0;
        target.frameGeometry = { x: slidePos.shownX, y: slidePos.shownY, width: dims.width, height: dims.height };
      }
    } else {
      console.log(`janq_grab: FAILED to find window for ${app.windowClass}`);
    }
  }
});
