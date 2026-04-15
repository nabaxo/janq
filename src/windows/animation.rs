//! Animation engine for Windows slide-in/out effects.
//!
//! Handles the core animation loop with:
//! - VSync-aligned frame timing via `DwmFlush` or fixed framerate
//! - Easing curves for smooth motion
//! - Opacity transitions
//! - Multi-window coordination (siblings)
//! - Monitor-aware positioning

use std::sync::atomic::{AtomicU32, Ordering};
use std::thread;
use std::time::{Duration, Instant};
use windows::Win32::Media::{timeBeginPeriod, timeEndPeriod};
use windows::Win32::{
  Foundation::{COLORREF, HWND, LPARAM, RECT, TRUE},
  Graphics::{
    Dwm::{DwmFlush, DwmSetWindowAttribute, DWMWA_TRANSITIONS_FORCEDISABLED},
    Gdi::{
      EnumDisplayMonitors, GetMonitorInfoW, MonitorFromPoint, MonitorFromWindow, MONITORINFO,
      MONITOR_DEFAULTTONEAREST,
    },
  },
  UI::WindowsAndMessaging::*,
};

use crate::windows::easing::get_easing;
use crate::windows::window::{
  apply_border_style, force_focus, get_animation_cancel, get_animation_state, get_app_cache,
  get_frame_insets, get_last_external_focus, is_shell_window, monitor_enum_proc,
  set_taskbar_hidden, AnimationState, CachedWindow, MonitorEnumCtx,
};
use janq::config::{
  compute_slide_positions, Config, DisplayMode, Framerate, PositionOffset, SlideDirection, WorkArea,
};

/// Runs the animation loop synchronously.
///
/// This function handles:
/// - Monitor selection based on display_mode
/// - Position calculation with slide_from/position_offset
/// - Sibling window coordination
/// - Frame-by-frame interpolation with easing
/// - Final window state enforcement
static ANIMATION_GENERATION: AtomicU32 = AtomicU32::new(0);

#[derive(Clone)]
struct SiblingAnimation {
  pub hwnd: HWND,
  pub start_x: i32,
  pub start_y: i32,
  pub width: i32,
  pub height: i32,
  pub end_x: i32,
  pub end_y: i32,
  pub alpha: u8,
  pub duration_secs: f64,
  pub easing: janq::config::Easing,
  pub animate_opacity: bool,
  pub no_borders: bool,
}

