package main

import (
	"bytes"
	_ "embed"
	"fmt"
	"image"
	_ "image/png"
	"log"
	"os"
	"os/signal"
	"syscall"
	"time"

	"github.com/godbus/dbus/v5"
	"github.com/godbus/dbus/v5/prop"
)

//go:embed icon.png
var iconData []byte

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
			"IconName": {
				Value:    "goake",
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
	prop.Export(conn, "/StatusNotifierItem", props)

	// Register with the tray daemon
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
