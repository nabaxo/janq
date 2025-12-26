package main

import (
	"bytes"
	_ "embed"
	"flag"
	"fmt"
	"image"
	_ "image/png"
	"log"
	"os"
	"time"

	"github.com/BurntSushi/toml"
	"github.com/godbus/dbus/v5"
	"github.com/godbus/dbus/v5/prop"
)

//go:embed icon.png
var iconData []byte

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
        target.minimized = false;
        target.keepAbove = true;
        target.onAllDesktops = true;
        target.noBorder = true;
        workspace.activeClient = target;

        var screenId = 0;
        if (displayMode === "follow-mouse") {
            screenId = workspace.activeScreen;
        } else if (displayMode === "specific") {
            screenId = displayIndex;
        } else {
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
            timer.interval = 16;
            timer.timeout.connect(function() {
                var now = new Date().getTime();
                var elapsed = now - startTime;
                var progress = Math.min(elapsed / animDuration, 1.0);
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

type QuakeDaemon struct {
	config *Config
}

func (d *QuakeDaemon) Toggle() *dbus.Error {
	toggleQuake(d.config)
	return nil
}

type StatusNotifierItem struct {
	config *Config
}

func (s *StatusNotifierItem) Activate(x, y int32) *dbus.Error {
	toggleQuake(s.config)
	return nil
}

func (s *StatusNotifierItem) ContextMenu(x, y int32) *dbus.Error {
	return nil
}

func (s *StatusNotifierItem) Scroll(delta int32, orientation string) *dbus.Error {
	return nil
}

func (s *StatusNotifierItem) SecondaryActivate(x, y int32) *dbus.Error {
	return nil
}

type Pixmap struct {
	Width  int32
	Height int32
	Data   []byte
}

func loadConfig() Config {
	configPath := "config.toml"
	var config Config
	if _, err := toml.DecodeFile(configPath, &config); err != nil {
		config = Config{
			WindowClass:       "wezquake",
			Hotkey:            "Meta+Grave",
			DisplayMode:       "follow-mouse",
			WidthPercent:      100,
			HeightPercent:     40,
			AnimationDuration: 300,
			AnimationType:     "slide",
		}
	}
	return config
}

func toggleQuake(config *Config) {
	conn, err := dbus.ConnectSessionBus()
	if err != nil {
		log.Printf("Failed to connect to session bus: %v", err)
		return
	}
	defer conn.Close()

	obj := conn.Object("org.kde.KWin", "/Scripting")

	tmpFile, err := os.CreateTemp("", "quake_toggle_*.js")
	if err != nil {
		log.Printf("Failed to create temp file: %v", err)
		return
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
		log.Printf("Failed to write to temp file: %v", err)
		return
	}
	tmpFile.Close()

	var scriptID int32
	err = obj.Call("org.kde.kwin.Scripting.loadScript", 0, tmpFile.Name(), "quake_toggle").Store(&scriptID)
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

	waitMs := config.AnimationDuration + 100
	if config.AnimationType == "none" {
		waitMs = 100
	}
	time.Sleep(time.Duration(waitMs) * time.Millisecond)

	scriptObj.Call("org.kde.kwin.Script.stop", 0).Store()
	obj.Call("org.kde.kwin.Scripting.unloadScript", 0, "quake_toggle").Store()
}

func main() {
	daemonMode := flag.Bool("daemon", false, "Run in daemon mode with tray icon")
	flag.Parse()

	config := loadConfig()

	if *daemonMode {
		runDaemon(&config)
	} else {
		conn, err := dbus.ConnectSessionBus()
		if err == nil {
			obj := conn.Object("com.nabaxo.vibullshit", "/com/nabaxo/vibullshit")
			err = obj.Call("com.nabaxo.vibullshit.Toggle", 0).Store()
			if err == nil {
				return
			}
		}
		toggleQuake(&config)
	}
}

func runDaemon(config *Config) {
	conn, err := dbus.ConnectSessionBus()
	if err != nil {
		log.Fatalf("Failed to connect to session bus: %v", err)
	}

	name := fmt.Sprintf("org.kde.StatusNotifierItem-vibullshit-%d", os.Getpid())
	reply, err := conn.RequestName(name, dbus.NameFlagReplaceExisting)
	if err != nil || reply != dbus.RequestNameReplyPrimaryOwner {
		log.Fatalf("Failed to request D-Bus name %s: %v", name, err)
	}

	// Single instance control service
	daemon := &QuakeDaemon{config: config}
	conn.Export(daemon, "/com/nabaxo/vibullshit", "com.nabaxo.vibullshit")
	conn.RequestName("com.nabaxo.vibullshit", dbus.NameFlagReplaceExisting)

	sni := &StatusNotifierItem{config: config}
	conn.Export(sni, "/StatusNotifierItem", "org.kde.StatusNotifierItem")

	// Decode icon for Pixmap
	img, _, _ := image.Decode(bytes.NewReader(iconData))
	bounds := img.Bounds()
	data := make([]byte, 0, bounds.Dx()*bounds.Dy()*4)
	for y := bounds.Min.Y; y < bounds.Max.Y; y++ {
		for x := bounds.Min.X; x < bounds.Max.X; x++ {
			r, g, b, a := img.At(x, y).RGBA()
			data = append(data, byte(a>>8), byte(r>>8), byte(g>>8), byte(b>>8))
		}
	}
	pixmaps := []Pixmap{{Width: int32(bounds.Dx()), Height: int32(bounds.Dy()), Data: data}}

	props := map[string]map[string]*prop.Prop{
		"org.kde.StatusNotifierItem": {
			"Category": {
				Value:    "ApplicationStatus",
				Writable: false,
				Emit:     prop.EmitTrue,
			},
			"Id": {
				Value:    "vibullshit",
				Writable: false,
				Emit:     prop.EmitTrue,
			},
			"Title": {
				Value:    "Vibullshit",
				Writable: false,
				Emit:     prop.EmitTrue,
			},
			"Status": {
				Value:    "Active",
				Writable: false,
				Emit:     prop.EmitTrue,
			},
			"WindowId": {
				Value:    int32(0),
				Writable: false,
				Emit:     prop.EmitTrue,
			},
			"IconPixmap": {
				Value:    pixmaps,
				Writable: false,
				Emit:     prop.EmitTrue,
			},
			"ItemIsMenu": {
				Value:    false,
				Writable: false,
				Emit:     prop.EmitTrue,
			},
		},
	}

	_, err = prop.Export(conn, "/StatusNotifierItem", props)
	if err != nil {
		log.Fatalf("Failed to export properties: %v", err)
	}

	// Register with StatusNotifierWatcher
	watcher := conn.Object("org.kde.StatusNotifierWatcher", "/StatusNotifierWatcher")
	err = watcher.Call("org.kde.StatusNotifierWatcher.RegisterStatusNotifierItem", 0, name).Store()
	if err != nil {
		log.Printf("Warning: Failed to register with StatusNotifierWatcher: %v", err)
	}

	fmt.Println("Vibullshit daemon (Pure D-Bus SNI) running...")
	select {}
}
