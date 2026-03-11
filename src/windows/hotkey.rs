//! Windows global hotkey parsing and registration.
//!
//! Converts user-friendly hotkey strings (e.g., "Meta+Grave") into the format
//! required by the `global-hotkey` crate. The crate handles platform-specific
//! registration with Windows' RegisterHotKey API.
//!
//! ## Supported Modifiers
//! - `ctrl`, `control` → CONTROL
//! - `alt` → ALT
//! - `shift` → SHIFT
//! - `meta`, `super`, `win`, `cmd` → SUPER (Windows key)
//!
//! ## Special Key Handling
//! - Grave/backtick (`` ` ``) maps to `Code::Backquote`
//! - Section sign (`§`) maps to `Code::IntlBackslash` (EU keyboards)
//! - Function keys F1-F12 supported

use global_hotkey::hotkey::{Code, HotKey, Modifiers};

/// Parses a hotkey string into a `HotKey` struct for registration.
///
/// # Format
/// `[Modifier+]...[Modifier+]Key` where modifiers are optional.
///
/// # Examples
/// - `"Meta+Grave"` → Super + Backtick
/// - `"Ctrl+Alt+F12"` → Control + Alt + F12
/// - `"F1"` → F1 with no modifiers
pub fn parse_hotkey(hotkey_str: &str) -> janq::error::Result<HotKey> {
  let parts = janq::validation::split_hotkey(hotkey_str);
  let mut mods = Modifiers::empty();
  let mut key_code: Option<Code> = None;

  for part in parts {
    let p = part.trim().to_lowercase();
    match janq::config::normalize_hotkey_modifier(&p) {
      "ctrl" => mods |= Modifiers::CONTROL,
      "alt" => mods |= Modifiers::ALT,
      "shift" => mods |= Modifiers::SHIFT,
      "meta" => mods |= Modifiers::SUPER,
      _ => {
        if let Some(base_key) = janq::validation::BaseKey::parse(&p) {
          key_code = Some(to_win_code(base_key));
        } else {
          return Err(janq::format_error_boxed!("Unknown key: {}", part));
        }
      }
    }
  }

  let code = key_code.ok_or_else(|| janq::format_error_boxed!("No key code specified"))?;
  Ok(HotKey::new(Some(mods), code))
}

