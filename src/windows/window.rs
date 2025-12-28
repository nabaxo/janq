use crate::config::Config;
use crate::windows::easing::get_easing;
use windows::Win32::Foundation::{BOOL, HWND, LPARAM, COLORREF};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetWindowThreadProcessId, IsWindowVisible, ShowWindow, SetForegroundWindow,
    SetWindowPos, SW_HIDE, HWND_TOPMOST, SWP_SHOWWINDOW, SWP_NOACTIVATE,
    SetLayeredWindowAttributes, GetWindowLongW, SetWindowLongW, GWL_EXSTYLE, WS_EX_LAYERED, LWA_ALPHA, SW_SHOWNOACTIVATE
};
use windows::Win32::Graphics::Gdi::{
    MonitorFromPoint, GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO
};
use windows::Win32::Foundation::POINT;
use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;
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
}

// Helper to convert string to wide string for Windows API
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
                target_struct.found_hwnd = Some(hwnd);
                return BOOL(0); // Stop enumeration
            }
        }
    }

    BOOL(1) // Continue
}

struct TargetSearch {
    name: String,
    found_hwnd: Option<HWND>,
}

fn find_window_by_process(name: &str) -> Option<HWND> {
    let mut search = TargetSearch {
        name: name.to_string(),
        found_hwnd: None,
    };

    unsafe {
        let _ = EnumWindows(Some(enum_windows_proc), LPARAM(&mut search as *mut _ as isize));
    }

    search.found_hwnd
}

pub async fn toggle_window(config: &Config) {
    // Prevent overlapping animations
    {
        let mut animating = IS_ANIMATING.lock().unwrap();
        if *animating {
             return;
        }
        *animating = true;
    }
    // Ensure lock is dropped before await loop

    let hwnd = match find_window_by_process(&config.window_class) {
        Some(h) => SendHwnd(h),
        None => {
            println!("Window not found for process: {}", config.window_class);
            let mut animating = IS_ANIMATING.lock().unwrap();
            *animating = false;
            return;
        }
    };

    unsafe {
        let is_visible = IsWindowVisible(hwnd.0).as_bool();


        // Use cursor position to determine target monitor
        let mut cursor_pos = POINT { x: 0, y: 0 };
        let _ = GetCursorPos(&mut cursor_pos);

        let monitor = MonitorFromPoint(cursor_pos, MONITOR_DEFAULTTONEAREST);
        let mut mi = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };

        if !GetMonitorInfoW(monitor, &mut mi).as_bool() {
             let mut animating = IS_ANIMATING.lock().unwrap();
             *animating = false;
             return;
        }

        let work_area = mi.rcWork;
        let screen_w = work_area.right - work_area.left;
        let screen_h = work_area.bottom - work_area.top;

        let width = (screen_w as f64 * (config.width_percent as f64 / 100.0)) as i32;
        let height = (screen_h as f64 * (config.height_percent as f64 / 100.0)) as i32;
        let x = work_area.left + (screen_w - width) / 2;
        // Target Y when fully shown
        let target_y = work_area.top;
        // Target Y when hidden (above screen)
        let hidden_y = work_area.top - height;

        // Ensure window has WS_EX_LAYERED style for opacity
        let ex_style = GetWindowLongW(hwnd.0, GWL_EXSTYLE);
        if (ex_style & WS_EX_LAYERED.0 as i32) == 0 {
             SetWindowLongW(hwnd.0, GWL_EXSTYLE, ex_style | WS_EX_LAYERED.0 as i32);
        }

        let duration_ms = if is_visible { config.hide_duration } else { config.show_duration } as u64;
        let easing_type = if is_visible { &config.hide_easing } else { &config.show_easing };
        let opacity_point = if is_visible { config.hide_opacity_point } else { config.show_opacity_point };
        let animate_opacity = config.animate_opacity;

        let start_time = Instant::now();
        let mut interval = interval(Duration::from_millis(16)); // ~60 FPS

        // Initial Setup
        if !is_visible {
             // SHOWING
             // Set initial pos (hidden) and show
             let _ = SetWindowPos(hwnd.0, HWND_TOPMOST, x, hidden_y, width, height, SWP_SHOWWINDOW | SWP_NOACTIVATE);
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
            let _ = SetWindowPos(hwnd.0, HWND_TOPMOST, x, current_y, width, height, SWP_NOACTIVATE);

            if progress >= 1.0 {
                 break;
            }
        }

        // Finalize
        if is_visible {
             // HIDDEN
             let _ = ShowWindow(hwnd.0, SW_HIDE);
        } else {
             // SHOWN
             let _ = SetLayeredWindowAttributes(hwnd.0, COLORREF(0), 255, LWA_ALPHA);
             let _ = SetWindowPos(hwnd.0, HWND_TOPMOST, x, target_y, width, height, SWP_SHOWWINDOW);
             let _ = SetForegroundWindow(hwnd.0);
        }
    }

    {
        let mut animating = IS_ANIMATING.lock().unwrap();
        *animating = false;
    }
}
