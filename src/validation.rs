//! Input validation for configuration values.
//!
//! This module provides validation functions for:
//! - Hotkey strings (e.g., "Meta+Grave", "Ctrl+Alt+F1")
//! - Easing curve names and cubic bezier specifications
//!
//! ## Hotkey Format
//!
//! Hotkeys consist of optional modifiers followed by a base key:
//! - Modifiers: `ctrl`, `alt`, `shift`, `meta`/`super`/`win`/`cmd`
//! - Base keys: Single letters, digits, function keys (F1-F12), punctuation, etc.
//!
//! ## Easing Curves
//!
//! Supports named curves and custom cubic bezier:
//! - Named: `ease`, `linear`, `sine-in-out`, `back-out`, `expo`, etc.
//! - Custom: `cubic-bezier(x1, y1, x2, y2)` or `(x1, y1, x2, y2)`

// =============================================================================
// Hotkey Validation
// =============================================================================

/// Valid identifiers for hotkey modifiers.
pub const MODIFIERS: &[&str] = &[
  "ctrl", "control", "alt", "shift", "meta", "super", "win", "cmd",
];

/// Checks if a string is a valid hotkey modifier.
pub fn is_modifier(s: &str) -> bool {
  MODIFIERS.iter().any(|&m| m.eq_ignore_ascii_case(s))
}

/// Split a hotkey string into its parts, supporting both `+` and `-` as separators.
pub fn split_hotkey(s: &str) -> Vec<&str> {
  let s_trimmed = s.trim();
  if s.contains('+') {
    s.split('+').collect()
  } else if s.contains('-') && s_trimmed != "-" && !s_trimmed.eq_ignore_ascii_case("minus") {
    // Only split by '-' if it's not the base key itself
    s.split('-').collect()
  } else {
    vec![s]
  }
}

/// Validates a hotkey string format.
///
/// # Format
/// `[Modifier+]...[Modifier+]Key`
///
/// # Example Valid Hotkeys
/// - `"Meta+Grave"` - Super key + backtick
/// - `"Ctrl+Alt+Delete"` - Control + Alt + Delete
/// - `"F1"` - F1 key alone
///
/// # Errors
/// Returns an error if:
/// - No base key is specified (modifiers only)
/// - Multiple base keys are found
/// - Unknown key name is used
/// - Empty parts exist (e.g., "Meta++Grave")
pub fn validate_hotkey(s: &str) -> Result<(), String> {
  let mut has_base_key = false;

  for part in split_hotkey(s) {
    let part = part.trim();
    if part.is_empty() {
      return Err("Empty key part (double separator or trailing separator?)".to_string());
    }

    if is_modifier(part) {
      continue;
    }

    if is_valid_base_key(part) {
      if has_base_key {
        return Err(
          "Multiple base keys found: use only one base key (e.g., 'F1') per shortcut.".to_string(),
        );
      }
      has_base_key = true;
      continue;
    }

    // It is unknown (not a modifier and not a valid base key)
    let part_lower = part.to_lowercase();
    let suggestion = if has_base_key {
      // If we already have a base key, this MUST have been intended as a modifier
      crate::matching::suggest_similar(&part_lower, MODIFIERS)
    } else {
      // Otherwise it could be a typo of either — check both and pick best
      {
        let all: Vec<&str> = MODIFIERS.iter().chain(VALID_KEYS.iter()).copied().collect();
        crate::matching::suggest_similar(&part_lower, &all)
      }
    };

    let hint = if let Some(s) = suggestion {
      format!(" Did you mean '{}'?", s)
    } else if has_base_key {
      format!(" Valid modifiers include: {}.", MODIFIERS.join(", "))
    } else {
      format!(
        " Valid modifiers: {}. Valid base keys include: {}.",
        MODIFIERS.join(", "),
        VALID_KEYS.join(", ")
      )
    };
    return Err(format!("Unknown key name: '{}'{}", part, hint));
  }

  if !has_base_key {
    return Err("No base key specified (e.g., 'Meta+F1' - 'Meta' is just a modifier)".to_string());
  }

  Ok(())
}

