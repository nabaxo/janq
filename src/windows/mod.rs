pub mod terminal;
pub mod daemon;
pub mod window;
pub mod easing;

pub fn show_error(message: &str) {
    eprintln!("{}", message);

    // On Windows, a Message Box is the standard "window" to show an error
    // when launched without a console.
    use windows::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONERROR, MB_OK};
    use windows::core::HSTRING;

    let title = HSTRING::from("Ruake Configuration Error");
    let msg = HSTRING::from(message);

    unsafe {
        MessageBoxW(None, &msg, &title, MB_OK | MB_ICONERROR);
    }
}
