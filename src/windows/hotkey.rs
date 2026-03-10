//! Windows global hotkey registration and management.
//!
//! Implements native Win32 `RegisterHotKey`/`UnregisterHotKey` directly,
//! avoiding the former `global-hotkey` crate's thread-per-keypress busy-poll
//! model that caused CPU/thread leaks during long daemon uptime.
//!
//! ## Supported Modifiers
//! - `ctrl`, `control` → MOD_CONTROL
//! - `alt` → MOD_ALT
//! - `shift` → MOD_SHIFT
//! - `meta`, `super`, `win`, `cmd` → MOD_WIN
//!
//! ## Special Key Handling
//! - Grave/backtick (`` ` ``) maps to VK_OEM_3
//! - Section sign (`§`) maps to VK_OEM_102 (EU keyboards)
//! - Function keys F1-F12 supported

use windows::Win32::{
  Foundation::HWND,
  UI::Input::KeyboardAndMouse::{
    RegisterHotKey, UnregisterHotKey, HOT_KEY_MODIFIERS, MOD_ALT, MOD_CONTROL, MOD_NOREPEAT,
    MOD_SHIFT, MOD_WIN,
  },
};

/// VK_OEM_102 — the `<>` or `§½` key on EU 102-key keyboards.
pub const VK_OEM_102: u32 = 0xE2;
/// VK_OEM_3 — backtick / grave accent key.
pub const VK_OEM_3: u32 = 0xC0;
/// VK_OEM_5 — backslash key.
pub const VK_OEM_5: u32 = 0xDC;

/// A parsed global hotkey with Win32 modifier flags and virtual key code.
#[derive(Clone, Copy, Debug)]
pub struct HotKey {
  pub mods: HOT_KEY_MODIFIERS,
  pub vk: u32,
}

impl HotKey {
  pub fn new(mods: HOT_KEY_MODIFIERS, vk: u32) -> Self {
    Self { mods, vk }
  }

  /// Deterministic ID derived from modifiers and virtual key code.
  /// Used as the `id` parameter for `RegisterHotKey`.
  pub fn id(&self) -> u32 {
    ((self.mods.0 & 0x000F) << 16) | (self.vk & 0xFFFF)
  }
}

/// Thin wrapper around Win32 `RegisterHotKey`/`UnregisterHotKey`.
///
/// Hotkeys are registered against a specific window (the bridge HWND),
/// so `WM_HOTKEY` messages are delivered reliably even during modal loops.
pub struct HotKeyManager {
  hwnd: HWND,
}

impl HotKeyManager {
  pub fn new(hwnd: HWND) -> Self {
    Self { hwnd }
  }

  pub fn register(&self, hotkey: &HotKey) -> janq::error::Result<()> {
    unsafe {
      RegisterHotKey(
        Some(self.hwnd),
        hotkey.id() as i32,
        hotkey.mods | MOD_NOREPEAT,
        hotkey.vk,
      )
      .map_err(|e| janq::format_error_boxed!("RegisterHotKey failed: {}", e))
    }
  }

  pub fn unregister(&self, hotkey: &HotKey) -> janq::error::Result<()> {
    unsafe {
      UnregisterHotKey(Some(self.hwnd), hotkey.id() as i32)
        .map_err(|e| janq::format_error_boxed!("UnregisterHotKey failed: {}", e))
    }
  }
}

/// Parses a hotkey string into a `HotKey` struct for registration.
///
/// # Format
/// `[Modifier+]...[Modifier+]Key` where modifiers are optional.
///
/// # Examples
/// - `"Meta+Grave"` → Win + VK_OEM_3
/// - `"Ctrl+Alt+F12"` → Control + Alt + F12
/// - `"F1"` → F1 with no modifiers
pub fn parse_hotkey(hotkey_str: &str) -> janq::error::Result<HotKey> {
  let parts = janq::validation::split_hotkey(hotkey_str);
  let mut mods = HOT_KEY_MODIFIERS(0);
  let mut vk: Option<u32> = None;

  for part in parts {
    let p = part.trim().to_lowercase();
    match janq::config::normalize_hotkey_modifier(&p) {
      "ctrl" => mods |= MOD_CONTROL,
      "alt" => mods |= MOD_ALT,
      "shift" => mods |= MOD_SHIFT,
      "meta" => mods |= MOD_WIN,
      _ => {
        if let Some(base_key) = janq::validation::BaseKey::parse(&p) {
          vk = Some(to_vk(base_key));
        } else {
          return Err(janq::format_error_boxed!("Unknown key: {}", part));
        }
      }
    }
  }

  let vk = vk.ok_or_else(|| janq::format_error_boxed!("No key code specified"))?;
  Ok(HotKey::new(mods, vk))
}

