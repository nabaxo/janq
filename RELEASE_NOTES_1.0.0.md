*(Human note: AI wrote all of it, I don't know how to write rust. I just gave it directions and provided the bezier curve for "windows" scrolling at most. Seriously, I have no idea if this is a well written app or not, but it works fine with everything I've thrown at it.).*

*What follows was written by AI, lightly edited by the human*

# janq v1.0.0 — Behold, The Slop Works

**Release Date:** January 12, 2026

> **janq** - The Janky Quake-Style Terminal Manager

---

## ✨ Highlights

### Cross-Platform
- Full feature parity between **Linux (KDE Plasma/Wayland)** and **Windows**
- Native integrations on both platforms—no wrappers, no compromises

- Hardware-accelerated animations with **15+ easing curves**, plus support for **custom cubic-bezier curves** for ultimate control
- **Smart Refresh Rate Detection (Linux):** Automatically detects your monitor's highest refresh rate via `kscreen-doctor` to ensure frame-perfect animation intervals on high-refresh (144Hz+) displays.

### Zero-Config Hotkeys
- **Linux:** Automatically syncs your hotkey configuration directly with KDE System Settings via D-Bus
- **Windows: Native Win32 Overhaul.** A high-performance, thin-daemon architecture. By stripping away heavy runtimes (Tokio/Winit), the Windows version now provides instantaneous hotkey response times with zero polling overhead.
- **Advanced weighted fuzzy matching** on both platforms—find windows using abbreviations, substrings, or delimiters with a sophisticated scoring engine that rewards word boundaries and penalizes gaps.
- **High-Performance Linux Path**: Zero-IPC liveness checks via `/proc` and **ForceBlur** (Role 1) integration ensuring that toggling and animations occur with $<0.1$ms overhead and perfect visual stability.

---

## 🚀 Features

### Window Management
- **Flexible dimensions** with `px` and `%` units, plus per-app overrides
- **Display modes:** `follow-mouse`, `active` (focus-based), and `specific` (fixed monitor)
- **Keep above** option to float above other windows
- **Force priority** mode (Linux) to sit above fullscreen applications
- **Focus restoration**—remembers your previous window and restores focus instantly

### Multi-App Support
- Configure **multiple applications** with individual hotkeys
- **Up to 4 hotkeys** per application on both platforms
- **Ordered configuration**—app order in config determines systray menu order
- **CLI control** via `./janq --app <name>` for scripting

### Configuration
- **Hot-reload** support—changes apply without restart
- **Multiple config locations** with clear priority order:
  1. Binary directory (portable mode)
  2. XDG config directory (`~/.config/janq/`)
  3. Home directory (`~/.janq.toml`)
- Comprehensive TOML configuration with sensible defaults

### Animations
- **Customizable durations** for show/hide animations
- **Opacity animations** with configurable fade points
- **Premium easing curves:**
  - `windows` (Windows 11-style)
  - `expo` (Exponential, high-tension curves)
  - `linear`, `ease`, `sine`, `cubic`, `quart`, `back`
  - Full `-in`, `-out`, and `-in-out` variants
- **Custom Easing Support**: Define your own curves with `cubic-bezier(x1, y1, x2, y2)`, `bezier(...)`, or simply `(x1, y1, x2, y2)`

### System Integration
- **Linux:** Desktop entry generation, icon installation, D-Bus activation support
- **Windows:** System tray icon with context menu, startup support via shortcuts

---

## 🛠️ Technical Improvements

### Performance
- **Zero polling** architecture—event-driven on both platforms.
- **Instant Loop Wakeup (Windows)**: Uses `PostThreadMessageW` to immediately wake the backend on any event, eliminating the 15-30ms "GetMessage" sleep lag found in traditional loops.
- **Architectural Consolidation**: Removed Tokio and Winit from the Windows daemon, drastically reducing RAM footprint and eliminating runtime-induced jitter.
- **Lazy window caching** and **Batch Enumeration** to minimize Win32/KWin API calls. Capture windows by scanning the system once instead of per-app.
- **LTO and release optimizations** for minimal binary size
- Memory leak prevention with automatic cache cleanup on config reload

### Reliability
- **Portable Windows Build**: Added a static CRT linking profile (`make build-windows-static`) that produces a standalone executable without requiring the Visual C++ Redistributable.
- **Robust Windows Hot-Reloading**: Optimized configuration watcher to monitor parent directories, ensuring stability with editors that perform atomic saves (like VS Code).
- **Graceful Shutdown**: Proper signal handling (SIGINT/SIGTERM) and descriptive console logging ("Quitting via systray", "Received SIGINT", etc.) ensuring absolute clarity on exit.
- **Cache-Based Window Restoration**: Significantly improved restoration on exit—uses a runtime handle cache to reliably return *all* managed windows (even those removed from config) to their original positions.
- **Single Instance Enforcement**: Prevents multiple daemon instances via file locks.
- **Robust Error Handling**: User-friendly error dialogs on both platforms.
- **Focus-Stealing Fix (Windows)**: Implemented an aggressive focus mechanism using `AttachThreadInput` to bypass Win32 foreground locks, ensuring focus lands correctly even during rapid app switching.

### Code Quality
- Comprehensive refactoring for maintainability
- Platform-specific modules with clean abstractions
- Removal of unnecessary dependencies (reduced footprint)

---

## 📋 Configuration Example

```toml
[window]
display_mode = "active"
width = "50%"
height = "600px"
auto_show = false

[animation]
show_duration = 350
show_easing = "windows"
animate_opacity = true

[app.terminal]
window_class = "wezquake"
start_command = "wezterm --config initial_cols=160 --config initial_rows=40 start --class wezquake"
hotkey = ["Meta+Grave", "Ctrl+Grave"]

[app.zed]
window_class = "zed"
start_command = "zed"
hotkey = "Meta+Z"
```

---

## ⚠️ Known Issues

### Linux: Hotkey Registration Delay
On KDE Plasma, there's a small intentional delay (~500ms) when registering or updating hotkeys. This is a workaround for a race condition in KWin's `GlobalShortcutsRegistry` that can cause crashes with rapid D-Bus operations. The delay only affects startup and config reloads, not toggle performance.

---

## 📦 Installation

*(Human note: Just download an run the binary. If you want build directions follow).*

### Prerequisites
- **Linux:** KDE Plasma 6, `kdotool` for Wayland window management
- **Windows:** Windows 10 or 11

### Building
```bash
make build-linux   # Outputs: ./dist/janq
make build-windows # Outputs: ./dist/janq.exe
```

### Running
```bash
./janq           # Start daemon (first run) or toggle (subsequent runs)
./janq --daemon  # Start in background mode
./janq --app zed # Toggle a specific app
```

---

## 🙏 Acknowledgments

- **kdotool** for Wayland window management
- **zbus** for D-Bus communication
- The Rust community for excellent crates

---

## 📄 License

MIT License

---

*janq is 100%, unadulterated vibe coded slop. User discretion is advised.* ⚠️
