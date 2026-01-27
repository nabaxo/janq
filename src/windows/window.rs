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
use std::sync::{Mutex, OnceLock, RwLock};

use windows::core::BOOL;
use windows::Win32::{
  Foundation::{HWND, LPARAM, RECT},
  Graphics::Gdi::{HDC, HMONITOR},
  System::Threading::AttachThreadInput,
  UI::WindowsAndMessaging::*,
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

static ANIMATION_TASK_CANCEL: OnceLock<std::sync::Arc<std::sync::atomic::AtomicBool>> =
  OnceLock::new();
static VISIBLE_APP: OnceLock<RwLock<Option<String>>> = OnceLock::new();
static PREVIOUS_FOCUS: OnceLock<Mutex<Option<CachedWindow>>> = OnceLock::new();
static APP_CACHE: OnceLock<RwLock<FxHashMap<String, CachedWindow>>> = OnceLock::new();
static ANIMATION_STATE: OnceLock<Mutex<Option<AnimationState>>> = OnceLock::new();

pub fn get_animation_cancel() -> std::sync::Arc<std::sync::atomic::AtomicBool> {
  ANIMATION_TASK_CANCEL
    .get_or_init(|| std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)))
    .clone()
}

pub fn get_visible_app() -> &'static RwLock<Option<String>> {
  VISIBLE_APP.get_or_init(|| RwLock::new(None))
}

pub fn get_previous_focus() -> &'static Mutex<Option<CachedWindow>> {
  PREVIOUS_FOCUS.get_or_init(|| Mutex::new(None))
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
  let is_visible = {
    let v = get_visible_app().read().unwrap();
    v.as_deref() == Some(app_name)
  };
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
    match find_window_by_process(&app_cfg.window_class, None) {
      Some(cw) => {
        let mut cache = get_app_cache().write().unwrap();
        cache.insert(app_name.to_string(), cw);
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
    let mut v = get_visible_app().write().unwrap();
    if should_show {
      *v = Some(app_name.to_string());
    } else {
      *v = None;
      unsafe {
        let fg_window = GetForegroundWindow();
        if fg_window == target_hwnd.inner() {
          restore_focus = true;
        }
      }
    }
  }

  unsafe {
    let fg_window = GetForegroundWindow();
    if !fg_window.0.is_null() && fg_window != target_hwnd.inner() {
      // Don't "save" desktop/taskbar as previous focus for restoration, as it's janky
      let mut class_buf = [0u16; 256];
      let len = GetClassNameW(fg_window, &mut class_buf);
      let class_name = String::from_utf16_lossy(&class_buf[..len as usize]).to_lowercase();
      if class_name != "progman" && class_name != "workerw" && class_name != "shell_traywnd" {
        let mut prev = get_previous_focus().lock().unwrap();
        *prev = Some(CachedWindow { hwnd: fg_window });
      }
    }
  }

  let config_clone = config.clone();
  let app_name_clone = app_name.to_string();

  // Update the global cancel flag to this NEW one
  {
    // We don't have a clean way to "swap" the Arc in OnceLock easily if it's already there
    // Actually, I should just use a Mutex<Arc<AtomicBool>> or just one AtomicBool that we reset.
    // Let's use one AtomicBool and reset it here.
    let cancel = get_animation_cancel();
    cancel.store(false, std::sync::atomic::Ordering::SeqCst);
  }

  std::thread::spawn(move || {
    run_animation_task_sync(
      &app_name_clone,
      &config_clone,
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
