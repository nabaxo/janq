//! Window discovery and enumeration for Windows.
//!
//! Uses `EnumWindows` with process inspection to find windows matching
//! user-configured `window_class` values via fuzzy matching.

use windows::core::BOOL;
use windows::Win32::{
  Foundation::{CloseHandle, HWND, LPARAM},
  System::{
    ProcessStatus::GetModuleBaseNameW,
    Threading::{OpenProcess, PROCESS_QUERY_INFORMATION, PROCESS_VM_READ},
  },
  UI::WindowsAndMessaging::*,
};

use crate::windows::window::{is_suitable_target, CachedWindow};
use janq::matching::{fuzzy_match_window, FoundWindow};

use std::sync::Arc;

/// Context struct for EnumWindows callback.
pub struct TargetSearch {
  pub found_data: Vec<FoundWindow>,
  pub cache_snapshot: Vec<(Arc<str>, usize)>, // app_name, hwnd
}

/// EnumWindows callback that collects window information for fuzzy matching.
pub unsafe extern "system" fn enum_windows_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
  let target_struct = &mut *(lparam.0 as *mut TargetSearch);

  if !is_suitable_target(hwnd) {
    return BOOL(1);
  }

  let mut pid = 0;
  unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
  if pid == 0 {
    return BOOL(1);
  }

  let mut class_buffer = [0u16; 256];
  let class_len = unsafe { GetClassNameW(hwnd, &mut class_buffer) } as usize;
  let class_slice = &class_buffer[..class_len];

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

  let hwnd_val = hwnd.0 as usize;
  let manager = target_struct
    .cache_snapshot
    .iter()
    .find(|(_, h)| *h == hwnd_val)
    .map(|(name, _)| Arc::clone(name));

  target_struct.found_data.push(FoundWindow {
    id: hwnd_val.to_string().into(),
    class_lowercase: class_name.into(),
    proc_lowercase: proc_name.into(),
    pid,
    is_visible,
    is_managed: manager.is_some(),
    managed_by: manager,
  });

  BOOL(1)
}

/// Fetches all visible windows from the system for fuzzy matching.
pub fn fetch_system_windows() -> Vec<FoundWindow> {
  let cache_snapshot = {
    let cache = crate::windows::window::get_app_cache().read().unwrap();
    cache
      .iter()
      .map(|(name, cw)| (Arc::clone(name), cw.hwnd.0 as usize))
      .collect()
  };

  let mut search = TargetSearch {
    found_data: Vec::with_capacity(128),
    cache_snapshot,
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
  requesting_app: Option<&str>,
) -> Option<CachedWindow> {
  let binding;
  let list = match candidates {
    Some(c) => c,
    None => {
      binding = fetch_system_windows();
      &binding
    }
  };

  if let Some(best) = fuzzy_match_window(name, list, requesting_app) {
    if let Ok(hwnd_val) = best.id.parse::<usize>() {
      return Some(CachedWindow {
        hwnd: HWND(hwnd_val as *mut _),
      });
    }
  }

  None
}
