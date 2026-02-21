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
- **Zero-Logout Installation (Linux)**: Integrated D-Bus config reloading and `kbuildsycoca6` triggers ensure that icons and hotkeys work on the very first run without requiring a session restart.
- Identical TOML configuration. You only have to learn one syntax to misconfigure your workspace.

### Window Management
- **Flexible dimensions** with `px` and `%` units, because fixed-pixel layouts are a relic of the past.
- **Display modes:**
  - `follow-mouse` - Window appears where your cursor is (default).
  - `active` - Window appears on the monitor with keyboard focus.
  - `specific` - For when you want to fight the automation.
- **Auto-Hide focus watcher**: Automatically hide the managed window when focus is lost to another application (configurable per-window via `auto_hide = true`).
- **Slide directions:** `top`, `bottom`, `left`, `right`. Choice is an illusion, but we provide it anyway.
- **Keep above** option to ensure your terminal stays on top, regardless of what you're trying to hide behind it.
- **No borders** option now supports per-app overrides. Remove window borders/chrome for specific managed windows or set a global default on both Windows and Linux.
- **Pager control**: `skip_pager` option to hide managed windows from task managers, pagers, and the task switcher (now defaults to `false`).
- **all_desktops setting (Linux)**: Choose whether managed windows follow you across virtual desktops (defaulting to `true`).
- **Desktop-Aware Focus (Linux)**: Closing the terminal no longer snaps you back to your previous desktop if you've moved desktops while the app was open.
- **Force priority** mode (Linux) to sit above fullscreen apps using KWin's Fullscreen state.
- **Focus restoration** - Attempting to put focus back where it was before we interrupted you. Results may vary.
- **(Windows) Taskbar hiding for stubborn apps**: Apps that set `WS_EX_APPWINDOW` (looking at you, Basitune) would force a taskbar button to persist even when janq was trying to hide them. janq now strips this flag when managing a window and politely restores it on exit. Your taskbar is yours again.

### System Tray
- **Full context menu (Linux)**: Right-click the tray icon and get a proper menu with per-app toggle items, shortcut display, and a Quit option. Implemented via a hand-rolled `com.canonical.dbusmenu` interface on the existing `zbus` connection, because apparently using a dedicated crate for this was too luxurious.
- **Hot-reload aware**: The menu updates instantly when you save your config. No restart, no delay, no excuses.
- **Left-click**: Toggles the first configured app. **Middle-click** (Linux): Quit. **Shift+Left-click** (Windows): Quit. Because platform consistency is overrated.

### Multi-App Support
- Configure **multiple applications** with individual hotkeys.
- **Up to 4 hotkeys** per application. Why you'd need four is between you and your god.
- **Ordered configuration** - The order in your TOML determines the order in the tray menu.
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
- **Automated Icon Rule Lifecycle (Linux)**: Added a sophisticated synchronization engine for KWin Window Rules. Janq now automatically detects your apps' `.desktop` associations and creates/updates system rules to force correct taskbar icons. It safely prunes stale rules and manages the `kwinrulesrc` registry automatically. This behavior can be toggled via the new `kde_window_rules = true` setting in the `[window]` section.
- **New `--setup` and `--cleanup` Flags (Linux)**: Dedicated commands to either force a full refresh of all system integrations or completely purge them (icons, desktop files, D-Bus services, and window rules) from your system.
- **Platform-Aware Path Discovery**: janq now provides helpful, platform-specific error messages when a configuration file isn't found, correctly identifying `%APPDATA%\janq\janq.toml` as the preferred location on Windows.
- **Platform-Specific Validation**: janq now blocks startup with a hard error if you try to use Linux-specific settings on Windows, ensuring your configuration is valid for your current platform.

---

## 🛠️ Performance & Architecture

