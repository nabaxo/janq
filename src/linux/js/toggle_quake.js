(function (
  windowClass, displayMode, displayIndex, width, isWidthPercent, height, isHeightPercent,
  duration, easingType, shouldShow, keepAbove, animateOpacity,
  showOpacityPoint, hideOpacityPoint, prevWindowId, targetWindowId, targetPid, janqClasses,
  forcePriority, refreshRate
) {
  /*{{COMMON_KWIN_JS}}*/

  var target = findTarget(windowClass, targetWindowId, targetPid);
  if (!target) return;

  var currentArea = workspace.clientArea(KWin.PlacementArea, target);
  var area = shouldShow ? resolveArea(target, displayMode, displayIndex, currentArea) : currentArea;

  var startX = target.frameGeometry.x;
  var startY = target.frameGeometry.y;
  var startOpacity = target.opacity;
  var areaTop = area.y;
  var offscreenY = areaTop - target.frameGeometry.height;

  var onWrongMonitor = (startX < area.x - 10) || (startX > area.x + area.width + 10);
  var needsReposition = onWrongMonitor || (startY < offscreenY - 50) || (startY > areaTop + 50);

  var finalWidth = target.frameGeometry.width;
  var finalHeight = target.frameGeometry.height;

  if (needsReposition) {
    var dims = resolveDimensions(width, isWidthPercent, height, isHeightPercent, area, target);
    finalWidth = dims.width;
    finalHeight = dims.height;
  }

  var finalX = area.x + (area.width - finalWidth) / 2;
  var finalY = area.y;

  var clients = workspace.windowList ? workspace.windowList() : workspace.clientList();
  var rawClasses = (janqClasses || "").toLowerCase().split(",");
  var allClasses = [];
  for (var k = 0; k < rawClasses.length; k++) {
    var trimmed = rawClasses[k].replace(/^\s+|\s+$/g, "");
    if (trimmed) allClasses.push(trimmed);
  }

  var siblingsToHide = [];
  for (var i = 0; i < clients.length; i++) {
    var c = clients[i];
    if (c === target) continue;
    var cClass = (c.resourceClass || "").toLowerCase();
    var cName = (c.resourceName || "").toLowerCase();

    var isManaged = false;
    for (var j = 0; j < allClasses.length; j++) {
      var siblingClass = allClasses[j];
      if (cClass.indexOf(siblingClass) !== -1 || cName.indexOf(siblingClass) !== -1) {
        isManaged = true;
        break;
      }
    }

    if (isManaged) {
      var cArea = workspace.clientArea(KWin.PlacementArea, c);
      if (c.opacity > 0.01 && c.frameGeometry.y + c.frameGeometry.height > cArea.y + 1) {
        siblingsToHide.push(c);
      }
    }
  }

  if (shouldShow) {
    setQuakeProperties(target, keepAbove, true, forcePriority);
    focusKick(target, false);

    function startAnimation() {
      if (duration > 0) {
        var startTime = Date.now();
        var diff = finalY - startY;
        var siblingDatas = [];
        for (var s = 0; s < siblingsToHide.length; s++) {
          var sib = siblingsToHide[s];
          var sibArea = workspace.clientArea(KWin.PlacementArea, sib);
          siblingDatas.push({
            client: sib,
            startY: sib.frameGeometry.y,
            startOpacity: sib.opacity,
            endY: sibArea.y - sib.frameGeometry.height
          });
        }

        var firstFrame = true;
        var timer = new QTimer();
        timer.interval = Math.max(1, Math.floor(1000 / refreshRate));
        timer.timeout.connect(function () {
          var now = Date.now();
          var elapsed = now - startTime;
          var progress = Math.min(elapsed / duration, 1.0);
          var ease = getEasing(progress, easingType);

          if (firstFrame) {
            focusKick(target, false);
            if (forcePriority) target.fullScreen = true;
            if (animateOpacity) target.opacity = 0.0;
            else target.opacity = 1.0;
            firstFrame = false;
          }

          var currentY = startY + diff * ease;
          if (animateOpacity) {
            var opacityEase = Math.min(1.0, Math.max(0, ease / (showOpacityPoint <= 0 ? 0.0001 : showOpacityPoint)));
            target.opacity = Math.max(target.opacity, startOpacity + (1.0 - startOpacity) * opacityEase);
          } else {
            target.opacity = 1.0;
          }
          target.frameGeometry = { x: finalX, y: currentY, width: finalWidth, height: finalHeight };

          for (var d = 0; d < siblingDatas.length; d++) {
            var data = siblingDatas[d];
            var sibY = data.startY + (data.endY - data.startY) * ease;
            data.client.frameGeometry = { x: data.client.frameGeometry.x, y: sibY, width: data.client.frameGeometry.width, height: data.client.frameGeometry.height };
            if (animateOpacity) {
              var denom = 1.0 - hideOpacityPoint;
              var opacityEase = Math.min(1.0, Math.max(0, (ease - hideOpacityPoint) / (denom <= 0 ? 0.0001 : denom)));
              data.client.opacity = Math.min(data.client.opacity, data.startOpacity * (1.0 - opacityEase));
            }
          }

          if (progress >= 1.0) {
            timer.stop();
            target.opacity = 1.0;
            target.frameGeometry = { x: finalX, y: finalY, width: finalWidth, height: finalHeight };
            focusKick(target, false);
            for (var d = 0; d < siblingDatas.length; d++) {
              var data = siblingDatas[d];
              data.client.opacity = 0.0;
              var sibArea = workspace.clientArea(KWin.PlacementArea, data.client);
              data.client.frameGeometry = { x: data.client.frameGeometry.x, y: sibArea.y - data.client.frameGeometry.height, width: data.client.frameGeometry.width, height: data.client.frameGeometry.height };
            }
          }
        });
        timer.start();
      } else {
        target.opacity = 1.0;
        target.frameGeometry = { x: finalX, y: finalY, width: finalWidth, height: finalHeight };
      }
    }

    if (needsReposition) {
      target.opacity = 0.0;
      target.fullScreen = false;
      var jumpY = areaTop - finalHeight;
      target.frameGeometry = { x: finalX, y: jumpY, width: finalWidth, height: finalHeight };
      startX = finalX;
      startY = jumpY;
      if (animateOpacity) startOpacity = 0.0;
      else startOpacity = 1.0;
      var delayTimer = new QTimer();
      delayTimer.interval = 200;
      delayTimer.singleShot = true;
      delayTimer.timeout.connect(startAnimation);
      delayTimer.start();
    } else {
      startAnimation();
    }
  } else {
    var endY = area.y - finalHeight;
    var wasActive = (workspace.activeWindow === target || workspace.activeClient === target);

    if (duration > 0) {
      var startTime = Date.now();
      var diff = endY - startY;

      var siblingDatas = [];
      for (var s = 0; s < siblingsToHide.length; s++) {
        var sib = siblingsToHide[s];
        var sibArea = workspace.clientArea(KWin.PlacementArea, sib);
        siblingDatas.push({
          client: sib,
          startY: sib.frameGeometry.y,
          startOpacity: sib.opacity,
          endY: sibArea.y - sib.frameGeometry.height
        });
      }

      var timer = new QTimer();
      timer.interval = Math.max(1, Math.floor(1000 / refreshRate));
      timer.timeout.connect(function () {
        var now = Date.now();
        var elapsed = now - startTime;
        var progress = Math.min(elapsed / duration, 1.0);
        var ease = getEasing(progress, easingType);
        var currentY = startY + diff * ease;

        if (animateOpacity) {
          var denom = 1.0 - hideOpacityPoint;
          var opacityEase = Math.min(1.0, Math.max(0, (ease - hideOpacityPoint) / (denom <= 0 ? 0.0001 : denom)));
          target.opacity = Math.min(target.opacity, startOpacity * (1.0 - opacityEase));
        }

        target.frameGeometry = { x: finalX, y: currentY, width: finalWidth, height: finalHeight };

        for (var d = 0; d < siblingDatas.length; d++) {
          var data = siblingDatas[d];
          var sibY = data.startY + (data.endY - data.startY) * ease;
          data.client.frameGeometry = { x: data.client.frameGeometry.x, y: sibY, width: data.client.frameGeometry.width, height: data.client.frameGeometry.height };
          if (animateOpacity) {
            var denom = 1.0 - hideOpacityPoint;
            var opacityEase = Math.min(1.0, Math.max(0, (ease - hideOpacityPoint) / (denom <= 0 ? 0.0001 : denom)));
            data.client.opacity = Math.min(data.client.opacity, data.startOpacity * (1.0 - opacityEase));
          }
        }

        if (progress >= 1.0) {
          timer.stop();
          target.opacity = 0.0;
          target.frameGeometry = { x: finalX, y: endY, width: finalWidth, height: finalHeight };
          target.fullScreen = false;
          if (target.skipSwitcher !== undefined) target.skipSwitcher = true;

          for (var d = 0; d < siblingDatas.length; d++) {
            var data = siblingDatas[d];
            data.client.opacity = 0.0;
            var sibArea = workspace.clientArea(KWin.PlacementArea, data.client);
            data.client.frameGeometry = { x: data.client.frameGeometry.x, y: sibArea.y - data.client.frameGeometry.height, width: data.client.frameGeometry.width, height: data.client.frameGeometry.height };
          }

          var stillActive = (workspace.activeWindow === target || workspace.activeClient === target);
          if (wasActive && stillActive) {
            var stacking = workspace.stackingOrder;
            var targetBehind = null;
            var targetIndex = -1;
            for (var s = 0; s < stacking.length; s++) {
              if (stacking[s] === target) {
                targetIndex = s;
                break;
              }
            }
            if (targetIndex > 0) {
              for (var s = targetIndex - 1; s >= 0; s--) {
                var c = stacking[s];
                if (c.normalWindow && c.opacity > 0 && (c.resourceClass || c.resourceName)) {
                  targetBehind = c;
                  break;
                }
              }
            }

            if (targetBehind) {
              focusKick(targetBehind, false);
            } else if (prevWindowId && prevWindowId !== "") {
              var allClients = workspace.windowList ? workspace.windowList() : workspace.clientList();
              for (var j = 0; j < allClients.length; j++) {
                var c = allClients[j];
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
      timer.start();
    } else {
      target.opacity = 0.0;
      target.frameGeometry = { x: finalX, y: endY, width: finalWidth, height: finalHeight };
      target.fullScreen = false;
      if (target.skipSwitcher !== undefined) target.skipSwitcher = true;
      for (var i = 0; i < siblingsToHide.length; i++) {
        var sib = siblingsToHide[i];
        sib.opacity = 0.0;
        var sibArea = workspace.clientArea(KWin.PlacementArea, sib);
        sib.frameGeometry = { x: sib.frameGeometry.x, y: sibArea.y - sib.frameGeometry.height, width: sib.frameGeometry.width, height: sib.frameGeometry.height };
      }
      if (KWin.callDBus) KWin.callDBus("org.kde.KWin", "/KWin", "org.kde.KWin", "reconfigure");
    }
  }
});
