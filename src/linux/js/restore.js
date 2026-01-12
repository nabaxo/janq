(function (windowClass) {
  /*{{COMMON_KWIN_JS}}*/
  const clients = workspace.windowList ? workspace.windowList() : workspace.clientList();
  const searchClass = windowClass.toLowerCase();

  for (const c of clients) {
    const cClass = (c.resourceClass || "").toLowerCase();
    const cName = (c.resourceName || "").toLowerCase();

    if (cClass.includes(searchClass) || cName.includes(searchClass)) {
      console.log(`janq_restore: Restoring window ${cClass}`);
      const area = workspace.clientArea(KWin.PlacementArea, c);
      const geo = c.frameGeometry;
      const needsCenter = (geo.y + geo.height <= area.y + 50 || c.opacity < 0.1 || geo.y < area.y + 10);

      resetQuakeProperties(c);
      focusKick(c, true);

      if (needsCenter) {
        c.frameGeometry = {
          x: area.x + (area.width - geo.width) / 2,
          y: area.y + 100,
          width: geo.width,
          height: geo.height
        };
      }
    }
  }
});
