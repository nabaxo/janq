//! Window parking and restoration for Windows.
//!
//! Handles positioning windows offscreen when "parked" and restoring them
//! to visible positions on daemon exit.

use windows::Win32::{
  Foundation::{COLORREF, HWND, RECT},
  Graphics::Gdi::{GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST},
  UI::WindowsAndMessaging::*,
};

use crate::config::{AppConfig, Config};
use crate::windows::discovery::find_window_by_process;
use crate::windows::window::{get_animation_cancel, get_app_cache, get_visible_app, CachedWindow};

/// Parks a window offscreen based on its slide direction config.
///
/// Makes the window transparent and positions it just outside the visible
/// screen area, ready to slide in when toggled.
pub fn park_window(cw: CachedWindow, config: &Config, app_cfg: &AppConfig) {
  let hwnd = cw.inner();
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

      // Resolve slide config
      let (slide_from, position_offset) = app_cfg.resolve_slide_config(&config.window);

      // Compute hidden position using shared logic
      let work_area_rect = crate::config::WorkArea {
        left: work_area.left,
        top: work_area.top,
        right: work_area.right,
        bottom: work_area.bottom,
      };
      let positions = crate::config::compute_slide_positions(
        &slide_from,
        &position_offset,
        work_area_rect,
        tw,
        th,
      );
      let (tx, ty) = (positions.hidden_x, positions.hidden_y);

      let _ = SetWindowPos(hwnd, HWND_NOTOPMOST, tx, ty, tw, th, SWP_NOACTIVATE);
    }
  }
}

/// Restores a window by its window_class config value.
pub fn restore_app_window(window_class: &str) {
  if let Some(cw) = find_window_by_process(window_class, None) {
    restore_hwnd(cw.inner());
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
  let cache = get_app_cache().read().unwrap();
  for cw in cache.values() {
    restore_hwnd(cw.inner());
  }
}

/// Clears the visible app state.
pub fn reset_visible_app() {
  let mut v = get_visible_app().write().unwrap();
  *v = None;
}
