(function (apps) {
  /*{{COMMON_KWIN_JS}}*/

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
});
