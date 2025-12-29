use crate::config::Config;
use crate::windows::easing::get_easing;
use windows::Win32::Foundation::{BOOL, HWND, LPARAM, COLORREF, RECT, POINT};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetWindowThreadProcessId, IsWindowVisible, ShowWindow, SetForegroundWindow, GetForegroundWindow,
    SetWindowPos, SW_HIDE, HWND_TOPMOST, HWND_NOTOPMOST, SWP_SHOWWINDOW, SWP_NOACTIVATE,
    SetLayeredWindowAttributes, GetWindowLongW, SetWindowLongW, GWL_EXSTYLE, WS_EX_LAYERED, LWA_ALPHA, SW_SHOWNOACTIVATE,
    GetCursorPos, GetWindowRect, IsIconic, SW_SHOW, SW_RESTORE, SWP_NOSIZE, SWP_NOMOVE
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

lazy_static::lazy_static! {
    static ref IS_ANIMATING: std::sync::Mutex<bool> = std::sync::Mutex::new(false);
    static ref PREVIOUS_FOCUS: std::sync::Mutex<Option<SendHwnd>> = std::sync::Mutex::new(None);
}

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

    // Heuristic: Find the "best" window
    // 1. Must have dimensions > 0 (avoid message-only/utility windows)
    // 2. Prefer Visible windows (if any)

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

                    // Simple tie-breaker: larger area?
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

// Helper for Specific Display Mode
struct MonitorEnumCtx {
    monitors: Vec<HMONITOR>,
}
unsafe extern "system" fn monitor_enum_proc(hmonitor: HMONITOR, _hdc: HDC, _rect: *mut RECT, lparam: LPARAM) -> BOOL {
    let ctx = &mut *(lparam.0 as *mut MonitorEnumCtx);
    ctx.monitors.push(hmonitor);
    BOOL(1)
}

struct AnimationGuard;

impl AnimationGuard {
    fn new() -> Option<Self> {
        let mut animating = IS_ANIMATING.lock().unwrap();
        if *animating {
            return None;
        }
        *animating = true;
        Some(Self)
    }
}

impl Drop for AnimationGuard {
    fn drop(&mut self) {
        let mut animating = IS_ANIMATING.lock().unwrap();
        *animating = false;
    }
}