- **Zero-Scan Logic (Linux)**: KWin scripts now perform a single-pass window discovery using cached IDs/PIDs, eliminating expensive O(n) scans during toggles.
- **Desktop-Aware Focus Logic**: Focus restoration now respects your current virtual desktop, avoiding cross-desktop displacement calls on Linux.
- **Synchronized Sibling Lifecycle**: Sibling windows compute their own individual paths while sharing the target window's base duration. Linux backends use precise ID/PID matching to ensure synchronized, velocity-scaled transitions. (See [Sibling Animation Inconsistency](#sibling-animation-inconsistency)).
- **JSON Argument Consolidation (Linux)**: Refactored D-Bus script injection to pass consolidated JSON objects, replacing 26+ fragile positional arguments.
- **Pre-calculated Geometry**: Both platforms now fully pre-compute sibling trajectories and durations before entering the high-frequency animation loop.
- **Unified Async Architecture**: Total migration to a cross-platform Tokio-based async runtime. Replaced fragmented bridge threads with a single unified event loop for IPC, animations, and heartbeats across both platforms.
- **Sub-millisecond liveness checks**: Native `/proc` on Linux and `IsWindow` on Windows.
- **Memory footprint**: ~2.5MB on Windows, **~1.7MB on Linux**. Down from ~3.5MB through a campaign of increasingly unhinged dependency elimination: `clap`, `anyhow`, `dirs`, `ksni`, and the `notify` crate (on Linux) were all shown the door. The standard library itself was recompiled with size optimizations. At some point this stopped being engineering and became a grudge.
- **Nightly Build Optimization**: The default Linux build now uses `cargo +nightly -Zbuild-std` with `panic=immediate-abort`, recompiling `std` with the project's optimization settings and stripping all panic formatting infrastructure. Panics silently abort instead of printing a message, which is fine because if janq panics you have bigger problems. Stable build remains available for the risk-averse.
- **ksni Elimination (Linux)**: Replaced the `ksni` systray crate (which ran its own entire D-Bus connection) with ~220 lines of hand-rolled dbusmenu interface on the existing `zbus` connection. Saved ~248 KiB RSS. The crate wasn't doing anything wrong per se, but it was paying rent for a whole D-Bus stack when we already had one.
- **Raw inotify (Linux)**: Replaced the `notify` crate with direct inotify syscalls for config file watching, because pulling in a cross-platform file watcher to watch one file on one platform felt excessive.
- **Minimalist Argument Parsing**: Replaced `clap` with a minimal manual parser in `main.rs` to reduce binary complexity and overhead.
- **Dependency Slimming**: Eliminated `anyhow` and `dirs` dependencies, replacing them with a lightweight custom `Result` type, local error macros, and a platform-native `paths` module.
- **Major Ecosystem Modernization**:
  - **Windows 0.62 Conversion**: Deep refactor of all Win32 API calls to match strict `Option<Handle>` type requirements.
  - **Notify 8.2 & TOML 0.9**: Modernized file watching and serialization stacks.
  - **Performance Optimization**: Integrated `FxHash` and `tokio::signal`, and migrated to `fs4` for better file locking.
- **The "Stupid Shit" Technical Audit**:
  - **Zero-Allocation Discovery**: Windows window enumeration now filters "junk" classes directly on stack-allocated buffers. Thousands of transient `String` allocations and `.to_lowercase()` calls were eliminated.
  - **Atomic Startup Guards**: Added thread-level atomicity to window discovery. Spamming a hotkey while an app is starting now results in graceful event-dropping instead of a race condition that could "grab" transient utility windows.
  - **Linear Backoff Polling**: Polling for new windows on Windows now scales from 100ms up to 1000ms, heavily reducing CPU overhead during application launch.
- **Async KWin Initialization**: KDE display queries are now fully non-blocking, preventing initialization-time executor stalls.
  - **Window ID-Strictness**: Replaced probabilistic PID-based sibling matching on Linux with strict internal window ID tracking to handle multi-vault Obsidian setups and Electron-based "hidden" windows.
