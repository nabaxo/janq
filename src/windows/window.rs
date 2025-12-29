use crate::config::Config;
use crate::windows::easing::get_easing;
use windows::Win32::Foundation::{BOOL, HWND, LPARAM, COLORREF, RECT, POINT};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetWindowThreadProcessId, IsWindowVisible, ShowWindow, SetForegroundWindow, GetForegroundWindow,
    SetWindowPos, SW_HIDE, HWND_TOPMOST, HWND_NOTOPMOST, SWP_SHOWWINDOW, SWP_NOACTIVATE,
    GetLayeredWindowAttributes, SetLayeredWindowAttributes, GetWindowLongW, SetWindowLongW, GWL_EXSTYLE, WS_EX_LAYERED, LWA_ALPHA,
    GetCursorPos, GetWindowRect, IsIconic, SW_SHOW, SWP_NOSIZE, SWP_NOCOPYBITS, SWP_DEFERERASE, SWP_NOMOVE, SWP_FRAMECHANGED
};
use windows::Win32::Graphics::Dwm::DwmFlush;
use windows::Win32::Graphics::Gdi::{
    MonitorFromPoint, GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromWindow, HMONITOR, HDC, EnumDisplayMonitors
};
use windows::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_INFORMATION, PROCESS_VM_READ};
use windows::Win32::System::ProcessStatus::GetModuleBaseNameW;
use std::ffi::OsString;
use std::os::windows::ffi::OsStrExt;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::time::Instant;

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
    static ref TARGET_VISIBLE: AtomicBool = AtomicBool::new(false);
    static ref PREVIOUS_FOCUS: std::sync::Mutex<Option<SendHwnd>> = std::sync::Mutex::new(None);
    static ref LAST_MONITOR: std::sync::Mutex<Option<isize>> = std::sync::Mutex::new(None);
    static ref CACHED_RUAKE_HWND: std::sync::Mutex<Option<SendHwnd>> = std::sync::Mutex::new(None);
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

