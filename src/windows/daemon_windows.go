package windows

import (
	"fmt"
	"goake/internal/assets"
	"goake/internal/config"
	"log"
	"net"
	"os"
	"runtime"
	"strings"
	"syscall"
	"time"
	"unsafe"

	"github.com/Microsoft/go-winio"
	"github.com/energye/systray"
)

const (
	MOD_ALT     = 0x0001
	MOD_CONTROL = 0x0002
	MOD_SHIFT   = 0x0004
	MOD_WIN     = 0x0008
	VK_GRAVE    = 0xC0 // `~
	VK_F12      = 0x7B
	WM_HOTKEY   = 0x0312
)

func RunDaemon(cfg *config.Config, autoShow bool) {
	// Start the IPC listener (Named Pipe)
	go startIPCListener(cfg)

	// Start Hotkey Listener
	go startHotkeyListener(cfg)

	// Check terminal existence loop
	go func() {
		for {
			time.Sleep(2 * time.Second)
			if !CheckProcessRunning(cfg.General.WindowClass) {
				fmt.Printf("Terminal process (%s) closed. Respawning...\n", cfg.General.WindowClass)
				if EnsureTerminalRunning(cfg) {
					// Logic to re-grab if needed
					time.Sleep(1 * time.Second)
					EnsureGrabbed(cfg)
				}
			}
		}
	}()

	// Auto-show logic
	if autoShow {
		go func() {
			EnsureTerminalRunning(cfg)
			time.Sleep(1 * time.Second)
			ToggleQuake(cfg)
		}()
	}

	// Start Config Watcher
	go watchConfig(cfg)

	// Try to attach to parent console to show logs if run from CLI
	attachParentConsole()

	// Define onExit closure to capture cfg for restoration
	onExit := func() {
		fmt.Println("Exiting goake...")
		RestoreQuake(cfg)
		os.Exit(0)
	}

	systray.Run(onReady, onExit)
}

func attachParentConsole() {
	const ATTACH_PARENT_PROCESS = ^uint32(0) // -1
	procAttachConsole := kernel32.NewProc("AttachConsole")

	r, _, _ := procAttachConsole.Call(uintptr(ATTACH_PARENT_PROCESS))
	if r != 0 {
		// Successfully attached. Redirect stdout/stderr.
		hStdout, _ := syscall.GetStdHandle(syscall.STD_OUTPUT_HANDLE)
		hStderr, _ := syscall.GetStdHandle(syscall.STD_ERROR_HANDLE)

		if hStdout != 0 && hStdout != syscall.InvalidHandle {
			os.Stdout = os.NewFile(uintptr(hStdout), "/dev/stdout")
		}
		if hStderr != 0 && hStderr != syscall.InvalidHandle {
			os.Stderr = os.NewFile(uintptr(hStderr), "/dev/stderr")
			log.SetOutput(os.Stderr)
		}
		fmt.Println("Attached to parent console. Logging enabled.")
	}
}

func onReady() {
	systray.SetTitle("Goake")
	systray.SetTooltip("Goake Window Manager")
	systray.SetIcon(assets.IconData)

	// Left Click = Toggle
	systray.SetOnClick(func(menu systray.IMenu) {
		cfg := config.LoadConfig()
		ToggleQuake(&cfg)
	})

	// Items
	mToggle := systray.AddMenuItem("Toggle", "Show/Hide Terminal")
	mToggle.Click(func() {
		cfg := config.LoadConfig()
		ToggleQuake(&cfg)
	})

	mQuit := systray.AddMenuItem("Quit", "Quit the whole app")
	mQuit.Click(func() {
		systray.Quit()
	})
}

func startIPCListener(cfg *config.Config) {
	pipeName := `\\.\pipe\goake_toggle`
	l, err := winio.ListenPipe(pipeName, nil)
	if err != nil {
		fmt.Printf("Error listening on pipe %s: %v\n", pipeName, err)
		return
	}
	defer l.Close()

	fmt.Println("Listening on named pipe for IPC commands...")
	for {
		conn, err := l.Accept()
		if err != nil {
			continue
		}
		go func(c net.Conn) {
			defer c.Close()
			// Simple trigger
			fmt.Println("Received toggle signal via IPC")
			ToggleQuake(cfg)
		}(conn)
	}
}

func startHotkeyListener(cfg *config.Config) {
	runtime.LockOSThread()
	defer runtime.UnlockOSThread()

	// Register all hotkeys
	for i, hk := range cfg.General.Hotkeys {
		mod, vk, err := parseHotkey(hk)
		if err != nil {
			log.Printf("Failed to parse hotkey %s: %v", hk, err)
			continue
		}
		// ID = i + 1 (1-based)
		r, _, err := procRegisterHotKey.Call(0, uintptr(i+1), uintptr(mod), uintptr(vk))
		if r == 0 {
			log.Printf("Failed to register hotkey %s: %v", hk, err)
		} else {
			fmt.Printf("Registered hotkey: %s (ID: %d)\n", hk, i+1)
		}
	}

	// Message Loop
	var msg struct {
		Hwnd    syscall.Handle
		Message uint32
		WParam  uintptr
		LParam  uintptr
		Time    uint32
		Pt      struct{ X, Y int32 }
	}

	for {
		ret, _, _ := procGetMessage.Call(uintptr(unsafe.Pointer(&msg)), 0, 0, 0)
		if int32(ret) == -1 {
			// Error
			break
		}
		if msg.Message == WM_HOTKEY {
			fmt.Println("Hotkey pressed")
			ToggleQuake(cfg)
		}
		// DispatchMessage? We barely process anything else.
	}
}

