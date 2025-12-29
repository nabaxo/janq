use crate::config::Config;
use crate::windows::easing::get_easing;
use windows::Win32::Foundation::{BOOL, HWND, LPARAM, COLORREF, RECT, POINT};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetWindowThreadProcessId, IsWindowVisible, ShowWindow, SetForegroundWindow, GetForegroundWindow,
    SetWindowPos, SW_HIDE, HWND_TOPMOST, HWND_NOTOPMOST, SWP_SHOWWINDOW, SWP_NOACTIVATE,
    SetLayeredWindowAttributes, GetWindowLongW, SetWindowLongW, GWL_EXSTYLE, WS_EX_LAYERED, LWA_ALPHA,
    GetCursorPos, GetWindowRect, IsIconic, SW_SHOW, SW_RESTORE, SWP_NOSIZE
};
use windows::Win32::Graphics::Gdi::{
    MonitorFromPoint, GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromWindow, HMONITOR, HDC, EnumDisplayMonitors
};
use windows::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_INFORMATION, PROCESS_VM_READ};
use windows::Win32::System::ProcessStatus::GetModuleBaseNameW;
use std::ffi::OsString;
use std::os::windows::ffi::OsStrExt;
use tokio::time::{interval, Duration, Instant};

// Wrapper to make HWND Send/Sync for async tasks
#[derive(Clone, Copy)]
struct SendHwnd(HWND);
unsafe impl Send for SendHwnd {}
unsafe impl Sync for SendHwnd {}

impl SendHwnd {
    fn inner(&self) -> HWND {
        self.0
    }
}

lazy_static::lazy_static! {
    static ref ANIMATION_TASK: std::sync::Mutex<Option<tokio::task::AbortHandle>> = std::sync::Mutex::new(None);
    static ref TARGET_VISIBLE: std::sync::Mutex<bool> = std::sync::Mutex::new(false);
    static ref PREVIOUS_FOCUS: std::sync::Mutex<Option<SendHwnd>> = std::sync::Mutex::new(None);
}

// ... (helpers omitted for brevity if unchanged, but for replace_file_content we need context)

// Helper to convert string to wide string for Windows API
#[allow(dead_code)]
fn to_wstring(s: &str) -> Vec<u16> {
    OsString::from(s).as_os_str().encode_wide().chain(Some(0)).collect()
}

unsafe extern "system" fn enum_windows_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
    let target_struct = &mut *(lparam.0 as *mut TargetSearch);

    let mut pid = 0;
    GetWindowThreadProcessId(hwnd, Some(&mut pid));

    // Open process to get name
    if let Ok(process) = OpenProcess(PROCESS_QUERY_INFORMATION | PROCESS_VM_READ, false, pid) {
        let mut buffer = [0u16; 1024];
        let len = GetModuleBaseNameW(process, None, &mut buffer);
        if len > 0 {
            let name = String::from_utf16_lossy(&buffer[..len as usize]);
            // Check if name matches (ignoring case)
            if name.to_lowercase().contains(&target_struct.name.to_lowercase()) {
                target_struct.found_hwnds.push(hwnd);
                // Continue enumeration to find ALL windows
            }
        }
    }

    BOOL(1) // Continue
}

struct TargetSearch {
    name: String,
    found_hwnds: Vec<HWND>,
}

