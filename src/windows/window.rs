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

use rustc_hash::FxHashMap;
use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};
use std::sync::{Mutex, OnceLock, RwLock};

use windows::core::BOOL;
use windows::Win32::{
  Foundation::{HWND, LPARAM, RECT, WPARAM},
  Graphics::Gdi::{HDC, HMONITOR},
  System::Threading::AttachThreadInput,
  UI::{Accessibility::*, WindowsAndMessaging::*},
};

use janq::config::Config;

// Re-export from submodules
pub use super::discovery::{fetch_system_windows, find_window_by_process};
pub use super::parking::{
  park_window, release_windows, reset_visible_app, restore_window_visibility,
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
    let mut class_buf = [0u16; 256];
    let len = GetClassNameW(hwnd, &mut class_buf);
    if len == 0 {
      return false;
    }
    let class_name = String::from_utf16_lossy(&class_buf[..len as usize]).to_lowercase();
    class_name == "progman"
      || class_name == "workerw"
      || class_name == "shell_traywnd"
      || class_name == "shell_secondarytraywnd"
      || class_name == "#32768" // Menu
      || class_name == "windows.ui.core.corewindow"
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

pub fn update_managed_hwnds_cache() {
  let cache = get_app_cache().read().unwrap();
  let mut pids = Vec::with_capacity(64);

  for cw in cache.values() {
    unsafe {
      let mut pid = 0;
      GetWindowThreadProcessId(cw.hwnd, Some(&mut pid));
      if pid != 0 && !pids.contains(&pid) {
        pids.push(pid);
        if pids.len() >= 64 {
          break;
        }
      }
    }
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
    } else if !is_shell_window(hwnd) && !is_internal_window(hwnd) {
      // If we are currently hiding a window, Windows might automatically shift focus
      // to the next window. We don't want to treat that transition as a valid
      // "last external focus" target.
      let current_last = LAST_EXTERNAL_FOCUS.load(Ordering::Relaxed);
      if hwnd.0 as isize != current_last {
        MANAGED_APP_HAS_FOCUS.store(false, Ordering::Relaxed);
        LAST_EXTERNAL_FOCUS.store(hwnd.0 as isize, Ordering::Relaxed);

        // Trigger FocusLost event for auto-hide (modal-loop safe)
        post_wake_message(WM_USER + 2);
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

    // 3. Robust attempt: Attach to the current foreground thread
    let fg_window = GetForegroundWindow();
    if !fg_window.0.is_null() && fg_window != hwnd {
      let target_thread_id = GetWindowThreadProcessId(hwnd, None);
      let current_fg_thread_id = GetWindowThreadProcessId(fg_window, None);

      if target_thread_id != current_fg_thread_id {
        let _ = AttachThreadInput(current_fg_thread_id, target_thread_id, true);
        let _ = SetForegroundWindow(hwnd);
        let _ = BringWindowToTop(hwnd);
        let _ = AttachThreadInput(current_fg_thread_id, target_thread_id, false);
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

    let managed_ids: std::collections::HashSet<isize> = {
      let cache = get_app_cache().read().unwrap();
      cache.values().map(|cw| cw.hwnd.0 as isize).collect()
    };

    match find_window_by_process(&app_cfg.window_class, None, &managed_ids) {
      Some(cw) => {
        let mut cache = get_app_cache().write().unwrap();
        cache.insert(app_name.to_string(), cw);
        drop(cache);
        update_managed_hwnds_cache();
        cw
      }
      None => {
        eprintln!(
          "janq: Window not found for app: {} (class: {})",
          app_name, app_cfg.window_class
        );
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
