#[cfg(target_os = "linux")]
pub use crate::linux::terminal::*;

#[cfg(target_os = "windows")]
#[allow(unused_imports)]
pub use crate::windows::terminal::*;