/// All valid base key names for hotkey configuration.
/// This is the single source of truth for both validation and fuzzy suggestions.
pub const VALID_KEYS: &[&str] = &[
  // Special keys
  "grave",
  "`",
  "backtick",
  "section",
  "§",
  "plusminus",
  "±",
  "minus",
  "-",
  "equal",
  "=",
  "dead_grave",
  // Punctuation
  "bracketleft",
  "[",
  "bracketright",
  "]",
  "backslash",
  "\\",
  "semicolon",
  ";",
  "quote",
  "'",
  "comma",
  ",",
  "period",
  ".",
  "slash",
  "/",
  // Editing
  "enter",
  "return",
  "space",
  "esc",
  "escape",
  "tab",
  "capslock",
  "caps_lock",
  "backspace",
  // Navigation
  "up",
  "arrowup",
  "down",
  "arrowdown",
  "left",
  "arrowleft",
  "right",
  "arrowright",
  "pgup",
  "pageup",
  "pgdn",
  "pagedown",
  "home",
  "end",
  "insert",
  "delete",
  "del",
  // Function keys
  "f1",
  "f2",
  "f3",
  "f4",
  "f5",
  "f6",
  "f7",
  "f8",
  "f9",
  "f10",
  "f11",
  "f12",
];

/// Canonicalizes a hotkey for duplicate detection.
///
/// Maps synonyms (e.g., "Win" -> "Meta") and aliases (e.g., "Return" -> "Enter")
/// to a stable string representation, with modifiers sorted alphabetically.
pub fn canonicalize_hotkey(s: &str) -> String {
  let mut mods = Vec::new();
  let mut base = String::new();

  for part in split_hotkey(s).into_iter().map(|s| s.trim().to_lowercase()) {
    match part.as_str() {
      // Modifier synonyms
      "ctrl" | "control" => mods.push("ctrl".to_string()),
      "meta" | "super" | "win" | "cmd" => mods.push("meta".to_string()),
      "alt" | "shift" => mods.push(part),
      // Base key aliases handled by the enum translation
      _ => {
        if let Some(key) = BaseKey::parse(&part) {
          base = key.to_canonical_string();
        } else {
          base = part;
        }
      }
    }
  }

  mods.sort();
  if mods.is_empty() {
    base
  } else {
    format!("{}+{}", mods.join("+"), base)
  }
}

/// Unified abstraction for a physical key, shared across all platforms.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BaseKey {
  Digit(u8),
  Letter(char),
  F(u8),
  Grave,
  Section,
  PlusMinus,
  Minus,
  Equal,
  BracketLeft,
  BracketRight,
  Backslash,
  Semicolon,
  Quote,
  Comma,
  Period,
  Slash,
  Enter,
  Space,
  Esc,
  Tab,
  CapsLock,
  Backspace,
  Up,
  Down,
  Left,
  Right,
  PageUp,
  PageDown,
  Home,
  End,
  Insert,
  Delete,
}

impl BaseKey {
  /// Parses a string into a BaseKey, handling all common synonyms and aliases.
  pub fn parse(s: &str) -> Option<Self> {
    let s = s.trim().to_lowercase();
    match s.as_str() {
      "grave" | "backtick" | "`" | "dead_grave" => Some(BaseKey::Grave),
      "section" | "§" => Some(BaseKey::Section),
      "plusminus" | "±" => Some(BaseKey::PlusMinus),
      "minus" | "-" => Some(BaseKey::Minus),
      "equal" | "=" => Some(BaseKey::Equal),
      "bracketleft" | "[" => Some(BaseKey::BracketLeft),
      "bracketright" | "]" => Some(BaseKey::BracketRight),
      "backslash" | "\\" => Some(BaseKey::Backslash),
      "semicolon" | ";" => Some(BaseKey::Semicolon),
      "quote" | "'" => Some(BaseKey::Quote),
      "comma" | "," => Some(BaseKey::Comma),
      "period" | "." => Some(BaseKey::Period),
      "slash" | "/" => Some(BaseKey::Slash),
      "enter" | "return" => Some(BaseKey::Enter),
      "space" => Some(BaseKey::Space),
      "esc" | "escape" => Some(BaseKey::Esc),
      "tab" => Some(BaseKey::Tab),
      "capslock" | "caps_lock" => Some(BaseKey::CapsLock),
      "backspace" => Some(BaseKey::Backspace),
      "up" | "arrowup" => Some(BaseKey::Up),
      "down" | "arrowdown" => Some(BaseKey::Down),
      "left" | "arrowleft" => Some(BaseKey::Left),
      "right" | "arrowright" => Some(BaseKey::Right),
      "pgup" | "pageup" => Some(BaseKey::PageUp),
      "pgdn" | "pagedown" => Some(BaseKey::PageDown),
      "home" => Some(BaseKey::Home),
      "end" => Some(BaseKey::End),
      "insert" => Some(BaseKey::Insert),
      "delete" | "del" => Some(BaseKey::Delete),
      _ => {
        if s.starts_with('f') && s.len() > 1 {
          if let Ok(num) = s[1..].parse::<u8>() {
            if num >= 1 && num <= 12 {
              return Some(BaseKey::F(num));
            }
          }
        }
        if s.len() == 1 {
          let c = s.chars().next().unwrap();
          if c.is_ascii_digit() {
            return Some(BaseKey::Digit(c.to_digit(10).unwrap() as u8));
          }
          if c.is_ascii_alphabetic() {
            return Some(BaseKey::Letter(c));
          }
        }
        None
      }
    }
  }

