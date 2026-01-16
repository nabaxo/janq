pub mod animation;
pub mod daemon;
pub mod discovery;
pub mod easing;
pub mod parking;
pub mod terminal;
pub mod window;

use windows::core::HSTRING;
use windows::Win32::UI::WindowsAndMessaging::{
  MessageBoxW, MB_ICONERROR, MB_OK, MB_SETFOREGROUND, MB_TOPMOST,
};

pub fn show_error(message: &str) {
  eprintln!("{}", message);

  let title = HSTRING::from("janq Configuration Error");
  let msg = HSTRING::from(message);

  unsafe {
    MessageBoxW(
      None,
      &msg,
      &title,
      MB_OK | MB_ICONERROR | MB_SETFOREGROUND | MB_TOPMOST,
    );
  }
}
