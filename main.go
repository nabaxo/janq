package main

import (
	"flag"
	"fmt"

	"github.com/godbus/dbus/v5"
)

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
