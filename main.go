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

	"io/ioutil"
	"os/exec"
	"strings"

	"github.com/BurntSushi/toml"
	"github.com/godbus/dbus/v5"
	"github.com/godbus/dbus/v5/prop"
)

//go:embed icon.png
var iconData []byte

type Config struct {
	WindowClass   string `toml:"window_class"`
	StartCommand  string `toml:"start_command"`
	Hotkey        string `toml:"hotkey"`
	DisplayMode   string `toml:"display_mode"`
	DisplayIndex  int    `toml:"display_index"`
	WidthPercent  int    `toml:"width_percent"`
	HeightPercent int    `toml:"height_percent"`

	ShowDuration int    `toml:"show_duration"`
	HideDuration int    `toml:"hide_duration"`
	ShowEasing   string `toml:"show_easing"`
	HideEasing   string `toml:"hide_easing"`
}

const kwinScriptTemplate = `
// Compatibility for Plasma 5 vs 6
var clients = workspace.windowList ? workspace.windowList() : workspace.clientList();
var target = null;
var windowClass = "%s";

print("Vibullshit: Script started for class " + windowClass);

for (var i = 0; i < clients.length; i++) {
    var c = clients[i];
    var match = false;
    if (c.resourceClass && c.resourceClass.toLowerCase() == windowClass.toLowerCase()) match = true;
    else if (c.resourceName && c.resourceName.toLowerCase() == windowClass.toLowerCase()) match = true;
    else if (c.caption && c.caption.toLowerCase().indexOf(windowClass.toLowerCase()) !== -1) match = true;

    if (match) {
        target = c;
        print("Vibullshit: Found target window: " + c.caption);
        break;
    }
}

function getEasing(progress, type) {
    switch (type) {
        case "linear": return progress;
        case "ease-in": return progress * progress;
        case "ease-out": return progress * (2 - progress);
        case "ease-in-out":
            return progress < .5 ? 2 * progress * progress : -1 + (4 - 2 * progress) * progress;
        default: return progress * (2 - progress); // ease-out default
    }
}

if (target) {
    var displayMode = "%s";
    var displayIndex = %d;
    var widthPct = %d / 100.0;
    var heightPct = %d / 100.0;
	var showDuration = %d;
	var hideDuration = %d;
	var showEasing = "%s";
	var hideEasing = "%s";

    print("Vibullshit: Toggle target state check...");

    // Get screen geometry
    var area = null;
    if (workspace.activeScreen && workspace.activeScreen.geometry) {
        area = workspace.activeScreen.geometry;
    } else {
        var screenId = workspace.activeScreen;
        area = workspace.clientArea(KWin.PlacementArea, screenId, target);
    }

    // Calculate geometry
    var finalWidth = area.width * widthPct;
    var finalHeight = area.height * heightPct;
    var finalX = area.x + (area.width - finalWidth) / 2;
    var finalY = area.y;

    // Detect state based on vertical position
    // If top of window is above the screen (allowing for some margin), it's hidden
    var isHidden = (target.frameGeometry.y + target.frameGeometry.height/2) < area.y;

    print("Vibullshit: Screen Y=" + area.y + " Window Y=" + target.frameGeometry.y + " Hidden? " + isHidden);

    if (isHidden) {
        // SHOW
        print("Vibullshit: Showing window...");

        target.minimized = false;
        target.keepAbove = true;
        target.onAllDesktops = true;
        target.noBorder = true;
        target.skipTaskbar = true;
        target.skipPager = true;
        workspace.activeClient = target;

        if (showDuration > 0) {
            var startY = target.frameGeometry.y;
            // Ensure we start at least from -height if it was way off
            if (startY > finalY) startY = finalY - finalHeight; // Should verify logic

            // Actually, best "Hidden" start pos is: finalY - finalHeight
            startY = finalY - finalHeight;

            target.frameGeometry = {
                x: finalX,
                y: startY,
                width: finalWidth,
                height: finalHeight
            };

            var endY = finalY;
            var startTime = new Date().getTime();

            var timer = new QTimer();
            timer.interval = 16;
            timer.timeout.connect(function() {
                var now = new Date().getTime();
                var elapsed = now - startTime;
                var progress = Math.min(elapsed / showDuration, 1.0);
                var ease = getEasing(progress, showEasing);

                var currentY = startY + (endY - startY) * ease;

                target.frameGeometry = {
                    x: finalX,
                    y: currentY,
                    width: finalWidth,
                    height: finalHeight
                };

                if (progress >= 1.0) {
                    timer.stop();
                    target.frameGeometry = {
                        x: finalX,
                        y: finalY,
                        width: finalWidth,
                        height: finalHeight
                    };
                }
            });
            timer.start();
        } else {
            // Instant show
            target.frameGeometry = {
                x: finalX,
                y: finalY,
                width: finalWidth,
                height: finalHeight
            };
        }

    } else {
        // HIDE
        print("Vibullshit: Hiding window...");

        if (hideDuration > 0) {
            var startY = target.frameGeometry.y;
            var endY = finalY - finalHeight; // Move up off-screen
            var startTime = new Date().getTime();

            var timer = new QTimer();
            timer.interval = 16;
            timer.timeout.connect(function() {
                var now = new Date().getTime();
                var elapsed = now - startTime;
                var progress = Math.min(elapsed / hideDuration, 1.0);
                var ease = getEasing(progress, hideEasing);

                var currentY = startY + (endY - startY) * ease;

                target.frameGeometry = {
                    x: finalX,
                    y: currentY,
                    width: finalWidth,
                    height: finalHeight
                };

                if (progress >= 1.0) {
                    timer.stop();
                    // target.minimized = true; // NO minimize! Just stay off-screen.
                }
            });
            timer.start();
        } else {
             target.frameGeometry = {
                x: finalX,
                y: finalY - finalHeight,
                width: finalWidth,
                height: finalHeight
             };
        }
    }
} else {
    print("Vibullshit: No target window found!");
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
	// Middle-click acts as Quit
	os.Exit(0)
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
			WindowClass:   "wezquake",
			Hotkey:        "Meta+Grave",
			DisplayMode:   "follow-mouse",
			WidthPercent:  100,
			HeightPercent: 40,
			ShowDuration:  300,
			HideDuration:  300,
			ShowEasing:    "ease-out",
			HideEasing:    "ease-in",
		}
	}
	// Default easings if missing
	if config.ShowEasing == "" {
		config.ShowEasing = "ease-out"
	}
	if config.HideEasing == "" {
		config.HideEasing = "ease-in"
	}
	// Fallback/Defaults for durations if 0 (optional, arguably 0 is valid for instant)
	return config
}

func checkProcessRunning(targetClass string) bool {
	procs, err := ioutil.ReadDir("/proc")
	if err != nil {
		return false
	}

	for _, p := range procs {
		if !p.IsDir() || !isNumeric(p.Name()) {
			continue
		}

		cmdline, err := ioutil.ReadFile(fmt.Sprintf("/proc/%s/cmdline", p.Name()))
		if err != nil {
			continue
		}

		// cmdline is null-separated
		cmd := string(bytes.ReplaceAll(cmdline, []byte{0}, []byte(" ")))
		if strings.Contains(cmd, targetClass) {
			return true
		}
	}
	return false
}

func isNumeric(s string) bool {
	for _, c := range s {
		if c < '0' || c > '9' {
			return false
		}
	}
	return true
}

func toggleQuake(config *Config) {
	fmt.Println("toggleQuake() called. Connecting to session bus...")
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
		config.ShowDuration,
		config.HideDuration,
		config.ShowEasing,
		config.HideEasing,
	)

	if _, err := tmpFile.WriteString(scriptCode); err != nil {
		log.Printf("Failed to write to temp file: %v", err)
		return
	}
	tmpFile.Close()

	var scriptID int32
	fmt.Println("Loading KWin script...")
	err = obj.Call("org.kde.kwin.Scripting.loadScript", 0, tmpFile.Name(), "quake_toggle").Store(&scriptID)
	if err != nil {
		log.Printf("Failed to load KWin script: %v", err)
		return
	}

	fmt.Printf("Running KWin script (ID: %d)...\n", scriptID)
	scriptObjPath := dbus.ObjectPath(fmt.Sprintf("/Scripting/Script%d", scriptID))
	scriptObj := conn.Object("org.kde.KWin", scriptObjPath)
	err = scriptObj.Call("org.kde.kwin.Script.run", 0).Store()
	if err != nil {
		log.Printf("Failed to run KWin script: %v", err)
		return
	}

	waitMs := config.ShowDuration + config.HideDuration + 100 // Safe upper bound
	time.Sleep(time.Duration(waitMs) * time.Millisecond)

	fmt.Println("Stopping KWin script...")
	scriptObj.Call("org.kde.kwin.Script.stop", 0).Store()
	obj.Call("org.kde.kwin.Scripting.unloadScript", 0, "quake_toggle").Store()
	fmt.Println("toggleQuake() finished.")
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

	// Init script to find window, set properties, and force minimize
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

    target.keepAbove = true;
    target.onAllDesktops = true;
    target.noBorder = true;
    target.minimized = true; // Start hidden

    var screenId = 0;
    if (displayMode === "follow-mouse") {
        screenId = workspace.activeScreen;
    } else if (displayMode === "specific") {
        screenId = displayIndex;
    } else {
        screenId = workspace.activeScreen;
    }

    // Plasma 6 likely
    var area = workspace.activeScreen.geometry;

    var finalWidth = area.width * widthPct;
    var finalHeight = area.height * heightPct;
    var finalX = area.x + (area.width - finalWidth) / 2;
    var finalY = area.y;

    target.keepAbove = true;
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

	scriptCode := fmt.Sprintf(initScriptTemplate,
		config.WindowClass,
		config.DisplayMode,
		config.DisplayIndex,
		config.WidthPercent,
		config.HeightPercent,
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

func main() {
	flag.Parse() // Keep flag parsing for help/usage, though we don't use --daemon anymore

	config := loadConfig()

	// Connect to session bus
	conn, err := dbus.ConnectSessionBus()
	if err != nil {
		log.Fatalf("Failed to connect to session bus: %v", err)
	}

	// Try to become the primary owner of the D-Bus service
	reply, err := conn.RequestName("com.nabaxo.vibullshit", dbus.NameFlagDoNotQueue)
	if err != nil {
		log.Fatalf("Failed to request D-Bus name: %v", err)
	}

	fmt.Printf("D-Bus RequestName reply: %v\n", reply)

	if reply == dbus.RequestNameReplyPrimaryOwner {
		// We are the first instance -> Run as Daemon
		fmt.Println("Instance became D-Bus PRIMARY OWNER. Starting Daemon...")

		// Run daemon logic (tray, service, grab)
		// Note: runDaemon handles its own connection and exports.
		// Ideally pass existing connection, but for now we can close this one or reuse it.
		// Given runDaemon creates a new connection, let's close this check-connection and call runDaemon.
		conn.Close()
		runDaemon(&config)
	} else if reply == dbus.RequestNameReplyExists {
		// Another instance owns the name -> Toggle it
		fmt.Println("Instance Detected EXISTING Daemon. Sending Toggle signal...")
		obj := conn.Object("com.nabaxo.vibullshit", "/com/nabaxo/vibullshit")
		err = obj.Call("com.nabaxo.vibullshit.Toggle", 0).Store()
		if err != nil {
			log.Printf("Failed to toggle remote daemon: %v", err)
		} else {
			fmt.Println("Toggle signal sent successfully.")
		}
	} else {
		log.Fatalf("Unexpected D-Bus name request reply: %v", reply)
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

	// Auto-start terminal if needed
	isRunning := checkProcessRunning(config.WindowClass)
	fmt.Printf("Process '%s' running? %v\n", config.WindowClass, isRunning)

	if config.StartCommand != "" && !isRunning {
		fmt.Printf("Auto-Starting terminal: %s\n", config.StartCommand)
		cmd := exec.Command("sh", "-c", config.StartCommand)
		if err := cmd.Start(); err != nil {
			log.Printf("Failed to start terminal: %v", err)
		}

		// Wait for window to appear (retry loop)
		fmt.Println("Waiting for window to appear...")
		for i := 0; i < 20; i++ {
			if checkProcessRunning(config.WindowClass) {
				fmt.Println("Window appeared!")
				time.Sleep(500 * time.Millisecond) // Give it a moment to map the window
				break
			}
			time.Sleep(200 * time.Millisecond)
		}
	} else if isRunning {
		fmt.Println("Terminal process already running. Skipping auto-start.")
	}

	// Initial grab/setup of the window
	fmt.Println("Running ensureGrabbed()...")
	ensureGrabbed(config)

	// Single instance control service
	daemon := &QuakeDaemon{config: config}
	conn.Export(daemon, "/com/nabaxo/vibullshit", "com.nabaxo.vibullshit")
	conn.RequestName("com.nabaxo.vibullshit", dbus.NameFlagReplaceExisting)

	sni := &StatusNotifierItem{config: config}
	conn.Export(sni, "/StatusNotifierItem", "org.kde.StatusNotifierItem")

	// Decode icon for Pixmap
	img, _, err := image.Decode(bytes.NewReader(iconData))
	if err != nil {
		log.Fatalf("Failed to decode icon: %v", err)
	}
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
				Value:    "Vibullshit (Left: Toggle | Middle: Quit)",
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
