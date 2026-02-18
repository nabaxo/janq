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

        // STILL_ACTIVE is 259
        if success.is_ok() && exit_code == 259 {
          if let Some(name) = expected_name {
            let mut buffer = [0u16; 256];
            let len =
              windows::Win32::System::ProcessStatus::GetModuleBaseNameW(handle, None, &mut buffer);
            let _ = CloseHandle(handle);
            if len > 0 {
              return crate::matching::u16_contains_ascii_ignore_case(
                &buffer[..len as usize],
                name,
              );
            }
            return false;
          }
          let _ = CloseHandle(handle);
          return true;
        }
        let _ = CloseHandle(handle);
      }
    }
    false
  }

  #[cfg(target_os = "linux")]
  {
    use std::io::Read;

    let mut path_buf = [0u8; 32];
    let path = if let Ok(n) = format_proc_path(&mut path_buf, pid, "") {
      n
    } else {
      return false;
    };

    if !std::path::Path::new(path).exists() {
      return false;
    }

    if let Some(name) = expected_name {
      let mut cmd_buf = [0u8; 32];
      if let Ok(cmd_path) = format_proc_path(&mut cmd_buf, pid, "/cmdline") {
        if let Ok(mut f) = std::fs::File::open(cmd_path) {
          let mut buffer = [0u8; 512];
          if let Ok(n) = f.read(&mut buffer) {
            if let Some(part) = buffer[..n].split(|&b| b == 0).next() {
              if let Some(comm) = part.split(|&b| b == b'/').next_back() {
                return comm.eq_ignore_ascii_case(name.as_bytes());
              }
            }
          }
        }
      }
      return false;
    }
    true
  }
}

#[cfg(target_os = "linux")]
fn format_proc_path<'a>(buf: &'a mut [u8], pid: u32, suffix: &str) -> Result<&'a str, ()> {
  use std::io::Write;
  let mut cursor = std::io::Cursor::new(buf);
  if write!(cursor, "/proc/{}{}", pid, suffix).is_ok() {
    let len = cursor.position() as usize;
    std::str::from_utf8(&cursor.into_inner()[..len]).map_err(|_| ())
  } else {
    Err(())
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
