//! Win32 window management, animation, and multi-monitor support.
//!
//! ## Core Responsibilities
//!
//! 1. **Window Discovery** - `EnumWindows` + `GetWindowThreadProcessId`
//! 2. **Toggle Animation** - Smooth slide-in/out with easing curves
//! 3. **Multi-Monitor** - Finds correct monitor for cursor position
//! 4. **Force Focus** - `AllowSetForegroundWindow` + `SetForegroundWindow`
//!
//! ## Animation Engine
//!
//! Uses `DwmFlush` for vsync-aligned frame timing:
//! 1. Calculate progress with easing function
//! 2. Compute interpolated position/opacity
//! 3. Apply position via `SetWindowPos`
//! 4. Apply opacity via `SetLayeredWindowAttributes`
//!
//! ## State Tracking
//!
//! `HWND_CACHE` maps app names to their window handles for fast toggle
//! and restoration on daemon exit.

use std::sync::{
  atomic::{AtomicBool, AtomicIsize, Ordering},
  Mutex, OnceLock, RwLock,
};

use rustc_hash::FxHashMap;
use windows::{
  core::BOOL,
  Win32::{
    Foundation::{HWND, LPARAM, RECT, WPARAM},
    Graphics::{
      Dwm::{DwmGetWindowAttribute, DWMWA_CLOAKED},
      Gdi::{HDC, HMONITOR},
    },
    System::Threading::{AttachThreadInput, GetCurrentThreadId},
    UI::{Accessibility::*, WindowsAndMessaging::*},
  },
};

use janq::{
  config::Config,
  matching::{u16_contains_ascii_ignore_case, u16_eq_ascii_ignore_case},
};

// Re-export from submodules
pub use super::{
  discovery::{fetch_system_windows, find_window_by_process},
  parking::{park_window, release_windows, reset_visible_app, restore_window_visibility},
};

// =============================================================================
// Thread-Safe HWND Wrapper
// =============================================================================

/// Wrapper for `HWND` that is safe to send across threads.
#[derive(Clone, Copy)]
pub struct CachedWindow {
  pub hwnd: HWND,
}
unsafe impl Send for CachedWindow {}
unsafe impl Sync for CachedWindow {}

impl CachedWindow {
  pub fn inner(&self) -> HWND {
    self.hwnd
  }
}

pub fn is_shell_window(hwnd: HWND) -> bool {
  if hwnd.0.is_null() {
    return true;
  }
  unsafe {
    let mut class_buffer = [0u16; 256];
    let len = GetClassNameW(hwnd, &mut class_buffer);
    if len == 0 {
      return false;
    }
    let class_slice = &class_buffer[..len as usize];

    if
    // 1. Core Shell Components
    u16_eq_ascii_ignore_case(class_slice, "progman")
        || u16_eq_ascii_ignore_case(class_slice, "workerw")
        || u16_eq_ascii_ignore_case(class_slice, "shell_traywnd")
        || u16_eq_ascii_ignore_case(class_slice, "shell_secondarytraywnd")
        || u16_eq_ascii_ignore_case(class_slice, "windows.ui.core.corewindow")
        // 2. Transients & Overlays (Alt-Tab, Menus, Tooltips)
        || u16_eq_ascii_ignore_case(class_slice, "#32768") // Menu
        || u16_eq_ascii_ignore_case(class_slice, "multitaskingviewframe")
        || u16_eq_ascii_ignore_case(class_slice, "taskswitcherwnd")
        || u16_eq_ascii_ignore_case(class_slice, "droplist")
        || u16_contains_ascii_ignore_case(class_slice, "tooltip")
        || u16_contains_ascii_ignore_case(class_slice, "ghost")
        // 3. Technical junk (Graphics hooks, IMEs)
        || u16_contains_ascii_ignore_case(class_slice, "nvopengl")
        || u16_contains_ascii_ignore_case(class_slice, "wgpu")
        || u16_eq_ascii_ignore_case(class_slice, "ime")
        || u16_eq_ascii_ignore_case(class_slice, "msctfime ui")
        || u16_contains_ascii_ignore_case(class_slice, "gdi+ hooks")
        // 4. Browsers/Frameworks often have un-titled helper windows
        || ((u16_contains_ascii_ignore_case(class_slice, "chrome_widgetwin")
          || u16_contains_ascii_ignore_case(class_slice, "nativehwndhost"))
          && GetWindowTextW(hwnd, &mut [0u16; 128]) == 0)
    {
      return true;
    }

    false
  }
}

