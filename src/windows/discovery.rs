//! Window discovery and enumeration for Windows.
//!
//! Uses `EnumWindows` with process inspection to find windows matching
//! user-configured `window_class` values via fuzzy matching.

use windows::core::BOOL;
use windows::Win32::{
  Foundation::{CloseHandle, HWND, LPARAM},
  Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_CLOAKED},
  System::{
    ProcessStatus::GetModuleBaseNameW,
    Threading::{OpenProcess, PROCESS_QUERY_INFORMATION, PROCESS_VM_READ},
  },
  UI::WindowsAndMessaging::*,
};

use crate::windows::window::{get_app_cache, get_hidden_owner, CachedWindow};
use janq::config::{fuzzy_match_window, FoundWindow};

/// Context struct for EnumWindows callback.
pub struct TargetSearch {
  pub found_data: Vec<FoundWindow>,
}

/// EnumWindows callback that collects window information for fuzzy matching.
pub unsafe extern "system" fn enum_windows_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
  let target_struct = &mut *(lparam.0 as *mut TargetSearch);

  unsafe {
    // 1. Instant check: Style (ignore tool windows, shadows, etc)
    let style = GetWindowLongW(hwnd, GWL_EXSTYLE) as u32;
    if (style & WS_EX_TOOLWINDOW.0) != 0 {
      return BOOL(1);
    }

    // 2. Instant check: Ownership (ignore child/helper windows that have an owner)
    // Most 'main' app windows are unowned (GetWindow(hwnd, GW_OWNER) == NULL)
    // Exception: Allow windows owned by our hidden owner (already managed by janq)
    let owner = GetWindow(hwnd, GW_OWNER).map(|h| h.0 as usize).unwrap_or(0);
    if owner != 0 {
      let our_owner = get_hidden_owner().map(|h| h.0 as usize).unwrap_or(0);
      if owner != our_owner {
        return BOOL(1);
      }
    }

    // 2. Instant check: Visibility
    // Note: We still want to catch windows that are "parked" (IsWindowVisible == false)
    // but for the INITIAL enumeration during hotkey trigger,
    // we often prioritize visible ones. Actually, the current logic is to collect
    // ALL so we can fuzzy match them. But we can skip obvious system "ghost" windows.
    let mut cloaked: u32 = 0;
    let dwm_result = DwmGetWindowAttribute(
      hwnd,
      DWMWA_CLOAKED,
      &mut cloaked as *mut u32 as *mut _,
      std::mem::size_of::<u32>() as u32,
    );
    if dwm_result.is_ok() && cloaked != 0 {
      return BOOL(1);
    }
  }

  let mut pid = 0;
  unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
  if pid == 0 {
    return BOOL(1);
  }

  let mut class_buffer = [0u16; 256];
  let class_len = unsafe { GetClassNameW(hwnd, &mut class_buffer) } as usize;
  let class_slice = &class_buffer[..class_len];

  let mut title_buf = [0u16; 512];
  let title_len = unsafe { GetWindowTextW(hwnd, &mut title_buf) };
  let has_title = title_len > 0;

  // Filter out known junk classes (Case-insensitive ASCII comparisons on u16 stack buffer)
  if u16_contains_ascii_ignore_case(class_slice, "nvopengl")
    || u16_contains_ascii_ignore_case(class_slice, "wgpu")
    || u16_eq_ascii_ignore_case(class_slice, "ime")
    || u16_eq_ascii_ignore_case(class_slice, "msctfime ui")
    || u16_contains_ascii_ignore_case(class_slice, "gdi+ hooks")
    || u16_eq_ascii_ignore_case(class_slice, "progman")
    || u16_eq_ascii_ignore_case(class_slice, "workerw")
    || u16_contains_ascii_ignore_case(class_slice, "shell_traywnd")
    || u16_contains_ascii_ignore_case(class_slice, "shell_secondarytraywnd")
    || u16_contains_ascii_ignore_case(class_slice, "windows.ui.core.corewindow")
    || ((u16_contains_ascii_ignore_case(class_slice, "chrome_widgetwin")
      || u16_contains_ascii_ignore_case(class_slice, "nativehwndhost"))
      && !has_title)
    || u16_contains_ascii_ignore_case(class_slice, "tooltip")
    || u16_contains_ascii_ignore_case(class_slice, "ghost")
  {
    return BOOL(1);
  }

  // Only allocate and convert strings if we passed the initial junk filter
  let class_name = String::from_utf16_lossy(class_slice).to_lowercase();
  let mut proc_name = String::new();
  if let Ok(process) =
    unsafe { OpenProcess(PROCESS_QUERY_INFORMATION | PROCESS_VM_READ, false, pid) }
  {
    let mut buffer = [0u16; 1024];
    let len = unsafe { GetModuleBaseNameW(process, None, &mut buffer) };
    if len > 0 {
      proc_name = String::from_utf16_lossy(&buffer[..len as usize]).to_lowercase();
    }
    unsafe {
      let _ = CloseHandle(process);
    }
  }

  let is_visible = unsafe { IsWindowVisible(hwnd).as_bool() };

  target_struct.found_data.push(FoundWindow {
    id: (hwnd.0 as usize).to_string(),
    hwnd: hwnd.0 as isize,
    class_lowercase: class_name,
    proc_lowercase: proc_name,
    #[cfg(target_os = "linux")]
    pid,
    is_visible,
  });

  BOOL(1)
}