pub fn find_window_by_process(name: &str) -> Option<HWND> {
    let mut search = TargetSearch {
        name: name.to_string(),
        found_hwnds: Vec::new(),
    };

    unsafe {
        let _ = EnumWindows(Some(enum_windows_proc), LPARAM(&mut search as *mut _ as isize));
    }

    let mut best_hwnd = None;
    let mut best_score = -1;

    for hwnd in search.found_hwnds {
        unsafe {
            let mut rect = RECT::default();
            if GetWindowRect(hwnd, &mut rect).is_ok() {
                let w = rect.right - rect.left;
                let h = rect.bottom - rect.top;

                if w > 10 && h > 10 {
                    let is_visible = IsWindowVisible(hwnd).as_bool();
                    let mut score = 0;
                    if is_visible { score += 10; }
                     score += if (w * h) > 10000 { 1 } else { 0 };

                    if score > best_score {
                         best_score = score;
                         best_hwnd = Some(hwnd);
                    }
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

pub async fn toggle_window(config: &Config) {
    println!("Toggling window...");

    let should_show = {
        let mut target_visible = TARGET_VISIBLE.lock().unwrap();
        *target_visible = !*target_visible;
        *target_visible
    };

    {
        let mut task_handle = ANIMATION_TASK.lock().unwrap();
        if let Some(handle) = task_handle.take() {
            handle.abort();
        }
    }

    // Find the SendHwnd wrapper
    let hwnd = match find_window_by_process(&config.general.window_class) {
        Some(h) => SendHwnd(h),
        None => {
            println!("Window not found for process: {}", config.general.window_class);
            return;
        }
    };

    // Synchronously handle Focus and Initial Visibility
    unsafe {
        if should_show {
            let fg_window = GetForegroundWindow();
            if fg_window.0 != std::ptr::null_mut() && fg_window.0 != hwnd.inner().0 {
                let mut prev = PREVIOUS_FOCUS.lock().unwrap();
                if prev.is_none() {
                    *prev = Some(SendHwnd(fg_window));
                }
            }

            // Immediately activate (steal focus)
            let _ = SetForegroundWindow(hwnd.inner());
            // Ensure visible (but maybe at old position, swiftly moved by animation loop?)
            // We need it visible to be focused.
            // If we SetWindowPos here, we might jump.
            // Best to let the animation loop handle position, but SetForegroundWindow might fail if hidden?
            // Actually ShowWindow(SW_SHOW) is needed if it was hidden.
            let _ = ShowWindow(hwnd.inner(), SW_SHOW);
        } else {
             // Immediately surrender focus
             let mut prev = PREVIOUS_FOCUS.lock().unwrap();
             if let Some(h) = *prev {
                 if IsWindowVisible(h.0).as_bool() {
                     let _ = SetForegroundWindow(h.0);
                 }
                 *prev = None;
             }
        }
    }

    let config = config.clone();

    let handle = tokio::spawn(async move {
        // Use the SendHwnd wrapper inside the async block
        // Animation Loop logic...
        unsafe {
            // Determine current state
             // ... [Logic similar to before but without SetForegroundWindow at end/start]

             // Determine target monitor based on config...
            let monitor = {
                 // Re-use logic to find monitor.
                 // Note: We can't use GetForegroundWindow logic for "active" display mode easily if we just stole focus!
                 // BUT we already stole focus if showing.
                 // So we should rely on where the mouse is or where it was?
                 // If display_mode = "active", and we just became active, we are on the active monitor.
                 // If we were hidden, we were not active.
                 // So we need to match the logic:
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
                    },
                    "active" => {
                         // We are now the foreground window if showing.
                         // But we want to spawn on the monitor where the USER WAS.
                         // Previous logic: "activeWin != target".
                         // Our PREVIOUS_FOCUS has the window where the user was.
                         // If PREVIOUS_FOCUS is set, use that.
                         // If not, use mouse.

                         // BUT we are inside the thread, PREVIOUS_FOCUS is locked/unlocked briefly.
                         // We can check PREVIOUS_FOCUS again? No, we cleared it if hiding.

                         // If Showing: PREVIOUS_FOCUS should be set.
                         // If Hiding: We don't care about monitor really, just animate out.
                         if should_show {
                             let prev = PREVIOUS_FOCUS.lock().unwrap();
                             if let Some(h) = *prev {
                                 MonitorFromWindow(h.0, MONITOR_DEFAULTTONEAREST)
                             } else {
                                  let mut cursor_pos = POINT { x: 0, y: 0 };
                                  let _ = GetCursorPos(&mut cursor_pos);
                                  MonitorFromPoint(cursor_pos, MONITOR_DEFAULTTONEAREST)
                             }
                         } else {
                             // Hiding: Use current monitor
                             MonitorFromWindow(hwnd.inner(), MONITOR_DEFAULTTONEAREST)
                         }
                    },
                    _ => {
                        // "follow-mouse" or default
                        let mut cursor_pos = POINT { x: 0, y: 0 };
                        let _ = GetCursorPos(&mut cursor_pos);
                        MonitorFromPoint(cursor_pos, MONITOR_DEFAULTTONEAREST)
                    }
                 }
            };

            let mut mi = MONITORINFO {
                cbSize: std::mem::size_of::<MONITORINFO>() as u32,
                ..Default::default()
            };

            if !GetMonitorInfoW(monitor, &mut mi).as_bool() {
                 return;
            }

            let work_area = mi.rcWork;
            let screen_w = work_area.right - work_area.left;
            let screen_h = work_area.bottom - work_area.top;

            let width_pct = (screen_w as f64 * (config.window.width_percent as f64 / 100.0)) as i32;
            let height_pct = (screen_h as f64 * (config.window.height_percent as f64 / 100.0)) as i32;

            // Target dimensions
            let (target_w, target_h) = if config.window.width_cols > 0 && config.window.height_rows > 0 {
                  // Use existing logic
                 let mut r = RECT::default();
                 if GetWindowRect(hwnd.inner(), &mut r).is_ok() {
                      (r.right - r.left, r.bottom - r.top)
                 } else {
                      (width_pct, height_pct)
                 }
            } else {
                 (width_pct, height_pct)
            };

            let width = target_w;
            let height = target_h;
            let target_x = work_area.left + (screen_w - width) / 2;
            let shown_y = work_area.top;
            let hidden_y = work_area.top - height;

            let target_y = if should_show { shown_y } else { hidden_y };

            // Ensure styles
            let ex_style = GetWindowLongW(hwnd.inner(), GWL_EXSTYLE);
            if (ex_style & WS_EX_LAYERED.0 as i32) == 0 {
                 SetWindowLongW(hwnd.inner(), GWL_EXSTYLE, ex_style | WS_EX_LAYERED.0 as i32);
            }

            // Current Pos
            let mut rect = RECT::default();
            let current_y = if GetWindowRect(hwnd.inner(), &mut rect).is_ok() {
                 rect.top
            } else {
                 if should_show { hidden_y } else { shown_y }
            };

            // If we are showing and current_y is far off (different monitor maybe?), snap to hidden_y on target monitor
            let start_y = if should_show {
                // If rect.left isn't close to target_x, we moved monitors.
                // Snap to hidden_y.
                if (rect.left - target_x).abs() > 500 {
                    hidden_y
                } else {
                    current_y
                }
            } else {
                current_y
            };

            let dist_y = target_y - start_y;

            let duration_ms = if should_show { config.animation.show_duration } else { config.animation.hide_duration } as u64;
            let easing_type = if should_show { &config.animation.show_easing } else { &config.animation.hide_easing };
            let opacity_point = if should_show { config.animation.show_opacity_point } else { config.animation.hide_opacity_point };
            let animate_opacity = config.animation.animate_opacity;

            let z_flag = SendHwnd(if config.window.keep_above { HWND_TOPMOST } else { HWND_NOTOPMOST });

            let start_time = Instant::now();
            let mut interval = interval(Duration::from_millis(16));

            loop {
                interval.tick().await;
                let elapsed = start_time.elapsed().as_millis() as f64;
                let progress = (elapsed / duration_ms as f64).min(1.0);

                let ease_val = get_easing(progress, easing_type);
                let new_y = start_y + (dist_y as f64 * ease_val) as i32;

                 let mut alpha = 255;
                 if animate_opacity {
                     let opacity_progress = if should_show {
                          (progress / opacity_point).min(1.0)
                     } else {
                          if progress < opacity_point { 0.0 } else { (progress - opacity_point) / (1.0 - opacity_point) }
                     };

                     let opacity_val = if should_show {
                          get_easing(opacity_progress, "linear")
                     } else {
                          1.0 - get_easing(opacity_progress, "linear")
                     };
                     alpha = (opacity_val * 255.0) as u8;
                 }

                let _ = SetLayeredWindowAttributes(hwnd.inner(), COLORREF(0), alpha, LWA_ALPHA);
                let _ = SetWindowPos(hwnd.inner(), z_flag.0, target_x, new_y, width, height, SWP_NOACTIVATE | SWP_SHOWWINDOW);

                if progress >= 1.0 { break; }
            }

            // Finalize
            if should_show {
                 let _ = SetLayeredWindowAttributes(hwnd.inner(), COLORREF(0), 255, LWA_ALPHA);
                 let _ = SetWindowPos(hwnd.inner(), z_flag.0, target_x, target_y, width, height, SWP_SHOWWINDOW | SWP_NOACTIVATE);
            } else {
                 let _ = ShowWindow(hwnd.inner(), SW_HIDE);
                 // Focus already restored synchronously at start for speed.
            }
        }
    });

    {
        let mut task_handle = ANIMATION_TASK.lock().unwrap();
        *task_handle = Some(handle.abort_handle());
    }
}

pub fn restore_window_visibility(config: &Config) {
    println!("DEBUG: restore_window_visibility started");
    let start = std::time::Instant::now();

    if let Some(hwnd) = find_window_by_process(&config.general.window_class) {
        let is_target_visible = *TARGET_VISIBLE.lock().unwrap();
        println!("DEBUG: Found window HWND: {:?}, is_target_visible={}", hwnd, is_target_visible);

        unsafe {
            // 1. Ensure Opacity is 255 (Opaque)
            let _ = SetLayeredWindowAttributes(hwnd, COLORREF(0), 255, LWA_ALPHA);

            // 2. Determine if we need to move the window
            // If it was hidden, move it to the visible work area.
            // If it was visible, leave it where it is.
            let (x, y, flags) = if is_target_visible {
                (0, 0, windows::Win32::UI::WindowsAndMessaging::SWP_NOMOVE | SWP_NOSIZE | SWP_SHOWWINDOW | SWP_NOACTIVATE)
            } else {
                let monitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
                let mut mi = MONITORINFO {
                    cbSize: std::mem::size_of::<MONITORINFO>() as u32,
                    ..Default::default()
                };
                if GetMonitorInfoW(monitor, &mut mi).as_bool() {
                    (mi.rcWork.left, mi.rcWork.top, SWP_NOSIZE | SWP_SHOWWINDOW | SWP_NOACTIVATE)
                } else {
                    (0, 0, SWP_NOSIZE | SWP_SHOWWINDOW | SWP_NOACTIVATE)
                }
            };

            // 3. Ensure visible, opaque, and NOT topmost.
            let _ = SetWindowPos(hwnd, HWND_NOTOPMOST, x, y, 0, 0, flags);

            // Ensure not minimized
            if IsIconic(hwnd).as_bool() {
                 let _ = ShowWindow(hwnd, SW_RESTORE);
            } else {
                 let _ = ShowWindow(hwnd, SW_SHOW);
            }

             let _ = SetForegroundWindow(hwnd);
        }
    } else {
        println!("DEBUG: No window found to restore!");
    }
    println!("DEBUG: restore_window_visibility output took {:?}", start.elapsed());
}
