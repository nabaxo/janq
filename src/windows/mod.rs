pub mod daemon;
pub mod easing;
pub mod terminal;
pub mod window;

pub fn show_error(message: &str) {
  eprintln!("{}", message);

  // On Windows, a Message Box is the standard "window" to show an error
  // when launched without a console.
  use windows::core::HSTRING;
  use windows::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONERROR, MB_OK};

  let title = HSTRING::from("janq Configuration Error");
  let msg = HSTRING::from(message);

  unsafe {
    MessageBoxW(None, &msg, &title, MB_OK | MB_ICONERROR);
  }
}
