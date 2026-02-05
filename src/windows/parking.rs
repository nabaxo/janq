//! Window parking and restoration for managed windows.
//!
//! Handles positioning windows offscreen when "parked" and restoring them
//! to visible positions on daemon exit.

use windows::Win32::{
  Foundation::{COLORREF, HWND, RECT},
  Graphics::Gdi::{GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST},
  UI::WindowsAndMessaging::*,
};

use crate::windows::window::{
  get_animation_cancel, get_app_cache, set_taskbar_hidden, visible_app_lock, CachedWindow,
};
use janq::config::{AppConfig, Config};

/// Parks a window offscreen based on its slide direction config.
///
/// Makes the window transparent and positions it just outside the visible
/// screen area, ready to slide in when toggled.
pub fn park_window(cw: CachedWindow, config: &Config, app_cfg: &AppConfig) {
  let hwnd = cw.inner();
  unsafe {
    let mut ex_style = GetWindowLongW(hwnd, GWL_EXSTYLE) as u32;
    if (ex_style & WS_EX_LAYERED.0) == 0 {
      ex_style |= WS_EX_LAYERED.0;
    }
    // WS_EX_TOOLWINDOW hides from Alt+Tab; only use when skip_pager=true
    if config.window.skip_pager {
      ex_style |= WS_EX_TOOLWINDOW.0;
    } else {
      ex_style &= !WS_EX_TOOLWINDOW.0;
    }
    SetWindowLongW(hwnd, GWL_EXSTYLE, ex_style as i32);

    // Always hide from taskbar via owner (keeps Alt+Tab when skip_pager=false)
    set_taskbar_hidden(hwnd, true);

    let mut style = GetWindowLongW(hwnd, GWL_STYLE) as u32;
    let original_style = style;
    let no_borders = app_cfg.get_no_borders(config.window.no_borders);
    if no_borders {
      style &= !(WS_CAPTION.0 | WS_THICKFRAME.0);
    } else {
      style |= WS_CAPTION.0 | WS_THICKFRAME.0;
    }
    if style != original_style {
      SetWindowLongW(hwnd, GWL_STYLE, style as i32);
    }

    let _ = SetWindowPos(
      hwnd,
      Some(HWND::default()),
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
      let (width, height) = app_cfg.resolve_dimensions(&config.window);
      let work_area = mi.rcWork;
      let screen_w = work_area.right - work_area.left;
      let screen_h = work_area.bottom - work_area.top;
      let tw = if width.val > 0.0 {
        (if width.is_percent {
          screen_w as f64 * width.val
        } else {
          width.val
        }) as i32
      } else {
        cur_w
      };
      let th = if height.val > 0.0 {
        (if height.is_percent {
          screen_h as f64 * height.val
        } else {
          height.val
        }) as i32
      } else {
        cur_h
      };

      // Resolve slide config
      let (slide_from, position_offset) = app_cfg.resolve_slide_config(&config.window);

      // Compute hidden position using shared logic
      let work_area_rect = janq::config::WorkArea {
        left: work_area.left,
        top: work_area.top,
        right: work_area.right,
        bottom: work_area.bottom,
      };
      let positions = janq::config::compute_slide_positions(
        &slide_from,
        &position_offset,
        work_area_rect,
        tw,
        th,
      );
      let (tx, ty) = (positions.hidden_x, positions.hidden_y);

      let _ = SetWindowPos(hwnd, Some(HWND_NOTOPMOST), tx, ty, tw, th, SWP_NOACTIVATE);
    }
  }
}

/// Restores a specific window to a visible state.
pub fn restore_hwnd(hwnd: HWND) {
  unsafe {
    let mut ex = GetWindowLongW(hwnd, GWL_EXSTYLE) as u32;
    if (ex & WS_EX_LAYERED.0) == 0 {
      ex |= WS_EX_LAYERED.0;
    }
    // Clear TOOLWINDOW and owner so window shows in taskbar and Alt+Tab
    ex &= !WS_EX_TOOLWINDOW.0;
    SetWindowLongW(hwnd, GWL_EXSTYLE, ex as i32);
    set_taskbar_hidden(hwnd, false);

    // Restore borders
    let mut style = GetWindowLongW(hwnd, GWL_STYLE) as u32;
    style |= WS_CAPTION.0 | WS_THICKFRAME.0;
    SetWindowLongW(hwnd, GWL_STYLE, style as i32);

    let _ = SetWindowPos(
      hwnd,
      Some(HWND::default()),
      0,
      0,
      0,
      0,
      SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_FRAMECHANGED,
    );
    let _ = SetLayeredWindowAttributes(hwnd, COLORREF(0), 255, LWA_ALPHA);
    let (x, y, flags) = (100, 100, SWP_NOSIZE | SWP_SHOWWINDOW | SWP_NOACTIVATE);
    let _ = SetWindowPos(hwnd, Some(HWND_NOTOPMOST), x, y, 0, 0, flags);
    if IsIconic(hwnd).as_bool() {
      let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
    } else {
      let _ = ShowWindow(hwnd, SW_SHOWNA);
    }
  }
}

/// Releases the given windows - cancels animation and restores them.
pub fn release_windows(windows: Vec<CachedWindow>) {
  if windows.is_empty() {
    return;
  }
  get_animation_cancel().store(true, std::sync::atomic::Ordering::SeqCst);
  for cw in windows {
    restore_hwnd(cw.inner());
  }
  get_animation_cancel().store(false, std::sync::atomic::Ordering::SeqCst);
}

/// Restores all cached windows to visible state (for daemon exit).
pub fn restore_window_visibility() {
  get_animation_cancel().store(true, std::sync::atomic::Ordering::SeqCst);
  let cache = get_app_cache().read().unwrap();
  for cw in cache.values() {
    restore_hwnd(cw.inner());
  }
}

/// Clears the visible app state.
pub fn reset_visible_app() {
  let mut v = visible_app_lock().write().unwrap();
  *v = None;
}