/// Determines if a window is a legitimate top-level application window
/// suitable for discovery or focus restoration.
pub fn is_suitable_target(hwnd: HWND) -> bool {
  if hwnd.0.is_null() || is_internal_window(hwnd) || is_shell_window(hwnd) {
    return false;
  }

  unsafe {
    // 1. Style check: Ignore tool windows, shadows, etc.
    let style = GetWindowLongW(hwnd, GWL_EXSTYLE) as u32;
    if (style & WS_EX_TOOLWINDOW.0) != 0 {
      return false;
    }

    // 2. Ownership check: Most app windows are unowned.
    // We allow windows owned by our hidden owner (already managed).
    let owner = GetWindow(hwnd, GW_OWNER).map(|h| h.0 as usize).unwrap_or(0);
    if owner != 0 {
      let our_owner = get_hidden_owner().map(|h| h.0 as usize).unwrap_or(0);
      if owner != our_owner {
        return false;
      }
    }

    // 3. Cloak check: Ignore windows that are rendered but hidden (Windows 10+ states).
    let mut cloaked: u32 = 0;
    let dwm_result = DwmGetWindowAttribute(
      hwnd,
      DWMWA_CLOAKED,
      &mut cloaked as *mut u32 as *mut _,
      std::mem::size_of::<u32>() as u32,
    );
    if dwm_result.is_ok() && cloaked != 0 {
      return false;
    }

    true
  }
}

// =============================================================================
// Hidden Owner Window for Taskbar Hiding
// =============================================================================

/// A hidden message-only window used as owner for managed windows.
/// When a window has an owner, Windows doesn't show it in the taskbar,
/// but it still appears in Alt+Tab (unlike WS_EX_TOOLWINDOW).
static HIDDEN_OWNER: OnceLock<CachedWindow> = OnceLock::new();

/// Creates a hidden message-only window to serve as owner for taskbar hiding.
/// Should be called once at daemon startup.
pub fn init_hidden_owner() {
  use windows::core::w;

  let hwnd = unsafe {
    CreateWindowExW(
      WINDOW_EX_STYLE::default(),
      w!("STATIC"),
      w!("janq_owner"),
      WS_POPUP,
      0,
      0,
      0,
      0,
      Some(HWND_MESSAGE),
      None,
      None,
      None,
    )
  };

  if let Ok(h) = hwnd {
    let _ = HIDDEN_OWNER.set(CachedWindow { hwnd: h });
  }
}

/// Gets the hidden owner window handle, if initialized.
pub fn get_hidden_owner() -> Option<HWND> {
  HIDDEN_OWNER.get().map(|cw| cw.hwnd)
}

/// Sets a window's owner to hide it from the taskbar while keeping Alt+Tab visibility.
pub fn set_taskbar_hidden(hwnd: HWND, hidden: bool) {
  if let Some(owner) = get_hidden_owner() {
    unsafe {
      let new_owner = if hidden { owner.0 as isize } else { 0 };
      SetWindowLongPtrW(hwnd, GWLP_HWNDPARENT, new_owner);
    }
  }
}

#[derive(Clone)]
pub struct AnimationState {
  pub hidden_x: i32,
  pub hidden_y: i32,
  pub shown_x: i32,
  pub shown_y: i32,
}

static ANIMATION_TASK_CANCEL: OnceLock<std::sync::atomic::AtomicBool> = OnceLock::new();
static VISIBLE_APP: OnceLock<RwLock<Option<String>>> = OnceLock::new();
static APP_CACHE: OnceLock<RwLock<FxHashMap<String, CachedWindow>>> = OnceLock::new();
static ANIMATION_STATE: OnceLock<Mutex<Option<AnimationState>>> = OnceLock::new();
static LAST_EXTERNAL_FOCUS: AtomicIsize = AtomicIsize::new(0);
static MANAGED_APP_HAS_FOCUS: AtomicBool = AtomicBool::new(false);
static MANAGED_PIDS_CACHE: [std::sync::atomic::AtomicU32; 64] = {
  const ATOMIC_ZERO: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
  [ATOMIC_ZERO; 64]
};

