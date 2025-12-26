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
	"path/filepath"
	"strconv"
	"sync" // Added for toggleMutex
	"syscall"
	"time"

	"os/exec"
	"os/signal"
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

	// Terminal logic
	WidthCols  int `toml:"width_cols"`
	HeightRows int `toml:"height_rows"`
}

// Global state to track toggle direction
var targetVisible bool = false

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

    // Current State Detection
    var currentArea = workspace.clientArea(KWin.PlacementArea, target);
    var mouseArea = workspace.activeScreen.geometry;

    // We consider it mostly hidden if it's minimized OR if less than 5px is visible.
    var isMostlyHidden = target.minimized || (target.frameGeometry.y + target.frameGeometry.height <= currentArea.y + 5);

    // Is the window currently on a different screen than the mouse?
    var isDifferentScreen = (currentArea.x != mouseArea.x || currentArea.y != mouseArea.y);

    // SUMMON Logic:
    // We only "Stick" if we are already visible AND on the correct (mouse) screen.
    // Otherwise, we "Summon" to the new position.
    var isSticky = (shouldShow && !isMostlyHidden && !isDifferentScreen) || (!shouldShow);

    var area = null;
    if (isSticky) {
        area = currentArea;
    } else {
        // SUMMON: Use workspace.activeScreen (mouse screen)
        area = mouseArea;
    }

    // Target Geometry
    var finalWidth = ( widthCols > 0 ) ? target.frameGeometry.width : area.width * widthPct;
    var finalHeight = ( heightRows > 0 ) ? target.frameGeometry.height : area.height * heightPct;
    var finalX = area.x + (area.width - finalWidth) / 2;
    var finalY = area.y;

    // Properties
    target.keepAbove = true;
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

        // If we decided to SUMMON (was hidden), snap X and hidden-Y onto the NEW screen
        if (!isSticky) {
             startY = finalY - finalHeight;
             target.frameGeometry = {
                x: finalX,
                y: startY,
                width: finalWidth,
                height: finalHeight
             };
        }

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

                target.frameGeometry = {
                    x: finalX,
                    y: currentY,
                    width: finalWidth,
                    height: finalHeight
                };

                if (progress >= 1.0) {
                    timer.stop();
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

                target.frameGeometry = {
                    x: startX,
                    y: currentY,
                    width: startW,
                    height: startH
                };

                if (progress >= 1.0) {
                    timer.stop();
                    // PROPER HIDE: Minimize with a small delay to avoid ghosting
                    // Opacity 0 makes it invisible immediately
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
	fmt.Println("Quit requested via tray icon...")
	restoreQuake(s.config)
	os.Exit(0)
	return nil
}

type Pixmap struct {
	Width  int32
	Height int32
	Data   []byte
}

func findConfigFile() string {
	configFiles := []string{".goake.toml"}
	home, err := os.UserHomeDir()
	if err == nil {
		configFiles = append(configFiles, filepath.Join(home, ".goake.toml"))
		xdgConfig := os.Getenv("XDG_CONFIG_HOME")
		if xdgConfig == "" {
			xdgConfig = filepath.Join(home, ".config")
		}
		configFiles = append(configFiles, filepath.Join(xdgConfig, "goake", ".goake.toml"))
	}
	for _, path := range configFiles {
		if _, err := os.Stat(path); err == nil {
			return path
		}
	}
	return ""
}

func loadConfig() Config {
	path := findConfigFile()

	var config Config
	if path != "" {
		if _, err := toml.DecodeFile(path, &config); err == nil {
			fmt.Printf("Loaded config from: %s\n", path)
		} else {
			fmt.Printf("Error decoding config: %v\n", err)
		}
	} else {
		fmt.Println("No config file found. Using defaults.")
		config = Config{
			WindowClass:   "wezquake",
			StartCommand:  "wezterm-gui start",
			Hotkey:        "Meta+Grave",
			DisplayMode:   "follow-mouse",
			WidthPercent:  40,
			HeightPercent: 40,
			ShowDuration:  300,
			HideDuration:  300,
			ShowEasing:    "ease-out",
			HideEasing:    "ease-in",
			WidthCols:     120,
			HeightRows:    40,
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

func ensureTerminalRunning(config *Config) bool {
	// 1. Precise process + class check
	if checkProcessRunning(config.WindowClass) {
		return false
	}

	if config.StartCommand == "" {
		return false
	}

	fullCmd := config.StartCommand
	if strings.Contains(strings.ToLower(config.StartCommand), "wezterm") {
		var flags string
		// Inject --class if missing
		if !strings.Contains(fullCmd, "--class") {
			flags += fmt.Sprintf(" --class %s", config.WindowClass)
		}
		if config.WidthCols > 0 {
			flags += fmt.Sprintf(" --config initial_cols=%d", config.WidthCols)
		}
		if config.HeightRows > 0 {
			flags += fmt.Sprintf(" --config initial_rows=%d", config.HeightRows)
		}

		if flags != "" {
			idx := strings.Index(strings.ToLower(fullCmd), "wezterm")
			if idx != -1 {
				// WezTerm CLI: wezterm [FLAGS] [COMMAND]
				// We inject flags immediately after 'wezterm' or 'wezterm-gui'
				firstSpace := strings.Index(fullCmd[idx:], " ")
				if firstSpace != -1 {
					insertIdx := idx + firstSpace
					fullCmd = fullCmd[:insertIdx] + flags + fullCmd[insertIdx:]
				} else {
					fullCmd += flags
				}
			} else {
				fullCmd += flags
			}
		}
	} else if !strings.Contains(fullCmd, "--class") {
		// Generic fallback for alacritty/kitty style flags
		fullCmd += fmt.Sprintf(" --class %s", config.WindowClass)
	}

	fmt.Printf("Starting terminal: %s\n", fullCmd)
	cmd := exec.Command("sh", "-c", fullCmd)
	cmd.SysProcAttr = &syscall.SysProcAttr{Setsid: true}
	if err := cmd.Start(); err != nil {
		log.Printf("Failed to start terminal: %v", err)
		return false
	}

	// Wait for process to appear
	for i := 0; i < 20; i++ {
		if checkProcessRunning(config.WindowClass) {
			fmt.Println("Terminal process detected.")
			time.Sleep(1 * time.Second) // Give it time to map the window
			ensureGrabbed(config)
			return true
		}
		time.Sleep(300 * time.Millisecond)
	}
	return false
}

func checkProcessRunning(targetClass string) bool {
	procs, err := os.ReadDir("/proc")
	if err != nil {
		return false
	}
	myPid := os.Getpid()

	for _, p := range procs {
		if !p.IsDir() || !isNumeric(p.Name()) {
			continue
		}
		pid, _ := strconv.Atoi(p.Name())
		if pid == myPid {
			continue
		}

		cmdline, err := os.ReadFile(fmt.Sprintf("/proc/%s/cmdline", p.Name()))
		if err != nil {
			continue
		}

		// Parts are null-terminated
		parts := bytes.Split(cmdline, []byte{0})
		for i, part := range parts {
			s := string(part)
			// Match exact --class arg OR exact string match if it's not a shell/system proc
			if s == "--class" && i+1 < len(parts) && strings.EqualFold(string(parts[i+1]), targetClass) {
				return true
			}
			if strings.HasPrefix(strings.ToLower(s), "--class=") && strings.EqualFold(s[8:], targetClass) {
				return true
			}
		}

		// Broad match fallback (only if terminal-like)
		fullCmd := string(bytes.Join(parts, []byte(" ")))
		if strings.Contains(fullCmd, targetClass) {
			exe, _ := os.Readlink(fmt.Sprintf("/proc/%d/exe", pid))
			if strings.Contains(exe, "wezterm") || strings.Contains(exe, "alacritty") || strings.Contains(exe, "kitty") {
				return true
			}
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

var toggleMutex sync.Mutex
var lastScriptID string // Track last script name to unload

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
	if targetVisible {
		duration = config.ShowDuration
		easing = config.ShowEasing
	} else {
		duration = config.HideDuration
		easing = config.HideEasing
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

		// We do NOT wait for completion anymore. We return immediately to allow interruption.
		// We only clean up this script the NEXT time toggle is called (or on exit).
		// Wait... if we don't clean it up, it stays loaded.
		// KWin has a limit on loaded scripts?
		// It's better to spawn a goroutine to cleanup after duration?

		go func(name string, d int) {
			time.Sleep(time.Duration(d+100) * time.Millisecond)
			// Cleanup if it's still the "last" one?
			// Or just always cleanup. Overlapping unloads are fine.
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
    var widthCols = %d;
    var heightRows = %d;

    // Plasma 6 likely
    var area = workspace.activeScreen.geometry;

    var finalWidth = ( widthCols > 0 ) ? target.frameGeometry.width : area.width * widthPct;
    var finalHeight = ( heightRows > 0 ) ? target.frameGeometry.height : area.height * heightPct;
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
		config.WidthCols,
		config.HeightRows,
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
	// Optional flag to force daemon mode, but default behavior is smart.
	daemonMode := flag.Bool("daemon", false, "Force run in daemon mode")
	flag.Parse()

	config := loadConfig()

	if *daemonMode {
		runDaemon(&config, false)
		return
	}

	// Smart Mode: Try to Toggle. If fails, Start Daemon.
	conn, err := dbus.ConnectSessionBus()
	if err != nil {
		// Bus failed? Probably can't run daemon either, but let's try.
		runDaemon(&config, false)
		return
	}

	// Try to call Toggle on existing service
	obj := conn.Object("dev.nabaxo.goake", "/dev/nabaxo/goake")
	err = obj.Call("dev.nabaxo.goake.Toggle", 0).Store()

	if err == nil {
		// Success! We triggered the daemon.
		conn.Close()
		return
	}

	// Failed to toggle (likely service not found), so we start the daemon.
	fmt.Println("Daemon not running (or reachable). Starting new daemon instance...")
	conn.Close()
	runDaemon(&config, true)
}

func restoreQuake(config *Config) {
	conn, err := dbus.ConnectSessionBus()
	if err != nil {
		return
	}
	defer conn.Close()

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

func runDaemon(config *Config, autoShow bool) {
	conn, err := dbus.ConnectSessionBus()
	if err != nil {
		log.Fatalf("Failed to connect to session bus: %v", err)
	}
	defer conn.Close()

	name := fmt.Sprintf("org.kde.StatusNotifierItem-goake-%d", os.Getpid())
	reply, err := conn.RequestName(name, dbus.NameFlagReplaceExisting)
	if err != nil || reply != dbus.RequestNameReplyPrimaryOwner {
		log.Fatalf("Failed to request D-Bus name %s: %v", name, err)
	}

	// Auto-start terminal if needed
	ensureTerminalRunning(config)

	// Initial grab/setup of the window
	fmt.Println("Running ensureGrabbed()...")
	ensureGrabbed(config)

	// Single instance control service
	daemon := &QuakeDaemon{config: config}
	conn.Export(daemon, "/dev/nabaxo/goake", "dev.nabaxo.goake")
	conn.RequestName("dev.nabaxo.goake", dbus.NameFlagReplaceExisting)

	if autoShow {
		fmt.Println("Waiting for terminal to settle before auto-show...")
		time.Sleep(500 * time.Millisecond) // Give KWin time to register the new window
		toggleQuake(config)
	}

	// Background Respawn Loop: Ensures terminal is always running
	go func() {
		for {
			time.Sleep(2 * time.Second)
			if !checkProcessRunning(config.WindowClass) {
				fmt.Printf("Terminal process (%s) closed. Respawning...\n", config.WindowClass)
				if ensureTerminalRunning(config) {
					fmt.Println("Respawn successful. Showing terminal...")
					// Reset visibility state so next toggle works correctly
					toggleMutex.Lock()
					targetVisible = false
					toggleMutex.Unlock()

					time.Sleep(500 * time.Millisecond) // Settle time
					toggleQuake(config)                // Drop it down immediately as requested
				}
			}
		}
	}()

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
				Value:    "goake",
				Writable: false,
				Emit:     prop.EmitTrue,
			},
			"Title": {
				Value:    "Goake (Left: Toggle | Middle: Quit)",
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

	// Capture Signals for Graceful Shutdown
	sigChan := make(chan os.Signal, 1)
	signal.Notify(sigChan, os.Interrupt, syscall.SIGTERM)

	fmt.Println("Goake daemon (Pure D-Bus SNI) running...")

	// Config hot-reload watcher
	go func() {
		path := findConfigFile()
		if path == "" {
			return
		}
		lastStat, _ := os.Stat(path)
		for {
			time.Sleep(1 * time.Second)
			stat, err := os.Stat(path)
			if err != nil {
				continue
			}
			if stat.ModTime().After(lastStat.ModTime()) {
				fmt.Println("Config change detected, reloading...")
				lastStat = stat
				newConfig := loadConfig()
				// Atomic-ish swap for basic fields
				*config = newConfig
				// Re-grab window to apply any geometry/class changes immediately
				ensureGrabbed(config)
			}
		}
	}()

	// Wait for signal
	<-sigChan

	fmt.Println("\nShutting down...")

	// Restore window on exit if needed
	restoreQuake(config)
	os.Exit(0)
}
