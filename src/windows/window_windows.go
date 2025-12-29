package windows

import (
	"context"
	"goake/internal/config"
	"strings"
	"sync"
	"syscall"
	"time"
	"unsafe"

	"golang.org/x/sys/windows"
)

var (
	user32                  = windows.NewLazySystemDLL("user32.dll")
	dwmapi                  = windows.NewLazySystemDLL("dwmapi.dll")
	procFindWindow          = user32.NewProc("FindWindowW")
	procIsWindow            = user32.NewProc("IsWindow")
	procShowWindow          = user32.NewProc("ShowWindow")
	procSetWindowPos        = user32.NewProc("SetWindowPos")
	procGetWindowRect       = user32.NewProc("GetWindowRect")
	procSetForegroundWindow = user32.NewProc("SetForegroundWindow")
	procGetForegroundWindow = user32.NewProc("GetForegroundWindow")
	procGetSystemMetrics    = user32.NewProc("GetSystemMetrics")
	procRegisterHotKey      = user32.NewProc("RegisterHotKey")
	procUnregisterHotKey    = user32.NewProc("UnregisterHotKey")
	procGetMessage          = user32.NewProc("GetMessageW")
	procIsWindowVisible     = user32.NewProc("IsWindowVisible")

	procDwmFlush = dwmapi.NewProc("DwmFlush")
)

var (
	targetVisible     bool = false
	toggleMutex       sync.Mutex
	previousWindow    windows.HWND
	terminalWindow    windows.HWND       // Cached hwnd
	currentAnimCancel context.CancelFunc // Track running animation
)

const (
	SW_HIDE        = 0
	SW_SHOW        = 5
	SWP_NOSIZE     = 0x0001
	SWP_NOMOVE     = 0x0002
	SWP_NOZORDER   = 0x0004
	SWP_NOACTIVATE = 0x0010
	SWP_SHOWWINDOW = 0x0040
	SWP_NOCOPYBITS = 0x0400
	SWP_DEFERERASE = 0x2000
	HWND_TOPMOST   = ^uintptr(0) // -1
	HWND_NOTOPMOST = ^uintptr(1) // -2
)

type RECT struct {
	Left, Top, Right, Bottom int32
}

func findWindow(className, windowName string) windows.HWND {
	// If className is empty, pass NULL (0)
	var c *uint16
	if className != "" {
		c, _ = windows.UTF16PtrFromString(className)
	}
	// If windowName is empty, pass NULL
	var w *uint16
	if windowName != "" {
		w, _ = windows.UTF16PtrFromString(windowName)
	}

	ret, _, _ := procFindWindow.Call(uintptr(unsafe.Pointer(c)), uintptr(unsafe.Pointer(w)))
	return windows.HWND(ret)
}

func EnsureGrabbed(cfg *config.Config) {
	// On Windows, "grabbing" mainly means ensuring we can find it.
	// We might want to apply initial styles (remove borders) here.
	hwnd := findWindow("", cfg.General.WindowClass) // Using WindowClass as title/caption often works for WezTerm if --class isn't perfectly mapped to Win32 class
	if hwnd == 0 {
		// Try searching by class strictly?
		// WezTerm usually uses "org.wezfurlong.wezterm" as class
		hwnd = findWindow("org.wezfurlong.wezterm", "")
	}

	if hwnd != 0 {
		forceOnScreen(hwnd)
		// Apply initial styles if needed
		// SetWindowPos(hwnd, HWND_TOPMOST, 0, 0, 0, 0, SWP_NOMOVE|SWP_NOSIZE)
	}
}

func forceOnScreen(hwnd windows.HWND) {
	// Ensure window is on-screen
	var rect RECT
	procGetWindowRect.Call(uintptr(hwnd), uintptr(unsafe.Pointer(&rect)))

	// Check against virtual screen bounds
	vLeft, _, _ := procGetSystemMetrics.Call(76)   // SM_XVIRTUALSCREEN
	vTop, _, _ := procGetSystemMetrics.Call(77)    // SM_YVIRTUALSCREEN
	vWidth, _, _ := procGetSystemMetrics.Call(78)  // SM_CXVIRTUALSCREEN
	vHeight, _, _ := procGetSystemMetrics.Call(79) // SM_CYVIRTUALSCREEN

	// Safe Logic: If top-left corner is completely outside virtual screen, reset.
	// Or simpler: if it's off-screen, typically coordinates are very large negatives or positives,
	// or just not visible.
	// Let's check intersection roughly.
	vl := int32(vLeft)
	vt := int32(vTop)
	vr := vl + int32(vWidth)
	vb := vt + int32(vHeight)

	// If center of window is outside virtual screen?
	midX := (rect.Left + rect.Right) / 2
	midY := (rect.Top + rect.Bottom) / 2

	isOffScreen := midX < vl || midX > vr || midY < vt || midY > vb

	if isOffScreen {
		// Reset to Primary Monitor (0,0)
		// SWP_NOSIZE | SWP_NOZORDER
		procSetWindowPos.Call(uintptr(hwnd), 0, 0, 0, 0, 0, SWP_NOSIZE|SWP_NOZORDER)
	}
}

