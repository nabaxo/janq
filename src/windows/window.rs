use crate::config::{AppConfig, Config};
use crate::windows::easing::get_easing;
use tokio::time::Instant;
use windows::Win32::Foundation::{BOOL, COLORREF, HWND, LPARAM, POINT, RECT, TRUE};
use windows::Win32::Graphics::Dwm::{DwmFlush, DwmSetWindowAttribute, DWMWA_TRANSITIONS_FORCEDISABLED};
use windows::Win32::Graphics::Gdi::{
  EnumDisplayMonitors, GetMonitorInfoW, MonitorFromPoint, MonitorFromWindow, HDC, HMONITOR, MONITORINFO,
  MONITOR_DEFAULTTONEAREST,
};
use windows::Win32::System::ProcessStatus::GetModuleBaseNameW;
use windows::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_INFORMATION, PROCESS_VM_READ};
use windows::Win32::UI::WindowsAndMessaging::{
  BeginDeferWindowPos, DeferWindowPos, EndDeferWindowPos, EnumWindows, GetCursorPos, GetForegroundWindow,
  GetLayeredWindowAttributes, GetWindowLongW, GetWindowRect, GetWindowThreadProcessId, IsIconic, IsWindowVisible,
  SetForegroundWindow, SetLayeredWindowAttributes, SetWindowLongW, SetWindowPos, ShowWindow, GWL_EXSTYLE,
  HWND_NOTOPMOST, HWND_TOPMOST, LWA_ALPHA, SWP_DEFERERASE, SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOCOPYBITS,
  SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, SWP_SHOWWINDOW, SW_HIDE, SW_SHOWNA, WS_EX_LAYERED,
};

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

use std::collections::HashMap;
use std::sync::{OnceLock, RwLock};

static ANIMATION_TASK: OnceLock<std::sync::Mutex<Option<tokio::task::AbortHandle>>> = OnceLock::new();
static VISIBLE_APP: OnceLock<RwLock<Option<String>>> = OnceLock::new();
static PREVIOUS_FOCUS: OnceLock<std::sync::Mutex<Option<SendHwnd>>> = OnceLock::new();
static HWND_CACHE: OnceLock<RwLock<HashMap<String, SendHwnd>>> = OnceLock::new();

fn get_animation_task() -> &'static std::sync::Mutex<Option<tokio::task::AbortHandle>> {
  ANIMATION_TASK.get_or_init(|| std::sync::Mutex::new(None))
}

fn get_visible_app() -> &'static RwLock<Option<String>> {
  VISIBLE_APP.get_or_init(|| RwLock::new(None))
}

fn get_previous_focus() -> &'static std::sync::Mutex<Option<SendHwnd>> {
  PREVIOUS_FOCUS.get_or_init(|| std::sync::Mutex::new(None))
}

pub fn get_hwnd_cache() -> &'static RwLock<HashMap<String, SendHwnd>> {
  HWND_CACHE.get_or_init(|| RwLock::new(HashMap::new()))
}

unsafe extern "system" fn enum_windows_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
  let target_struct = &mut *(lparam.0 as *mut TargetSearch);
  let mut pid = 0;
  GetWindowThreadProcessId(hwnd, Some(&mut pid));

  let mut class_buffer = [0u16; 256];
  let class_len = windows::Win32::UI::WindowsAndMessaging::GetClassNameW(hwnd, &mut class_buffer);
  let class_name = String::from_utf16_lossy(&class_buffer[..class_len as usize]).to_lowercase();

  let mut title_buf = [0u16; 256];
  let title_len = windows::Win32::UI::WindowsAndMessaging::GetWindowTextW(hwnd, &mut title_buf);
  let title = String::from_utf16_lossy(&title_buf[..title_len as usize]).to_lowercase();

  // Fast path: check class_name and title first (no syscall)
  let matches_class = class_name.contains(&target_struct.name);
  let matches_title = title.contains(&target_struct.name);

  // Lazy path: only open process if class/title didn't match
  let mut proc_name = String::new();
  if !matches_class && !matches_title {
    if let Ok(process) = OpenProcess(PROCESS_QUERY_INFORMATION | PROCESS_VM_READ, false, pid) {
      let mut buffer = [0u16; 1024];
      let len = GetModuleBaseNameW(process, None, &mut buffer);
      if len > 0 {
        proc_name = String::from_utf16_lossy(&buffer[..len as usize]).to_lowercase();
      }
    }
  }

  if matches_class || matches_title || proc_name.contains(&target_struct.name) {
    target_struct.found_data.push(FoundWindow {
      hwnd,
      class_name,
      _proc_name: proc_name,
      title,
    });
  }
  BOOL(1)
}

struct FoundWindow {
  hwnd: HWND,
  class_name: String,
  _proc_name: String,
  title: String,
}

struct TargetSearch {
  name: String,
  found_data: Vec<FoundWindow>,
}