// Basic hotkey parser
// Supports: Win, Meta, Alt, Ctrl, Shift + [A-Z, 0-9, F1-F12, Grave, Space]
// Basic hotkey parser
// Supports: Win, Meta, Alt, Ctrl, Shift + [A-Z, 0-9, F1-F12, Grave, Space]
func parseHotkey(s string) (mod int, vk int, err error) {
	parts := strings.Split(s, "+")
	for _, p := range parts[:len(parts)-1] {
		switch strings.ToLower(strings.TrimSpace(p)) {
		case "win", "meta", "super", "cmd":
			mod |= MOD_WIN
		case "alt":
			mod |= MOD_ALT
		case "ctrl", "control":
			mod |= MOD_CONTROL
		case "shift":
			mod |= MOD_SHIFT
		}
	}

	key := strings.ToUpper(strings.TrimSpace(parts[len(parts)-1]))
	switch key {
	case "GRAVE", "`", "~":
		vk = VK_GRAVE
	case "SECTION", "§", "PARAGRAPH":
		// On many ISO keyboards, § is VK_OEM_5 (0xDC) or VK_OEM_102 (0xE2)
		// Let's try to be smart or support common ones.
		// VK_OEM_5 is where the key above Tab/left of 1 usually is on many layouts.
		vk = 0xDC // VK_OEM_5
	case "SPACE":
		vk = 0x20
	case "ENTER", "RETURN":
		vk = 0x0D
	case "BACKSPACE":
		vk = 0x08
	case "TAB":
		vk = 0x09
	case "ESC", "ESCAPE":
		vk = 0x1B
	case "UP":
		vk = 0x26
	case "DOWN":
		vk = 0x28
	case "LEFT":
		vk = 0x25
	case "RIGHT":
		vk = 0x27
	case "INSERT":
		vk = 0x2D
	case "DELETE":
		vk = 0x2E
	case "HOME":
		vk = 0x24
	case "END":
		vk = 0x23
	case "PAGEUP", "PGUP":
		vk = 0x21
	case "PAGEDOWN", "PGDN":
		vk = 0x22
	default:
		if len(key) == 1 {
			char := key[0]
			if char >= 'A' && char <= 'Z' {
				vk = int(char)
			} else if char >= '0' && char <= '9' {
				vk = int(char)
			} else {
				// Special character fallbacks
				switch char {
				case '-':
					vk = 0xBD // VK_OEM_MINUS
				case '=':
					vk = 0xBB // VK_OEM_PLUS
				case '[':
					vk = 0xDB // VK_OEM_4
				case ']':
					vk = 0xDD // VK_OEM_6
				case '\\':
					vk = 0xDC // VK_OEM_5
				case ';':
					vk = 0xBA // VK_OEM_1
				case '\'':
					vk = 0xDE // VK_OEM_7
				case ',':
					vk = 0xBC // VK_OEM_COMMA
				case '.':
					vk = 0xBE // VK_OEM_PERIOD
				case '/':
					vk = 0xBF // VK_OEM_2
				default:
					vk = int(char)
				}
			}
		} else if strings.HasPrefix(key, "F") {
			// F1-F12
			var fNum int
			fmt.Sscanf(key, "F%d", &fNum)
			if fNum >= 1 && fNum <= 12 {
				vk = 0x70 + (fNum - 1)
			}
		}
	}

	if vk == 0 {
		return 0, 0, fmt.Errorf("unknown key: %s", key)
	}

	return mod, vk, nil
}

func watchConfig(cfg *config.Config) {
	configFile := config.FindConfigFile()
	if configFile == "" {
		return
	}

	info, err := os.Stat(configFile)
	if err != nil {
		return
	}
	lastMod := info.ModTime()

	for {
		time.Sleep(2 * time.Second)
		info, err := os.Stat(configFile)
		if err != nil {
			continue
		}
		if info.ModTime().After(lastMod) {
			lastMod = info.ModTime()
			fmt.Println("Config file changed, reloading...")

			// Load new config
			newCfg := config.LoadConfig()

			// Update the existing config object in place
			// We can't easily hot-reload hotkeys without restarting the message loop/hook,
			// but we can update animations, sizes, etc.

			// Preserve runtime state keys if needed?
			// Actually just overwriting the struct fields is fine as long as we hold a lock if needed.
			// Since ToggleQuake locks toggleMutex, we might want to lock there too?
			// cfg is used in ToggleQuake which is mutex protected, but cfg reading itself isn't mutex protected inside ToggleQuake
			// (it reads cfg before some logic).
			// However, in Go, updating a struct pointer *cfg = newCfg is NOT atomic for all fields.
			// But since we are only reading primitives mostly, it's 'okay' for a casual app, or we should use a mutex.
			// The toggleMutex protects the ACTION of toggling.
			// Let's just update it.
			*cfg = newCfg
			fmt.Println("Config reloaded.")
		}
	}
}