func ToggleQuake(cfg *config.Config) {
	toggleMutex.Lock()
	defer toggleMutex.Unlock()

	// 1. Efficient Window Discovery
	hwnd := terminalWindow
	res, _, _ := procIsWindow.Call(uintptr(hwnd))
	isFresh := false
	if res == 0 {
		// Cached window is gone or not yet found
		// Try finding by process first (fast lookup in current session)
		hwnd = findWindowByProcess("wezterm-gui.exe")
		if hwnd == 0 {
			// Not running at all? Call EnsureTerminalRunning (slow path)
			isFresh = true
			EnsureTerminalRunning(cfg)
			hwnd = findWindowByProcess("wezterm-gui.exe")
			if hwnd == 0 && cfg.General.WindowClass != "" {
				hwnd = findWindow("", cfg.General.WindowClass)
			}
		}
		terminalWindow = hwnd
	}

	if hwnd == 0 {
		return
	}

	// 2. Handle Animation Interruption
	isAnimating := (currentAnimCancel != nil)
	if isAnimating {
		// Cancel current animation
		currentAnimCancel()
		currentAnimCancel = nil
		// Reverse current intended state
		targetVisible = !targetVisible
	} else if isFresh {
		// Fresh launch: Always Show
		targetVisible = true
	} else {
		// Not animating: Check current reality
		ret, _, _ := procIsWindowVisible.Call(uintptr(hwnd))
		isVisible := ret != 0
		targetVisible = !isVisible
	}

	// 3. Prepare Animation Context
	ctx, cancel := context.WithCancel(context.Background())
	currentAnimCancel = cancel

	if targetVisible {
		// Showing
		// Capture previous window for restore
		ret, _, _ := procGetForegroundWindow.Call()
		previousWindow = windows.HWND(ret)

		// Get Current Window Size (Do NOT Resize)
		var rect RECT
		procGetWindowRect.Call(uintptr(hwnd), uintptr(unsafe.Pointer(&rect)))
		width := rect.Right - rect.Left
		height := rect.Bottom - rect.Top

		// Calculate Monitor Dimensions based on MOUSE position or CONFIG
		var monitorHandle uintptr

		if cfg.Window.DisplayMode == "follow-mouse" {
			var pt POINT
			procGetCursorPos.Call(uintptr(unsafe.Pointer(&pt)))
			// Use MonitorFromRect as it takes a pointer, avoiding struct-by-value ABI issues with MonitorFromPoint
			rect := RECT{Left: pt.X, Top: pt.Y, Right: pt.X + 1, Bottom: pt.Y + 1}
			// MONITOR_DEFAULTTONEAREST = 2
			monitorHandle, _, _ = procMonitorFromRect.Call(uintptr(unsafe.Pointer(&rect)), 2)
		} else {
			// Fallback (Primary)
			// MONITOR_DEFAULTTOPRIMARY = 1
			// Use a dummy rect at 0,0
			rect := RECT{Left: 0, Top: 0, Right: 1, Bottom: 1}
			monitorHandle, _, _ = procMonitorFromRect.Call(uintptr(unsafe.Pointer(&rect)), 1)
		}

		var mi MONITORINFO
		mi.CbSize = uint32(unsafe.Sizeof(mi))
		procGetMonitorInfo.Call(monitorHandle, uintptr(unsafe.Pointer(&mi)))

		screenWidth := mi.RcWork.Right - mi.RcWork.Left
		// screenHeight := mi.RcWork.Bottom - mi.RcWork.Top
		monitorX := mi.RcWork.Left
		monitorY := mi.RcWork.Top

		// Center horizontally on the correct monitor
		x := monitorX + (screenWidth-width)/2
		y := monitorY // Top of the work area

		// Determine Z-Order
		zOrder := uintptr(HWND_TOPMOST)
		if !cfg.Window.KeepAbove {
			zOrder = uintptr(HWND_NOTOPMOST)
		}

		if cfg.Animation.ShowDuration > 0 {
			go animateWindow(ctx, hwnd, x, y-height, x, y, width, height, cfg.Animation.ShowDuration, true, zOrder)
		} else {
			procSetWindowPos.Call(uintptr(hwnd), zOrder, uintptr(x), uintptr(y), uintptr(width), uintptr(height), SWP_SHOWWINDOW|SWP_NOSIZE)
			// Clear canceller if immediate
			cancel()
			currentAnimCancel = nil
		}

		procSetForegroundWindow.Call(uintptr(hwnd))

	} else {
		// Hiding
		var rect RECT
		procGetWindowRect.Call(uintptr(hwnd), uintptr(unsafe.Pointer(&rect)))

		width := rect.Right - rect.Left
		height := rect.Bottom - rect.Top

		// Determine Z-Order
		zOrder := uintptr(HWND_TOPMOST)
		if !cfg.Window.KeepAbove {
			zOrder = uintptr(HWND_NOTOPMOST)
		}

		if cfg.Animation.HideDuration > 0 {
			go animateWindow(ctx, hwnd, rect.Left, rect.Top, rect.Left, rect.Top-height, width, height, cfg.Animation.HideDuration, false, zOrder)
		} else {
			procShowWindow.Call(uintptr(hwnd), SW_HIDE)
			cancel()
			currentAnimCancel = nil
		}

		if previousWindow != 0 {
			procSetForegroundWindow.Call(uintptr(previousWindow))
		}
	}
}

