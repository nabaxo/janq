package main

import (
	"fmt"
	"log"
	"os"
	"sync"
	"time"

	"github.com/godbus/dbus/v5"
)

// Global state to track toggle direction
var targetVisible bool = false
var toggleMutex sync.Mutex
var lastScriptID string // Track last script name to unload

// Updated KWin script with Interruptible Animation logic & New Easings
const kwinScriptTemplate = `
// Compatibility
var clients = workspace.windowList ? workspace.windowList() : workspace.clientList();
var target = null;
var windowClass = "%s";

for (var i = 0; i < clients.length; i++) {
  var c = clients[i];
  var match = false;
  if (c.resourceClass && c.resourceClass.toLowerCase() == windowClass.toLowerCase()) match = true;
  else if (c.resourceName && c.resourceName.toLowerCase() == windowClass.toLowerCase()) match = true;
  else if (c.caption && c.caption.toLowerCase().indexOf(windowClass.toLowerCase()) !== -1) match = true;
  if (match) { target = c; break; }
}

function getEasing(progress, type) {
  switch (type) {
    case "linear": return progress;
    case "ease-in": return progress * progress;
    case "ease-out": return progress * (2 - progress);
    case "ease-in-out":
      return progress < .5 ? 2 * progress * progress : -1 + (4 - 2 * progress) * progress;
    // New Easings (and aliases)
    case "sine-in": case "ease-in-sine": return 1 - Math.cos((progress * Math.PI) / 2);
    case "sine-out": case "ease-out-sine": return Math.sin((progress * Math.PI) / 2);
    case "sine-in-out": case "ease-in-out-sine": return -(Math.cos(Math.PI * progress) - 1) / 2;
    case "quart-in": case "ease-in-quart": return progress * progress * progress * progress;
    case "quart-out": case "ease-out-quart": return 1 - Math.pow(1 - progress, 4);
    case "quart-in-out": case "ease-in-out-quart": return progress < 0.5 ? 8 * Math.pow(progress, 4) : 1 - Math.pow(-2 * progress + 2, 4) / 2;
    case "cubic-in": case "ease-in-cubic": return progress * progress * progress;
    case "cubic-out": case "ease-out-cubic": return 1 - Math.pow(1 - progress, 3);
    case "cubic-in-out": case "ease-in-out-cubic": return progress < 0.5 ? 4 * Math.pow(progress, 3) : 1 - Math.pow(-2 * progress + 2, 3) / 2;
    case "back-in": case "ease-in-back": var c1 = 1.70158; var c3 = c1 + 1; return c3 * progress * progress * progress - c1 * progress * progress;
    case "back-out": case "ease-out-back": var c1 = 1.70158; var c3 = c1 + 1; return 1 + c3 * Math.pow(progress - 1, 3) + c1 * Math.pow(progress - 1, 2);
    default: return progress * (2 - progress); // ease-out default
  }
}

if (target) {
  // Config Paramters
  var widthPct = %d / 100.0;
  var heightPct = %d / 100.0;
  var widthCols = %d;
  var heightRows = %d;
  var duration = %d;
  var easingType = "%s";
  var shouldShow = %t;
  var keepAbove = %t;
  var animateOpacity = %t;
  var opacityPoint = %f;

  // Get actual cursor position and find which screen it's on
  var cursorPos = workspace.cursorPos;
  var mouseArea = null;

  // Find the screen containing the cursor
  var screens = workspace.screens;
  for (var i = 0; i < screens.length; i++) {
    var geo = screens[i].geometry;
    if (cursorPos.x >= geo.x && cursorPos.x < geo.x + geo.width &&
      cursorPos.y >= geo.y && cursorPos.y < geo.y + geo.height) {
      mouseArea = geo;
      break;
    }
  }
  // Fallback to activeScreen if cursor detection fails
  if (!mouseArea) {
    mouseArea = workspace.activeScreen.geometry;
  }

  var currentArea = workspace.clientArea(KWin.PlacementArea, target);

  // We consider it mostly hidden if it's minimized OR if less than 5px is visible.
  var isMostlyHidden = target.minimized || (target.frameGeometry.y + target.frameGeometry.height <= currentArea.y + 5);

  // When SHOWING: always use mouse screen to prevent see-sawing between displays
  // When HIDING: use the window's current screen so it hides upward from where it is
  var area = shouldShow ? mouseArea : currentArea;

  // Track if we need to reposition (showing on a different screen than current)
  var needsReposition = shouldShow && (isMostlyHidden || currentArea.x != mouseArea.x || currentArea.y != mouseArea.y);

  // Target Geometry
  var finalWidth = ( widthCols > 0 ) ? target.frameGeometry.width : area.width * widthPct;
  var finalHeight = ( heightRows > 0 ) ? target.frameGeometry.height : area.height * heightPct;
  var finalX = area.x + (area.width - finalWidth) / 2;
  var finalY = area.y;

  // Properties
  target.keepAbove = keepAbove;
  target.onAllDesktops = true;
  target.noBorder = true;
  target.skipTaskbar = true;
  target.skipPager = true;

  if (shouldShow) {
    // SHOWING
    if (target.minimized) {
      target.minimized = false;
    }
    target.opacity = 1.0;

    if (workspace.activeWindow !== undefined) workspace.activeWindow = target;
    else workspace.activeClient = target;

    // Animation Start Point
    var startY = target.frameGeometry.y;

    // If we need to reposition (was hidden or on different screen), snap to new screen off-screen
    if (needsReposition) {
      startY = finalY - finalHeight;
      if (animateOpacity) {
        target.opacity = 0.0; // Start invisible for fade-in
      }
      target.frameGeometry = {
        x: finalX,
        y: startY,
        width: finalWidth,
        height: finalHeight
      };
    }
    var startOpacity = animateOpacity ? target.opacity : 1.0;

    // Setup timer
    if (duration > 0) {
      var endY = finalY;
      var startTime = new Date().getTime();
      var diff = endY - startY;

      var timer = new QTimer();
      timer.interval = 16;
      timer.timeout.connect(function() {
        var now = new Date().getTime();
        var elapsed = now - startTime;
        var progress = Math.min(elapsed / duration, 1.0);
        var ease = getEasing(progress, easingType);

        var currentY = startY + diff * ease;

        if (animateOpacity) {
          // Opacity completes at opacityPoint of animation (faster fade-in)
          var opacityProgress = Math.min(progress / opacityPoint, 1.0);
          var currentOpacity = startOpacity + (1.0 - startOpacity) * opacityProgress;
          target.opacity = currentOpacity;
        }

        target.frameGeometry = {
          x: finalX,
          y: currentY,
          width: finalWidth,
          height: finalHeight
        };

        if (progress >= 1.0) {
          timer.stop();
          target.opacity = 1.0;
          target.frameGeometry = { x: finalX, y: finalY, width: finalWidth, height: finalHeight };
        }
      });
      timer.start();
    } else {
      target.frameGeometry = { x: finalX, y: finalY, width: finalWidth, height: finalHeight };
    }

  } else {
    // HIDING
    var currentGeo = target.frameGeometry;
    var startY = currentGeo.y;
    var startX = currentGeo.x;
    var startW = currentGeo.width;
    var startH = currentGeo.height;

    // Goal: Move up until completely off screen
    var endY = area.y - startH;

    if (duration > 0) {
      var startTime = new Date().getTime();
      var diff = endY - startY;

      var timer = new QTimer();
      timer.interval = 16;
      timer.timeout.connect(function() {
        var now = new Date().getTime();
        var elapsed = now - startTime;
        var progress = Math.min(elapsed / duration, 1.0);
        var ease = getEasing(progress, easingType);

        var currentY = startY + diff * ease;

        if (animateOpacity) {
          // Opacity starts fading at opacityPoint of animation (delayed fade-out)
          var opacityProgress = Math.max((progress - opacityPoint) / (1.0 - opacityPoint), 0.0);
          var currentOpacity = 1.0 - opacityProgress;
          target.opacity = currentOpacity;
        }

        target.frameGeometry = {
          x: startX,
          y: currentY,
          width: startW,
          height: startH
        };

        if (progress >= 1.0) {
          timer.stop();
          target.opacity = 0.0;

          // var minTimer = new QTimer();
          // minTimer.interval = 32; // Small 32ms delay (2 frames) for KWin state sync
          // minTimer.singleShot = true;
          // minTimer.timeout.connect(function() {
          //     target.minimized = true;
          // });
          // minTimer.start();
        }
      });
      timer.start();
    } else {
      target.frameGeometry = { x: startX, y: endY, width: startW, height: startH };
      target.opacity = 0.0;
      target.minimized = true;
    }
  }
} else {
  // No target window found!
}
`

