package main

import (
	"bytes"
	"fmt"
	"log"
	"os"
	"os/exec"
	"strconv"
	"strings"
	"syscall"
	"time"
)

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