pub static MAIN_THREAD_ID: OnceLock<u32> = OnceLock::new();
/// Bridge window handle for PostMessage-based signaling (modal-loop safe).
pub static BRIDGE_HWND: OnceLock<CachedWindow> = OnceLock::new();

pub fn get_last_external_focus() -> HWND {
  HWND(LAST_EXTERNAL_FOCUS.load(Ordering::Relaxed) as *mut _)
}

pub fn get_managed_app_has_focus() -> bool {
  MANAGED_APP_HAS_FOCUS.load(Ordering::Relaxed)
}

/// Checks if the window is one of janq's own internal helper windows.
pub fn is_internal_window(hwnd: HWND) -> bool {
  if let Some(owner) = get_hidden_owner() {
    if hwnd == owner {
      return true;
    }
  }
  if let Some(bridge) = BRIDGE_HWND.get() {
    if hwnd == bridge.hwnd {
      return true;
    }
  }
  false
}

pub fn is_managed_process(pid: u32) -> bool {
  if pid == 0 {
    return false;
  }
  // Fast check against cache
  let exists = MANAGED_PIDS_CACHE
    .iter()
    .any(|slot| slot.load(Ordering::Relaxed) == pid);

  if exists {
    // Verify it isn't a recycled PID
    return janq::process::is_process_running(pid, None);
  }
  false
}

pub fn update_managed_hwnds_cache() {
  let mut to_remove = Vec::new();
  let mut pids = Vec::with_capacity(64);

  {
    let cache = get_app_cache().read().unwrap();
    for (name, cw) in cache.iter() {
      unsafe {
        if !IsWindow(Some(cw.hwnd)).as_bool() {
          to_remove.push(name.clone());
          continue;
        }

        let mut pid = 0;
        GetWindowThreadProcessId(cw.hwnd, Some(&mut pid));
        if pid != 0 {
          if janq::process::is_process_running(pid, None) {
            if !pids.contains(&pid) {
              pids.push(pid);
            }
          } else {
            to_remove.push(name.clone());
          }
        }
      }
    }
  }

  if !to_remove.is_empty() {
    let mut cache = get_app_cache().write().unwrap();
    for name in to_remove {
      cache.remove(&name);
    }
  }

  if pids.len() >= 64 {
    pids.truncate(64);
  }

  // Update indices 1..64 first, then index 0 last to minimize race impact
  for i in (1..64).rev() {
    let val = if i < pids.len() {
      pids[i]
    } else if i == pids.len() {
      0
    } else {
      // Don't overwrite if we don't need to, but for safety we can
      0
    };
    MANAGED_PIDS_CACHE[i].store(val, Ordering::Relaxed);
  }
  // Store the first one last
  let first = if !pids.is_empty() { pids[0] } else { 0 };
  MANAGED_PIDS_CACHE[0].store(first, Ordering::Relaxed);
}

pub fn is_managed_window(hwnd: HWND) -> bool {
  if hwnd.0.is_null() {
    return false;
  }

  unsafe {
    let mut pid = 0;
    let tid = GetWindowThreadProcessId(hwnd, Some(&mut pid));
    if tid == 0 || pid == 0 {
      return false;
    }

    // Lock-free check of managed process IDs
    for i in 0..64 {
      let cached_pid = MANAGED_PIDS_CACHE[i].load(Ordering::Relaxed);
      if cached_pid == 0 {
        break;
      }
      if cached_pid == pid {
        return true;
      }
    }
  }
  false
}

/// Posts a wake-up message to the main loop via the bridge window.
/// This is safe during modal loops (e.g., tray menu open).
pub fn post_wake_message(msg_id: u32) {
  use windows::Win32::UI::WindowsAndMessaging::PostMessageW;
  if let Some(bridge) = BRIDGE_HWND.get() {
    unsafe {
      let _ = PostMessageW(Some(bridge.hwnd), msg_id, WPARAM(0), LPARAM(0));
    }
  }
}