- **Library Split & Refactor**: Extracted shared core logic into a dedicated library crate (`src/lib.rs`), reducing binary size and eliminating platform-specific code duplication.
- **Error Handling Polish**:
  - **GUI Notifications**: Critical errors now display as GUI pop-ups (MessageBox on Windows, terminal on Linux) when running in non-interactive sessions, preventing silent failures. Warning pop-ups added for non-critical issues.
  - **Single-Instance Lock Hardening**: Lock file mechanism now correctly handles platform-specific `fs4` behavior. Lock acquisition moved to cache directory and only occurs when starting daemon, not during client IPC. Lock handle lifetime fixed using `Box::leak()` to prevent premature release during async yields.
  - **CLI Error Feedback**: Enhanced argument parsing with fuzzy-match suggestions for unknown flags.

---

## 📦 Installation

*(Sloperator note: If on windows Just download an run the binary (x64). If on Linux (also x64), run the install script: `curl -f https://git.nabaxo.dev/nabaxo/janq/raw/branch/main/install.sh | sh -s -- --help`. If you want build directions follow).*

### Prerequisites
- **Linux:** KDE Plasma 6 (Wayland)
- **Windows:** Windows 10 or 11

### Linux: The "First Run" Reality Check
If you're on a fresh KDE Plasma 6 install and janq starts up but refuses to show an icon or complains that "the name is not activatable" when you hit your hotkey—don't panic. It's not (entirely) janq's fault. D-Bus and the icon cache sometimes need a gentle, recursive shove.

Run this to force the system to acknowledge janq's existence without having to log out like it's 1999:
```bash
./janq --setup
```
This re-registers the D-Bus service, re-installs the icon, and forces `kbuildsycoca6` to actually do its job. It's the "Did you try turning it off and on again?" of Linux window management.

### Building
```bash
make build                     # Default: nightly Linux + stable Windows
make build-linux-nightly       # Optimized Linux binary (~1.6 MiB RSS)
make build-linux-musl          # Stable fallback Linux binary (~2.4 MiB RSS)
make build-windows-static      # Static Windows binary
```

> The nightly build requires `rustup component add rust-src --toolchain nightly` and the musl target. If nightly isn't your thing, `make build-linux-musl` works with stable and produces a perfectly functional binary that just happens to be slightly larger.

### Selection Engine & Stability Overhaul
- **Unified matching logic**: Windows and Linux now share the same "brain" for finding your windows. No more platform-specific bugs where Windows finds your terminal but Linux gets confused by the class name.
- **Aggressive PID Pruning**: Fixed "Ghost Terminals" on Windows where killing a terminal via Task Manager would leave janq staring at a dead window handle. It now detects process death instantly and spawns a fresh one.
- **Momentum-Aware Animations**: Toggling mid-animation now "Hands over" the state. The window picks up from its current opacity and position rather than snapping back to the starting line. No more flickering.
- **Focus Inheritance (Linux)**: Rapidly cycling through managed apps now correctly "Sticky-tracks" your original browser/IDE focus, so you always return to where you started.

---

## ⚠️ Known Issues

### Linux: Hotkey Registration Delay
On KDE, there's a 500ms delay when registering hotkeys. It's a workaround for a race condition in KWin that can cause crashes. It only affects startup. Consider it a feature for system stability.

### Linux: App Compatibility: Opacity Animations & Coordination
Some applications (especially Electron-based ones or XWayland clients) may experience unreliable transparency or "stutter" during motion on Linux. Due to the asynchronous nature of Wayland property updates, opacity and position may occasionally update in different frames. janq uses the "Fullscreen role" or `ForceBlur` to stabilize this, but for some apps, disabling `animate_opacity` remains the most stable choice.

### Sibling Animation Inconsistency
When multiple applications are configured, sibling windows use the target window's duration instead of their own configured `hide_duration`. This creates a minor visual inconsistency during transitions that I've attempted to fix multiple times with absolutely zero improvement over the original behavior. Every "fix" attempted made it worse. This is what we're shipping.

---

## 📄 License

MIT License
