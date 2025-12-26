package main

import (
	"fmt"
	"log"
	"os"
	"time"

	"github.com/BurntSushi/toml"
	"github.com/godbus/dbus/v5"
)

type Config struct {
	WindowClass       string `toml:"window_class"`
	Hotkey            string `toml:"hotkey"`
	DisplayMode       string `toml:"display_mode"`
	DisplayIndex      int    `toml:"display_index"`
	WidthPercent      int    `toml:"width_percent"`
	HeightPercent     int    `toml:"height_percent"`
	AnimationDuration int    `toml:"animation_duration"`
	AnimationType     string `toml:"animation_type"`
}

const kwinScriptTemplate = `
var clients = workspace.clientList();
var target = null;
var windowClass = "%s";

for (var i = 0; i < clients.length; i++) {
    if (clients[i].resourceClass == windowClass || clients[i].resourceName == windowClass) {
        target = clients[i];
        break;
    }
}

if (target) {
    var displayMode = "%s";
    var displayIndex = %d;
    var widthPct = %d / 100.0;
    var heightPct = %d / 100.0;
    var animDuration = %d;
    var animType = "%s";

    if (target.minimized) {
        // Restore and Style
        target.minimized = false;
        target.keepAbove = true;
        target.onAllDesktops = true;
        target.noBorder = true;
        workspace.activeClient = target;

        // Determine target screen
        var screenId = 0;
        if (displayMode === "follow-mouse") {
            screenId = workspace.activeScreen;
        } else if (displayMode === "specific") {
            screenId = displayIndex;
        } else {
            // "active" or default
            screenId = workspace.activeScreen;
        }

        var area = workspace.clientArea(KWin.PlacementArea, screenId, target);
        var finalWidth = area.width * widthPct;
        var finalHeight = area.height * heightPct;
        var finalX = area.x + (area.width - finalWidth) / 2;
        var finalY = area.y;

        if (animType === "slide" && animDuration > 0) {
            target.geometry = {
                x: finalX,
                y: finalY - finalHeight,
                width: finalWidth,
                height: finalHeight
            };

            var startY = finalY - finalHeight;
            var endY = finalY;
            var startTime = new Date().getTime();

            var timer = new QTimer();
            timer.interval = 16; // ~60fps
            timer.timeout.connect(function() {
                var now = new Date().getTime();
                var elapsed = now - startTime;
                var progress = Math.min(elapsed / animDuration, 1.0);

                // Ease out quad
                var ease = progress * (2 - progress);

                target.geometry = {
                    x: finalX,
                    y: startY + (endY - startY) * ease,
                    width: finalWidth,
                    height: finalHeight
                };

                if (progress >= 1.0) {
                    timer.stop();
                }
            });
            timer.start();
        } else {
            target.geometry = {
                x: finalX,
                y: finalY,
                width: finalWidth,
                height: finalHeight
            };
        }
    } else {
        if (animType === "slide" && animDuration > 0) {
            var startY = target.geometry.y;
            var endY = startY - target.geometry.height;
            var startTime = new Date().getTime();
            var finalX = target.geometry.x;
            var finalW = target.geometry.width;
            var finalH = target.geometry.height;

            var timer = new QTimer();
            timer.interval = 16;
            timer.timeout.connect(function() {
                var now = new Date().getTime();
                var elapsed = now - startTime;
                var progress = Math.min(elapsed / animDuration, 1.0);

                // Ease in quad
                var ease = progress * progress;

                target.geometry = {
                    x: finalX,
                    y: startY + (endY - startY) * ease,
                    width: finalW,
                    height: finalH
                };

                if (progress >= 1.0) {
                    timer.stop();
                    target.minimized = true;
                }
            });
            timer.start();
        } else {
            target.minimized = true;
        }
    }
}
`

func main() {
	configPath := "config.toml"
	var config Config
	if _, err := toml.DecodeFile(configPath, &config); err != nil {
		log.Printf("Warning: Could not load config.toml, using defaults: %v", err)
		config = Config{
			WindowClass:       "wezquake",
			DisplayMode:       "follow-mouse",
			WidthPercent:      100,
			HeightPercent:     40,
			AnimationDuration: 300,
			AnimationType:     "slide",
		}
	}

	conn, err := dbus.ConnectSessionBus()
	if err != nil {
		log.Fatalf("Failed to connect to session bus: %v", err)
	}
	defer conn.Close()

	obj := conn.Object("org.kde.KWin", "/Scripting")

	// Create a temporary file for the script
	tmpFile, err := os.CreateTemp("", "quake_toggle_*.js")
	if err != nil {
		log.Fatalf("Failed to create temp file: %v", err)
	}
	defer os.Remove(tmpFile.Name())

	scriptCode := fmt.Sprintf(kwinScriptTemplate,
		config.WindowClass,
		config.DisplayMode,
		config.DisplayIndex,
		config.WidthPercent,
		config.HeightPercent,
		config.AnimationDuration,
		config.AnimationType,
	)

	if _, err := tmpFile.WriteString(scriptCode); err != nil {
		log.Fatalf("Failed to write to temp file: %v", err)
	}
	if err := tmpFile.Close(); err != nil {
		log.Fatalf("Failed to close temp file: %v", err)
	}

	var scriptID int32
	err = obj.Call("org.kde.kwin.Scripting.loadScript", 0, tmpFile.Name(), "quake_toggle").Store(&scriptID)
	if err != nil {
		log.Fatalf("Failed to load KWin script: %v", err)
	}

	// Start the script
	scriptObjPath := dbus.ObjectPath(fmt.Sprintf("/Scripting/Script%d", scriptID))
	scriptObj := conn.Object("org.kde.KWin", scriptObjPath)
	err = scriptObj.Call("org.kde.kwin.Script.run", 0).Store()
	if err != nil {
		log.Fatalf("Failed to run KWin script: %v", err)
	}

	// Wait for the script to execute. If animation is used, we need to wait long enough.
	// But the script runs asynchronously in KWin, so we just need to wait long enough
	// for the script to *start* and for KWin to process the calls.
	// However, if we unload it immediately, the timer might be killed.
	// For now, let's wait a bit longer than the animation duration.
	waitMs := config.AnimationDuration + 100
	if config.AnimationType == "none" {
		waitMs = 100
	}
	time.Sleep(time.Duration(waitMs) * time.Millisecond)

	err = scriptObj.Call("org.kde.kwin.Script.stop", 0).Store()
	if err != nil {
		// Ignore
	}

	err = obj.Call("org.kde.kwin.Scripting.unloadScript", 0, "quake_toggle").Store()
	if err != nil {
		// log.Printf("Warning: Failed to unload script: %v", err)
	}

	fmt.Printf("Toggled quake terminal (Mode: %s, Anim: %s)\n", config.DisplayMode, config.AnimationType)
}