/// Fetches all visible windows from the system for fuzzy matching.
pub fn fetch_system_windows() -> Vec<FoundWindow> {
  let mut search = TargetSearch {
    found_data: Vec::new(),
  };
  unsafe {
    let _ = EnumWindows(
      Some(enum_windows_proc),
      LPARAM(&mut search as *mut _ as isize),
    );
  }
  search.found_data
}

/// Finds a window by process/class name using fuzzy matching.
pub fn find_window_by_process(
  name: &str,
  candidates: Option<&[FoundWindow]>,
) -> Option<CachedWindow> {
  let cache = get_app_cache().read().unwrap();
  let managed_ids: std::collections::HashSet<isize> =
    cache.values().map(|cw| cw.hwnd.0 as isize).collect();

  if let Some(list) = candidates {
    if let Some(best) = fuzzy_match_window(name, list, &managed_ids) {
      return Some(CachedWindow {
        hwnd: HWND(best.hwnd as *mut _),
      });
    }
    return None;
  }

  let found_data = fetch_system_windows();
  if let Some(best) = fuzzy_match_window(name, &found_data, &managed_ids) {
    return Some(CachedWindow {
      hwnd: HWND(best.hwnd as *mut _),
    });
  }

  None
}

// =============================================================================
// Case-Insensitive ASCII Helpers for u16 (Stack-safe)
// =============================================================================

fn u16_contains_ascii_ignore_case(haystack: &[u16], needle: &str) -> bool {
  let needle_bytes = needle.as_bytes();
  if haystack.len() < needle_bytes.len() {
    return false;
  }
  haystack.windows(needle_bytes.len()).any(|window| {
    window.iter().zip(needle_bytes).all(|(&h, &n)| {
      let h_lower = if h <= 127 {
        (h as u8).to_ascii_lowercase() as u16
      } else {
        h
      };
      h_lower == n.to_ascii_lowercase() as u16
    })
  })
}

fn u16_eq_ascii_ignore_case(haystack: &[u16], needle: &str) -> bool {
  let needle_bytes = needle.as_bytes();
  if haystack.len() != needle_bytes.len() {
    return false;
  }
  haystack.iter().zip(needle_bytes).all(|(&h, &n)| {
    let h_lower = if h <= 127 {
      (h as u8).to_ascii_lowercase() as u16
    } else {
      h
    };
    h_lower == n.to_ascii_lowercase() as u16
  })
}