pub fn find_window_by_process(name: &str) -> Option<HWND> {
  let lower_name = name.to_lowercase();
  let mut search = TargetSearch {
    name: lower_name,
    found_data: Vec::new(),
  };
  unsafe {
    let _ = EnumWindows(Some(enum_windows_proc), LPARAM(&mut search as *mut _ as isize));
  }
  let mut best_hwnd = None;
  let mut best_score = -5000;
  for data in search.found_data {
    let hwnd = data.hwnd;
    unsafe {
      let mut rect = RECT::default();
      if GetWindowRect(hwnd, &mut rect).is_ok() {
        let w = rect.right - rect.left;
        let h = rect.bottom - rect.top;
        let is_minimized = IsIconic(hwnd).as_bool();
        if (w <= 0 || h <= 0) && !is_minimized {
          continue;
        }
        let style = GetWindowLongW(hwnd, GWL_EXSTYLE) as u32;
        if (style & windows::Win32::UI::WindowsAndMessaging::WS_EX_TOOLWINDOW.0) != 0 {
          continue;
        }
        let is_visible = IsWindowVisible(hwnd).as_bool();
        let mut score = 0;
        if data.class_name == search.name {
          score += 5000;
        } else if data.class_name.contains(&search.name) {
          score += 1000;
        }
        if is_visible {
          score += 2000;
        }
        if is_minimized {
          score += 1000;
        }
        let lower_title = data.title.to_lowercase();
        if lower_title.contains("dummy")
          || lower_title.contains("invisible")
          || data.class_name.contains("nvopengl")
          || data.class_name.contains("wgpu")
        {
          score -= 4000;
        }
        if data.class_name == "ime" || data.class_name == "msctfime ui" {
          score -= 4500;
        }
        let style_regular = GetWindowLongW(hwnd, windows::Win32::UI::WindowsAndMessaging::GWL_STYLE) as u32;
        if (style_regular & windows::Win32::UI::WindowsAndMessaging::WS_CAPTION.0) != 0 {
          score += 500;
        }
        if !data.title.is_empty() {
          score += 200;
        }
        score += ((w * h) / 10000).min(100);
        if score > best_score {
          best_score = score;
          best_hwnd = Some(hwnd);
        }
      }
    }
  }
  best_hwnd
}

struct MonitorEnumCtx {
  monitors: Vec<HMONITOR>,
}
unsafe extern "system" fn monitor_enum_proc(hmonitor: HMONITOR, _hdc: HDC, _rect: *mut RECT, lparam: LPARAM) -> BOOL {
  let ctx = &mut *(lparam.0 as *mut MonitorEnumCtx);
  ctx.monitors.push(hmonitor);
  BOOL(1)
}

