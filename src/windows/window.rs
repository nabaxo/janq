use rustc_hash::FxHashMap;
use std::sync::{Mutex, OnceLock, RwLock};

use std::time::Instant;
use windows::Win32::{
  Foundation::{BOOL, COLORREF, HWND, LPARAM, POINT, RECT, TRUE},
  Graphics::{
    Dwm::{DwmFlush, DwmSetWindowAttribute, DWMWA_TRANSITIONS_FORCEDISABLED},
    Gdi::{
      EnumDisplayMonitors, GetMonitorInfoW, MonitorFromPoint, MonitorFromWindow, HDC, HMONITOR,
      MONITORINFO, MONITOR_DEFAULTTONEAREST,
    },
  },
  System::{
    ProcessStatus::GetModuleBaseNameW,
    Threading::{AttachThreadInput, OpenProcess, PROCESS_QUERY_INFORMATION, PROCESS_VM_READ},
  },
  UI::WindowsAndMessaging::{
    AllowSetForegroundWindow, BeginDeferWindowPos, BringWindowToTop, DeferWindowPos,
    EndDeferWindowPos, EnumWindows, GetClassNameW, GetCursorPos, GetForegroundWindow,
    GetLayeredWindowAttributes, GetWindow, GetWindowLongW, GetWindowRect, GetWindowThreadProcessId,
    IsIconic, IsWindow, IsWindowVisible, SetForegroundWindow, SetLayeredWindowAttributes,
    SetWindowLongW, SetWindowPos, ShowWindow, ASFW_ANY, GWL_EXSTYLE, GW_HWNDNEXT, HWND_NOTOPMOST,
    HWND_TOPMOST, LWA_ALPHA, SWP_DEFERERASE, SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOCOPYBITS,
    SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, SWP_SHOWWINDOW, SW_HIDE, SW_SHOW, SW_SHOWNA,
    SW_SHOWNOACTIVATE, WS_EX_LAYERED, WS_EX_TOOLWINDOW,
  },
};

use crate::config::{fuzzy_match_window, AppConfig, Config, FoundWindow};
use crate::windows::easing::get_easing;

// Wrapper to make HWND Send/Sync for async tasks
#[derive(Clone, Copy)]
pub struct SendHwnd(pub HWND);
unsafe impl Send for SendHwnd {}
unsafe impl Sync for SendHwnd {}

impl SendHwnd {
  fn inner(&self) -> HWND {
    self.0
  }
}

static ANIMATION_TASK_CANCEL: OnceLock<std::sync::Arc<std::sync::atomic::AtomicBool>> =
  OnceLock::new();
static VISIBLE_APP: OnceLock<RwLock<Option<String>>> = OnceLock::new();
static PREVIOUS_FOCUS: OnceLock<Mutex<Option<SendHwnd>>> = OnceLock::new();
static HWND_CACHE: OnceLock<RwLock<FxHashMap<String, SendHwnd>>> = OnceLock::new();

fn get_animation_cancel() -> std::sync::Arc<std::sync::atomic::AtomicBool> {
  ANIMATION_TASK_CANCEL
    .get_or_init(|| std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)))
    .clone()
}

fn get_visible_app() -> &'static RwLock<Option<String>> {
  VISIBLE_APP.get_or_init(|| RwLock::new(None))
}

fn get_previous_focus() -> &'static Mutex<Option<SendHwnd>> {
  PREVIOUS_FOCUS.get_or_init(|| Mutex::new(None))
}

pub fn get_hwnd_cache() -> &'static RwLock<FxHashMap<String, SendHwnd>> {
  HWND_CACHE.get_or_init(|| RwLock::new(FxHashMap::default()))
}

