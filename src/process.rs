//! Platform-agnostic process liveness and metadata utilities.

/// Verifies if a process is still running and optionally matches a name.
///
/// This provides a unified interface for checking PID liveness without
/// relying on OS-specific error codes in the core logic.
pub fn is_process_running(pid: u32, expected_name: Option<&str>) -> bool {
  if pid == 0 {
    return false;
  }

  #[cfg(target_os = "windows")]
  {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Threading::{
      GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    };

    unsafe {
      let handle_res = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid);
      if let Ok(handle) = handle_res {
        let mut exit_code: u32 = 0;
        let success = GetExitCodeProcess(handle, &mut exit_code);
        let _ = CloseHandle(handle);

        // STILL_ACTIVE is 259
        if success.is_ok() && exit_code == 259 {
          if let Some(name) = expected_name {
            return get_process_name(pid)
              .map(|n| n.to_lowercase().contains(&name.to_lowercase()))
              .unwrap_or(false);
          }
          return true;
        }
      }
    }
    false
  }

  #[cfg(target_os = "linux")]
  {
    let proc_path = format!("/proc/{}", pid);
    if !std::path::Path::new(&proc_path).exists() {
      return false;
    }

    if let Some(name) = expected_name {
      return get_process_name(pid)
        .map(|n| n.to_lowercase().contains(&name.to_lowercase()))
        .unwrap_or(false);
    }
    true
  }
}

/// Retrieves the name of a process by its PID.
pub fn get_process_name(pid: u32) -> Option<String> {
  #[cfg(target_os = "windows")]
  {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::ProcessStatus::GetModuleBaseNameW;
    use windows::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};

    unsafe {
      if let Ok(handle) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) {
        let mut buffer = [0u16; 256];
        let len = GetModuleBaseNameW(handle, None, &mut buffer);
        let _ = CloseHandle(handle);
        if len > 0 {
          return Some(String::from_utf16_lossy(&buffer[..len as usize]));
        }
      }
    }
    None
  }

  #[cfg(target_os = "linux")]
  {
    use std::fs;
    let proc_path = format!("/proc/{}/cmdline", pid);
    if let Ok(cmdline) = fs::read(proc_path) {
      if let Some(part) = cmdline.split(|&b| b == 0).next() {
        let s = String::from_utf8_lossy(part);
        if let Some(name) = s.split('/').next_back() {
          return Some(name.to_string());
        }
      }
    }
    None
  }
}
