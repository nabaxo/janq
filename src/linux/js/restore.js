(function (windowClass) {
  /*{{COMMON_KWIN_JS}}*/
  var clients = workspace.windowList ? workspace.windowList() : workspace.clientList();
  for (var i = 0; i < clients.length; i++) {
    var c = clients[i];
    var cClass = (c.resourceClass || "").toLowerCase();
    var cName = (c.resourceName || "").toLowerCase();
    if (cClass.indexOf(windowClass.toLowerCase()) !== -1 || cName.indexOf(windowClass.toLowerCase()) !== -1) {
      console.log("janq_restore: Restoring window " + cClass);
      var area = workspace.clientArea(KWin.PlacementArea, c);
      var geo = c.frameGeometry;
      var needsCenter = (geo.y + geo.height <= area.y + 50 || c.opacity < 0.1 || geo.y < area.y + 10);

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
