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

      resetQuakeProperties(c);
      focusKick(c, true);

      c.frameGeometry = {
        x: area.x + (area.width - geo.width) / 2,
        y: area.y + 100,
        width: geo.width,
        height: geo.height
      };
    }
  }
});