const initScriptTemplate = `
var clients = workspace.windowList ? workspace.windowList() : workspace.clientList();
var target = null;
var windowClass = "%s";

for (var i = 0; i < clients.length; i++) {
  var c = clients[i];
  var match = false;
  if (c.resourceClass && c.resourceClass.toLowerCase() == windowClass.toLowerCase()) match = true;
  else if (c.resourceName && c.resourceName.toLowerCase() == windowClass.toLowerCase()) match = true;
  else if (c.caption && c.caption.toLowerCase().indexOf(windowClass.toLowerCase()) !== -1) match = true;

  if (match) {
    target = c;
    break;
  }
}

if (target) {
  var displayMode = "%s";
  var displayIndex = %d;
  var widthPct = %d / 100.0;
  var heightPct = %d / 100.0;
  var widthCols = %d;
  var heightRows = %d;
  var keepAbove = %t;

  // Plasma 6 likely
  var area = workspace.activeScreen.geometry;

  var finalWidth = ( widthCols > 0 ) ? target.frameGeometry.width : area.width * widthPct;
  var finalHeight = ( heightRows > 0 ) ? target.frameGeometry.height : area.height * heightPct;
  var finalX = area.x + (area.width - finalWidth) / 2;
  var finalY = area.y;

  target.keepAbove = keepAbove;
  target.onAllDesktops = true;
  target.noBorder = true;
  target.skipTaskbar = true;

  // Force off-screen
  target.frameGeometry = {
    x: finalX,
    y: finalY - finalHeight,
    width: finalWidth,
    height: finalHeight
  };
}
`

