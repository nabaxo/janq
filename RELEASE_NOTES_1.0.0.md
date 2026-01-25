*(Sloperator note: AI wrote all of it, I don't know how to write rust. I just gave it directions and provided the bezier curve for "windows" scrolling at most. Seriously, I have no idea if this is a well written app or not, but it works fine with everything I've thrown at it.).*

*What follows was written by AI (I told it to be sarcastic ¯\_(ツ)_/¯), lightly edited by the Sloperator*

# janq v1.0.0 — The Inaugural Release of Questionable Decisions

**Release Date:** January 24, 2026

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
- **No borders** option now cross-platform. Remove window borders/chrome for managed windows on both Windows and Linux.
- **Pager control**: `skip_pager` option to hide managed windows from task managers, pagers, and the task switcher (now defaults to `false`).
- **all_desktops setting (Linux)**: Choose whether managed windows follow you across virtual desktops (defaulting to `true`).
- **Desktop-Aware Focus (Linux)**: Closing the terminal no longer snaps you back to your previous desktop if you've moved desktops while the app was open.
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
- **Platform-Specific Validation**: janq now blocks startup with a hard error if you try to use Linux-specific settings on Windows, ensuring your configuration is valid for your current platform.

---

## 🛠️ Performance & Architecture

- **Zero-Scan Logic (Linux)**: KWin scripts now perform a single-pass window discovery using cached IDs/PIDs, eliminating expensive O(n) scans during toggles.
- **Desktop-Aware Focus Logic**: Focus restoration now respects your current virtual desktop, avoiding cross-desktop displacement calls on Linux.
- **Deterministic Sibling Lifecycle**: Sibling windows compute their own individual paths and durations. Linux backends use precise ID/PID matching to ensure siblings always respect their own individual settings (avoiding the "half-faded sibling" bug).
- **JSON Argument Consolidation (Linux)**: Refactored D-Bus script injection to pass consolidated JSON objects, replacing 26+ fragile positional arguments.
- **Pre-calculated Geometry**: Both platforms now fully pre-compute sibling trajectories and durations before entering the high-frequency animation loop.
- **Event-Driven Core**: Event-driven on both platforms; uses `PostThreadMessageW` on Windows to avoid the 15ms `GetMessage` sleep tax.
- **Sub-millisecond liveness checks**: Native `/proc` on Linux and `IsWindow` on Windows.
- **Memory footprint**: <2MB on Windows, ~3.4MB on Linux. Light enough to be ignored.
- **Flattened Proxy Architecture**: Eliminated redundant layers and split the Win32 monolith into focused sub-modules.

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

## ⚠️ Known Issues

### Animation Restart on Rapid Toggles
If you spam your hotkeys faster than the animation can finish, the window might "hitch" as it recalculates its journey. This is a design choice to prevent the window from just teleporting. You're welcome.

### Linux: Hotkey Registration Delay
On KDE, there's a 500ms delay when registering hotkeys. It's a workaround for a race condition in KWin that can cause crashes. It only affects startup. Consider it a feature for system stability.

### Linux: App Compatibility: Opacity Animations & Coordination
Some applications (especially Electron-based ones or XWayland clients) may experience unreliable transparency or "stutter" during motion on Linux. Due to the asynchronous nature of Wayland property updates, opacity and position may occasionally update in different frames. janq uses the "Fullscreen role" or `ForceBlur` to stabilize this, but for some apps, disabling `animate_opacity` remains the most stable choice.

### Sibling Animation Inconsistency
When multiple applications are configured, sibling windows use the target window's duration instead of their own configured `hide_duration`. This creates a minor visual inconsistency during transitions that I've attempted to fix multiple times with absolutely zero improvement over the original behavior. Every "fix" attempted made it worse. This is what we're shipping.

---

## 📄 License

MIT License
