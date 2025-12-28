#[cfg(target_os = "linux")]
pub use crate::linux::daemon::*;

#[cfg(target_os = "windows")]
pub use crate::windows::daemon::*;