unsafe extern "system" fn enum_windows_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
  let target_struct = &mut *(lparam.0 as *mut TargetSearch);

  unsafe {
    // 1. Instant check: Style (ignore tool windows, shadows, etc)
    let style = GetWindowLongW(hwnd, GWL_EXSTYLE) as u32;
    if (style & WS_EX_TOOLWINDOW.0) != 0 {
      return BOOL(1);
    }

    // 2. Instant check: Visibility
    // Note: We still want to catch windows that are "parked" (IsWindowVisible == false)
    // but for the INITIAL enumeration during hotkey trigger,
    // we often prioritize visible ones. Actually, the current logic is to collect
    // ALL so we can fuzzy match them. But we can skip obvious system "ghost" windows.
  }

  let mut pid = 0;
  unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
  if pid == 0 {
    return BOOL(1);
  }

  let mut class_buffer = [0u16; 256];
  let class_len = unsafe { GetClassNameW(hwnd, &mut class_buffer) };
  let class_name = String::from_utf16_lossy(&class_buffer[..class_len as usize]).to_lowercase();

  // Filter out known junk classes
  if class_name.contains("nvopengl")
    || class_name.contains("wgpu")
    || class_name == "ime"
    || class_name == "msctfime ui"
    || class_name.contains("gdi+ hooks")
  {
    return BOOL(1);
  }

  // Only open process if we passed the class name filter
  let mut proc_name = String::new();
  if let Ok(process) =
    unsafe { OpenProcess(PROCESS_QUERY_INFORMATION | PROCESS_VM_READ, false, pid) }
  {
    let mut buffer = [0u16; 1024];
    let len = unsafe { GetModuleBaseNameW(process, None, &mut buffer) };
    if len > 0 {
      proc_name = String::from_utf16_lossy(&buffer[..len as usize]).to_lowercase();
    }
  }

  let is_visible = unsafe { IsWindowVisible(hwnd).as_bool() };

  target_struct.found_data.push(FoundWindow {
    id: (hwnd.0 as usize).to_string(),
    class_name,
    proc_name,
    pid,
    is_visible,
  });

  BOOL(1)
}

pub struct TargetSearch {
  pub found_data: Vec<FoundWindow>,
}

pub fn fetch_system_windows() -> Vec<FoundWindow> {
  let mut search = TargetSearch {
    found_data: Vec::new(),
  };
  unsafe {
    let _ = EnumWindows(
      Some(enum_windows_proc),
      LPARAM(&mut search as *mut _ as isize),
    );
  }
  search.found_data
}

pub fn find_window_by_process(name: &str, candidates: Option<&[FoundWindow]>) -> Option<HWND> {
  let cache = get_hwnd_cache().read().unwrap();
  let managed_ids: Vec<String> = cache
    .values()
    .map(|sh| (sh.inner().0 as usize).to_string())
    .collect();

  if let Some(list) = candidates {
    if let Some(best) = fuzzy_match_window(name, list, &managed_ids) {
      let handle = best.id.parse::<usize>().unwrap();
      return Some(HWND(handle as *mut _));
    }
    return None;
  }

  let found_data = fetch_system_windows();
  if let Some(best) = fuzzy_match_window(name, &found_data, &managed_ids) {
    let handle = best.id.parse::<usize>().unwrap();
    return Some(HWND(handle as *mut _));
  }

  None
}

