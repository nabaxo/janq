(function (requestId) {
  const active = workspace.activeWindow || workspace.activeClient;
  if (!active) {
    callDBus(
      "dev.nabaxo.janq",
      "/dev/nabaxo/janq",
      "dev.nabaxo.janq",
      "ReportActiveWindow",
      requestId + "::"
    );
    return;
  }
  const id = active.internalId ? active.internalId.toString() : "";
  const cls = (active.resourceClass || "").toString();
  callDBus(
    "dev.nabaxo.janq",
    "/dev/nabaxo/janq",
    "dev.nabaxo.janq",
    "ReportActiveWindow",
    requestId + ":" + id + ":" + cls
  );
});