pub async fn toggle_window(app_name: &str, config: &Config) -> bool {
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
        if windows::Win32::UI::WindowsAndMessaging::IsWindow(h.inner()).as_bool() {
          cached_hwnd = Some(*h);
        }
      }
    }
  }
  let target_hwnd = if let Some(h) = cached_hwnd {
    h
  } else {
    match find_window_by_process(&app_cfg.window_class) {
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
  if should_show {
    for (name, cfg) in &config.app {
      if name == app_name {
        continue;
      }
      let mut cached_h = None;
      {
        let cache = get_hwnd_cache().read().unwrap();
        if let Some(h) = cache.get(name) {
          unsafe {
            if windows::Win32::UI::WindowsAndMessaging::IsWindow(h.inner()).as_bool() {
              cached_h = Some(*h);
            }
          }
        }
      }
      let found_hwnd = if let Some(h) = cached_h {
        h
      } else {
        match find_window_by_process(&cfg.window_class) {
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
  }

  // Abort current animation
  {
    let mut task_handle = get_animation_task().lock().unwrap();
    if let Some(handle) = task_handle.take() {
      handle.abort();
    }
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

  if should_show {
    unsafe {
      let fg_window = GetForegroundWindow();
      let mut prev = get_previous_focus().lock().unwrap();
      if !fg_window.is_invalid() && fg_window != target_hwnd.inner() {
        *prev = Some(SendHwnd(fg_window));
      }
    }
  }

  let config_clone = config.clone();
  let app_name_clone = app_name.to_string();

  let handle = tokio::spawn(async move {
    let _ = tokio::task::spawn_blocking(move || {
      run_animation_task_sync(
        &app_name_clone,
        &config_clone,
        target_hwnd,
        should_show,
        siblings,
        restore_focus,
      );
    })
    .await;
  });

  {
    let mut task_handle = get_animation_task().lock().unwrap();
    *task_handle = Some(handle.abort_handle());
  }
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
          let mut ctx = MonitorEnumCtx { monitors: Vec::new() };
          let _ = EnumDisplayMonitors(None, None, Some(monitor_enum_proc), LPARAM(&mut ctx as *mut _ as isize));
          if (config.window.display_index as usize) < ctx.monitors.len() {
            ctx.monitors[config.window.display_index as usize]
          } else {
            let mut cursor_pos = POINT { x: 0, y: 0 };
            let _ = GetCursorPos(&mut cursor_pos);
            MonitorFromPoint(cursor_pos, MONITOR_DEFAULTTONEAREST)
          }
        }
        "active" => {
          let prev = get_previous_focus().lock().unwrap();
          if let Some(h) = *prev {
            MonitorFromWindow(h.0, MONITOR_DEFAULTTONEAREST)
          } else {
            let mut cursor_pos = POINT { x: 0, y: 0 };
            let _ = GetCursorPos(&mut cursor_pos);
            MonitorFromPoint(cursor_pos, MONITOR_DEFAULTTONEAREST)
          }
        }
        _ => {
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
    let ((width_val, width_is_pct), (height_val, height_is_pct)) = app_cfg.resolve_dimensions(&config.window);

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
    let t_on_correct_monitor = MonitorFromWindow(target_hwnd.inner(), MONITOR_DEFAULTTONEAREST) == monitor;

    // --- Sibling Data ---
    let mut siblings_data = Vec::new();
    for ohwnd in siblings {
      if ohwnd.0 == target_hwnd.0 {
        continue;
      }
      let mut r = RECT::default();
      if GetWindowRect(ohwnd.inner(), &mut r).is_ok() {
        let smon = MonitorFromWindow(ohwnd.inner(), MONITOR_DEFAULTTONEAREST);
        let mut smi = MONITORINFO::default();
        smi.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
        if GetMonitorInfoW(smon, &mut smi).as_bool() {
          let s_work = smi.rcWork;
          let osy = r.top;
          let oty = s_work.top - (r.bottom - r.top) - 10;
          if r.bottom > s_work.top - 100 {
            // Capture sibling start alpha
            let mut sa: u8 = 255;
            let _ = GetLayeredWindowAttributes(ohwnd.inner(), None, Some(&mut sa), None);
            siblings_data.push((ohwnd, r.left, r.right - r.left, r.bottom - r.top, osy, oty, sa));
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
    .clamp(0.01, 1.0);

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
    prep_layer(target_hwnd.inner());
    for (h, _, _, _, _, _, _) in &siblings_data {
      prep_layer(h.inner());
    }

    let start_time = Instant::now();
    let mut first_frame = true;

    if dur_secs > 0.0 {
      let mut last_y = t_start_y;
      let mut last_alpha = t_curr_alpha;
      let mut last_sibling_ys: Vec<i32> = siblings_data.iter().map(|(_, _, _, _, osy, _, _)| *osy).collect();
      let mut last_sibling_alphas: Vec<u8> = siblings_data.iter().map(|(_, _, _, _, _, _, sa)| *sa).collect();

      loop {
        // 1. Bail Check - Check if we are still the intended animation
        {
          let v = get_visible_app().read().unwrap();
          let still_target = if should_show {
            v.as_deref() == Some(app_name)
          } else {
            v.as_deref() != Some(app_name)
          };
          if !still_target {
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
            let computed = (t_curr_alpha as f64 + (target_alpha_val - t_curr_alpha as f64) * opacity_ease) as u8;
            if should_show {
              computed.max(last_alpha)
            } else {
              computed.min(last_alpha)
            }
          };

          if t_alpha != last_alpha {
            let _ = SetLayeredWindowAttributes(target_hwnd.inner(), COLORREF(0), t_alpha, LWA_ALPHA);
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
            let t_z = if first_frame { z_order } else { HWND::default() };
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
              let _ = SetWindowPos(target_hwnd.inner(), t_z, target_x, next_y, target_w, target_h, t_flags);
            }
            last_y = next_y;
          }
        }

        if first_frame && should_show {
          let _ = ShowWindow(target_hwnd.inner(), SW_SHOWNA);
          let _ = SetForegroundWindow(target_hwnd.inner());
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
        SWP_SHOWWINDOW | SWP_NOACTIVATE,
      );
    } else {
      let _ = ShowWindow(target_hwnd.inner(), SW_HIDE);
      if restore_focus {
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

pub async fn park_window(send_hwnd: SendHwnd, config: &Config, app_cfg: &AppConfig) {
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

pub fn restore_app_window(_app_name: &str, window_class: &str) {
  if let Some(hwnd) = find_window_by_process(window_class) {
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
        let _ = ShowWindow(hwnd, windows::Win32::UI::WindowsAndMessaging::SW_SHOWNOACTIVATE);
      } else {
        let _ = ShowWindow(hwnd, windows::Win32::UI::WindowsAndMessaging::SW_SHOWNA);
      }
    }
  }
}

pub fn restore_window_visibility(config: &Config) {
  for (name, cfg) in &config.app {
    restore_app_window(name, &cfg.window_class);
  }
}
pub fn reset_visible_app() {
  let mut v = get_visible_app().write().unwrap();
  *v = None;
}