func animateWindow(ctx context.Context, hwnd windows.HWND, startX, startY, endX, endY, width, height int32, durationMs int, show bool, zOrder uintptr) {
	// Cleanup global cancel on exit if context wasn't cancelled externally
	// Note: We need to be careful not to nil a NEW animation's cancel.
	// But since this runs in goroutine, simpler to just let ToggleQuake overwrite currentAnimCancel.

	if show {
		procShowWindow.Call(uintptr(hwnd), SW_SHOW)
	}

	// Optimization: Use DWM Flush for hardware-accelerated V-Sync
	// Instead of a fixed ticker, we sync with the monitor's refresh rate.

	// Flags for animation loop:
	// SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_NOCOPYBITS | SWP_DEFERERASE
	// NOCOPYBITS is critical for smooth movement as it prevents Windows from copying bits on every frame.
	loopFlags := uintptr(SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_NOCOPYBITS | SWP_DEFERERASE)

	start := time.Now()
	duration := time.Duration(durationMs) * time.Millisecond

	for {
		// Wait for V-Sync
		procDwmFlush.Call()

		select {
		case <-ctx.Done():
			return
		default:
			elapsed := time.Since(start)
			if elapsed >= duration {
				goto Finish
			}

			progress := float64(elapsed) / float64(duration)
			// Ease out
			progress = progress * (2 - progress)

			curX := int32(float64(startX) + (float64(endX-startX) * progress))
			curY := int32(float64(startY) + (float64(endY-startY) * progress))

			procSetWindowPos.Call(uintptr(hwnd), zOrder, uintptr(curX), uintptr(curY), uintptr(width), uintptr(height), loopFlags)
		}
	}

Finish:
	// Final pos - Ensure Z-Order is correct and bits are synced
	procSetWindowPos.Call(uintptr(hwnd), zOrder, uintptr(endX), uintptr(endY), uintptr(width), uintptr(height), SWP_NOSIZE)

	if !show {
		procShowWindow.Call(uintptr(hwnd), SW_HIDE)
	}

	// Interaction: If we finished naturally, we should clear currentAnimCancel so next Toggle handles logic as "not animating"?
	// But clearing a global var from a goroutine needs locking.
	toggleMutex.Lock()
	if ctx.Err() == nil {
		// Only clear if WE are the current animation (roughly).
		// Since we don't have ID, we just check if currentAnim is "us".
		// Actually, if we finished, future Toggles see currentAnimCancel != nil and think we are animating?
		// Is ctx == the current one?
		// We can't easily check Eq on functions.
		// However, preventing "zombie" currentAnimCancel is good.
		// We'll trust ToggleQuake to set it.
		// BUT if animation finishes, ToggleQuake thinks it's still animating next time?
		// YES. This IS A BUG in my plan.
		// We MUST clear currentAnimCancel when animation finishes naturally.
		// But we need to ensure we don't clear a NEW animation's cancel.
		// We can pass the cancel func to the goroutine and compare? pointers? cannot compare funcs.
		// Pass a ptr to bool?
		// Or just always clear? If we are running, lock is held? No.
		// If we finish, we grab lock. If currentAnimCancel is still "us" (how do we know?)...
		// If we cancelled, we returned early (above).
		// If we are here, we finished naturally.
		// Unless a new toggle started right as we finished?
		// If new toggle started, it replaced currentAnimCancel and didn't cancel "us" yet? Impossible, it cancels us first.
		// So if we are here, we were NOT cancelled.
		// So we are the active animation.
		// So it is safe to set currentAnimCancel = nil.
		currentAnimCancel = nil
	}
	toggleMutex.Unlock()
}

