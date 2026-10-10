//! Platform-agnostic process liveness utilities.

/// Verifies if a process is still running.
///
/// This provides a unified interface for checking PID liveness without
/// relying on OS-specific error codes in the core logic.
pub fn is_process_running(pid: u32) -> bool {
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
      if let Ok(handle) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) {
        let mut exit_code: u32 = 0;
        let success = GetExitCodeProcess(handle, &mut exit_code);
        let _ = CloseHandle(handle);
        // STILL_ACTIVE is 259
        return success.is_ok() && exit_code == 259;
      }
    }
    false
  }

  #[cfg(target_os = "linux")]
  {
    let mut path_buf = [0u8; 32];
    match format_proc_path(&mut path_buf, pid) {
      Ok(path) => std::path::Path::new(path).exists(),
      Err(()) => false,
    }
  }
}

#[cfg(target_os = "linux")]
fn format_proc_path(buf: &mut [u8], pid: u32) -> Result<&str, ()> {
  use std::io::Write;
  let mut cursor = std::io::Cursor::new(buf);
  if write!(cursor, "/proc/{}", pid).is_ok() {
    let len = cursor.position() as usize;
    std::str::from_utf8(&cursor.into_inner()[..len]).map_err(|_| ())
  } else {
    Err(())
  }
}