const restoreScript = `
var clients = workspace.windowList ? workspace.windowList() : workspace.clientList();
var target = null;
var windowClass = "%s";
for (var i = 0; i < clients.length; i++) {
  var c = clients[i];
  if ((c.resourceClass && c.resourceClass.toLowerCase() == windowClass.toLowerCase()) ||
    (c.resourceName && c.resourceName.toLowerCase() == windowClass.toLowerCase())) {
    target = c;
    break;
  }
}
if (target) {
  target.minimized = false;
  target.keepAbove = false;
  target.onAllDesktops = false;
  target.noBorder = false;
  target.opacity = 1.0;

  var geo = target.frameGeometry;
  var area = workspace.clientArea(KWin.PlacementArea, target);

  // If window is mostly hidden (offscreen), snap it back into the visible area
  if (geo.y + geo.height <= area.y + 50) {
    target.frameGeometry = {
      x: geo.x,
      y: area.y + 100, // Move to a visible top position
      width: geo.width,
      height: geo.height
    };
  }
}
`

func toggleQuake(config *Config) {
	toggleMutex.Lock()
	defer toggleMutex.Unlock()

	conn, err := dbus.ConnectSessionBus()
	if err != nil {
		log.Printf("Failed to connect to session bus: %v", err)
		return
	}
	defer conn.Close()

	obj := conn.Object("org.kde.KWin", "/Scripting")

	// 0. Ensure Terminal is running
	if ensureTerminalRunning(config) {
		targetVisible = false
	}

	// 1. Unload Previous Script if exists
	if lastScriptID != "" {
		obj.Call("org.kde.kwin.Scripting.unloadScript", 0, lastScriptID).Store()
	}

	// 2. Toggle State
	targetVisible = !targetVisible

	// Choose params based on state
	var duration int
	var easing string
	var opacityPoint float64
	if targetVisible {
		duration = config.ShowDuration
		easing = config.ShowEasing
		opacityPoint = config.ShowOpacityPoint
	} else {
		duration = config.HideDuration
		easing = config.HideEasing
		opacityPoint = config.HideOpacityPoint
	}

	uniqueName := fmt.Sprintf("goake_toggle_%d", time.Now().UnixNano())
	lastScriptID = uniqueName

	tmpFile, err := os.CreateTemp("", uniqueName+"_*.js")
	if err != nil {
		return
	}
	defer os.Remove(tmpFile.Name())

	scriptCode := fmt.Sprintf(kwinScriptTemplate,
		config.WindowClass,
		config.WidthPercent,
		config.HeightPercent,
		config.WidthCols,
		config.HeightRows,
		duration,
		easing,
		targetVisible,
		config.KeepAbove,
		config.AnimateOpacity,
		opacityPoint,
	)

	tmpFile.WriteString(scriptCode)
	tmpFile.Close()

	var scriptID int32
	err = obj.Call("org.kde.kwin.Scripting.loadScript", 0, tmpFile.Name(), uniqueName).Store(&scriptID)
	if err != nil {
		log.Printf("Failed load: %v", err)
		return
	}

	if scriptID >= 0 {
		scriptObjPath := dbus.ObjectPath(fmt.Sprintf("/Scripting/Script%d", scriptID))
		scriptObj := conn.Object("org.kde.KWin", scriptObjPath)
		scriptObj.Call("org.kde.kwin.Script.run", 0).Store()

		go func(name string, d int) {
			time.Sleep(time.Duration(d+100) * time.Millisecond)
			conn2, err := dbus.ConnectSessionBus()
			if err == nil {
				obj2 := conn2.Object("org.kde.KWin", "/Scripting")
				obj2.Call("org.kde.kwin.Scripting.unloadScript", 0, name).Store()
				conn2.Close()
			}
		}(uniqueName, duration)
	}
}