/// Translates the platform-agnostic BaseKey into Win32 Code.
fn to_win_code(key: janq::validation::BaseKey) -> Code {
  use janq::validation::BaseKey;
  match key {
    BaseKey::Grave => Code::Backquote,
    BaseKey::Digit(1) => Code::Digit1,
    BaseKey::Digit(2) => Code::Digit2,
    BaseKey::Digit(3) => Code::Digit3,
    BaseKey::Digit(4) => Code::Digit4,
    BaseKey::Digit(5) => Code::Digit5,
    BaseKey::Digit(6) => Code::Digit6,
    BaseKey::Digit(7) => Code::Digit7,
    BaseKey::Digit(8) => Code::Digit8,
    BaseKey::Digit(9) => Code::Digit9,
    BaseKey::Digit(0) => Code::Digit0,
    BaseKey::Minus => Code::Minus,
    BaseKey::Equal => Code::Equal,
    BaseKey::Letter('q') => Code::KeyQ,
    BaseKey::Letter('w') => Code::KeyW,
    BaseKey::Letter('e') => Code::KeyE,
    BaseKey::Letter('r') => Code::KeyR,
    BaseKey::Letter('t') => Code::KeyT,
    BaseKey::Letter('y') => Code::KeyY,
    BaseKey::Letter('u') => Code::KeyU,
    BaseKey::Letter('i') => Code::KeyI,
    BaseKey::Letter('o') => Code::KeyO,
    BaseKey::Letter('p') => Code::KeyP,
    BaseKey::BracketLeft => Code::BracketLeft,
    BaseKey::BracketRight => Code::BracketRight,
    BaseKey::Backslash => Code::Backslash,
    BaseKey::Letter('a') => Code::KeyA,
    BaseKey::Letter('s') => Code::KeyS,
    BaseKey::Letter('d') => Code::KeyD,
    BaseKey::Letter('f') => Code::KeyF,
    BaseKey::Letter('g') => Code::KeyG,
    BaseKey::Letter('h') => Code::KeyH,
    BaseKey::Letter('j') => Code::KeyJ,
    BaseKey::Letter('k') => Code::KeyK,
    BaseKey::Letter('l') => Code::KeyL,
    BaseKey::Semicolon => Code::Semicolon,
    BaseKey::Quote => Code::Quote,
    BaseKey::Enter => Code::Enter,
    BaseKey::Letter('z') => Code::KeyZ,
    BaseKey::Letter('x') => Code::KeyX,
    BaseKey::Letter('c') => Code::KeyC,
    BaseKey::Letter('v') => Code::KeyV,
    BaseKey::Letter('b') => Code::KeyB,
    BaseKey::Letter('n') => Code::KeyN,
    BaseKey::Letter('m') => Code::KeyM,
    BaseKey::Comma => Code::Comma,
    BaseKey::Period => Code::Period,
    BaseKey::Slash => Code::Slash,
    BaseKey::Space => Code::Space,
    BaseKey::Esc => Code::Escape,
    BaseKey::Tab => Code::Tab,
    BaseKey::CapsLock => Code::CapsLock,
    BaseKey::Backspace => Code::Backspace,
    BaseKey::Up => Code::ArrowUp,
    BaseKey::Down => Code::ArrowDown,
    BaseKey::Left => Code::ArrowLeft,
    BaseKey::Right => Code::ArrowRight,
    BaseKey::PageUp => Code::PageUp,
    BaseKey::PageDown => Code::PageDown,
    BaseKey::Home => Code::Home,
    BaseKey::End => Code::End,
    BaseKey::Insert => Code::Insert,
    BaseKey::Delete => Code::Delete,
    BaseKey::F(1) => Code::F1,
    BaseKey::F(2) => Code::F2,
    BaseKey::F(3) => Code::F3,
    BaseKey::F(4) => Code::F4,
    BaseKey::F(5) => Code::F5,
    BaseKey::F(6) => Code::F6,
    BaseKey::F(7) => Code::F7,
    BaseKey::F(8) => Code::F8,
    BaseKey::F(9) => Code::F9,
    BaseKey::F(10) => Code::F10,
    BaseKey::F(11) => Code::F11,
    BaseKey::F(12) => Code::F12,
    BaseKey::Section => Code::IntlBackslash,
    BaseKey::PlusMinus => Code::Backquote,
    _ => Code::Backquote, // Fallback for specialized dead keys
  }
}

/// Normalizes a shortcut for the Windows tray accelerator parser.
pub fn normalize_for_win(shortcut: &str) -> String {
  let parts: Vec<&str> = shortcut.split('+').collect();
  let mut normalized = Vec::with_capacity(parts.len());

  for part in parts {
    let p = part.trim().to_lowercase();
    match janq::config::normalize_hotkey_modifier(&p) {
      "ctrl" => normalized.push("Ctrl".to_string()),
      "alt" => normalized.push("Alt".to_string()),
      "shift" => normalized.push("Shift".to_string()),
      "meta" => normalized.push("Win".to_string()),
      _ => {
        // Handle special keys
        match p.as_str() {
          "grave" | "backtick" | "`" => normalized.push("`".to_string()),
          "section" | "§" => normalized.push("§".to_string()),
          _ => {
            // Capitalize first letter (e.g., "f1" -> "F1", "enter" -> "Enter")
            if !p.is_empty() {
              let mut c = p.chars();
              let capitalized = c.next().unwrap().to_uppercase().collect::<String>() + c.as_str();
              normalized.push(capitalized);
            }
          }
        }
      }
    }
  }

  normalized.join("+")
}
