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
          let hint = crate::matching::suggest_similar(&p, VALID_KEYS)
            .map(|s| format!(" Did you mean '{}'?", s))
            .unwrap_or_else(|| format!(" Valid base keys include: {}.", VALID_KEYS.join(", ")));
          return Err(format!("Unknown key name: '{}'{}", part, hint));
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
  // Check against our canonical list
  VALID_KEYS.contains(&s)
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