/// Robustly forces a window into the foreground, even if the current process
/// doesn't have focus. Uses the "AttachThreadInput" trick to bypass locks.
pub fn force_focus(hwnd: HWND) {
  unsafe {
    if hwnd.is_invalid() || !IsWindow(hwnd).as_bool() {
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
    if !fg_window.is_invalid() && fg_window != hwnd {
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

struct MonitorEnumCtx {
  monitors: Vec<HMONITOR>,
}
unsafe extern "system" fn monitor_enum_proc(
  hmonitor: HMONITOR,
  _hdc: HDC,
  _rect: *mut RECT,
  lparam: LPARAM,
) -> BOOL {
  let ctx = &mut *(lparam.0 as *mut MonitorEnumCtx);
  ctx.monitors.push(hmonitor);
  BOOL(1)
}

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
    let cache = get_hwnd_cache().read().unwrap();
    if let Some(h) = cache.get(app_name) {
      unsafe {
        if IsWindow(h.inner()).as_bool() {
          cached_hwnd = Some(*h);
        }
      }
    }
  }
  let target_hwnd = if let Some(h) = cached_hwnd {
    h
  } else {
    match find_window_by_process(&app_cfg.window_class, None) {
      Some(h) => {
        let mut cache = get_hwnd_cache().write().unwrap();
        let wrapper = SendHwnd(h);
        cache.insert(app_name.to_string(), wrapper);
        wrapper
      }
      None => {
        println!(
          "Window not found for app: {} (class: {})",
          app_name, app_cfg.window_class
        );
        return false;
      }
    }
  };

  // 2. Discover siblings
  let mut siblings = Vec::new();
  for (name, cfg) in &config.app {
    if name == app_name {
      continue;
    }
    let mut cached_h = None;
    {
      let cache = get_hwnd_cache().read().unwrap();
      if let Some(h) = cache.get(name) {
        unsafe {
          if IsWindow(h.inner()).as_bool() {
            cached_h = Some(*h);
          }
        }
      }
    }
    let found_hwnd = if let Some(h) = cached_h {
      h
    } else {
      match find_window_by_process(&cfg.window_class, None) {
        Some(h) => {
          let mut cache = get_hwnd_cache().write().unwrap();
          let wrapper = SendHwnd(h);
          cache.insert(name.clone(), wrapper);
          wrapper
        }
        None => continue,
      }
    };
    if found_hwnd.0 == target_hwnd.0 {
      continue;
    }
    siblings.push(found_hwnd);
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
    if !fg_window.is_invalid() && fg_window != target_hwnd.inner() {
      // Don't "save" desktop/taskbar as previous focus for restoration, as it's janky
      let mut class_buf = [0u16; 256];
      let len = GetClassNameW(fg_window, &mut class_buf);
      let class_name = String::from_utf16_lossy(&class_buf[..len as usize]).to_lowercase();
      if class_name != "progman" && class_name != "workerw" && class_name != "shell_traywnd" {
        let mut prev = get_previous_focus().lock().unwrap();
        *prev = Some(SendHwnd(fg_window));
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

fn run_animation_task_sync(
  app_name: &str,
  config: &Config,
  target_hwnd: SendHwnd,
  should_show: bool,
  siblings: Vec<SendHwnd>,
  restore_focus: bool,
) {
  let app_cfg = match config.app.get(app_name) {
    Some(c) => c,
    None => return,
  };

  unsafe {
    let monitor = if should_show {
      match config.window.display_mode.as_str() {
        "specific" => {
          let mut ctx = MonitorEnumCtx {
            monitors: Vec::new(),
          };
          let _ = EnumDisplayMonitors(
            None,
            None,
            Some(monitor_enum_proc),
            LPARAM(&mut ctx as *mut _ as isize),
          );
          if (config.window.display_index as usize) < ctx.monitors.len() {
            ctx.monitors[config.window.display_index as usize]
          } else {
            let mut cursor_pos = POINT { x: 0, y: 0 };
            let _ = GetCursorPos(&mut cursor_pos);
            MonitorFromPoint(cursor_pos, MONITOR_DEFAULTTONEAREST)
          }
        }
        "active" => {
          let fg = GetForegroundWindow();
          let mut use_fallback = fg.is_invalid() || fg == target_hwnd.inner();

          if !use_fallback {
            let mut class_buf = [0u16; 256];
            let len = GetClassNameW(fg, &mut class_buf);
            let class_name = String::from_utf16_lossy(&class_buf[..len as usize]).to_lowercase();
            // If focus is on Desktop or Taskbar, it's safer to use mouse position
            if class_name == "progman" || class_name == "workerw" || class_name == "shell_traywnd" {
              use_fallback = true;
            }
          }

          if !use_fallback {
            MonitorFromWindow(fg, MONITOR_DEFAULTTONEAREST)
          } else {
            // STICKY: Only for 'active' mode fallback to prevent toggle see-sawing
            if IsWindowVisible(target_hwnd.inner()).as_bool() {
              MonitorFromWindow(target_hwnd.inner(), MONITOR_DEFAULTTONEAREST)
            } else {
              let mut cursor_pos = POINT { x: 0, y: 0 };
              let _ = GetCursorPos(&mut cursor_pos);
              MonitorFromPoint(cursor_pos, MONITOR_DEFAULTTONEAREST)
            }
          }
        }
        _ => {
          // follow-mouse
          let mut cursor_pos = POINT { x: 0, y: 0 };
          let _ = GetCursorPos(&mut cursor_pos);
          MonitorFromPoint(cursor_pos, MONITOR_DEFAULTTONEAREST)
        }
      }
    } else {
      MonitorFromWindow(target_hwnd.inner(), MONITOR_DEFAULTTONEAREST)
    };

    let mut mi = MONITORINFO::default();
    mi.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
    if !GetMonitorInfoW(monitor, &mut mi).as_bool() {
      return;
    }

    let work_area = mi.rcWork;
    let screen_w = work_area.right - work_area.left;
    let screen_h = work_area.bottom - work_area.top;

    // --- Geometry & Current State Capture ---
    let ((width_val, width_is_pct), (height_val, height_is_pct)) =
      app_cfg.resolve_dimensions(&config.window);

    let mut r_target = RECT::default();
    let _ = GetWindowRect(target_hwnd.inner(), &mut r_target);

    let target_w = if width_val > 0.0 {
      (if width_is_pct {
        screen_w as f64 * width_val
      } else {
        width_val
      }) as i32
    } else {
      r_target.right - r_target.left
    };
    let target_h = if height_val > 0.0 {
      (if height_is_pct {
        screen_h as f64 * height_val
      } else {
        height_val
      }) as i32
    } else {
      r_target.bottom - r_target.top
    };

    let target_x = work_area.left + (screen_w - target_w) / 2;
    let shown_y = work_area.top;
    let hidden_y = work_area.top - target_h;
    let final_target_y = if should_show { shown_y } else { hidden_y };

    let mut t_curr_alpha: u8 = 255;
    let _ = GetLayeredWindowAttributes(target_hwnd.inner(), None, Some(&mut t_curr_alpha), None);

    // Capture current Y. We use the current rect, but if perfectly hidden/invisible, we assume start/end state.
    let t_curr_y = r_target.top;
    let t_on_correct_monitor =
      MonitorFromWindow(target_hwnd.inner(), MONITOR_DEFAULTTONEAREST) == monitor;

    // --- Sibling Data ---
    let mut siblings_data = Vec::new();
    for ohwnd in siblings {
      if ohwnd.0 == target_hwnd.0 {
        continue;
      }
      let mut r = RECT::default();
      if GetWindowRect(ohwnd.inner(), &mut r).is_ok() {
        let is_visible = IsWindowVisible(ohwnd.inner()).as_bool();
        if is_visible {
          let smon = MonitorFromWindow(ohwnd.inner(), MONITOR_DEFAULTTONEAREST);
          let mut smi = MONITORINFO::default();
          smi.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
          if GetMonitorInfoW(smon, &mut smi).as_bool() {
            let s_work = smi.rcWork;
            let osy = r.top;
            let oty = s_work.top - (r.bottom - r.top) - 10;
            // Capture sibling start alpha
            let mut sa: u8 = 255;
            let _ = GetLayeredWindowAttributes(ohwnd.inner(), None, Some(&mut sa), None);
            siblings_data.push((
              ohwnd,
              r.left,
              r.right - r.left,
              r.bottom - r.top,
              osy,
              oty,
              sa,
            ));
          }
        }
      }
    }

    // --- Target Catching & Teleport ---
    let t_start_y = if should_show {
      // "Never teleport" on the same monitor. Only jump if we are switching displays.
      if !t_on_correct_monitor {
        let _ = SetLayeredWindowAttributes(target_hwnd.inner(), COLORREF(0), 0, LWA_ALPHA);
        let _ = SetWindowPos(
          target_hwnd.inner(),
          HWND::default(),
          target_x,
          hidden_y,
          target_w,
          target_h,
          SWP_NOACTIVATE | SWP_NOZORDER,
        );
        t_curr_alpha = 0; // Reset alpha to match what we just set
        hidden_y
      } else {
        t_curr_y
      }
    } else {
      t_curr_y
    };

    let t_dist_total = (final_target_y - t_start_y).abs();
    let max_dist = target_h;
    // Scale duration based on remaining distance to be travel
    let animate_opacity = app_cfg.get_animate_opacity(config.animation.animate_opacity);
    let base_dur_ms = if should_show {
      config.animation.show_duration
    } else {
      config.animation.hide_duration
    };
    let dur_ms = if max_dist > 0 {
      (base_dur_ms as f64 * (t_dist_total as f64 / max_dist as f64)).min(base_dur_ms as f64)
    } else {
      base_dur_ms as f64
    };
    let dur_secs = dur_ms / 1000.0;
    let easing = if should_show {
      &config.animation.show_easing
    } else {
      &config.animation.hide_easing
    };
    let z_order = if config.window.keep_above {
      HWND_TOPMOST
    } else {
      HWND_NOTOPMOST
    };
    let op_point = if should_show {
      config.animation.show_opacity_point
    } else {
      config.animation.hide_opacity_point
    }
    .clamp(0.0, 1.0);

    // --- Style & Layering Prep ---
    let _ = DwmSetWindowAttribute(
      target_hwnd.inner(),
      DWMWA_TRANSITIONS_FORCEDISABLED,
      &TRUE as *const _ as *const _,
      4,
    );
    for (h, _, _, _, _, _, _) in &siblings_data {
      let _ = DwmSetWindowAttribute(
        h.inner(),
        DWMWA_TRANSITIONS_FORCEDISABLED,
        &TRUE as *const _ as *const _,
        4,
      );
    }

    let prep_layer = |h: HWND| {
      let ex = GetWindowLongW(h, GWL_EXSTYLE);
      if (ex & WS_EX_LAYERED.0 as i32) == 0 {
        SetWindowLongW(h, GWL_EXSTYLE, ex | WS_EX_LAYERED.0 as i32);
        let _ = SetWindowPos(
          h,
          HWND::default(),
          0,
          0,
          0,
          0,
          SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_FRAMECHANGED,
        );
      }
    };
    if !should_show {
      // During hide, only prep if NOT already layered to minimize noise
      let ex = GetWindowLongW(target_hwnd.inner(), GWL_EXSTYLE);
      if (ex & WS_EX_LAYERED.0 as i32) == 0 {
        prep_layer(target_hwnd.inner());
      }
    } else {
      prep_layer(target_hwnd.inner());
    }
    for (h, _, _, _, _, _, _) in &siblings_data {
      let ex = GetWindowLongW(h.inner(), GWL_EXSTYLE);
      if (ex & WS_EX_LAYERED.0 as i32) == 0 {
        prep_layer(h.inner());
      }
    }

    let start_time = Instant::now();
    let mut first_frame = true;

    if dur_secs > 0.0 {
      let mut last_y = t_start_y;
      let mut last_alpha = t_curr_alpha;
      let mut last_sibling_ys: Vec<i32> = siblings_data
        .iter()
        .map(|(_, _, _, _, osy, _, _)| *osy)
        .collect();
      let mut last_sibling_alphas: Vec<u8> = siblings_data
        .iter()
        .map(|(_, _, _, _, _, _, sa)| *sa)
        .collect();

      loop {
        // 1. Bail Check - Check if we are still the intended animation
        {
          let v = get_visible_app().read().unwrap();
          let still_target = if should_show {
            v.as_deref() == Some(app_name)
          } else {
            v.as_deref() != Some(app_name)
          };
          if !still_target || get_animation_cancel().load(std::sync::atomic::Ordering::SeqCst) {
            return;
          }
        }

        let elapsed = start_time.elapsed().as_secs_f64();
        let progress = (elapsed / dur_secs).min(1.0);
        let ease_val = get_easing(progress, easing);

        let mut needs_pos_update = false;
        if animate_opacity {
          let target_alpha_val = if should_show { 255.0 } else { 0.0 };
          let t_alpha = {
            let opacity_ease = if should_show {
              (ease_val / op_point).clamp(0.0, 1.0)
            } else {
              let denom = 1.0 - op_point;
              ((ease_val - op_point) / if denom <= 0.0 { 0.0001 } else { denom }).clamp(0.0, 1.0)
            };
            let computed =
              (t_curr_alpha as f64 + (target_alpha_val - t_curr_alpha as f64) * opacity_ease) as u8;
            if should_show {
              computed.max(last_alpha)
            } else {
              computed.min(last_alpha)
            }
          };

          if t_alpha != last_alpha {
            let _ =
              SetLayeredWindowAttributes(target_hwnd.inner(), COLORREF(0), t_alpha, LWA_ALPHA);
            last_alpha = t_alpha;
          }

          for (i, (h, _, _, _, _, _, sa)) in siblings_data.iter().enumerate() {
            let s_denom = 1.0 - config.animation.hide_opacity_point;
            let s_opacity_ease = ((ease_val - config.animation.hide_opacity_point)
              / if s_denom <= 0.0 { 0.0001 } else { s_denom })
            .clamp(0.0, 1.0);
            let s_target_alpha = {
              let computed = (*sa as f64 * (1.0 - s_opacity_ease)) as u8;
              computed.min(last_sibling_alphas[i])
            };
            if s_target_alpha != last_sibling_alphas[i] {
              let _ = SetLayeredWindowAttributes(h.inner(), COLORREF(0), s_target_alpha, LWA_ALPHA);
              last_sibling_alphas[i] = s_target_alpha;
            }
          }
        } else if first_frame && should_show {
          let _ = SetLayeredWindowAttributes(target_hwnd.inner(), COLORREF(0), 255, LWA_ALPHA);
          last_alpha = 255;
        }

        // --- Position Update ---
        let t_dist_y = final_target_y - t_start_y;
        let next_y = t_start_y + (t_dist_y as f64 * ease_val) as i32;

        if next_y != last_y || first_frame {
          needs_pos_update = true;
        } else {
          for (i, (_, _, _, _, osy, oty, _)) in siblings_data.iter().enumerate() {
            let on_y = osy + ((*oty - *osy) as f64 * ease_val) as i32;
            if on_y != last_sibling_ys[i] {
              needs_pos_update = true;
              break;
            }
          }
        }

        if needs_pos_update {
          if let Ok(mut hdwp) = BeginDeferWindowPos((1 + siblings_data.len()) as i32) {
            let mut t_flags = SWP_NOACTIVATE | SWP_NOCOPYBITS | SWP_DEFERERASE;
            let t_z = if first_frame {
              z_order
            } else {
              HWND::default()
            };
            if first_frame {
              if should_show {
                t_flags |= SWP_SHOWWINDOW;
              }
            } else {
              t_flags |= SWP_NOZORDER;
            }

            let mut t_ok = false;
            match DeferWindowPos(
              hdwp,
              target_hwnd.inner(),
              t_z,
              target_x,
              next_y,
              target_w,
              target_h,
              t_flags,
            ) {
              Ok(h) if !h.is_invalid() => {
                hdwp = h;
                t_ok = true;
              }
              _ => {}
            }

            for (i, (h, ox, ow, oh, osy, oty, _)) in siblings_data.iter().enumerate() {
              let on_y = osy + ((*oty - *osy) as f64 * ease_val) as i32;
              match DeferWindowPos(
                hdwp,
                h.inner(),
                HWND::default(),
                *ox,
                on_y,
                *ow,
                *oh,
                SWP_NOACTIVATE | SWP_NOCOPYBITS | SWP_NOZORDER,
              ) {
                Ok(nh) if !nh.is_invalid() => hdwp = nh,
                _ => {
                  let _ = SetWindowPos(
                    h.inner(),
                    HWND::default(),
                    *ox,
                    on_y,
                    *ow,
                    *oh,
                    SWP_NOACTIVATE | SWP_NOCOPYBITS | SWP_NOZORDER,
                  );
                }
              }
              last_sibling_ys[i] = on_y;
            }
            let _ = EndDeferWindowPos(hdwp);
            if !t_ok {
              let _ = SetWindowPos(
                target_hwnd.inner(),
                t_z,
                target_x,
                next_y,
                target_w,
                target_h,
                t_flags,
              );
            }
            last_y = next_y;
          }
        }

        if first_frame && should_show {
          let _ = ShowWindow(target_hwnd.inner(), SW_SHOW);
          force_focus(target_hwnd.inner());
        }

        first_frame = false;

        // Sync with monitor refresh
        let _ = DwmFlush();

        if progress >= 1.0 {
          break;
        }
      }
    }

    // --- Finalize ---
    if should_show {
      let _ = SetLayeredWindowAttributes(target_hwnd.inner(), COLORREF(0), 255, LWA_ALPHA);
      let _ = SetWindowPos(
        target_hwnd.inner(),
        z_order,
        target_x,
        final_target_y,
        target_w,
        target_h,
        SWP_SHOWWINDOW | SWP_NOACTIVATE | SWP_NOZORDER,
      );
      force_focus(target_hwnd.inner());
    } else {
      if IsWindowVisible(target_hwnd.inner()).as_bool() {
        let _ = ShowWindow(target_hwnd.inner(), SW_HIDE);
      }
      if restore_focus {
        // Z-order discovery: find the window immediately behind us
        let mut next = windows::Win32::UI::WindowsAndMessaging::GetWindow(
          target_hwnd.inner(),
          windows::Win32::UI::WindowsAndMessaging::GW_HWNDNEXT,
        );
        while let Ok(valid_next) = next {
          if valid_next.is_invalid() {
            break;
          }
          if IsWindowVisible(valid_next).as_bool() {
            let style = GetWindowLongW(valid_next, GWL_EXSTYLE) as u32;
            if (style & WS_EX_TOOLWINDOW.0) == 0 {
              let mut prev = get_previous_focus().lock().unwrap();
              *prev = Some(SendHwnd(valid_next));
              break;
            }
          }
          next = GetWindow(valid_next, GW_HWNDNEXT);
        }

        let prev = get_previous_focus().lock().unwrap();
        if let Some(h) = *prev {
          if IsWindowVisible(h.0).as_bool() {
            let _ = SetForegroundWindow(h.0);
          }
        }
      }
    }
    for (h, _, _, _, _, _, _) in siblings_data {
      let _ = ShowWindow(h.inner(), SW_HIDE);
    }
  }
}

pub fn park_window(send_hwnd: SendHwnd, config: &Config, app_cfg: &AppConfig) {
  let hwnd = send_hwnd.inner();
  unsafe {
    let ex_style = GetWindowLongW(hwnd, GWL_EXSTYLE);
    if (ex_style & WS_EX_LAYERED.0 as i32) == 0 {
      SetWindowLongW(hwnd, GWL_EXSTYLE, ex_style | WS_EX_LAYERED.0 as i32);
    }
    let _ = SetWindowPos(
      hwnd,
      HWND::default(),
      0,
      0,
      0,
      0,
      SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_FRAMECHANGED,
    );
    let _ = SetLayeredWindowAttributes(hwnd, COLORREF(0), 0, LWA_ALPHA);
    let _ = ShowWindow(hwnd, SW_HIDE);
    let mut r = RECT::default();
    let _ = GetWindowRect(hwnd, &mut r);
    let cur_w = r.right - r.left;
    let cur_h = r.bottom - r.top;
    let mon = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
    let mut mi = MONITORINFO::default();
    mi.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
    if GetMonitorInfoW(mon, &mut mi).as_bool() {
      let ((w, w_is_pct), (h, h_is_pct)) = app_cfg.resolve_dimensions(&config.window);
      let work_area = mi.rcWork;
      let sw = work_area.right - work_area.left;
      let tw = if w > 0.0 {
        (if w_is_pct { sw as f64 * w } else { w }) as i32
      } else {
        cur_w
      };
      let th = if h > 0.0 {
        (if h_is_pct {
          (work_area.bottom - work_area.top) as f64 * h
        } else {
          h
        }) as i32
      } else {
        cur_h
      };
      let tx = work_area.left + (sw - tw) / 2;
      let ty = work_area.top - th - 10;
      let _ = SetWindowPos(hwnd, HWND_NOTOPMOST, tx, ty, tw, th, SWP_NOACTIVATE);
    }
  }
}

pub fn restore_app_window(window_class: &str) {
  if let Some(hwnd) = find_window_by_process(window_class, None) {
    restore_hwnd(hwnd);
  }
}

fn restore_hwnd(hwnd: HWND) {
  unsafe {
    let ex = GetWindowLongW(hwnd, GWL_EXSTYLE);
    if (ex & WS_EX_LAYERED.0 as i32) == 0 {
      SetWindowLongW(hwnd, GWL_EXSTYLE, ex | WS_EX_LAYERED.0 as i32);
      let _ = SetWindowPos(
        hwnd,
        HWND::default(),
        0,
        0,
        0,
        0,
        SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_FRAMECHANGED,
      );
    }
    let _ = SetLayeredWindowAttributes(hwnd, COLORREF(0), 255, LWA_ALPHA);
    let (x, y, flags) = (100, 100, SWP_NOSIZE | SWP_SHOWWINDOW | SWP_NOACTIVATE);
    let _ = SetWindowPos(hwnd, HWND_NOTOPMOST, x, y, 0, 0, flags);
    if IsIconic(hwnd).as_bool() {
      let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
    } else {
      let _ = ShowWindow(hwnd, SW_SHOWNA);
    }
  }
}

pub fn restore_window_visibility() {
  // 1. Abort current animation
  get_animation_cancel().store(true, std::sync::atomic::Ordering::SeqCst);

  // 2. Restore all cached windows
  let cache = get_hwnd_cache().read().unwrap();
  for hwnd in cache.values() {
    restore_hwnd(hwnd.inner());
  }
}
pub fn reset_visible_app() {
  let mut v = get_visible_app().write().unwrap();
  *v = None;
}