func ensureGrabbed(config *Config) {
	conn, err := dbus.ConnectSessionBus()
	if err != nil {
		log.Printf("Failed to connect to session bus: %v", err)
		return
	}
	defer conn.Close()

	obj := conn.Object("org.kde.KWin", "/Scripting")

	tmpFile, err := os.CreateTemp("", "quake_init_*.js")
	if err != nil {
		log.Printf("Failed to create temp file: %v", err)
		return
	}
	defer os.Remove(tmpFile.Name())

	scriptCode := fmt.Sprintf(initScriptTemplate,
		config.WindowClass,
		config.DisplayMode,
		config.DisplayIndex,
		config.WidthPercent,
		config.HeightPercent,
		config.WidthCols,
		config.HeightRows,
		config.KeepAbove,
	)

	if _, err := tmpFile.WriteString(scriptCode); err != nil {
		log.Printf("Failed to write to temp file: %v", err)
		return
	}
	tmpFile.Close()

	var scriptID int32
	err = obj.Call("org.kde.kwin.Scripting.loadScript", 0, tmpFile.Name(), "quake_init").Store(&scriptID)
	if err != nil {
		log.Printf("Failed to load KWin script: %v", err)
		return
	}

	scriptObjPath := dbus.ObjectPath(fmt.Sprintf("/Scripting/Script%d", scriptID))
	scriptObj := conn.Object("org.kde.KWin", scriptObjPath)
	err = scriptObj.Call("org.kde.kwin.Script.run", 0).Store()
	if err != nil {
		log.Printf("Failed to run KWin script: %v", err)
		return
	}

	time.Sleep(100 * time.Millisecond) // Short delay to let it run
	scriptObj.Call("org.kde.kwin.Script.stop", 0).Store()
	obj.Call("org.kde.kwin.Scripting.unloadScript", 0, "quake_init").Store()
}

func restoreQuake(config *Config) {
	conn, err := dbus.ConnectSessionBus()
	if err != nil {
		return
	}
	defer conn.Close()

	obj := conn.Object("org.kde.KWin", "/Scripting")
	uniqueName := fmt.Sprintf("goake_restore_%d", time.Now().UnixNano())
	scriptCode := fmt.Sprintf(restoreScript, config.WindowClass)

	tmpFile, err := os.CreateTemp("", "quake_restore_*.js")
	if err != nil {
		return
	}
	defer os.Remove(tmpFile.Name())
	tmpFile.WriteString(scriptCode)
	tmpFile.Close()

	var scriptID int32
	err = obj.Call("org.kde.kwin.Scripting.loadScript", 0, tmpFile.Name(), uniqueName).Store(&scriptID)
	if err == nil && scriptID > 0 {
		scriptObjPath := dbus.ObjectPath(fmt.Sprintf("/Scripting/Script%d", scriptID))
		scriptObj := conn.Object("org.kde.KWin", scriptObjPath)
		scriptObj.Call("org.kde.kwin.Script.run", 0).Store()
		time.Sleep(300 * time.Millisecond) // Give it more time to execute
		scriptObj.Call("org.kde.kwin.Script.stop", 0).Store()
		obj.Call("org.kde.kwin.Scripting.unloadScript", 0, uniqueName).Store()
	}
}