pub async fn toggle_window(config: &Config) {
    // Acquire RAII guard for animation state
    // If already animating, this returns None and we exit
    let _guard = match AnimationGuard::new() {
        Some(g) => g,
        None => return,
    };

    println!("Toggling window...");

    let hwnd = match find_window_by_process(&config.general.window_class) {
        Some(h) => SendHwnd(h),
        None => {
            println!("Window not found for process: {}", config.general.window_class);
            return;
        }
    };

    unsafe {
        let is_visible = IsWindowVisible(hwnd.0).as_bool();

        let monitor = if is_visible {
            // IF HIDING: Stay on current monitor
            MonitorFromWindow(hwnd.0, MONITOR_DEFAULTTONEAREST)
        } else {
            // IF SHOWING: Capture previous focus first
            let fg_window = GetForegroundWindow();
            if fg_window.0 != std::ptr::null_mut() && fg_window.0 != hwnd.0.0 {
                 let mut prev = PREVIOUS_FOCUS.lock().unwrap();
                 *prev = Some(SendHwnd(fg_window));
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
                         // Fallback to primary/mouse if no active window found (shouldn't happen often)
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

        // Check if window has existing dimensions (user might have resized it)
        let mut rect = RECT::default();
        let (width, height, current_x) = if GetWindowRect(hwnd.0, &mut rect).is_ok() {
             (rect.right - rect.left, rect.bottom - rect.top, rect.left)
        } else {
             (width_pct, height_pct, work_area.left + (screen_w - width_pct) / 2)
        };

        let x = if is_visible {
             // If hiding, stay at current X
             current_x
        } else {
             // If showing, center on target monitor
             work_area.left + (screen_w - width) / 2
        };

        // Target Y when fully shown
        let target_y = work_area.top;
        // Target Y when hidden (above screen)
        let hidden_y = work_area.top - height;

        // Ensure window has WS_EX_LAYERED style for opacity
        let ex_style = GetWindowLongW(hwnd.0, GWL_EXSTYLE);
        if (ex_style & WS_EX_LAYERED.0 as i32) == 0 {
             SetWindowLongW(hwnd.0, GWL_EXSTYLE, ex_style | WS_EX_LAYERED.0 as i32);
        }

        let duration_ms = if is_visible { config.animation.hide_duration } else { config.animation.show_duration } as u64;
        let easing_type = if is_visible { &config.animation.hide_easing } else { &config.animation.show_easing };
        let opacity_point = if is_visible { config.animation.hide_opacity_point } else { config.animation.show_opacity_point };
        let animate_opacity = config.animation.animate_opacity;

        // Determine Z-Order flag
        let z_flag = SendHwnd(if config.window.keep_above { HWND_TOPMOST } else { HWND_NOTOPMOST });

        let start_time = Instant::now();
        let mut interval = interval(Duration::from_millis(16)); // ~60 FPS

        // Initial Setup
        if !is_visible {
             // SHOWING
             // Set initial pos (hidden) and show
             let _ = SetWindowPos(hwnd.0, z_flag.0, x, hidden_y, width, height, SWP_SHOWWINDOW | SWP_NOACTIVATE);
             let _ = SetLayeredWindowAttributes(hwnd.0, COLORREF(0), 0, LWA_ALPHA); // Start transparent
             let _ = ShowWindow(hwnd.0, SW_SHOWNOACTIVATE); // Show without stealing focus yet?
             // Actually we want focus eventually, but let's animate first
        }

        let start_y = if is_visible { target_y } else { hidden_y };
        let end_y = if is_visible { hidden_y } else { target_y };
        let dist_y = end_y - start_y;

        loop {
            interval.tick().await;
            let elapsed = start_time.elapsed().as_millis() as f64;
            let progress = (elapsed / duration_ms as f64).min(1.0);

            let ease_val = get_easing(progress, easing_type);
            let current_y = start_y + (dist_y as f64 * ease_val) as i32;

            // Opacity Logic
            let mut alpha = 255;
            if animate_opacity {
                let opacity_progress = if !is_visible {
                     // Fading In (Show)
                     (progress / opacity_point).min(1.0)
                } else {
                     // Fading Out (Hide)
                     // Starts fading at opacity_point
                     if progress < opacity_point { 0.0 } else { (progress - opacity_point) / (1.0 - opacity_point) }
                };

                let opacity_val = if !is_visible {
                     // Show: 0 -> 1
                     get_easing(opacity_progress, "linear") // Linear fade usually feels best
                } else {
                     // Hide: 1 -> 0
                     1.0 - get_easing(opacity_progress, "linear")
                };
                alpha = (opacity_val * 255.0) as u8;
            }

            // Apply updates
            let _ = SetLayeredWindowAttributes(hwnd.0, COLORREF(0), alpha, LWA_ALPHA);
            let _ = SetWindowPos(hwnd.0, z_flag.0, x, current_y, width, height, SWP_NOACTIVATE);

            if progress >= 1.0 {
                 break;
            }
        }

        // Finalize
        if is_visible {
             // HIDDEN
             let _ = ShowWindow(hwnd.0, SW_HIDE);
             // Restore Focus
             let mut prev = PREVIOUS_FOCUS.lock().unwrap();
             if let Some(h) = *prev {
                 if IsWindowVisible(h.0).as_bool() {
                     let _ = SetForegroundWindow(h.0);
                 }
                 *prev = None;
             }
        } else {
             // SHOWN
             let _ = SetLayeredWindowAttributes(hwnd.0, COLORREF(0), 255, LWA_ALPHA);
             let _ = SetWindowPos(hwnd.0, z_flag.0, x, target_y, width, height, SWP_SHOWWINDOW);
             if !SetForegroundWindow(hwnd.0).as_bool() {
                  println!("ERROR: SetForegroundWindow failed! Error: {:?}", windows::core::Error::from_win32());
             } else {
                  println!("DEBUG: SetForegroundWindow success");
             }
        }
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
