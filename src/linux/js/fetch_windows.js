(function (requestId) {
  /*{{COMMON_KWIN_JS}}*/
  const windows = workspace.windowList ? workspace.windowList() : workspace.clientList();
  const results = [];
  for (const w of windows) {
    results.push(`${w.internalId}|${w.resourceClass || ""}|${w.pid}|${w.visible ? "1" : "0"}`);
  }
  callDBus(
    "dev.nabaxo.janq",
    "/dev/nabaxo/janq",
    "dev.nabaxo.janq",
    "ReportWindowMetadata",
    requestId + ":" + results.join(";")
  );
});
