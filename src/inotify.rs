//! Raw inotify config file watcher (zero dependencies).
//!
//! Watches the parent directory of the config file and filters events by
//! filename. Sends a `()` signal through a tokio unbounded channel whenever a
//! relevant filesystem event is detected.

use std::ffi::CString;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::io::FromRawFd;
use std::path::PathBuf;

use tokio::sync::mpsc as tokio_mpsc;

// =============================================================================
// Syscall bindings
// =============================================================================

use std::ffi::c_char;

const IN_CLOEXEC: i32 = 0o2_000_000;

const IN_MODIFY: u32 = 0x0000_0002;
const IN_CREATE: u32 = 0x0000_0100;
const IN_DELETE: u32 = 0x0000_0200;
const IN_MOVED_FROM: u32 = 0x0000_0040;
const IN_MOVED_TO: u32 = 0x0000_0080;

/// Fixed-size header of `struct inotify_event` (wd + mask + cookie + len).
const EVENT_HEADER: usize = 16;

extern "C" {
  fn inotify_init1(flags: i32) -> i32;
  fn inotify_add_watch(fd: i32, pathname: *const c_char, mask: u32) -> i32;
  fn close(fd: i32) -> i32;
}

// =============================================================================
// Event parsing
// =============================================================================

/// Returns `true` if any event in the raw inotify buffer names `target`.
fn has_matching_event(buf: &[u8], target: &[u8]) -> bool {
  let mut off = 0;
  while off + EVENT_HEADER <= buf.len() {
    let name_len =
      u32::from_ne_bytes([buf[off + 12], buf[off + 13], buf[off + 14], buf[off + 15]]) as usize;

    if name_len > 0 && off + EVENT_HEADER + name_len <= buf.len() {
      let name = &buf[off + EVENT_HEADER..off + EVENT_HEADER + name_len];
      let end = name.iter().position(|&b| b == 0).unwrap_or(name.len());
      if name[..end] == *target {
        return true;
      }
    }
    off += EVENT_HEADER + name_len;
  }
  false
}

// =============================================================================
// Public API
// =============================================================================

/// Starts an inotify watch on the config file's parent directory and returns a
/// receiver that fires `()` whenever a relevant event is detected.
///
/// A dedicated blocking reader thread (`janq-inotify`) is spawned to drain the
/// inotify fd; the receiver can be polled from async code.
pub fn watch_config(config_path: Option<PathBuf>) -> Option<tokio_mpsc::UnboundedReceiver<()>> {
  // Resolve the config file path and its parent directory.
  let config_file = config_path.unwrap_or_else(|| {
    crate::paths::home_dir()
      .map(|h| h.join(".janq.toml"))
      .unwrap_or_default()
  });

  let abs_config = config_file
    .canonicalize()
    .unwrap_or_else(|_| config_file.clone());
  let watch_dir = abs_config
    .parent()
    .map(|p| p.to_path_buf())
    .unwrap_or_else(|| abs_config.clone());
  let target_filename: Vec<u8> = abs_config
    .file_name()
    .map(|n| n.as_bytes().to_vec())
    .unwrap_or_default();

  if target_filename.is_empty() {
    crate::error::show_error("Config watcher: cannot determine config filename");
    return None;
  }

  println!("Watcher: Monitoring config file: {:?}", abs_config);

  // --- Create inotify fd (blocking, close-on-exec) --------------------------
  let fd = unsafe { inotify_init1(IN_CLOEXEC) };
  if fd < 0 {
    crate::error::show_error(&format!(
      "Failed to create inotify: {}",
      std::io::Error::last_os_error()
    ));
    return None;
  }

  let c_path = match CString::new(watch_dir.as_os_str().as_bytes()) {
    Ok(p) => p,
    Err(_) => {
      crate::error::show_error("Config watcher: invalid watch path (interior NUL)");
      unsafe { close(fd) };
      return None;
    }
  };

  let mask = IN_MODIFY | IN_CREATE | IN_DELETE | IN_MOVED_FROM | IN_MOVED_TO;

  if unsafe { inotify_add_watch(fd, c_path.as_ptr(), mask) } < 0 {
    crate::error::show_error(&format!(
      "Failed to watch {}: {}",
      watch_dir.display(),
      std::io::Error::last_os_error()
    ));
    unsafe { close(fd) };
    return None;
  }

  // Wrap in File so Drop closes the fd; move into the reader thread.
  let file = unsafe { std::fs::File::from_raw_fd(fd) };

  let (tx, rx) = tokio_mpsc::unbounded_channel::<()>();

  // --- Blocking reader thread ------------------------------------------------
  std::thread::Builder::new()
    .name("janq-inotify".into())
    .spawn(move || {
      use std::io::Read;
      let mut file = file; // take ownership so Drop closes fd on exit
      let mut buf = [0u8; 4096];
      loop {
        match file.read(&mut buf) {
          Ok(0) => break,
          Ok(n) => {
            if has_matching_event(&buf[..n], &target_filename) {
              if tx.send(()).is_err() {
                break; // receiver dropped — daemon shutting down
              }
            }
          }
          Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
          Err(_) => break,
        }
      }
    })
    .ok();

  Some(rx)
}
