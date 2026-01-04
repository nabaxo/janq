use crate::config::Config;
use crate::windows::easing::get_easing;
use windows::Win32::Foundation::{BOOL, HWND, LPARAM, COLORREF, RECT, POINT};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetWindowThreadProcessId, IsWindowVisible, ShowWindow, SetForegroundWindow, GetForegroundWindow,
    SetWindowPos, SW_HIDE, HWND_TOPMOST, HWND_NOTOPMOST, SWP_SHOWWINDOW, SWP_NOACTIVATE, SWP_NOZORDER,
    GetLayeredWindowAttributes, SetLayeredWindowAttributes, GetWindowLongW, SetWindowLongW, GWL_EXSTYLE, WS_EX_LAYERED, LWA_ALPHA,
    GetCursorPos, GetWindowRect, IsIconic, SW_SHOW, SWP_NOSIZE, SWP_NOCOPYBITS, SWP_DEFERERASE, SWP_NOMOVE, SWP_FRAMECHANGED
};
use windows::Win32::Graphics::Dwm::DwmFlush;
use windows::Win32::Graphics::Gdi::{
    MonitorFromPoint, GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromWindow, HMONITOR, HDC, EnumDisplayMonitors
};
use windows::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_INFORMATION, PROCESS_VM_READ};
use windows::Win32::System::ProcessStatus::GetModuleBaseNameW;
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

use std::collections::HashMap;
use std::sync::{RwLock, OnceLock};

static ANIMATION_TASK: OnceLock<std::sync::Mutex<Option<tokio::task::AbortHandle>>> = OnceLock::new();
static VISIBLE_APP: OnceLock<RwLock<Option<String>>> = OnceLock::new();
static PREVIOUS_FOCUS: OnceLock<std::sync::Mutex<Option<SendHwnd>>> = OnceLock::new();
static LAST_MONITOR: OnceLock<std::sync::Mutex<Option<isize>>> = OnceLock::new();
static HWND_CACHE: OnceLock<RwLock<HashMap<String, SendHwnd>>> = OnceLock::new();

fn get_animation_task() -> &'static std::sync::Mutex<Option<tokio::task::AbortHandle>> {
    ANIMATION_TASK.get_or_init(|| std::sync::Mutex::new(None))
}

fn get_visible_app() -> &'static RwLock<Option<String>> {
    VISIBLE_APP.get_or_init(|| RwLock::new(None))
}

fn get_previous_focus() -> &'static std::sync::Mutex<Option<SendHwnd>> {
    PREVIOUS_FOCUS.get_or_init(|| std::sync::Mutex::new(None))
}

fn get_last_monitor() -> &'static std::sync::Mutex<Option<isize>> {
    LAST_MONITOR.get_or_init(|| std::sync::Mutex::new(None))
}

fn get_hwnd_cache() -> &'static RwLock<HashMap<String, SendHwnd>> {
    HWND_CACHE.get_or_init(|| RwLock::new(HashMap::new()))
}