pub fn run_animation_task_sync(
  app_name: &str,
  config: &Config,
  target_hwnd: CachedWindow,
  should_show: bool,
  siblings: Vec<CachedWindow>,
  restore_focus: bool,
) {
  let app_cfg = match config.app.get(app_name) {
    Some(c) => c,
    None => return,
  };

  // Generation check to cancel old animations
  let my_gen = ANIMATION_GENERATION.fetch_add(1, Ordering::SeqCst) + 1;

  unsafe {
    // 1. Determine target monitor
    let monitor = if should_show {
      match &config.window.display_mode {
        DisplayMode::Specific => {
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
            let mut cursor_pos = windows::Win32::Foundation::POINT { x: 0, y: 0 };
            let _ = GetCursorPos(&mut cursor_pos);
            MonitorFromPoint(cursor_pos, MONITOR_DEFAULTTONEAREST)
          }
        }
        DisplayMode::Active => {
          let fg = GetForegroundWindow();
          let use_fallback = fg.is_invalid() || fg == target_hwnd.inner() || is_shell_window(fg);

          if !use_fallback {
            MonitorFromWindow(fg, MONITOR_DEFAULTTONEAREST)
          } else {
            // If already visible, use its current monitor, otherwise fallback to cursor
            if IsWindowVisible(target_hwnd.inner()).as_bool() {
              MonitorFromWindow(target_hwnd.inner(), MONITOR_DEFAULTTONEAREST)
            } else {
              let mut cursor_pos = windows::Win32::Foundation::POINT { x: 0, y: 0 };
              let _ = GetCursorPos(&mut cursor_pos);
              MonitorFromPoint(cursor_pos, MONITOR_DEFAULTTONEAREST)
            }
          }
        }
        DisplayMode::FollowMouse => {
          let mut cursor_pos = windows::Win32::Foundation::POINT { x: 0, y: 0 };
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

    // 2. Resolve dimensions
    let (width, height) = app_cfg.resolve_dimensions(&config.window);

    let mut r_target = RECT::default();
    let _ = GetWindowRect(target_hwnd.inner(), &mut r_target);

    let target_w = if width.val > 0.0 {
      (if width.is_percent {
        screen_w as f64 * width.val
      } else {
        width.val
      }) as i32
    } else {
      r_target.right - r_target.left
    };
    let target_h = if height.val > 0.0 {
      (if height.is_percent {
        screen_h as f64 * height.val
      } else {
        height.val
      }) as i32
    } else {
      r_target.bottom - r_target.top
    };

    // 3. Resolve positioning & slide
    let (slide_from, position_offset) = app_cfg.resolve_slide_config(&config.window);
    let depth_offset = app_cfg.resolve_depth_offset(&config.window);
    let hide_titlebar = app_cfg.resolve_hide_titlebar(&config.window);

    let work_area_rect = WorkArea {
      left: work_area.left,
      top: work_area.top,
      right: work_area.right,
      bottom: work_area.bottom,
    };
    let positions = compute_slide_positions(
      &slide_from,
      &position_offset,
      &depth_offset,
      work_area_rect,
      target_w,
      target_h,
    );
    let titlebar_adjust = if hide_titlebar && matches!(slide_from, SlideDirection::Top) {
      super::window::get_titlebar_height(target_hwnd.inner())
    } else {
      0
    };
    let (shown_x, shown_y, hidden_x, hidden_y) = (
      positions.shown_x,
      positions.shown_y - titlebar_adjust,
      positions.hidden_x,
      positions.hidden_y,
    );

    let (final_target_x, final_target_y) = if should_show {
      (shown_x, shown_y)
    } else {
      (hidden_x, hidden_y)
    };

    // Current state for delta calculation
    let mut t_curr_alpha: u8 = 255;
    let _ = GetLayeredWindowAttributes(target_hwnd.inner(), None, Some(&mut t_curr_alpha), None);

    let t_curr_x = r_target.left;
    let t_curr_y = r_target.top;
    let t_on_correct_monitor =
      MonitorFromWindow(target_hwnd.inner(), MONITOR_DEFAULTTONEAREST) == monitor;

    // If showing, check if we need to "teleport" to start position
    // Similar to Linux needsReposition logic - detects if window is far from expected position
    let needs_reposition = if should_show {
      let tolerance = 50; // Same as Linux
      let is_horizontal = matches!(slide_from, SlideDirection::Top | SlideDirection::Bottom);

      !t_on_correct_monitor
        || if is_horizontal {
          match slide_from {
            SlideDirection::Top => {
              t_curr_y < hidden_y - tolerance || t_curr_y > shown_y + tolerance
            }
            SlideDirection::Bottom => {
              t_curr_y > hidden_y + tolerance || t_curr_y < shown_y - tolerance
            }
            _ => false,
          }
        } else {
          match slide_from {
            SlideDirection::Left => {
              t_curr_x < hidden_x - tolerance || t_curr_x > shown_x + tolerance
            }
            SlideDirection::Right => {
              t_curr_x > hidden_x + tolerance || t_curr_x < shown_x - tolerance
            }
            _ => false,
          }
        }
    } else {
      false
    };

    // 4. Gather Sibling Animations
    let mut siblings_data = Vec::new();
    for ocw in siblings {
      let ohwnd = ocw.hwnd;
      if ohwnd == target_hwnd.inner() {
        continue;
      }
      let mut r = RECT::default();
      if GetWindowRect(ohwnd, &mut r).is_ok() {
        let is_visible = IsWindowVisible(ohwnd).as_bool();
        if is_visible {
          let smon = MonitorFromWindow(ohwnd, MONITOR_DEFAULTTONEAREST);
          let mut smi = MONITORINFO::default();
          smi.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
          if GetMonitorInfoW(smon, &mut smi).as_bool() {
            let s_work = smi.rcWork;
            let s_w = r.right - r.left;
            let s_h = r.bottom - r.top;

            let sib_app_name = {
              let cache = get_app_cache().read().unwrap();
              cache
                .iter()
                .find(|(_, cw)| cw.hwnd == ohwnd)
                .map(|(name, _)| name.clone())
            };
            let sib_slide = sib_app_name
              .as_ref()
              .and_then(|name| config.app.get(name.as_ref()))
              .map(|a| a.resolve_slide_config(&config.window))
              .unwrap_or_else(|| (SlideDirection::Top, PositionOffset::Center));
            let sib_depth = sib_app_name
              .as_ref()
              .and_then(|name| config.app.get(name.as_ref()))
              .map(|a| a.resolve_depth_offset(&config.window))
              .unwrap_or_else(|| PositionOffset::Center);

            let sib_dir = &sib_slide.0;
            let sib_offset = &sib_slide.1;

            let sib_work_area = WorkArea {
              left: s_work.left,
              top: s_work.top,
              right: s_work.right,
              bottom: s_work.bottom,
            };
            let sib_positions =
              compute_slide_positions(sib_dir, sib_offset, &sib_depth, sib_work_area, s_w, s_h);

            let (sib_easing_cfg, sib_dur_ms) = (
              &config.animation.hide_easing,
              config.animation.hide_duration,
            );

            let (target_dur_ms, target_easing, sib_anim_op, sib_no_brd) = sib_app_name
              .as_ref()
              .and_then(|name| config.app.get(name.as_ref()))
              .map(|a| {
                (
                  config.animation.hide_duration,
                  &config.animation.hide_easing,
                  a.get_animate_opacity(config.animation.animate_opacity),
                  a.get_no_borders(config.window.no_borders),
                )
              })
              .unwrap_or((
                sib_dur_ms,
                sib_easing_cfg,
                config.animation.animate_opacity,
                config.window.no_borders,
              ));

            let (sib_end_x, sib_end_y) = (sib_positions.hidden_x, sib_positions.hidden_y);

            let s_dist_total = match sib_dir {
              SlideDirection::Top | SlideDirection::Bottom => (sib_end_y - r.top).abs(),
              SlideDirection::Left | SlideDirection::Right => (sib_end_x - r.left).abs(),
            } as f64;
            let s_max_dist = match sib_dir {
              SlideDirection::Top | SlideDirection::Bottom => s_h as f64,
              SlideDirection::Left | SlideDirection::Right => s_w as f64,
            } as f64;
            let s_dur_ms = if s_max_dist > 0.0 {
              (target_dur_ms as f64 * (s_dist_total / s_max_dist)).min(target_dur_ms as f64)
            } else {
              target_dur_ms as f64
            };

            let mut sa: u8 = 255;
            let _ = GetLayeredWindowAttributes(ohwnd, None, Some(&mut sa), None);
            siblings_data.push(SiblingAnimation {
              hwnd: ohwnd,
              start_x: r.left,
              start_y: r.top,
              width: s_w,
              height: s_h,
              end_x: sib_end_x,
              end_y: sib_end_y,
              alpha: sa,
              duration_secs: s_dur_ms / 1000.0,
              easing: target_easing.clone(),
              animate_opacity: sib_anim_op,
              no_borders: sib_no_brd,
            });
          }
        }
      }
    }

    // Prepare target window start state
    let (t_start_x, t_start_y) = if should_show && needs_reposition {
      let _ = SetLayeredWindowAttributes(target_hwnd.inner(), COLORREF(0), 0, LWA_ALPHA);
      let _ = SetWindowPos(
        target_hwnd.inner(),
        Some(HWND::default()),
        hidden_x,
        hidden_y,
        target_w,
        target_h,
        SWP_NOACTIVATE | SWP_NOZORDER,
      );
      (hidden_x, hidden_y)
    } else {
      (t_curr_x, t_curr_y)
    };

    let t_curr_alpha = if should_show && needs_reposition {
      0
    } else {
      t_curr_alpha
    };

    // 5. Setup Animation Params
    let t_dist_x = (final_target_x - t_start_x).abs();
    let t_dist_y = (final_target_y - t_start_y).abs();
    let t_dist_total = (t_dist_x.max(t_dist_y)) as f64;
    let max_dist = match slide_from {
      SlideDirection::Top | SlideDirection::Bottom => target_h,
      SlideDirection::Left | SlideDirection::Right => target_w,
    } as f64;

    let animate_opacity = if matches!(config.animation.framerate, Framerate::Specific(0)) {
      false
    } else {
      app_cfg.get_animate_opacity(config.animation.animate_opacity)
    };
    let base_dur_ms = if should_show {
      config.animation.show_duration
    } else {
      config.animation.hide_duration
    };
    let dur_ms = if max_dist > 0.0 {
      (base_dur_ms as f64 * (t_dist_total / max_dist)).min(base_dur_ms as f64)
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

    // Disable DWM transitions for cleaner movement
    let _ = DwmSetWindowAttribute(
      target_hwnd.inner(),
      DWMWA_TRANSITIONS_FORCEDISABLED,
      &TRUE as *const _ as *const _,
      4,
    );
    for sib in &siblings_data {
      let _ = DwmSetWindowAttribute(
        sib.hwnd,
        DWMWA_TRANSITIONS_FORCEDISABLED,
        &TRUE as *const _ as *const _,
        4,
      );
    }

    // Prep Layering and Borders
    let prep_layer = |h: HWND, app_no_borders: bool| {
      let mut ex = GetWindowLongW(h, GWL_EXSTYLE) as u32;
      let mut changed = false;
      if (ex & WS_EX_LAYERED.0) == 0 {
        ex |= WS_EX_LAYERED.0;
        changed = true;
      }
      // WS_EX_TOOLWINDOW hides from Alt+Tab; only use when skip_pager=true
      if config.window.skip_pager {
        if (ex & WS_EX_TOOLWINDOW.0) == 0 {
          ex |= WS_EX_TOOLWINDOW.0;
          changed = true;
        }
      } else if (ex & WS_EX_TOOLWINDOW.0) != 0 {
        ex &= !WS_EX_TOOLWINDOW.0;
        changed = true;
      }
      // WS_EX_APPWINDOW forces a taskbar button even with an owner; strip it
      if (ex & WS_EX_APPWINDOW.0) != 0 {
        ex &= !WS_EX_APPWINDOW.0;
        changed = true;
      }

      if changed {
        SetWindowLongW(h, GWL_EXSTYLE, ex as i32);
      }

      // Always hide from taskbar via owner (keeps Alt+Tab when skip_pager=false)
      set_taskbar_hidden(h, true);

      let border_changed = apply_border_style(h, app_no_borders);
      if border_changed {
        changed = true;
      }

      // If only ex-style changed (not borders), still need SWP_FRAMECHANGED
      if changed && !border_changed {
        let _ = SetWindowPos(
          h,
          Some(HWND::default()),
          0,
          0,
          0,
          0,
          SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_FRAMECHANGED,
        );
      }
    };
    let target_no_borders = app_cfg.get_no_borders(config.window.no_borders);
    if !should_show {
      let ex = GetWindowLongW(target_hwnd.inner(), GWL_EXSTYLE);
      if (ex & WS_EX_LAYERED.0 as i32) == 0 {
        prep_layer(target_hwnd.inner(), target_no_borders);
      }
    } else {
      prep_layer(target_hwnd.inner(), target_no_borders);
    }
    for sib in &siblings_data {
      let ex = GetWindowLongW(sib.hwnd, GWL_EXSTYLE);
      if (ex & WS_EX_LAYERED.0 as i32) == 0 {
        prep_layer(sib.hwnd, sib.no_borders);
      }
    }

    // Compensate for DWM invisible frame insets so the visual edge
    // of the window sits flush against the screen/work-area edge.
    let (inset_l, inset_t, inset_r, inset_b) = get_frame_insets(target_hwnd.inner());
    // Only add insets to dimensions that came from config (percent/pixels).
    // When dimensions are Unset, GetWindowRect already includes the invisible
    // DWM frame — adding insets again would grow the window on every toggle.
    let target_w = if width.val > 0.0 {
      target_w + inset_l + inset_r
    } else {
      target_w
    };
    let target_h = if height.val > 0.0 {
      target_h + inset_t + inset_b
    } else {
      target_h
    };
    let shown_x = shown_x - inset_l;
    let shown_y = shown_y - inset_t;
    let hidden_x = hidden_x - inset_l;
    let hidden_y = hidden_y - inset_t;

    // Reuse logical positions from state if monitor unchanged
    let t_on_correct_monitor =
      MonitorFromWindow(target_hwnd.inner(), MONITOR_DEFAULTTONEAREST) == monitor;
    let (hidden_x, hidden_y, shown_x, shown_y) = {
      let state = get_animation_state().lock().unwrap();
      if let Some(ref st) = *state {
        if t_on_correct_monitor {
          (st.hidden_x, st.hidden_y, st.shown_x, st.shown_y)
        } else {
          drop(state);
          (hidden_x, hidden_y, shown_x, shown_y)
        }
      } else {
        (hidden_x, hidden_y, shown_x, shown_y)
      }
    };

    {
      let mut state = get_animation_state().lock().unwrap();
      *state = Some(AnimationState {
        hidden_x,
        hidden_y,
        shown_x,
        shown_y,
      });
    }

    // 6. Animation Loop
    let max_dur_secs = if matches!(config.animation.framerate, Framerate::Specific(0)) {
      0.0
    } else {
      siblings_data
        .iter()
        .map(|s| s.duration_secs)
        .fold(dur_secs, |a, b| a.max(b))
    };

    if max_dur_secs > 0.0 {
      let mut last_x = t_start_x;
      let mut last_y = t_start_y;
      let mut last_alpha = t_curr_alpha;
      let mut last_sibling_xs: Vec<i32> = siblings_data.iter().map(|s| s.start_x).collect();
      let mut last_sibling_ys: Vec<i32> = siblings_data.iter().map(|s| s.start_y).collect();
      let mut last_sibling_alphas: Vec<u8> = siblings_data.iter().map(|s| s.alpha).collect();

      // Fix: Pre-allocate outside the loop to prevent heap allocation jitter
      let mut sibling_next_pos = vec![(0i32, 0i32); siblings_data.len()];

      let _ = timeBeginPeriod(1);
      // Fix: Reset start_time RIGHT BEFORE the loop so 'elapsed' starts at 0.0
      let start_time = Instant::now();
      let mut last_frame_time = Instant::now();
      let mut first_frame = true;

      loop {
        // Exit checks
        if get_animation_cancel().load(Ordering::SeqCst)
          || ANIMATION_GENERATION.load(Ordering::SeqCst) != my_gen
        {
          let _ = timeEndPeriod(1);
          return;
        }

        let elapsed = start_time.elapsed().as_secs_f64();
        let target_progress = if dur_secs > 0.0 {
          (elapsed / dur_secs).min(1.0)
        } else {
          1.0
        };
        let target_ease_val = get_easing(target_progress, easing);

        // Update target opacity
        let mut needs_pos_update = false;
        if animate_opacity {
          let t_alpha = {
            let raw_op_progress = if should_show {
              (target_progress / op_point).clamp(0.0, 1.0)
            } else {
              let denom = 1.0 - op_point;
              ((target_progress - op_point) / if denom <= 0.0 { 0.0001 } else { denom })
                .clamp(0.0, 1.0)
            };
            let eased_op = get_easing(raw_op_progress, easing);
            let start_a = t_curr_alpha as f64;
            let end_a = if should_show { 255.0 } else { 0.0 };
            (start_a + (end_a - start_a) * eased_op) as u8
          };

          if t_alpha != last_alpha {
            let _ =
              SetLayeredWindowAttributes(target_hwnd.inner(), COLORREF(0), t_alpha, LWA_ALPHA);
            last_alpha = t_alpha;
          }
        } else if first_frame && should_show {
          let _ = SetLayeredWindowAttributes(target_hwnd.inner(), COLORREF(0), 255, LWA_ALPHA);
          last_alpha = 255;
        }

        // Update siblings opacity
        for (i, sib) in siblings_data.iter().enumerate() {
          if sib.animate_opacity {
            let s_progress = (elapsed / sib.duration_secs).min(1.0);
            // Siblings always follow the "hide" path in synchronization with their own duration
            let s_denom = 1.0 - config.animation.hide_opacity_point;
            let raw_op_progress = ((s_progress - config.animation.hide_opacity_point)
              / if s_denom <= 0.0 { 0.0001 } else { s_denom })
            .clamp(0.0, 1.0);
            let eased_op = get_easing(raw_op_progress, &sib.easing);
            let s_target_alpha = (sib.alpha as f64 * (1.0 - eased_op)) as u8;

            if s_target_alpha != last_sibling_alphas[i] {
              let _ = SetLayeredWindowAttributes(sib.hwnd, COLORREF(0), s_target_alpha, LWA_ALPHA);
              last_sibling_alphas[i] = s_target_alpha;
            }
          }
        }

        // Target interpolation
        let next_x = t_start_x + ((final_target_x - t_start_x) as f64 * target_ease_val) as i32;
        let next_y = t_start_y + ((final_target_y - t_start_y) as f64 * target_ease_val) as i32;

        // Siblings interpolation
        let mut sib_needs_move = false;

        for (i, sib) in siblings_data.iter().enumerate() {
          let s_progress = (elapsed / sib.duration_secs).min(1.0);
          let s_ease_val = get_easing(s_progress, &sib.easing);
          let on_x = sib.start_x + ((sib.end_x - sib.start_x) as f64 * s_ease_val) as i32;
          let on_y = sib.start_y + ((sib.end_y - sib.start_y) as f64 * s_ease_val) as i32;

          sibling_next_pos[i] = (on_x, on_y);
          if on_x != last_sibling_xs[i] || on_y != last_sibling_ys[i] {
            sib_needs_move = true;
          }
        }

        // Change check
        if next_x != last_x || next_y != last_y || first_frame || sib_needs_move {
          needs_pos_update = true;
        }

        // Commit frame via DeferWindowPos
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
              Some(t_z),
              next_x,
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

            for (i, sib) in siblings_data.iter().enumerate() {
              let (on_x, on_y) = sibling_next_pos[i];
              match DeferWindowPos(
                hdwp,
                sib.hwnd,
                Some(HWND::default()),
                on_x,
                on_y,
                sib.width,
                sib.height,
                SWP_NOACTIVATE | SWP_NOCOPYBITS | SWP_NOZORDER,
              ) {
                Ok(nh) if !nh.is_invalid() => hdwp = nh,
                _ => {
                  let _ = SetWindowPos(
                    sib.hwnd,
                    Some(HWND::default()),
                    on_x,
                    on_y,
                    sib.width,
                    sib.height,
                    SWP_NOACTIVATE | SWP_NOCOPYBITS | SWP_NOZORDER,
                  );
                }
              }
              last_sibling_xs[i] = on_x;
              last_sibling_ys[i] = on_y;
            }
            let _ = EndDeferWindowPos(hdwp);
            if !t_ok {
              let _ = SetWindowPos(
                target_hwnd.inner(),
                Some(t_z),
                next_x,
                next_y,
                target_w,
                target_h,
                t_flags,
              );
            }
            last_x = next_x;
            last_y = next_y;
          }
        }

        if first_frame && should_show {
          force_focus(target_hwnd.inner());
        }

        first_frame = false;

        // Regulate framerate
        match config.animation.framerate {
          Framerate::Auto => {
            let _ = DwmFlush();
          }
          Framerate::Specific(fps) if fps > 0 => {
            // Cast u16 to u64 for the nanosecond calculation
            let target_ns = 1_000_000_000u64 / fps as u64;
            let frame_elapsed = last_frame_time.elapsed().as_nanos() as u64;

            if frame_elapsed < target_ns {
              thread::sleep(Duration::from_nanos(target_ns - frame_elapsed));
            }
            last_frame_time = Instant::now();
          }
          _ => {
            let _ = DwmFlush();
          }
        }

        if elapsed >= max_dur_secs {
          break;
        }
      }
      let _ = timeEndPeriod(1);
    }

    // --- Finalize ---
    if should_show {
      let _ = SetLayeredWindowAttributes(target_hwnd.inner(), COLORREF(0), 255, LWA_ALPHA);
      let _ = SetWindowPos(
        target_hwnd.inner(),
        Some(z_order),
        final_target_x,
        final_target_y,
        target_w,
        target_h,
        SWP_SHOWWINDOW,
      );
      let _ = ShowWindow(target_hwnd.inner(), SW_SHOW);
      force_focus(target_hwnd.inner());
    } else {
      if IsWindowVisible(target_hwnd.inner()).as_bool() {
        let _ = ShowWindow(target_hwnd.inner(), SW_HIDE);
      }
      if restore_focus {
        let last_focus = get_last_external_focus();
        if !last_focus.0.is_null() && IsWindowVisible(last_focus).as_bool() {
          force_focus(last_focus);
        }
      }
    }
    for sib in siblings_data {
      // Snap to final state before hiding
      let _ = SetWindowPos(
        sib.hwnd,
        Some(HWND::default()),
        sib.end_x,
        sib.end_y,
        sib.width,
        sib.height,
        SWP_NOACTIVATE | SWP_NOZORDER,
      );
      if sib.animate_opacity {
        let _ = SetLayeredWindowAttributes(sib.hwnd, COLORREF(0), 0, LWA_ALPHA);
      }
      let _ = ShowWindow(sib.hwnd, SW_HIDE);
    }
  }
}