pub unsafe extern "system" fn focus_hook_proc(
  _hwineventhook: HWINEVENTHOOK,
  event: u32,
  hwnd: HWND,
  _idobject: i32,
  _idchild: i32,
  _ideventthread: u32,
  _dwmseventtime: u32,
) {
  if event == EVENT_SYSTEM_FOREGROUND && !hwnd.0.is_null() {
    if is_managed_window(hwnd) {
      MANAGED_APP_HAS_FOCUS.store(true, Ordering::Relaxed);
    } else {
      MANAGED_APP_HAS_FOCUS.store(false, Ordering::Relaxed);

      // 1. Record restoration target (skip shell/transient/internal)
      if is_suitable_target(hwnd) {
        LAST_EXTERNAL_FOCUS.store(hwnd.0 as isize, Ordering::Relaxed);
      }

      // 2. Trigger auto-hide if a managed window IS actually visible
      if let Some(visible_app) = get_visible_app() {
        // Double-check visibility to prevent race with hotkey hide
        let mut cached_hwnd = None;
        if let Some(cw) = get_app_cache().read().unwrap().get(&visible_app) {
          if IsWindowVisible(cw.hwnd).as_bool() {
            cached_hwnd = Some(cw.hwnd);
          }
        }

        if let Some(_) = cached_hwnd {
          post_wake_message(WM_USER + 2);
        }
      }
    }
  }
}

pub fn init_focus_hook() -> Option<HWINEVENTHOOK> {
  unsafe {
    let hook = SetWinEventHook(
      EVENT_SYSTEM_FOREGROUND,
      EVENT_SYSTEM_FOREGROUND,
      None,
      Some(focus_hook_proc),
      0,
      0,
      WINEVENT_OUTOFCONTEXT,
    );
    if hook.is_invalid() {
      None
    } else {
      Some(hook)
    }
  }
}

pub fn get_animation_cancel() -> &'static std::sync::atomic::AtomicBool {
  ANIMATION_TASK_CANCEL.get_or_init(|| std::sync::atomic::AtomicBool::new(false))
}

pub fn visible_app_lock() -> &'static RwLock<Option<String>> {
  VISIBLE_APP.get_or_init(|| RwLock::new(None))
}

pub fn get_visible_app() -> Option<String> {
  visible_app_lock().read().unwrap().clone()
}

pub fn get_app_cache() -> &'static RwLock<FxHashMap<String, CachedWindow>> {
  APP_CACHE.get_or_init(|| RwLock::new(FxHashMap::default()))
}

pub fn get_animation_state() -> &'static Mutex<Option<AnimationState>> {
  ANIMATION_STATE.get_or_init(|| Mutex::new(None))
}

// Discovery logic moved to discovery.rs

/// Robustly forces a window into the foreground, even if the current process
/// doesn't have focus. Uses the "AttachThreadInput" trick to bypass locks.
pub fn force_focus(hwnd: HWND) {
  unsafe {
    if hwnd.0.is_null() || !IsWindow(Some(hwnd)).as_bool() {
      return;
    }

    // 1. Give ourselves permission
    let _ = AllowSetForegroundWindow(ASFW_ANY);

    // 2. Initial attempt
    if SetForegroundWindow(hwnd).as_bool() {
      let _ = BringWindowToTop(hwnd);
      return;
    }

    // 3. Robust attempt: "Borrow" foreground status from current foreground thread
    let fg_window = GetForegroundWindow();
    let current_thread_id = GetCurrentThreadId();
    if !fg_window.0.is_null() {
      let fg_thread_id = GetWindowThreadProcessId(fg_window, None);
      if fg_thread_id != current_thread_id {
        let _ = AttachThreadInput(current_thread_id, fg_thread_id, true);
        let _ = SetForegroundWindow(hwnd);
        let _ = BringWindowToTop(hwnd);
        let _ = AttachThreadInput(current_thread_id, fg_thread_id, false);
      }
    }

    // 4. Fallback: ShowWindow with SW_SHOW is often more forceful than SetForegroundWindow
    let _ = ShowWindow(hwnd, SW_SHOW);
    let _ = SetForegroundWindow(hwnd);
  }
}

