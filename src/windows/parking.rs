//! Window parking and restoration for Windows.
//!
//! Handles positioning windows offscreen when "parked" and restoring them
//! to visible positions on daemon exit.

use windows::Win32::{
  Foundation::{COLORREF, HWND, RECT},
  Graphics::Gdi::{GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST},
  UI::WindowsAndMessaging::*,
};

use crate::config::{AppConfig, Config, PositionOffset, SlideDirection};
use crate::windows::discovery::find_window_by_process;
use crate::windows::window::{get_animation_cancel, get_hwnd_cache, get_visible_app, SendHwnd};

/// Parks a window offscreen based on its slide direction config.
///
/// Makes the window transparent and positions it just outside the visible
/// screen area, ready to slide in when toggled.
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
      let screen_w = work_area.right - work_area.left;
      let screen_h = work_area.bottom - work_area.top;
      let tw = if w > 0.0 {
        (if w_is_pct { screen_w as f64 * w } else { w }) as i32
      } else {
        cur_w
      };
      let th = if h > 0.0 {
        (if h_is_pct { screen_h as f64 * h } else { h }) as i32
      } else {
        cur_h
      };

      // Compute hidden position based on slide direction and offset
      let (slide_from, position_offset) = app_cfg.resolve_slide_config(&config.window);
      let is_horizontal = matches!(slide_from, SlideDirection::Top | SlideDirection::Bottom);

      let along_pos = if is_horizontal {
        match &position_offset {
          PositionOffset::Center => work_area.left + (screen_w - tw) / 2,
          PositionOffset::Pixels(px) => {
            if *px >= 0 {
              work_area.left + *px
            } else {
              work_area.right - tw + *px
            }
          }
          PositionOffset::Percent(pct) => {
            if *pct >= 0.0 {
              work_area.left + (screen_w as f64 * *pct) as i32
            } else {
              work_area.right - tw - (screen_w as f64 * pct.abs()) as i32
            }
          }
        }
      } else {
        match &position_offset {
          PositionOffset::Center => work_area.top + (screen_h - th) / 2,
          PositionOffset::Pixels(px) => {
            if *px >= 0 {
              work_area.top + *px
            } else {
              work_area.bottom - th + *px
            }
          }
          PositionOffset::Percent(pct) => {
            if *pct >= 0.0 {
              work_area.top + (screen_h as f64 * *pct) as i32
            } else {
              work_area.bottom - th - (screen_h as f64 * pct.abs()) as i32
            }
          }
        }
      };

      let (tx, ty) = match slide_from {
        SlideDirection::Top => (along_pos, work_area.top - th - 10),
        SlideDirection::Bottom => (along_pos, work_area.bottom + 10),
        SlideDirection::Left => (work_area.left - tw - 10, along_pos),
        SlideDirection::Right => (work_area.right + 10, along_pos),
      };

      let _ = SetWindowPos(hwnd, HWND_NOTOPMOST, tx, ty, tw, th, SWP_NOACTIVATE);
    }
  }
}

/// Restores a window by its window_class config value.
pub fn restore_app_window(window_class: &str) {
  if let Some(hwnd) = find_window_by_process(window_class, None) {
    restore_hwnd(hwnd);
  }
}

/// Restores a specific window to a visible state.
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

/// Restores all cached windows to visible state (for daemon exit).
pub fn restore_window_visibility() {
  // 1. Abort current animation
  get_animation_cancel().store(true, std::sync::atomic::Ordering::SeqCst);

  // 2. Restore all cached windows
  let cache = get_hwnd_cache().read().unwrap();
  for hwnd in cache.values() {
    restore_hwnd(hwnd.inner());
  }
}

/// Clears the visible app state.
pub fn reset_visible_app() {
  let mut v = get_visible_app().write().unwrap();
  *v = None;
}
