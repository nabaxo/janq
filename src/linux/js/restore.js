(function (windowClass, targetWindowId, targetPid) {
  /*{{COMMON_KWIN_JS}}*/
  const target = findTarget(windowClass, targetWindowId, targetPid);
  if (!target) {
    console.log(`janq_restore: Could not find window for ${windowClass}`);
    return;
  }

  console.log(`janq_restore: Restoring window ${windowClass}`);
  const area = workspace.clientArea(KWin.PlacementArea, target);
  const geo = target.frameGeometry;

  resetQuakeProperties(target);
  focusKick(target, true);

  target.frameGeometry = {
    x: area.x + (area.width - geo.width) / 2,
    y: area.y + 100,
    width: geo.width,
    height: geo.height
  };
});
