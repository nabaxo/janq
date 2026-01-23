*(Sloperator note: AI wrote all of it, I don't know how to write rust. I just gave it directions and provided the bezier curve for "windows" scrolling at most. Seriously, I have no idea if this is a well written app or not, but it works fine with everything I've thrown at it.).*

*What follows was written by AI (I told it to be sarcastic ¯\_(ツ)_/¯), lightly edited by the Sloperator*

# janq v1.0.0 — The Inaugural Release of Questionable Decisions

**Release Date:** January 12, 2026

> **janq** - The Janky Quake-Style Terminal Manager (Because apparently, the existing ones weren't janky enough)

---

## 🏗️ What is this?

Welcome to janq 1.0.0, a cross-platform terminal manager that somehow manages to hide windows without crashing your compositor. This release represents a significant quantity of code, most of which was written while the AI was contemplating the heat death of the universe. It animates, it toggles, and it generally behaves itself unless you try to do something clever.

---

## ✨ Features (The things that actually shipped)

### Cross-Platform Parity
- **Full symmetry** between Linux (KDE Plasma 6 / Wayland) and Windows 10/11.
- Native platform integrations—Win32 on Windows and D-Bus/KWin on Linux—ensuring you get the same performance profile (and system journal spam) regardless of your OS choice.
- Identical TOML configuration. You only have to learn one syntax to misconfigure your workspace.

### Window Management
- **Flexible dimensions** with `px` and `%` units, because fixed-pixel layouts are a relic of the past.
- **Display modes:**
  - `follow-mouse` - Window appears where your cursor is (default).
  - `active` - Window appears on the monitor with keyboard focus.
  - `specific` - For when you want to fight the automation.
- **Slide directions:** `top`, `bottom`, `left`, `right`. Choice is an illusion, but we provide it anyway.
- **Keep above** option to ensure your terminal stays on top, regardless of what you're trying to hide behind it.
- **No borders** option (Linux) to remove window chrome. Enabled by default because Quake terminals look better without frames.
- **Force priority** mode (Linux) to sit above fullscreen apps using KWin's Fullscreen state.
- **Focus restoration** - Attempting to put focus back where it was before we interrupted you. Results may vary.

### Multi-App Support
- Configure **multiple applications** with individual hotkeys.
- **Up to 4 hotkeys** per application. Why you'd need four is between you and your god.
- **Ordered configuration** - The order in your TOML determines the order in the tray.
- **Atomic switching** - Synchronized transitions where outgoing windows clear the way for incoming ones. It looks professional, which helps hide the internal chaos.

### Animation System
- **Hardware-accelerated animations** that support high refresh rate monitors (144Hz+).
- **15+ easing curves** including the `impulse` (Windows 11) preset and custom `cubic-bezier` support.
- **Velocity-based duration scaling** - Windows travel at a constant speed rather than a constant time. It’s basically physics.
- **Opacity animations** with configurable fade points.

### Hotkey System
- **Linux:** Native D-Bus sync with KDE. Your hotkeys will appear in System Settings, just like the real ones.
- **Windows:** Native Win32 registration. Instant response, unlike most things on Windows.
- **Weighted matching** - Find windows by abbreviation or substring (e.g., `wt` → `WindowsTerminal`).

---

## 🛠️ Technical Details (For the curious or the jaded)

### Performance & Reliability
- **Zero-polling architecture**: Event-driven on both platforms to save your CPU for more important things.
- **Instant loop wakeup (Windows)**: Uses `PostThreadMessageW` to avoid the 15ms `GetMessage` sleep tax.
- **Platform Cache Parity**: Consolidated Linux caches and a optimized `APP_CACHE` on Windows. Sub-millisecond liveness checks using native `/proc` on Linux and `IsWindow` on Windows.
- **Memory footprint**: <2MB on Windows, ~3.4MB on Linux. Light enough to be ignored.

### Code Quality
- **Flattened Proxy Architecture**: Eliminated the redundant `daemon.rs` and `terminal.rs` files.
- **Windows refactoring**: Split the 1,200-line Win32 monolith into focused, manageable sub-modules.
- **Unified Cache Architecture**: Both platforms now agree on how to track a Window ID without double-caching.
- **Robust Hot-Reloading**: The daemon now survives your configuration typos. If a reload fails, it shows the error and continues using the last valid state instead of shutting down.

---

## 📋 Configuration Example

```toml
[window]
display_mode = "active"
width = "50%"
height = "600px"
slide_from = "top"
offset = "center"

[animation]
duration = 350
easing = "impulse"
animate_opacity = true

[app.terminal]
window_class = "wezquake"
start_command = "wezterm start --class wezquake"
hotkey = ["Meta+Grave", "Ctrl+Grave"]
```

---

## ⚠️ Known Issues (The things we've accepted)

### Animation Restart on Rapid Toggles
If you spam your hotkeys faster than the animation can finish, the window might "hitch" as it recalculates its journey. This is a design choice to prevent the window from just teleporting. You're welcome.

### Linux: Hotkey Registration Delay
On KDE, there's a 500ms delay when registering hotkeys. It's a workaround for a race condition in KWin that can cause crashes. It only affects startup. Consider it a feature for system stability.

### Sibling Animation Inconsistency
Sometimes multiple windows hide at slightly different speeds because they share the target window's duration. Every "fix" attempted made it worse. This is what we're shipping.

### App Compatibility: Opacity Animations
Electron apps (Obsidian, VS Code, etc.) may experience unreliable transparency during motion on Linux.

---

## 📦 Installation

*(Sloperator note: Just download an run the binary. If you want build directions follow).*

### Prerequisites
- **Linux:** KDE Plasma 6
- **Windows:** Windows 10 or 11

### Building
```bash
make build-linux-musl          # Static Linux binary
make build-windows-static      # Static Windows binary
```

---

## 🧹 The `utilities/` Folder

The `utilities/` directory contains cleanup scripts for Linux. These exist because, during development, we managed to break KDE shortcuts and leave zombie processes more times than we'd like to admit. If things get weird, run these.

---

## 🙏 Acknowledgments

- **KWin Scripting API** for making Wayland window management possible (mostly).
- **The Rust community** for crates that saved us from manual memory management.
- **The Sloperator** for providing directions and enduring the regressions.
- **Coffee**, for keeping the AI's servers powered (presumably).

---

## 📄 License

MIT License

---

*janq is exactly what it says on the tin: a janky terminal manager with aspirations of greatness.*