pub struct MonitorEnumCtx {
  pub monitors: Vec<HMONITOR>,
}
pub unsafe extern "system" fn monitor_enum_proc(
  hmonitor: HMONITOR,
  _hdc: HDC,
  _rect: *mut RECT,
  lparam: LPARAM,
) -> BOOL {
  let ctx = &mut *(lparam.0 as *mut MonitorEnumCtx);
  ctx.monitors.push(hmonitor);
  BOOL(1)
}

// =============================================================================
// Toggle Logic
// =============================================================================

pub fn toggle_window(app_name: &str, config: &Config) -> bool {
  let is_visible = get_visible_app().as_deref() == Some(app_name);
  let should_show = !is_visible;
  let app_cfg = match config.app.get(app_name) {
    Some(c) => c,
    None => return false,
  };

  // 1. Find Target HWND
  let mut cached_hwnd = None;
  {
    let cache = get_app_cache().read().unwrap();
    if let Some(cw) = cache.get(app_name) {
      let is_alive = unsafe { IsWindow(Some(cw.hwnd)).as_bool() };
      if is_alive {
        cached_hwnd = Some(*cw);
      }
    }
  }
  let target_hwnd = if let Some(h) = cached_hwnd {
    h
  } else {
    // If the window isn't cached, check if we are already in the middle of spawning/searching for it.
    // This prevents hotkey spam from triggering multiple full-system EnumWindows scans and
    // potentially grabbing transient windows during startup.
    {
      let spawning = janq::spawn_guard::get_spawning_apps().lock().unwrap();
      if spawning.contains(app_name) {
        return false;
      }
    }

    match find_window_by_process(&app_cfg.window_class, None) {
      Some(cw) => {
        let mut cache = get_app_cache().write().unwrap();
        cache.insert(app_name.to_string(), cw);
        drop(cache);
        update_managed_hwnds_cache();
        cw
      }
      None => {
        crate::windows::show_error(&format!(
          "janq: Window not found for app: {} (class: {})\n\nIf the app is not running, janq will attempt to start it on the next toggle.",
          app_name, app_cfg.window_class
        ));
        return false;
      }
    }
  };

  // 2. Discover siblings via APP_CACHE (Performance optimized)
  let mut siblings = Vec::new();
  {
    let cache = get_app_cache().read().unwrap();
    for (name, cw) in cache.iter() {
      if name == app_name {
        continue;
      }
      unsafe {
        if IsWindow(Some(cw.hwnd)).as_bool() && IsWindowVisible(cw.hwnd).as_bool() {
          siblings.push(*cw);
        }
      }
    }
  }

  // Abort current animation
  {
    get_animation_cancel().store(true, std::sync::atomic::Ordering::SeqCst);
  }

  let mut restore_focus = false;
  {
    let mut v = visible_app_lock().write().unwrap();
    if should_show {
      *v = Some(app_name.to_string());
    } else {
      *v = None;
      let fg_window = unsafe { GetForegroundWindow() };
      // Only restore focus if WE currently have focus, or if the shell
      // just stole it from us (e.g. user clicked the system tray).
      if fg_window == target_hwnd.inner()
        || (is_shell_window(fg_window) && get_managed_app_has_focus())
      {
        restore_focus = true;
      }
    }
  }

  // We no longer need to manually capture focus here, the WinEventHook does it for us constantly.
  // The animation task will use get_last_external_focus() to know where to return.

  let config_shared = config.clone();
  let app_name_clone = app_name.to_string();

  // Ensure the animation cancel flag is reset before starting a new task.
  get_animation_cancel().store(false, std::sync::atomic::Ordering::SeqCst);

  std::thread::spawn(move || {
    run_animation_task_sync(
      &app_name_clone,
      &config_shared,
      target_hwnd,
      should_show,
      siblings,
      restore_focus,
    );
  });

  true
}

// Animation logic moved to animation.rs
use super::animation::run_animation_task_sync;

// Parking and restoration logic moved to parking.rs