// ... (helpers omitted for brevity if unchanged, but for replace_file_content we need context)

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
            if name.to_lowercase().contains(&target_struct.name) {
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
        name: name.to_string().to_lowercase(),
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

                if w > 50 && h > 50 {
                    let is_visible = IsWindowVisible(hwnd).as_bool();
                    let style = GetWindowLongW(hwnd, GWL_EXSTYLE) as u32;
                    let is_tool = (style & windows::Win32::UI::WindowsAndMessaging::WS_EX_TOOLWINDOW.0) != 0;

                    if is_tool { continue; }

                    let mut score = 0;
                    if is_visible { score += 1000; }

                    // Main window typically has a caption/title bar
                    let style_regular = GetWindowLongW(hwnd, windows::Win32::UI::WindowsAndMessaging::GWL_STYLE) as u32;
                    if (style_regular & windows::Win32::UI::WindowsAndMessaging::WS_CAPTION.0) != 0 {
                        score += 500;
                    }

                    // Prefer larger windows
                    score += ((w * h) / 10000).min(100);

                    // Prefer windows with titles
                    let mut title_buf = [0u16; 256];
                    let title_len = windows::Win32::UI::WindowsAndMessaging::GetWindowTextW(hwnd, &mut title_buf);
                    if title_len > 0 {
                        score += 200;
                    }

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

pub async fn toggle_window(app_name: &str, config: &Config) -> bool {
    let is_visible = {
        let v = get_visible_app().read().unwrap();
        v.as_deref() == Some(app_name)
    };
    let should_show = !is_visible;

    println!("[Ruake] Toggling app: {} (should_show: {})", app_name, should_show);

    let app_cfg = match config.app.get(app_name) {
        Some(c) => c,
        None => return false,
    };

    // Identify siblings to hide aggressively (all Ruake windows except target)
    let mut siblings = Vec::new();
    if should_show {
        let cache = get_hwnd_cache().read().unwrap();
        for (name, hwnd) in cache.iter() {
            if name != app_name {
                 siblings.push((name.clone(), *hwnd));
            }
        }
    }

    // Abort current animation
    {
        let mut task_handle = get_animation_task().lock().unwrap();
        if let Some(handle) = task_handle.take() {
            handle.abort();
        }
    }

    // Find Target HWND
    let mut cached_hwnd = None;
    {
        let cache = get_hwnd_cache().read().unwrap();
        if let Some(h) = cache.get(app_name) {
             unsafe {
                 if windows::Win32::UI::WindowsAndMessaging::IsWindow(h.inner()).as_bool() {
                     cached_hwnd = Some(*h);
                 }
             }
        }
    }

    let target_hwnd = if let Some(h) = cached_hwnd {
        h
    } else {
        match find_window_by_process(&app_cfg.window_class) {
            Some(h) => {
                let mut cache = get_hwnd_cache().write().unwrap();
                let wrapper = SendHwnd(h);
                cache.insert(app_name.to_string(), wrapper);
                wrapper
            },
            None => {
                println!("Window not found for app: {} (class: {})", app_name, app_cfg.window_class);
                return false;
            }
        }
    };

    // Update visibility state
    let mut restore_focus = false;
    {
        let mut v = get_visible_app().write().unwrap();
        if should_show {
            *v = Some(app_name.to_string());
        } else {
            *v = None;
            unsafe {
                let fg_window = GetForegroundWindow();
                if fg_window == target_hwnd.inner() {
                    restore_focus = true;
                }
            }
        }
    }

    // Immediate Activation/Prep
    if should_show {
        unsafe {
            let fg_window = GetForegroundWindow();
            // Capture focus when showing - update previous focus to current focused app
            let mut prev = get_previous_focus().lock().unwrap();
            if fg_window.0 != 0 && fg_window != target_hwnd.inner() {
                *prev = Some(SendHwnd(fg_window));
            }

            // Ensure window is shown before animation starts
            if !IsWindowVisible(target_hwnd.inner()).as_bool() {
                let _ = ShowWindow(target_hwnd.inner(), SW_SHOW);
            }
            let _ = SetForegroundWindow(target_hwnd.inner());
        }
    }

    let config_clone = config.clone();
    let app_name_clone = app_name.to_string();

    let handle = tokio::spawn(async move {
        run_animation_task(&app_name_clone, &config_clone, target_hwnd, should_show, siblings, restore_focus).await;
    });

    {
        let mut task_handle = get_animation_task().lock().unwrap();
        *task_handle = Some(handle.abort_handle());
    }

    true
}

async fn run_animation_task(
    app_name: &str,
    config: &Config,
    target_hwnd: SendHwnd,
    should_show: bool,
    siblings: Vec<(String, SendHwnd)>,
    restore_focus: bool,
) {
    println!("[Ruake] Starting animation for '{}' (siblings: {})", app_name, siblings.len());
    let app_cfg = match config.app.get(app_name) {
        Some(c) => c,
        None => return,
    };

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
                    let prev = get_previous_focus().lock().unwrap();
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
            MonitorFromWindow(target_hwnd.inner(), MONITOR_DEFAULTTONEAREST)
        };

        let final_monitor = if should_show {
            let mut last = get_last_monitor().lock().unwrap();
            *last = Some(monitor.0 as isize);
            monitor
        } else {
                let last = get_last_monitor().lock().unwrap();
                if let Some(h) = *last {
                    HMONITOR(h as isize)
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

        // --- Target Geometry Resolution ---
        let (width_val, height_val) = app_cfg.resolve_dimensions(&config.window);

        let target_w = if width_val > 0.0 {
            if width_val <= 1.0 { (screen_w as f64 * width_val) as i32 }
            else { width_val as i32 }
        } else {
            let mut r = RECT::default();
            if GetWindowRect(target_hwnd.inner(), &mut r).is_ok() { r.right - r.left } else { 400 }
        };

        let target_h = if height_val > 0.0 {
            if height_val <= 1.0 { (screen_h as f64 * height_val) as i32 }
            else { height_val as i32 }
        } else {
            let mut r = RECT::default();
            if GetWindowRect(target_hwnd.inner(), &mut r).is_ok() { r.bottom - r.top } else { 300 }
        };

        let target_x = work_area.left + (screen_w - target_w) / 2;
        let shown_y = work_area.top;
        let hidden_y = work_area.top - target_h;
        let final_target_y = if should_show { shown_y } else { hidden_y };

        // --- Sibling Geometry (if any) ---
        let mut siblings_data = Vec::new();
        for (name, ohwnd) in siblings {
            let mut r = RECT::default();
            if GetWindowRect(ohwnd.inner(), &mut r).is_ok() {
                if IsWindowVisible(ohwnd.inner()).as_bool() || (r.bottom > work_area.top) {
                    let ow = r.right - r.left;
                    let oh = r.bottom - r.top;
                    let ox = r.left;
                    let o_start_y = r.top;
                    let o_target_y = work_area.top - oh; // Slide UP

                    siblings_data.push((name, ohwnd, ox, ow, oh, o_start_y, o_target_y));
                }
            }
        }

        // --- Prep Target ---
        let ex_style = GetWindowLongW(target_hwnd.inner(), GWL_EXSTYLE);
        if (ex_style & WS_EX_LAYERED.0 as i32) == 0 {
            SetWindowLongW(target_hwnd.inner(), GWL_EXSTYLE, ex_style | WS_EX_LAYERED.0 as i32);
            let _ = SetWindowPos(target_hwnd.inner(), HWND::default(), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_FRAMECHANGED);
            let _ = SetLayeredWindowAttributes(target_hwnd.inner(), COLORREF(0), 255, LWA_ALPHA);
        }

        let mut target_rect = RECT::default();
        let mut target_start_alpha: u8 = 255;
        let _ = GetLayeredWindowAttributes(target_hwnd.inner(), None, Some(&mut target_start_alpha), None);
        let _ = GetWindowRect(target_hwnd.inner(), &mut target_rect);

        let target_start_y = target_rect.top;
        let target_dist_y = final_target_y - target_start_y;

        let animate_opacity = app_cfg.get_animate_opacity(config.animation.animate_opacity);

        let duration_ms = if should_show { config.animation.show_duration } else { config.animation.hide_duration };
        let duration_secs = duration_ms as f64 / 1000.0;

        let easing_type = if should_show { &config.animation.show_easing } else { &config.animation.hide_easing };
        let z_flag = SendHwnd(if should_show || config.window.keep_above { HWND_TOPMOST } else { HWND_NOTOPMOST });
        let opacity_point = if should_show { config.animation.show_opacity_point } else { config.animation.hide_opacity_point };

        let start_time = Instant::now();

        if duration_secs > 0.0 {
            loop {
                let _ = DwmFlush();
                tokio::task::yield_now().await;

                // Stop if state changed globally (someone else started a new toggle)
                {
                    let v = get_visible_app().read().unwrap();
                    let still_target = if should_show { v.as_deref() == Some(app_name) } else { v.as_deref() != Some(app_name) };
                    if !still_target { return; }
                }

                let elapsed = start_time.elapsed().as_secs_f64();
                let progress = (elapsed / duration_secs).min(1.0);
                let ease_val = get_easing(progress, easing_type);

                // --- Animate Target ---
                let new_y = target_start_y + (target_dist_y as f64 * ease_val) as i32;

                if animate_opacity {
                    let safe_opacity_point = opacity_point.clamp(0.01, 1.0);
                    let opacity_progress = if should_show {
                        (progress / safe_opacity_point).min(1.0)
                    } else {
                        let fade_start = 1.0 - safe_opacity_point;
                        if progress <= fade_start { 0.0 } else { ((progress - fade_start) / safe_opacity_point).min(1.0) }
                    };

                    let new_alpha = if should_show {
                         (target_start_alpha as f64 + (255.0 - target_start_alpha as f64) * opacity_progress).clamp(0.0, 255.0) as u8
                    } else {
                         (target_start_alpha as f64 * (1.0 - opacity_progress)).clamp(0.0, 255.0) as u8
                    };
                    let _ = SetLayeredWindowAttributes(target_hwnd.inner(), COLORREF(0), new_alpha, LWA_ALPHA);
                }

                let _ = SetWindowPos(target_hwnd.inner(), z_flag.0, target_x, new_y, target_w, target_h, SWP_NOACTIVATE | SWP_NOCOPYBITS | SWP_DEFERERASE);

                // --- Animate Siblings ---
                for (_, ohwnd, ox, ow, oh, osy, oty) in &siblings_data {
                    let o_dist_y = oty - osy;
                    let o_new_y = osy + (o_dist_y as f64 * ease_val) as i32;

                    if animate_opacity {
                        // Siblings always fade out
                        let fade_point = config.animation.hide_opacity_point.clamp(0.01, 1.0);
                        let fade_start = 1.0 - fade_point;
                        let o_opacity_progress = if progress <= fade_start { 0.0 } else { ((progress - fade_start) / fade_point).min(1.0) };
                        let o_new_alpha = (255.0 * (1.0 - o_opacity_progress)).clamp(0.0, 255.0) as u8;
                        let _ = SetLayeredWindowAttributes(ohwnd.inner(), COLORREF(0), o_new_alpha, LWA_ALPHA);
                    }

                    let _ = SetWindowPos(ohwnd.inner(), HWND::default(), *ox, o_new_y, *ow, *oh, SWP_NOACTIVATE | SWP_NOCOPYBITS | SWP_DEFERERASE | SWP_NOZORDER);
                }

                if progress >= 1.0 { break; }
            }
        }

        // --- Final Cleanup ---
        if should_show {
            let _ = SetLayeredWindowAttributes(target_hwnd.inner(), COLORREF(0), 255, LWA_ALPHA);
            let _ = SetWindowPos(target_hwnd.inner(), z_flag.0, target_x, final_target_y, target_w, target_h, SWP_SHOWWINDOW | SWP_NOACTIVATE | SWP_NOCOPYBITS);
        } else {
            let _ = ShowWindow(target_hwnd.inner(), SW_HIDE);
            if restore_focus {
                let prev = get_previous_focus().lock().unwrap();
                if let Some(h) = *prev {
                    if IsWindowVisible(h.0).as_bool() {
                        let _ = SetForegroundWindow(h.0);
                    }
                    // DON'T clear previous focus here - preserve it for rapid toggle scenarios
                    // It will be overwritten on the next show when we capture new focus
                }
            }
        }

        for (_, ohwnd, _, _, _, _, _) in siblings_data {
            let _ = ShowWindow(ohwnd.inner(), SW_HIDE);
        }
    }
}

pub fn restore_app_window(app_name: &str, window_class: &str) {
    if let Some(hwnd) = find_window_by_process(window_class) {
        let is_target_visible = {
            let v = get_visible_app().read().unwrap();
            v.as_deref() == Some(app_name)
        };

        unsafe {
            let ex_style = GetWindowLongW(hwnd, GWL_EXSTYLE);
            if (ex_style & WS_EX_LAYERED.0 as i32) == 0 {
                SetWindowLongW(hwnd, GWL_EXSTYLE, ex_style | WS_EX_LAYERED.0 as i32);
                let _ = SetWindowPos(hwnd, HWND::default(), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_FRAMECHANGED);
            }
            let _ = SetLayeredWindowAttributes(hwnd, COLORREF(0), 255, LWA_ALPHA);

            let (x, y, flags) = if is_target_visible {
                (0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_SHOWWINDOW | SWP_NOACTIVATE)
            } else {
                let monitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
                let mut mi = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
                if GetMonitorInfoW(monitor, &mut mi).as_bool() {
                    (mi.rcWork.left, mi.rcWork.top, SWP_NOSIZE | SWP_SHOWWINDOW | SWP_NOACTIVATE)
                } else {
                    (0, 0, SWP_NOSIZE | SWP_SHOWWINDOW | SWP_NOACTIVATE)
                }
            };
            let _ = SetWindowPos(hwnd, HWND_NOTOPMOST, x, y, 0, 0, flags);
            if IsIconic(hwnd).as_bool() {
                 let _ = ShowWindow(hwnd, windows::Win32::UI::WindowsAndMessaging::SW_SHOWNOACTIVATE);
            } else {
                 let _ = ShowWindow(hwnd, windows::Win32::UI::WindowsAndMessaging::SW_SHOWNA);
            }
        }
    }
}

pub fn restore_window_visibility(config: &Config) {
    for (name, app_cfg) in &config.app {
        restore_app_window(name, &app_cfg.window_class);
    }
}

pub fn reset_visible_app() {
    let mut v = get_visible_app().write().unwrap();
    *v = None;
}
