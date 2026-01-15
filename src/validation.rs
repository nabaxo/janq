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
  let parts: Vec<&str> = s.split('+').map(|p| p.trim()).collect();
  let mut has_base_key = false;

  for part in parts {
    let p = part.to_lowercase();
    match p.as_str() {
      "ctrl" | "control" | "alt" | "shift" | "meta" | "super" | "win" | "cmd" => {}
      "" => return Err("Empty key part (double plus or trailing plus?)".to_string()),
      _ => {
        // Must be the base key
        if has_base_key {
          return Err(
            "Multiple base keys found: use only one base key (e.g., 'F1') per shortcut."
              .to_string(),
          );
        }

        // Validate base key name
        if !is_valid_base_key(&p) {
          return Err(format!("Unknown or invalid key name: '{}'", part));
        }
        has_base_key = true;
      }
    }
  }

  if !has_base_key {
    return Err("No base key specified (e.g., 'Meta+F1' - 'Meta' is just a modifier)".to_string());
  }

  Ok(())
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
  match s {
    // Alphanumeric
    s if s.len() == 1 && s.chars().next().unwrap().is_ascii_alphanumeric() => true,
    // Special keys
    "grave" | "`" | "backtick" | "section" | "§" | "plusminus" | "±" | "minus" | "-" | "equal"
    | "=" | "dead_grave" => true,
    "bracketleft" | "[" | "bracketright" | "]" | "backslash" | "\\" | "semicolon" | ";"
    | "quote" | "'" | "comma" | "," | "period" | "." | "slash" | "/" => true,
    "enter" | "return" | "space" | "esc" | "escape" | "tab" | "capslock" | "caps_lock"
    | "backspace" => true,
    "up" | "arrowup" | "down" | "arrowdown" | "left" | "arrowleft" | "right" | "arrowright" => true,
    "pgup" | "pageup" | "pgdn" | "pagedown" | "home" | "end" | "insert" | "delete" | "del" => true,
    "f1" | "f2" | "f3" | "f4" | "f5" | "f6" | "f7" | "f8" | "f9" | "f10" | "f11" | "f12" => true,
    _ => false,
  }
}

// =============================================================================
// Easing Validation
// =============================================================================

/// Checks if a string is a valid easing curve name or bezier specification.
///
/// # Valid Named Curves
/// - Standard: `ease`, `ease-in`, `ease-out`, `ease-in-out`, `linear`
/// - Sine: `sine`, `sine-in`, `sine-out`, `sine-in-out`
/// - Cubic: `cubic`, `cubic-in`, `cubic-out`, `cubic-in-out`
/// - Quart: `quart`, `quart-in`, `quart-out`, `quart-in-out`
/// - Back: `back`, `back-in`, `back-out`, `back-in-out`
/// - Expo: `expo`, `expo-in`, `expo-out`, `expo-in-out`
/// - Special: `windows` (Windows native animation curve)
///
/// # Custom Bezier
/// Also accepts custom cubic bezier curves:
/// - `cubic-bezier(0.25, 0.1, 0.25, 1.0)`
/// - `bezier(0.25, 0.1, 0.25, 1.0)`
/// - `(0.25, 0.1, 0.25, 1.0)`
pub fn is_valid_easing(s: &str) -> bool {
  match s {
    "sine" | "sine-in-out" | "in-out-sine" | "sine-in" | "in-sine" | "sine-out" | "out-sine"
    | "quart" | "quart-in-out" | "in-out-quart" | "quart-in" | "in-quart" | "quart-out"
    | "out-quart" | "cubic" | "cubic-in-out" | "in-out-cubic" | "cubic-in" | "in-cubic"
    | "cubic-out" | "out-cubic" | "back" | "back-in-out" | "in-out-back" | "back-in"
    | "in-back" | "back-out" | "out-back" | "expo" | "expo-in-out" | "in-out-expo" | "expo-in"
    | "in-expo" | "expo-out" | "out-expo" | "ease" | "ease-in-out" | "linear" | "ease-in"
    | "ease-out" | "windows" => true,
    _ => parse_bezier(s).is_some(),
  }
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
  let content = if s.starts_with("cubic-bezier(") && s.ends_with(')') {
    &s["cubic-bezier(".len()..s.len() - 1]
  } else if s.starts_with("bezier(") && s.ends_with(')') {
    &s["bezier(".len()..s.len() - 1]
  } else if s.starts_with('(') && s.ends_with(')') {
    &s[1..s.len() - 1]
  } else {
    return None;
  };

  let parts: Vec<&str> = content.split(',').map(|p| p.trim()).collect();
  if parts.len() != 4 {
    return None;
  }

  let x1 = parts[0].parse::<f64>().ok()?;
  let y1 = parts[1].parse::<f64>().ok()?;
  let x2 = parts[2].parse::<f64>().ok()?;
  let y2 = parts[3].parse::<f64>().ok()?;

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

  // --- Easing validation tests ---

  #[test]
  fn test_valid_easing_names() {
    assert!(is_valid_easing("ease"));
    assert!(is_valid_easing("linear"));
    assert!(is_valid_easing("ease-in"));
    assert!(is_valid_easing("ease-out"));
    assert!(is_valid_easing("sine-in-out"));
    assert!(is_valid_easing("back-out"));
    assert!(is_valid_easing("expo"));
    assert!(is_valid_easing("windows"));
  }

  #[test]
  fn test_valid_bezier() {
    assert!(is_valid_easing("cubic-bezier(0, 1, 1, 0)"));
    assert!(is_valid_easing("bezier(0, 1, 1, 0)"));
    assert!(is_valid_easing("(0, 1, 1, 0)"));
  }

  #[test]
  fn test_invalid_easing() {
    assert!(!is_valid_easing("invalid"));
    assert!(!is_valid_easing("cubic-bezier(1, 2)")); // Wrong number of params
    assert!(!is_valid_easing("cubic-bezier(a, b, c, d)")); // Non-numeric
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