/// Translates a platform-agnostic `BaseKey` into a Win32 virtual key code.
fn to_vk(key: janq::validation::BaseKey) -> u32 {
  use janq::validation::BaseKey;
  match key {
    BaseKey::Grave => 0xC0, // VK_OEM_3
    BaseKey::Digit(1) => 0x31,
    BaseKey::Digit(2) => 0x32,
    BaseKey::Digit(3) => 0x33,
    BaseKey::Digit(4) => 0x34,
    BaseKey::Digit(5) => 0x35,
    BaseKey::Digit(6) => 0x36,
    BaseKey::Digit(7) => 0x37,
    BaseKey::Digit(8) => 0x38,
    BaseKey::Digit(9) => 0x39,
    BaseKey::Digit(0) => 0x30,
    BaseKey::Minus => 0xBD, // VK_OEM_MINUS
    BaseKey::Equal => 0xBB, // VK_OEM_PLUS
    BaseKey::Letter('q') => 0x51,
    BaseKey::Letter('w') => 0x57,
    BaseKey::Letter('e') => 0x45,
    BaseKey::Letter('r') => 0x52,
    BaseKey::Letter('t') => 0x54,
    BaseKey::Letter('y') => 0x59,
    BaseKey::Letter('u') => 0x55,
    BaseKey::Letter('i') => 0x49,
    BaseKey::Letter('o') => 0x4F,
    BaseKey::Letter('p') => 0x50,
    BaseKey::BracketLeft => 0xDB,  // VK_OEM_4
    BaseKey::BracketRight => 0xDD, // VK_OEM_6
    BaseKey::Backslash => 0xDC,    // VK_OEM_5
    BaseKey::Letter('a') => 0x41,
    BaseKey::Letter('s') => 0x53,
    BaseKey::Letter('d') => 0x44,
    BaseKey::Letter('f') => 0x46,
    BaseKey::Letter('g') => 0x47,
    BaseKey::Letter('h') => 0x48,
    BaseKey::Letter('j') => 0x4A,
    BaseKey::Letter('k') => 0x4B,
    BaseKey::Letter('l') => 0x4C,
    BaseKey::Semicolon => 0xBA, // VK_OEM_1
    BaseKey::Quote => 0xDE,     // VK_OEM_7
    BaseKey::Enter => 0x0D,     // VK_RETURN
    BaseKey::Letter('z') => 0x5A,
    BaseKey::Letter('x') => 0x58,
    BaseKey::Letter('c') => 0x43,
    BaseKey::Letter('v') => 0x56,
    BaseKey::Letter('b') => 0x42,
    BaseKey::Letter('n') => 0x4E,
    BaseKey::Letter('m') => 0x4D,
    BaseKey::Comma => 0xBC,     // VK_OEM_COMMA
    BaseKey::Period => 0xBE,    // VK_OEM_PERIOD
    BaseKey::Slash => 0xBF,     // VK_OEM_2
    BaseKey::Space => 0x20,     // VK_SPACE
    BaseKey::Esc => 0x1B,       // VK_ESCAPE
    BaseKey::Tab => 0x09,       // VK_TAB
    BaseKey::CapsLock => 0x14,  // VK_CAPITAL
    BaseKey::Backspace => 0x08, // VK_BACK
    BaseKey::Up => 0x26,        // VK_UP
    BaseKey::Down => 0x28,      // VK_DOWN
    BaseKey::Left => 0x25,      // VK_LEFT
    BaseKey::Right => 0x27,     // VK_RIGHT
    BaseKey::PageUp => 0x21,    // VK_PRIOR
    BaseKey::PageDown => 0x22,  // VK_NEXT
    BaseKey::Home => 0x24,      // VK_HOME
    BaseKey::End => 0x23,       // VK_END
    BaseKey::Insert => 0x2D,    // VK_INSERT
    BaseKey::Delete => 0x2E,    // VK_DELETE
    BaseKey::F(1) => 0x70,      // VK_F1
    BaseKey::F(2) => 0x71,
    BaseKey::F(3) => 0x72,
    BaseKey::F(4) => 0x73,
    BaseKey::F(5) => 0x74,
    BaseKey::F(6) => 0x75,
    BaseKey::F(7) => 0x76,
    BaseKey::F(8) => 0x77,
    BaseKey::F(9) => 0x78,
    BaseKey::F(10) => 0x79,
    BaseKey::F(11) => 0x7A,
    BaseKey::F(12) => 0x7B,
    BaseKey::Section => VK_OEM_102, // EU keyboard § key
    BaseKey::PlusMinus => 0xC0,     // VK_OEM_3 (backtick position)
    _ => 0xC0,                      // Fallback to backtick
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