  /// Returns the canonical, stable string representation of the key.
  pub fn to_canonical_string(&self) -> String {
    match self {
      BaseKey::Digit(n) => n.to_string(),
      BaseKey::Letter(c) => c.to_string(),
      BaseKey::F(n) => format!("f{}", n),
      BaseKey::Grave => "grave".to_string(),
      BaseKey::Section => "section".to_string(),
      BaseKey::PlusMinus => "plusminus".to_string(),
      BaseKey::Minus => "minus".to_string(),
      BaseKey::Equal => "equal".to_string(),
      BaseKey::BracketLeft => "bracketleft".to_string(),
      BaseKey::BracketRight => "bracketright".to_string(),
      BaseKey::Backslash => "backslash".to_string(),
      BaseKey::Semicolon => "semicolon".to_string(),
      BaseKey::Quote => "quote".to_string(),
      BaseKey::Comma => "comma".to_string(),
      BaseKey::Period => "period".to_string(),
      BaseKey::Slash => "slash".to_string(),
      BaseKey::Enter => "enter".to_string(),
      BaseKey::Space => "space".to_string(),
      BaseKey::Esc => "esc".to_string(),
      BaseKey::Tab => "tab".to_string(),
      BaseKey::CapsLock => "capslock".to_string(),
      BaseKey::Backspace => "backspace".to_string(),
      BaseKey::Up => "up".to_string(),
      BaseKey::Down => "down".to_string(),
      BaseKey::Left => "left".to_string(),
      BaseKey::Right => "right".to_string(),
      BaseKey::PageUp => "pgup".to_string(),
      BaseKey::PageDown => "pgdn".to_string(),
      BaseKey::Home => "home".to_string(),
      BaseKey::End => "end".to_string(),
      BaseKey::Insert => "insert".to_string(),
      BaseKey::Delete => "delete".to_string(),
    }
  }
}

/// Checks if a string is a valid base key name.
///
/// Supports:
/// - Single alphanumeric characters (a-z, 0-9)
/// - Special keys (grave, section, minus, etc.)
/// - Punctuation keys
/// - Navigation keys (arrows, page up/down, home, end)
/// - Function keys (F1-F12)
pub fn is_valid_base_key(s: &str) -> bool {
  // Single alphanumeric characters are always valid
  if s.len() == 1 && s.chars().next().unwrap().is_ascii_alphanumeric() {
    return true;
  }
  // Check against our canonical list (case-insensitive to avoid allocations)
  VALID_KEYS.iter().any(|&k| k.eq_ignore_ascii_case(s))
}