pub async fn toggle_window(config: &Config) -> bool {
    println!("Toggling window...");

    let should_show = !TARGET_VISIBLE.load(Ordering::Relaxed);
    TARGET_VISIBLE.store(should_show, Ordering::Relaxed);

    {
        let mut task_handle = ANIMATION_TASK.lock().unwrap();
        if let Some(handle) = task_handle.take() {
            handle.abort();
        }
    }

    // Find the SendHwnd wrapper
    // Check cache first
    let mut cached_hwnd = None;
    {
        let cache = CACHED_RUAKE_HWND.lock().unwrap();
        if let Some(h) = *cache {
             unsafe {
                 if windows::Win32::UI::WindowsAndMessaging::IsWindow(h.inner()).as_bool() {
                     cached_hwnd = Some(h);
                 }
             }
        }
    }

    let hwnd = if let Some(h) = cached_hwnd {
        h
    } else {
        match find_window_by_process(&config.general.window_class) {
            Some(h) => {
                let mut cache = CACHED_RUAKE_HWND.lock().unwrap();
                let wrapper = SendHwnd(h);
                *cache = Some(wrapper);
                wrapper
            },
            None => {
                println!("Window not found for process: {}", config.general.window_class);
                return false;
            }
        }
    };

    // Unconditionally capture valid foreground window
    // This handles:
    // 1. SHOW: Captures the app you are working on.
    // 2. HIDE: If you clicked another window (B) while Ruake was open, captures B.
    // 3. HIDE: If Ruake is focused, ignores it (preserves previous capture).
    unsafe {
        let fg_window = GetForegroundWindow();
        if fg_window.0 != std::ptr::null_mut() && fg_window != hwnd.inner() {
            let mut prev = PREVIOUS_FOCUS.lock().unwrap();
            *prev = Some(SendHwnd(fg_window));
        }
    }

    // Synchronously handle Initial Visibility
    unsafe {
        if should_show {
            // No focus capture here - we capture when hiding instead!

            // Immediately activate (steal focus)
            let _ = SetForegroundWindow(hwnd.inner());

            // Only ShowWindow if it's not already visible to reduce flicker
            if !IsWindowVisible(hwnd.inner()).as_bool() {
                let _ = ShowWindow(hwnd.inner(), SW_SHOW);
            }
        }
    }

    let config = config.clone();

    let handle = tokio::spawn(async move {
        // Use the SendHwnd wrapper inside the async block
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
                    },
                    "active" => {
                        let prev = PREVIOUS_FOCUS.lock().unwrap();
                        if let Some(h) = *prev {
                            MonitorFromWindow(h.0, MONITOR_DEFAULTTONEAREST)
                        } else {
                            let mut cursor_pos = POINT { x: 0, y: 0 };
                            let _ = GetCursorPos(&mut cursor_pos);
                            MonitorFromPoint(cursor_pos, MONITOR_DEFAULTTONEAREST)
                        }
                    },
                    _ => {
                        let mut cursor_pos = POINT { x: 0, y: 0 };
                        let _ = GetCursorPos(&mut cursor_pos);
                        MonitorFromPoint(cursor_pos, MONITOR_DEFAULTTONEAREST)
                    }
                }
            } else {
                MonitorFromWindow(hwnd.inner(), MONITOR_DEFAULTTONEAREST)
            };

            // STRICT PERSISTENCE LOGIC:
            // 1. If Showing: Update LAST_MONITOR and use current choice.
            // 2. If Hiding: Use LAST_MONITOR if available (ignore mouse).
            let final_monitor = if should_show {
                let mut last = LAST_MONITOR.lock().unwrap();
                *last = Some(monitor.0 as isize);
                monitor
            } else {
                let last = LAST_MONITOR.lock().unwrap();
                if let Some(h) = *last {
                    HMONITOR(h as *mut std::ffi::c_void)
                } else {
                    monitor
                }
            };

            let mut mi = MONITORINFO {
                cbSize: std::mem::size_of::<MONITORINFO>() as u32,
                ..Default::default()
            };
            if !GetMonitorInfoW(final_monitor, &mut mi).as_bool() { return; }

            let work_area = mi.rcWork;
            let screen_w = work_area.right - work_area.left;
            let screen_h = work_area.bottom - work_area.top;

            let width_pct = (screen_w as f64 * (config.window.width_percent as f64 / 100.0)) as i32;
            let height_pct = (screen_h as f64 * (config.window.height_percent as f64 / 100.0)) as i32;

            let (width, height) = if config.window.width_cols > 0 && config.window.height_rows > 0 {
                let mut r = RECT::default();
                if GetWindowRect(hwnd.inner(), &mut r).is_ok() {
                    (r.right - r.left, r.bottom - r.top)
                } else {
                    (width_pct, height_pct)
                }
            } else {
                (width_pct, height_pct)
            };

            let target_x = work_area.left + (screen_w - width) / 2;
            let shown_y = work_area.top;
            let hidden_y = work_area.top - height;
            let target_y = if should_show { shown_y } else { hidden_y };

            // Styles & State Capture
            let ex_style = GetWindowLongW(hwnd.inner(), GWL_EXSTYLE);
            if (ex_style & WS_EX_LAYERED.0 as i32) == 0 {
                SetWindowLongW(hwnd.inner(), GWL_EXSTYLE, ex_style | WS_EX_LAYERED.0 as i32);
                // Important: Notify the system that the frame has changed to apply the new style
                let _ = SetWindowPos(hwnd.inner(), HWND::default(), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_FRAMECHANGED);
                // Initialize the layered state
                let _ = SetLayeredWindowAttributes(hwnd.inner(), COLORREF(0), 255, LWA_ALPHA);
            }

            let mut rect = RECT::default();
            let mut current_alpha: u8 = 255;
            // Best effort to get current alpha, but if it fails, assume 255
            let _ = GetLayeredWindowAttributes(hwnd.inner(), None, Some(&mut current_alpha), None);
            let _ = GetWindowRect(hwnd.inner(), &mut rect);

            let start_y = if (rect.left - target_x).abs() > 500 {
                if should_show { hidden_y } else { shown_y }
            } else {
                rect.top
            };

            let start_alpha = current_alpha;

            let dist_y = target_y - start_y;

            if !config.animation.animate_opacity {
                let _ = SetLayeredWindowAttributes(hwnd.inner(), COLORREF(0), 255, LWA_ALPHA);
            }

            let full_duration = if should_show { config.animation.show_duration } else { config.animation.hide_duration };
            let dynamic_duration = if dist_y.abs() > 0 {
                (dist_y.abs() as f64 / height as f64) * (full_duration as f64 / 1000.0)
            } else {
                0.0
            };

            let easing_type = if should_show { &config.animation.show_easing } else { &config.animation.hide_easing };
            let z_flag = SendHwnd(if config.window.keep_above { HWND_TOPMOST } else { HWND_NOTOPMOST });

            let start_time = Instant::now();
            let opacity_point = if should_show { config.animation.show_opacity_point } else { config.animation.hide_opacity_point };

            if dynamic_duration > 0.0 {
                loop {
                    let _ = DwmFlush();
                    tokio::task::yield_now().await;

                    if TARGET_VISIBLE.load(Ordering::Relaxed) != should_show { return; }

                    let elapsed = start_time.elapsed().as_secs_f64();
                    let progress = (elapsed / dynamic_duration).min(1.0);
                    let ease_val = get_easing(progress, easing_type);

                    let new_y = start_y + (dist_y as f64 * ease_val) as i32;

                    if config.animation.animate_opacity {
                        // Opacity points control when opacity animation completes relative to position animation
                        // 0.0 = opacity completes immediately (no fade)
                        // 1.0 = opacity completes at end (fade throughout entire animation)
                        // Clamp to prevent extreme values but allow full range
                        let safe_opacity_point = opacity_point.clamp(0.0, 1.0);

                        let opacity_progress = if should_show {
                            // When showing: fade in proportionally to progress
                            // If opacity_point = 1.0, fade matches position animation
                            // If opacity_point = 0.5, fade completes at 50% of animation
                            if safe_opacity_point > 0.0 {
                                (progress / safe_opacity_point).min(1.0)
                            } else {
                                1.0 // Instant fade if opacity_point is 0
                            }
                        } else {
                            // When hiding: fade out proportionally
                            // If opacity_point = 1.0, fade matches position animation
                            // If opacity_point = 0.5, fade starts at 50% of animation
                            let fade_start = 1.0 - safe_opacity_point;
                            if progress <= fade_start {
                                0.0 // No fade yet
                            } else if safe_opacity_point > 0.0 {
                                ((progress - fade_start) / safe_opacity_point).min(1.0)
                            } else {
                                1.0 // Instant fade if opacity_point is 0
                            }
                        };

                        let new_alpha = if should_show {
                             (start_alpha as f64 + (255.0 - start_alpha as f64) * opacity_progress).clamp(0.0, 255.0) as u8
                        } else {
                             (start_alpha as f64 * (1.0 - opacity_progress)).clamp(0.0, 255.0) as u8
                        };
                        let _ = SetLayeredWindowAttributes(hwnd.inner(), COLORREF(0), new_alpha, LWA_ALPHA);
                    }

                    let _ = SetWindowPos(hwnd.inner(), z_flag.0, target_x, new_y, 0, 0, SWP_NOACTIVATE | SWP_NOCOPYBITS | SWP_DEFERERASE | SWP_NOSIZE);

                    if progress >= 1.0 { break; }
                }
            }

            // Finalize
            if should_show {
                let _ = SetLayeredWindowAttributes(hwnd.inner(), COLORREF(0), 255, LWA_ALPHA);
                let _ = SetWindowPos(hwnd.inner(), z_flag.0, target_x, target_y, width, height, SWP_SHOWWINDOW | SWP_NOACTIVATE | SWP_NOCOPYBITS);
            } else {


                let _ = ShowWindow(hwnd.inner(), SW_HIDE);
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

    {
        let mut task_handle = ANIMATION_TASK.lock().unwrap();
        *task_handle = Some(handle.abort_handle());
    }
    true
}

pub fn restore_window_visibility(config: &Config) {

    if let Some(hwnd) = find_window_by_process(&config.general.window_class) {
        let is_target_visible = TARGET_VISIBLE.load(Ordering::Relaxed);

        unsafe {
            // 1. Ensure Layered Style & Opacity is 255 (Opaque)
            let ex_style = GetWindowLongW(hwnd, GWL_EXSTYLE);
            if (ex_style & WS_EX_LAYERED.0 as i32) == 0 {
                SetWindowLongW(hwnd, GWL_EXSTYLE, ex_style | WS_EX_LAYERED.0 as i32);
                let _ = SetWindowPos(hwnd, HWND::default(), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_FRAMECHANGED);
            }
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

            // Ensure not minimized without stealing focus
            if IsIconic(hwnd).as_bool() {
                 let _ = ShowWindow(hwnd, windows::Win32::UI::WindowsAndMessaging::SW_SHOWNOACTIVATE);
            } else {
                 let _ = ShowWindow(hwnd, windows::Win32::UI::WindowsAndMessaging::SW_SHOWNA);
            }
        }
    }
}
