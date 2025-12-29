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

    let config = config.clone();

    let handle = tokio::spawn(async move {
        // Use the SendHwnd wrapper inside the async block

        unsafe {
            // Determine current state/position
            let monitor = if should_show {
                 let fg_window = GetForegroundWindow();
                 if fg_window.0 != std::ptr::null_mut() && fg_window.0 != hwnd.inner().0 {
                      let mut prev = PREVIOUS_FOCUS.lock().unwrap();
                      if prev.is_none() {
                          *prev = Some(SendHwnd(fg_window));
                      }
                 }

                // Determine target monitor based on config
                match config.window.display_mode.as_str() {
                    "specific" => {
                        let mut ctx = MonitorEnumCtx { monitors: Vec::new() };
                        let _ = EnumDisplayMonitors(None, None, Some(monitor_enum_proc), LPARAM(&mut ctx as *mut _ as isize));
                        if (config.window.display_index as usize) < ctx.monitors.len() {
                             ctx.monitors[config.window.display_index as usize]
                        } else {
                             // Fallback to primary/mouse
                             let mut cursor_pos = POINT { x: 0, y: 0 };
                             let _ = GetCursorPos(&mut cursor_pos);
                             MonitorFromPoint(cursor_pos, MONITOR_DEFAULTTONEAREST)
                        }
                    },
                    "active" => {
                        let fg_window = GetForegroundWindow();
                        if fg_window.0 != std::ptr::null_mut() {
                            MonitorFromWindow(fg_window, MONITOR_DEFAULTTONEAREST)
                        } else {
                             // Fallback
                            let mut cursor_pos = POINT { x: 0, y: 0 };
                            let _ = GetCursorPos(&mut cursor_pos);
                            MonitorFromPoint(cursor_pos, MONITOR_DEFAULTTONEAREST)
                        }
                    },
                    _ => {
                        // "follow-mouse" or default
                        let mut cursor_pos = POINT { x: 0, y: 0 };
                        let _ = GetCursorPos(&mut cursor_pos);
                        MonitorFromPoint(cursor_pos, MONITOR_DEFAULTTONEAREST)
                    }
                }
            } else {
                // If HIDING: Stay on current monitor
                MonitorFromWindow(hwnd.inner(), MONITOR_DEFAULTTONEAREST)
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
                // We trust the terminal to have these dimensions roughly, or we force it?
                // Actually if we just resize the window rect it might work but terminals handle resize events.
                // Let's stick to using current window dims if they exist and are reasonable?
                // For now use pct or existing
                 let mut r = RECT::default();
                 if GetWindowRect(hwnd.inner(), &mut r).is_ok() {
                      (r.right - r.left, r.bottom - r.top)
                 } else {
                      (width_pct, height_pct)
                 }
            } else {
                 (width_pct, height_pct)
            };

            // Re-calculate target W/H if we want to enforce config
            // But usually we respect current size if it's already open.
            let width = target_w;
            let height = target_h;

            let target_x = work_area.left + (screen_w - width) / 2;

            // Target Y (Show) vs Hidden Y
            let shown_y = work_area.top;
            let hidden_y = work_area.top - height;

            let target_y = if should_show { shown_y } else { hidden_y };

            // Ensure window styles
            let ex_style = GetWindowLongW(hwnd.inner(), GWL_EXSTYLE);
            if (ex_style & WS_EX_LAYERED.0 as i32) == 0 {
                 SetWindowLongW(hwnd.inner(), GWL_EXSTYLE, ex_style | WS_EX_LAYERED.0 as i32);
            }

            // Get Current Position
            let mut rect = RECT::default();
            let current_y = if GetWindowRect(hwnd.0, &mut rect).is_ok() {
                 rect.top
            } else {
                 if should_show { hidden_y } else { shown_y }
            };

            // If we are showing but we are way off (e.g. user moved monitor), prevent jumping?
            // If rect.top is far away, rely on hidden_y
            // But if we are interrupting, current_y should be valid.

            // Animation Params
            let duration_ms = if should_show { config.animation.show_duration } else { config.animation.hide_duration } as u64;
            let easing_type = if should_show { &config.animation.show_easing } else { &config.animation.hide_easing };
            let opacity_point = if should_show { config.animation.show_opacity_point } else { config.animation.hide_opacity_point };
            let animate_opacity = config.animation.animate_opacity;

            // Determine Z-Order flag
            let z_flag = SendHwnd(if config.window.keep_above { HWND_TOPMOST } else { HWND_NOTOPMOST });

            // Ensure window is visible initially if we are showing
            if should_show {
                 let _ = SetWindowPos(hwnd.inner(), z_flag.0, target_x, current_y, width, height, SWP_SHOWWINDOW | SWP_NOACTIVATE);
            }
            // If hiding, we are already visible, just moving.

            let start_y = current_y;
            let dist_y = target_y - start_y; // Can be positive (down) or negative (up)

            let start_time = Instant::now();
            let mut interval = interval(Duration::from_millis(16)); // ~60 FPS

            loop {
                interval.tick().await;
                let elapsed = start_time.elapsed().as_millis() as f64;
                let progress = (elapsed / duration_ms as f64).min(1.0);

                let ease_val = get_easing(progress, easing_type);
                let new_y = start_y + (dist_y as f64 * ease_val) as i32;

                // Opacity Calculation
                 let mut alpha = 255;
                 if animate_opacity {
                     // We need to map position to opacity roughly? Or just time?
                     // Currently mapping time.
                     let opacity_progress = if should_show {
                          // Fade In
                          (progress / opacity_point).min(1.0)
                     } else {
                          // Fade Out
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
                let _ = SetWindowPos(hwnd.inner(), z_flag.0, target_x, new_y, width, height, SWP_NOACTIVATE);

                if progress >= 1.0 {
                     break;
                }
            }

            // Finalize State
            if should_show {
                 // Fully Shown
                 let _ = SetLayeredWindowAttributes(hwnd.inner(), COLORREF(0), 255, LWA_ALPHA);
                 let _ = SetWindowPos(hwnd.inner(), z_flag.0, target_x, target_y, width, height, SWP_SHOWWINDOW);
                 if !SetForegroundWindow(hwnd.inner()).as_bool() {
                      println!("ERROR: SetForegroundWindow failed! Error: {:?}", windows::core::Error::from_win32());
                 }
            } else {
                 // Fully Hidden
                 let _ = ShowWindow(hwnd.inner(), SW_HIDE);
                 // Restore Focus
                 let mut prev = PREVIOUS_FOCUS.lock().unwrap();
                 if let Some(h) = *prev {
                     if IsWindowVisible(h.0).as_bool() {
                         let _ = SetForegroundWindow(h.0);
                     }
                     *prev = None;
                 }
            }
        }
    });

    // Store handle
    {
        let mut task_handle = ANIMATION_TASK.lock().unwrap();
        *task_handle = Some(handle.abort_handle());
    }
}

pub fn restore_window_visibility(config: &Config) {
    println!("DEBUG: restore_window_visibility started");
    let start = std::time::Instant::now();

    if let Some(hwnd) = find_window_by_process(&config.general.window_class) {
        println!("DEBUG: Found window HWND: {:?}, finding monitor...", hwnd);

        unsafe {
            // 2. Find Monitor to restore to (Primary or current)
            let monitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
            let mut mi = MONITORINFO {
                cbSize: std::mem::size_of::<MONITORINFO>() as u32,
                ..Default::default()
            };

            let (x, y) = if GetMonitorInfoW(monitor, &mut mi).as_bool() {
                // Restore to top-left of work area to ensure visibility
                (mi.rcWork.left, mi.rcWork.top)
            } else {
                (0, 0) // Fallback
            };

            println!("DEBUG: Restoring to x={}, y={}", x, y);

            // 1. Ensure Opacity is 255 (Opaque)
            // Note: GWL_EXSTYLE must have WS_EX_LAYERED for this to work, but if we want to be safe,
            // maybe we REMOVE WS_EX_LAYERED to force opacity?
            // Actually, setting 255 is safe if LAYERED is set.
            let _ = SetLayeredWindowAttributes(hwnd, COLORREF(0), 255, LWA_ALPHA);

            // 3. Force Move to visual area and Show
            // Use SW_SHOW (5) instead of SW_RESTORE (9) to force visibility without "restoring" min/max animation if possible?
            // Actually SW_RESTORE is good if it was minimized. SW_SHOW doesn't restore from Minimize.
            // Let's use ShowWindowAsync if we want to be fast? No, we want to ensure it happens before we exit.

            // 3. Force Move to visual area and Show
            // We use HWND_NOTOPMOST so it doesn't get stuck on top of everything after we quit.
            let _ = SetWindowPos(hwnd, HWND_NOTOPMOST, x, y, 0, 0,
                SWP_NOSIZE | SWP_SHOWWINDOW
            );

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