/// Parses a cubic-bezier easing curve specification.
///
/// # Supported Formats
/// - `"cubic-bezier(x1, y1, x2, y2)"` (CSS standard)
/// - `"bezier(x1, y1, x2, y2)"` (shorthand)
/// - `"(x1, y1, x2, y2)"` (minimal)
///
/// # Returns
/// The four control points `(x1, y1, x2, y2)` for the cubic bezier.
/// The x values should be in [0, 1], but y values can exceed this range
/// for overshoot effects (like "back" easing).
///
/// Returns `None` if the format is invalid or values cannot be parsed.
///
/// # Example
/// ```
/// # use janq::validation::parse_bezier;
/// assert_eq!(parse_bezier("cubic-bezier(0.25, 0.1, 0.25, 1.0)"), Some((0.25, 0.1, 0.25, 1.0)));
/// assert_eq!(parse_bezier("(0, 1, 1, 0)"), Some((0.0, 1.0, 1.0, 0.0)));
/// assert_eq!(parse_bezier("invalid"), None);
/// ```
pub fn parse_bezier(type_: &str) -> Option<(f64, f64, f64, f64)> {
  let s = type_.trim().to_lowercase();

  let content = if s.ends_with(')') {
    if let Some(stripped) = s.strip_prefix("cubic-bezier(") {
      stripped.strip_suffix(')')?
    } else if let Some(stripped) = s.strip_prefix("bezier(") {
      stripped.strip_suffix(')')?
    } else if s.starts_with('(') {
      &s[1..s.len() - 1]
    } else {
      return None;
    }
  } else {
    return None;
  };

  let mut parts = content.split(',');
  let x1 = parts.next()?.trim().parse::<f64>().ok()?;
  let y1 = parts.next()?.trim().parse::<f64>().ok()?;
  let x2 = parts.next()?.trim().parse::<f64>().ok()?;
  let y2 = parts.next()?.trim().parse::<f64>().ok()?;

  if parts.next().is_some() {
    return None;
  }

  Some((x1, y1, x2, y2))
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
  use super::*;

  // --- Hotkey validation tests ---

  #[test]
  fn test_valid_hotkeys() {
    assert!(validate_hotkey("Meta+Grave").is_ok());
    assert!(validate_hotkey("Ctrl+Alt+Delete").is_ok());
    assert!(validate_hotkey("F1").is_ok());
    assert!(validate_hotkey("Shift+F12").is_ok());
    assert!(validate_hotkey("Meta+Section").is_ok());
  }

  #[test]
  fn test_invalid_hotkeys() {
    // No base key
    assert!(validate_hotkey("Meta").is_err());
    assert!(validate_hotkey("Ctrl+Alt").is_err());

    // Multiple base keys
    assert!(validate_hotkey("Meta+A+B").is_err());

    // Unknown key
    assert!(validate_hotkey("Meta+UnknownKey").is_err());

    // Empty part
    assert!(validate_hotkey("Meta++Grave").is_err());
    assert!(validate_hotkey("Meta+").is_err());
  }

  // --- Base key validation tests ---

  #[test]
  fn test_valid_base_keys() {
    // Single letters
    assert!(is_valid_base_key("a"));
    assert!(is_valid_base_key("z"));

    // Digits
    assert!(is_valid_base_key("0"));
    assert!(is_valid_base_key("9"));

    // Function keys
    assert!(is_valid_base_key("f1"));
    assert!(is_valid_base_key("f12"));

    // Special keys
    assert!(is_valid_base_key("grave"));
    assert!(is_valid_base_key("space"));
    assert!(is_valid_base_key("enter"));
  }

  #[test]
  fn test_invalid_base_keys() {
    assert!(!is_valid_base_key("unknownkey"));
    assert!(!is_valid_base_key("f13"));
    assert!(!is_valid_base_key(""));
  }

  // --- Bezier parsing tests ---

  #[test]
  fn test_parse_bezier() {
    assert_eq!(
      parse_bezier("cubic-bezier(0, 0.5, 0.5, 1)"),
      Some((0.0, 0.5, 0.5, 1.0))
    );
    assert_eq!(parse_bezier("(0, 1, 1, 0)"), Some((0.0, 1.0, 1.0, 0.0)));
    assert_eq!(
      parse_bezier(" ( 0.1 , 0.2 , 0.3 , 0.4 ) "),
      Some((0.1, 0.2, 0.3, 0.4))
    );
    assert_eq!(parse_bezier("linear"), None);
    assert_eq!(parse_bezier("cubic-bezier(1, 2, 3)"), None);
    assert_eq!(parse_bezier("(1, 2, 3, 4, 5)"), None);
    assert_eq!(parse_bezier("cubic-bezier(a, b, c, d)"), None);
  }
}
