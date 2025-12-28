#[cfg(target_os = "linux")]
pub use crate::linux::terminal::*;

#[cfg(target_os = "windows")]
pub use crate::windows::terminal::*;