func RestoreQuake(cfg *config.Config) {
	// Restore logic if crashes/exits
	hwnd := findWindowByProcess("wezterm-gui.exe")
	if hwnd == 0 && cfg.General.WindowClass != "" {
		hwnd = findWindow("", cfg.General.WindowClass)
	}
	if hwnd != 0 {
		forceOnScreen(hwnd)
		// Force show, No Topmost (normal window), No Size, No Move
		procSetWindowPos.Call(uintptr(hwnd), uintptr(HWND_NOTOPMOST), 0, 0, 0, 0, SWP_SHOWWINDOW|SWP_NOSIZE|SWP_NOMOVE)
	}
}

// --- Helpers for Window Finding by Process ---

var (
	procGetWindowThreadProcessId = user32.NewProc("GetWindowThreadProcessId")
	procEnumWindows              = user32.NewProc("EnumWindows")
	// kernel32 and snapshot procs are already defined in process_windows.go which is in the same package
	procCreateToolhelp32Snapshot = kernel32.NewProc("CreateToolhelp32Snapshot")

	// Monitor API
	procMonitorFromRect = user32.NewProc("MonitorFromRect")
	procGetMonitorInfo  = user32.NewProc("GetMonitorInfoW")
	procGetCursorPos    = user32.NewProc("GetCursorPos")
)

func findWindowByProcess(exeName string) windows.HWND {
	pid := getProcessID(exeName)
	if pid == 0 {
		return 0
	}

	var foundHwnd windows.HWND
	cb := syscall.NewCallback(func(hwnd windows.HWND, lParam uintptr) uintptr {
		var wPid uint32
		procGetWindowThreadProcessId.Call(uintptr(hwnd), uintptr(unsafe.Pointer(&wPid)))
		if wPid == pid {
			// Check if visible to avoid hidden tool windows?
			// For now, accept first match, often the main window.
			// Ideally check GetWindow(GW_OWNER) == 0 && IsWindowVisible()
			foundHwnd = hwnd
			return 0 // Stop enumeration
		}
		return 1 // Continue
	})

	procEnumWindows.Call(cb, 0)
	return foundHwnd
}

func getProcessID(exeName string) uint32 {
	hSnap, _, _ := procCreateToolhelp32Snapshot.Call(0x00000002, 0) // TH32CS_SNAPPROCESS
	if hSnap == uintptr(windows.InvalidHandle) {
		return 0
	}
	defer windows.CloseHandle(windows.Handle(hSnap))

	var pe32 PROCESSENTRY32
	pe32.Size = uint32(unsafe.Sizeof(pe32))

	if ret, _, _ := procProcess32First.Call(hSnap, uintptr(unsafe.Pointer(&pe32))); ret == 0 {
		return 0
	}

	for {
		name := syscall.UTF16ToString(pe32.ExeFile[:])
		if strings.EqualFold(name, exeName) {
			return pe32.ProcessID
		}
		if ret, _, _ := procProcess32Next.Call(hSnap, uintptr(unsafe.Pointer(&pe32))); ret == 0 {
			break
		}
	}
	return 0
}

// Structs

type POINT struct {
	X, Y int32
}

type MONITORINFO struct {
	CbSize    uint32
	RcMonitor RECT
	RcWork    RECT
	DwFlags   uint32
}
