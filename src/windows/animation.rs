//! Animation engine for Windows slide-in/out effects.
//!
//! Handles the core animation loop with:
//! - VSync-aligned frame timing via `DwmFlush`
//! - Easing curves for smooth motion
//! - Opacity transitions
//! - Multi-window coordination (siblings)
//! - Monitor-aware positioning

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;
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

use crate::config::{
  compute_slide_positions, Config, DisplayMode, PositionOffset, SlideDirection, WorkArea,
};
use crate::windows::easing::get_easing;
use crate::windows::window::{
  force_focus, get_animation_cancel, get_animation_state, get_app_cache, get_previous_focus,
  get_visible_app, monitor_enum_proc, AnimationState, CachedWindow, MonitorEnumCtx,
};

/// Runs the animation loop synchronously.
///
/// This function handles:
/// - Monitor selection based on display_mode
/// - Position calculation with slide_from/position_offset
/// - Sibling window coordination
/// - Frame-by-frame interpolation with easing
/// - Opacity animation (if enabled)
static ANIMATION_GENERATION: AtomicU64 = AtomicU64::new(0);

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

  let my_gen = ANIMATION_GENERATION.fetch_add(1, Ordering::SeqCst) + 1;

  unsafe {
    let monitor = if should_show {
      match &config.window.display_mode {
        DisplayMode::Specific(_) => {
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
          let mut use_fallback = fg.is_invalid() || fg == target_hwnd.inner();

          if !use_fallback {
            let mut class_buf = [0u16; 256];
            let len = GetClassNameW(fg, &mut class_buf);
            let class_name = String::from_utf16_lossy(&class_buf[..len as usize]).to_lowercase();
            if class_name == "progman" || class_name == "workerw" || class_name == "shell_traywnd" {
              use_fallback = true;
            }
          }

          if !use_fallback {
            MonitorFromWindow(fg, MONITOR_DEFAULTTONEAREST)
          } else {
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

    // Resolve slide direction and position offset
    let (slide_from, position_offset) = app_cfg.resolve_slide_config(&config.window);

    // Compute shown/hidden positions using shared logic
    let work_area_rect = WorkArea {
      left: work_area.left,
      top: work_area.top,
      right: work_area.right,
      bottom: work_area.bottom,
    };
    let positions = compute_slide_positions(
      &slide_from,
      &position_offset,
      work_area_rect,
      target_w,
      target_h,
    );
    let (shown_x, shown_y, hidden_x, hidden_y) = (
      positions.shown_x,
      positions.shown_y,
      positions.hidden_x,
      positions.hidden_y,
    );

    let (final_target_x, final_target_y) = if should_show {
      (shown_x, shown_y)
    } else {
      (hidden_x, hidden_y)
    };

    let mut t_curr_alpha: u8 = 255;
    let _ = GetLayeredWindowAttributes(target_hwnd.inner(), None, Some(&mut t_curr_alpha), None);

    let t_curr_x = r_target.left;
    let t_curr_y = r_target.top;
    let t_on_correct_monitor =
      MonitorFromWindow(target_hwnd.inner(), MONITOR_DEFAULTTONEAREST) == monitor;

    // Check if window needs repositioning (monitor change OR config change)
    // Similar to Linux needsReposition logic - detects if window is far from expected position
    let needs_reposition = if should_show {
      let tolerance = 50; // Same as Linux
      let is_horizontal = matches!(slide_from, SlideDirection::Top | SlideDirection::Bottom);

      !t_on_correct_monitor
        || if is_horizontal {
          // For top/bottom slides, check if Y is outside expected range
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
          // For left/right slides, check if X is outside expected range
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

    // --- Sibling Data: (hwnd, start_x, start_y, width, height, end_x, end_y, alpha, duration_secs, easing_cfg) ---
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

            // Look up sibling's app name from APP_CACHE, then get its config
            let sib_app_name = {
              let cache = get_app_cache().read().unwrap();
              cache
                .iter()
                .find(|(_, cw)| cw.hwnd == ohwnd)
                .map(|(name, _)| name.clone())
            };
            let sib_slide = sib_app_name
              .as_ref()
              .and_then(|name| config.app.get(name))
              .map(|a| a.resolve_slide_config(&config.window))
              .unwrap_or_else(|| (SlideDirection::Top, PositionOffset::Center));

            let sib_dir = &sib_slide.0;
            let sib_offset = &sib_slide.1;

            // Use shared position calculation for sibling
            let sib_work_area = WorkArea {
              left: s_work.left,
              top: s_work.top,
              right: s_work.right,
              bottom: s_work.bottom,
            };
            let sib_positions =
              compute_slide_positions(sib_dir, sib_offset, sib_work_area, s_w, s_h);

            let (sib_easing_cfg, sib_dur_ms) = (
              &config.animation.hide_easing,
              config.animation.hide_duration,
            );

            // Sibling-specific overrides
            let (target_dur_ms, target_easing) = sib_app_name
              .as_ref()
              .and_then(|name| config.app.get(name))
              .map(|_a| {
                (
                  config.animation.hide_duration, // Use app-specific hide duration if we had it, but for now global hide
                  &config.animation.hide_easing,
                )
              })
              .unwrap_or((sib_dur_ms, sib_easing_cfg));

            // Siblings slide out using their hidden positions from compute_slide_positions
            let (sib_end_x, sib_end_y) = (sib_positions.hidden_x, sib_positions.hidden_y);

            // Distance scaling for sibling hide
            let s_dist_total = match sib_dir {
              SlideDirection::Top | SlideDirection::Bottom => (sib_end_y - r.top).abs(),
              SlideDirection::Left | SlideDirection::Right => (sib_end_x - r.left).abs(),
            } as f64;
            let s_max_dist = match sib_dir {
              SlideDirection::Top | SlideDirection::Bottom => s_h,
              SlideDirection::Left | SlideDirection::Right => s_w,
            } as f64;
            let s_dur_ms = if s_max_dist > 0.0 {
              (target_dur_ms as f64 * (s_dist_total / s_max_dist)).min(target_dur_ms as f64)
            } else {
              target_dur_ms as f64
            };

            let mut sa: u8 = 255;
            let _ = GetLayeredWindowAttributes(ohwnd, None, Some(&mut sa), None);
            siblings_data.push((
              ohwnd,
              r.left, // start_x
              r.top,  // start_y
              s_w,
              s_h,
              sib_end_x,
              sib_end_y,
              sa,
              s_dur_ms / 1000.0,
              target_easing.clone(),
            ));
          }
        }
      }
    }

    // --- Target Catching & Teleport ---
    // Teleport if on wrong monitor OR if config changed (position mismatch)
    let (t_start_x, t_start_y) = if should_show && needs_reposition {
      let _ = SetLayeredWindowAttributes(target_hwnd.inner(), COLORREF(0), 0, LWA_ALPHA);
      let _ = SetWindowPos(
        target_hwnd.inner(),
        HWND::default(),
        hidden_x,
        hidden_y,
        target_w,
        target_h,
        SWP_NOACTIVATE | SWP_NOZORDER,
      );
      (hidden_x, hidden_y)
    } else if should_show {
      (t_curr_x, t_curr_y)
    } else {
      (t_curr_x, t_curr_y)
    };

    let t_curr_alpha = if should_show && needs_reposition {
      0
    } else {
      t_curr_alpha
    };

    let t_dist_x = (final_target_x - t_start_x).abs();
    let t_dist_y = (final_target_y - t_start_y).abs();
    let t_dist_total = (t_dist_x.max(t_dist_y)) as f64;
    let max_dist = match slide_from {
      SlideDirection::Top | SlideDirection::Bottom => target_h,
      SlideDirection::Left | SlideDirection::Right => target_w,
    } as f64;

    let animate_opacity = app_cfg.get_animate_opacity(config.animation.animate_opacity);
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

    // --- Style & Layering Prep ---
    let _ = DwmSetWindowAttribute(
      target_hwnd.inner(),
      DWMWA_TRANSITIONS_FORCEDISABLED,
      &TRUE as *const _ as *const _,
      4,
    );
    for (h, _, _, _, _, _, _, _, _, _) in &siblings_data {
      let _ = DwmSetWindowAttribute(
        *h,
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
      let ex = GetWindowLongW(target_hwnd.inner(), GWL_EXSTYLE);
      if (ex & WS_EX_LAYERED.0 as i32) == 0 {
        prep_layer(target_hwnd.inner());
      }
    } else {
      prep_layer(target_hwnd.inner());
    }
    for (h, _, _, _, _, _, _, _, _, _) in &siblings_data {
      let ex = GetWindowLongW(*h, GWL_EXSTYLE);
      if (ex & WS_EX_LAYERED.0 as i32) == 0 {
        prep_layer(*h);
      }
    }

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

    let start_time = Instant::now();
    let mut first_frame = true;

    // Store animation state
    {
      let mut state = get_animation_state().lock().unwrap();
      *state = Some(AnimationState {
        hidden_x,
        hidden_y,
        shown_x,
        shown_y,
      });
    }

    let max_dur_secs = siblings_data
      .iter()
      .map(|(_, _, _, _, _, _, _, _, ds, _)| *ds)
      .fold(dur_secs, |a, b| a.max(b));

    if max_dur_secs > 0.0 {
      let mut last_x = t_start_x;
      let mut last_y = t_start_y;
      let mut last_alpha = t_curr_alpha;
      let mut last_sibling_xs: Vec<i32> = siblings_data
        .iter()
        .map(|(_, sx, _, _, _, _, _, _, _, _)| *sx)
        .collect();
      let mut last_sibling_ys: Vec<i32> = siblings_data
        .iter()
        .map(|(_, _, sy, _, _, _, _, _, _, _)| *sy)
        .collect();
      let mut last_sibling_alphas: Vec<u8> = siblings_data
        .iter()
        .map(|(_, _, _, _, _, _, _, sa, _, _)| *sa)
        .collect();

      let loop_start_time = Instant::now();
      loop {
        // 1. Bail Check
        {
          let v = get_visible_app().read().unwrap();
          let still_target = if should_show {
            v.as_deref() == Some(app_name)
          } else {
            v.as_deref() != Some(app_name)
          };
          if !still_target
            || get_animation_cancel().load(Ordering::SeqCst)
            || ANIMATION_GENERATION.load(Ordering::SeqCst) != my_gen
          {
            return;
          }
        }

        let elapsed = start_time.elapsed().as_secs_f64();
        let target_progress = if dur_secs > 0.0 {
          (elapsed / dur_secs).min(1.0)
        } else {
          1.0
        };
        let target_ease_val = get_easing(target_progress, easing);

        let mut needs_pos_update = false;
        if animate_opacity {
          let target_alpha_val = if should_show { 255.0 } else { 0.0 };
          let t_alpha = {
            let opacity_ease = if should_show {
              (target_ease_val / op_point).clamp(0.0, 1.0)
            } else {
              let denom = 1.0 - op_point;
              ((target_ease_val - op_point) / if denom <= 0.0 { 0.0001 } else { denom })
                .clamp(0.0, 1.0)
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

          for (i, (h, _, _, _, _, _, _, sa, s_dur, _)) in siblings_data.iter().enumerate() {
            let elapsed_since_start = loop_start_time.elapsed().as_secs_f64();
            let s_progress = (elapsed_since_start / *s_dur).min(1.0);
            let s_ease_val = get_easing(s_progress, &config.animation.hide_easing);
            let s_denom = 1.0 - config.animation.hide_opacity_point;
            let s_opacity_ease = ((s_ease_val - config.animation.hide_opacity_point)
              / if s_denom <= 0.0 { 0.0001 } else { s_denom })
            .clamp(0.0, 1.0);
            let s_target_alpha = {
              let computed = (*sa as f64 * (1.0 - s_opacity_ease)) as u8;
              computed.min(last_sibling_alphas[i])
            };
            if s_target_alpha != last_sibling_alphas[i] {
              let _ = SetLayeredWindowAttributes(*h, COLORREF(0), s_target_alpha, LWA_ALPHA);
              last_sibling_alphas[i] = s_target_alpha;
            }
          }
        } else if first_frame && should_show {
          let _ = SetLayeredWindowAttributes(target_hwnd.inner(), COLORREF(0), 255, LWA_ALPHA);
          last_alpha = 255;
        }

        // --- Position Update ---
        let t_dist_x_anim = final_target_x - t_start_x;
        let t_dist_y_anim = final_target_y - t_start_y;
        let next_x = t_start_x + (t_dist_x_anim as f64 * target_ease_val) as i32;
        let next_y = t_start_y + (t_dist_y_anim as f64 * target_ease_val) as i32;

        if next_x != last_x || next_y != last_y || first_frame {
          needs_pos_update = true;
        } else {
          for (i, (_, sx, sy, _, _, ex, ey, _, s_dur, s_easing)) in siblings_data.iter().enumerate()
          {
            let elapsed_since_start = loop_start_time.elapsed().as_secs_f64();
            let s_progress = (elapsed_since_start / *s_dur).min(1.0);
            let s_ease_val = get_easing(s_progress, s_easing);
            let on_x = sx + ((*ex - *sx) as f64 * s_ease_val) as i32;
            let on_y = sy + ((*ey - *sy) as f64 * s_ease_val) as i32;
            if on_x != last_sibling_xs[i] || on_y != last_sibling_ys[i] {
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

            for (i, (h, sx, sy, sw, sh, ex, ey, _, s_dur, s_easing)) in
              siblings_data.iter().enumerate()
            {
              let elapsed_since_start = loop_start_time.elapsed().as_secs_f64();
              let s_progress = (elapsed_since_start / *s_dur).min(1.0);
              let s_ease_val = get_easing(s_progress, s_easing);
              let on_x = sx + ((*ex - *sx) as f64 * s_ease_val) as i32;
              let on_y = sy + ((*ey - *sy) as f64 * s_ease_val) as i32;
              match DeferWindowPos(
                hdwp,
                *h,
                HWND::default(),
                on_x,
                on_y,
                *sw,
                *sh,
                SWP_NOACTIVATE | SWP_NOCOPYBITS | SWP_NOZORDER,
              ) {
                Ok(nh) if !nh.is_invalid() => hdwp = nh,
                _ => {
                  let _ = SetWindowPos(
                    *h,
                    HWND::default(),
                    on_x,
                    on_y,
                    *sw,
                    *sh,
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
                t_z,
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
          let _ = ShowWindow(target_hwnd.inner(), SW_SHOW);
          force_focus(target_hwnd.inner());
        }

        first_frame = false;

        // Sync with monitor refresh
        let _ = DwmFlush();

        if elapsed >= max_dur_secs {
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
        final_target_x,
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
        let mut next = GetWindow(target_hwnd.inner(), GW_HWNDNEXT);
        while let Ok(valid_next) = next {
          if valid_next.is_invalid() {
            break;
          }
          if IsWindowVisible(valid_next).as_bool() {
            let style = GetWindowLongW(valid_next, GWL_EXSTYLE) as u32;
            if (style & WS_EX_TOOLWINDOW.0) == 0 {
              let mut prev = get_previous_focus().lock().unwrap();
              *prev = Some(CachedWindow { hwnd: valid_next });
              break;
            }
          }
          next = GetWindow(valid_next, GW_HWNDNEXT);
        }

        let prev_lock = get_previous_focus().lock().unwrap();
        if let Some(cw) = *prev_lock {
          if IsWindowVisible(cw.hwnd).as_bool() {
            force_focus(cw.hwnd);
          }
        }
      }
    }
    for (h, _, _, _, _, _, _, _, _, _) in siblings_data {
      let _ = ShowWindow(h, SW_HIDE);
    }
  }
}
