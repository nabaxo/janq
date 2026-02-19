//! KDE keyboard shortcut registration via D-Bus.
//!
//! ## Architecture
//!
//! janq registers hotkeys with KDE's global shortcut system (KGlobalAccel)
//! rather than grabbing keys directly. This provides:
//! - Native KDE System Settings integration
//! - Conflict detection with other applications
//! - Persistence across reboots (via desktop file)
//!
//! ## Registration Flow
//!
//! 1. Generate `.desktop` file with `X-KDE-Shortcuts` entries
//! 2. Run `kbuildsycoca6` to update KDE's cache
//! 3. Register actions via `org.kde.KGlobalAccel.doRegister`
//! 4. Set keybindings via `org.kde.KGlobalAccel.setShortcut`
//!
//! ## Key Mapping
//!
//! Shortcuts are converted to Qt keycodes (e.g., Meta+Grave → 0x10000060).
//! The `map_qt_key` function handles this translation.

use std::collections::HashSet;

use janq::config::Config;
use janq::validation;

// =============================================================================
// Shortcut Normalization
// =============================================================================

/// Normalize a shortcut string to KDE's expected format for display in System Settings.
/// Converts internal names like "Grave", "Section" to the format KDE uses.
pub fn normalize_shortcut_for_kde(shortcut: &str) -> String {
  let mut normalized = String::with_capacity(shortcut.len());
  for (i, part) in validation::split_hotkey(shortcut).into_iter().enumerate() {
    if i > 0 {
      normalized.push('+');
    }
    let p = part.trim().to_lowercase();

    match janq::config::normalize_hotkey_modifier(&p) {
      "meta" => normalized.push_str("Meta"),
      "ctrl" => normalized.push_str("Ctrl"),
      "alt" => normalized.push_str("Alt"),
      "shift" => normalized.push_str("Shift"),
      _ => {
        if let Some(base_key) = validation::BaseKey::parse(&p) {
          normalized.push_str(&to_kde_display(base_key));
        } else {
          normalized.push_str(&p);
        }
      }
    }
  }
  normalized
}

fn to_kde_display(key: validation::BaseKey) -> String {
  use validation::BaseKey;
  match key {
    BaseKey::Grave => "`".to_string(),
    BaseKey::Section => "§".to_string(),
    BaseKey::PlusMinus => "±".to_string(),
    BaseKey::F(n) => format!("F{}", n),
    BaseKey::Esc => "Escape".to_string(),
    BaseKey::Tab => "Tab".to_string(),
    BaseKey::Space => "Space".to_string(),
    BaseKey::Enter => "Return".to_string(),
    BaseKey::Backspace => "Backspace".to_string(),
    BaseKey::Delete => "Delete".to_string(),
    BaseKey::Insert => "Insert".to_string(),
    BaseKey::Home => "Home".to_string(),
    BaseKey::End => "End".to_string(),
    BaseKey::PageUp => "PgUp".to_string(),
    BaseKey::PageDown => "PgDown".to_string(),
    BaseKey::Up => "Up".to_string(),
    BaseKey::Down => "Down".to_string(),
    BaseKey::Left => "Left".to_string(),
    BaseKey::Right => "Right".to_string(),
    BaseKey::CapsLock => "Caps Lock".to_string(),
    BaseKey::Minus => "-".to_string(),
    BaseKey::Equal => "=".to_string(),
    BaseKey::BracketLeft => "[".to_string(),
    BaseKey::BracketRight => "]".to_string(),
    BaseKey::Backslash => "\\".to_string(),
    BaseKey::Semicolon => ";".to_string(),
    BaseKey::Quote => "'".to_string(),
    BaseKey::Comma => ",".to_string(),
    BaseKey::Period => ".".to_string(),
    BaseKey::Slash => "/".to_string(),
    BaseKey::Letter(c) => c.to_ascii_uppercase().to_string(),
    BaseKey::Digit(n) => n.to_string(),
  }
}

// =============================================================================
// Qt Key Mapping
// =============================================================================
// NOTE: This now uses the shared BaseKey abstraction, ensuring that if a key
// name is valid in the config, it is guaranteed to be handled here.

fn map_qt_key(s: &str) -> i32 {
  let s_lower = s.to_lowercase();
  match janq::config::normalize_hotkey_modifier(&s_lower) {
    "meta" => 0x10000000,
    "ctrl" => 0x04000000,
    "alt" => 0x08000000,
    "shift" => 0x02000000,
    _ => {
      if let Some(base_key) = validation::BaseKey::parse(&s_lower) {
        to_qt_code(base_key)
      } else {
        0
      }
    }
  }
}

fn to_qt_code(key: validation::BaseKey) -> i32 {
  use validation::BaseKey;
  match key {
    BaseKey::Digit(0) => 0x30,
    BaseKey::Digit(1) => 0x31,
    BaseKey::Digit(2) => 0x32,
    BaseKey::Digit(3) => 0x33,
    BaseKey::Digit(4) => 0x34,
    BaseKey::Digit(5) => 0x35,
    BaseKey::Digit(6) => 0x36,
    BaseKey::Digit(7) => 0x37,
    BaseKey::Digit(8) => 0x38,
    BaseKey::Digit(9) => 0x39,
    BaseKey::Minus => 0x2d,
    BaseKey::Equal => 0x3d,
    BaseKey::Letter('q') => 0x51,
    BaseKey::Letter('w') => 0x57,
    BaseKey::Letter('e') => 0x45,
    BaseKey::Letter('r') => 0x52,
    BaseKey::Letter('t') => 0x54,
    BaseKey::Letter('y') => 0x59,
    BaseKey::Letter('u') => 0x55,
    BaseKey::Letter('i') => 0x49,
    BaseKey::Letter('o') => 0x4f,
    BaseKey::Letter('p') => 0x50,
    BaseKey::BracketLeft => 0x5b,
    BaseKey::BracketRight => 0x5d,
    BaseKey::Backslash => 0x5c,
    BaseKey::Letter('a') => 0x41,
    BaseKey::Letter('s') => 0x53,
    BaseKey::Letter('d') => 0x44,
    BaseKey::Letter('f') => 0x46,
    BaseKey::Letter('g') => 0x47,
    BaseKey::Letter('h') => 0x48,
    BaseKey::Letter('j') => 0x4a,
    BaseKey::Letter('k') => 0x4b,
    BaseKey::Letter('l') => 0x4c,
    BaseKey::Semicolon => 0x3b,
    BaseKey::Quote => 0x27,
    BaseKey::Enter => 0x01000004,
    BaseKey::Letter('z') => 0x5a,
    BaseKey::Letter('x') => 0x58,
    BaseKey::Letter('c') => 0x43,
    BaseKey::Letter('v') => 0x56,
    BaseKey::Letter('b') => 0x42,
    BaseKey::Letter('n') => 0x4e,
    BaseKey::Letter('m') => 0x4d,
    BaseKey::Comma => 0x2c,
    BaseKey::Period => 0x2e,
    BaseKey::Slash => 0x2f,
    BaseKey::Space => 0x20,
    BaseKey::Esc => 0x01000000,
    BaseKey::Tab => 0x01000001,
    BaseKey::CapsLock => 0x01000024,
    BaseKey::Backspace => 0x01000003,
    BaseKey::Up => 0x01000013,
    BaseKey::Down => 0x01000015,
    BaseKey::Left => 0x01000012,
    BaseKey::Right => 0x01000014,
    BaseKey::PageUp => 0x01000016,
    BaseKey::PageDown => 0x01000017,
    BaseKey::Home => 0x01000010,
    BaseKey::End => 0x01000011,
    BaseKey::Insert => 0x01000006,
    BaseKey::Delete => 0x01000007,
    BaseKey::F(1) => 0x01000030,
    BaseKey::F(2) => 0x01000031,
    BaseKey::F(3) => 0x01000032,
    BaseKey::F(4) => 0x01000033,
    BaseKey::F(5) => 0x01000034,
    BaseKey::F(6) => 0x01000035,
    BaseKey::F(7) => 0x01000036,
    BaseKey::F(8) => 0x01000037,
    BaseKey::F(9) => 0x01000038,
    BaseKey::F(10) => 0x01000039,
    BaseKey::F(11) => 0x0100003a,
    BaseKey::F(12) => 0x0100003b,
    BaseKey::Grave => 0x60,
    BaseKey::Section | BaseKey::PlusMinus => 0xa7,
    _ => 0,
  }
}

fn shortcut_to_keycode(shortcut: &str) -> janq::error::Result<i32> {
  let mut total = 0;
  for part in validation::split_hotkey(shortcut)
    .into_iter()
    .map(|p: &str| p.trim())
  {
    if part.is_empty() {
      continue;
    }
    let p = part.to_lowercase();
    let key = map_qt_key(&p);
    if key == 0 {
      return Err(janq::format_error_boxed!(
        "Unknown Linux key name: '{}'",
        part
      ));
    }
    total += key;
  }

  Ok(total)
}

#[zbus::proxy(
  interface = "org.kde.KGlobalAccel",
  default_service = "org.kde.kglobalaccel",
  default_path = "/kglobalaccel"
)]
trait KGlobalAccel {
  #[zbus(name = "allActionsForComponent")]
  fn all_actions_for_component(&self, action_id: Vec<String>) -> zbus::Result<Vec<Vec<String>>>;

  #[zbus(name = "setShortcut")]
  fn set_shortcut(
    &self,
    action_id: Vec<String>,
    keys: Vec<i32>,
    flags: u32,
  ) -> zbus::Result<Vec<i32>>;

  #[zbus(name = "doRegister")]
  fn do_register(&self, action_id: Vec<String>) -> zbus::Result<()>;

  #[zbus(name = "unregister")]
  fn unregister(&self, component_unique: &str, shortcut_unique: &str) -> zbus::Result<bool>;
}

// =============================================================================
// D-Bus Shortcut Registration
// =============================================================================

pub async fn register_via_dbus(
  config: &Config,
  old_config: Option<&Config>,
  conn: &zbus::Connection,
) -> janq::error::Result<()> {
  let component = "dev.nabaxo.janq.desktop";
  let proxy = KGlobalAccelProxy::new(conn).await?;

  // 1. FAST PATH: Return immediately if state is correct
  let mut needs_refresh = false;

  // Detection A: Configuration Change (Compared to memory)
  // On cold startup (old_config is None), always force a full refresh
  // to ensure clean D-Bus state after reboot or abrupt shutdown
  if let Some(old) = old_config {
    if old.app.len() != config.app.len() || old.app.keys().ne(config.app.keys()) {
      needs_refresh = true;
    } else {
      for (name, app_cfg) in &config.app {
        if let Some(old_app) = old.app.get(name) {
          if old_app.hotkey != app_cfg.hotkey {
            needs_refresh = true;
            break;
          }
        } else {
          needs_refresh = true;
          break;
        }
      }
    }
  } else {
    // Cold startup: always refresh to ensure clean state
    println!("Hotkey: Cold startup detected, performing full sync...");
    needs_refresh = true;
  }

  // Detection B: D-Bus State Validation (Startup or corruption check)
  if !needs_refresh {
    if let Ok(all_actions) = proxy
      .all_actions_for_component(vec![component.to_string()])
      .await
    {
      let mut found_actions = HashSet::new();
      for action_info in all_actions {
        if action_info.len() >= 2 {
          let action_name = &action_info[1];
          if action_name == "_launch" {
            continue;
          }
          found_actions.insert(action_name.clone());
        }
      }

      // Refresh if we are missing any apps or have extras we shouldn't
      for app_name in config.app.keys() {
        if !found_actions.contains(app_name) {
          needs_refresh = true;
          break;
        }
      }
      if !needs_refresh {
        for action_name in &found_actions {
          if !config.app.contains_key(action_name) {
            needs_refresh = true;
            break;
          }
        }
      }
    } else {
      needs_refresh = true;
    }
  }

  if !needs_refresh {
    return Ok(());
  }

  // 2. SLOW PATH: Full Refresh (Proven working method for "Default" status)
  println!("Hotkey: Configuration changed or missing in KDE, performing full sync...");

  // D-BUS UNREGISTER: Release legacy and current actions to ensure a clean slate
  let components_to_clean = ["dev.nabaxo.janq.desktop", "janq"];
  for comp in components_to_clean {
    if let Ok(all_actions) = proxy
      .all_actions_for_component(vec![comp.to_string()])
      .await
    {
      for action_info in all_actions {
        if action_info.len() >= 2 {
          let action_name = &action_info[1];
          let _ = proxy.unregister(comp, action_name).await;
        }
      }
    }
  }

  // Force KDE to reload desktop files (where our X-KDE-Shortcuts defaults are)
  let _ = tokio::process::Command::new("kbuildsycoca6")
    .arg("--noincremental")
    .status()
    .await;

  // Reliable delay ensuring Plasma 6 internal registry update
  tokio::time::sleep(tokio::time::Duration::from_millis(1500)).await;

  // Register all apps and set shortcuts
  for app_name in config.app.keys() {
    if let Some(app_cfg) = config.app.get(app_name) {
      let hotkeys = app_cfg.hotkey.as_vec();
      if hotkeys.is_empty() {
        continue;
      }

      let display_name = format!("Toggle {}", app_name);
      let action_id = vec![
        component.to_string(),
        app_name.to_string(),
        "janq".to_string(),
        display_name,
      ];

      // Perform standard registration sequence
      proxy
        .do_register(action_id.clone())
        .await
        .map_err(|e| janq::format_error_boxed!("do_register failed for '{}': {}", app_name, e))?;
      tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

      // Build the key sequence (up to 4 keys)
      let mut key_seq = vec![0; 4];
      for (i, hk_str) in hotkeys.iter().enumerate() {
        // Validation already performed as pre-step at the top of the function
        key_seq[i] = shortcut_to_keycode(hk_str).unwrap_or(0);
      }

      proxy
        .set_shortcut(action_id, key_seq, 3)
        .await
        .map_err(|e| janq::format_error_boxed!("set_shortcut failed for '{}': {}", app_name, e))?;

      tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    }
  }

  Ok(())
}

pub async fn sync_kde_shortcuts(
  config: &Config,
  old_config: Option<&Config>,
  conn: &zbus::Connection,
) -> janq::error::Result<()> {
  tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
  register_via_dbus(config, old_config, conn).await
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn test_normalize_shortcut_for_kde() {
    assert_eq!(normalize_shortcut_for_kde("Meta+Grave"), "Meta+`");
    assert_eq!(normalize_shortcut_for_kde("meta+grave"), "Meta+`");
    assert_eq!(normalize_shortcut_for_kde("Ctrl+Alt+F12"), "Ctrl+Alt+F12");
    assert_eq!(normalize_shortcut_for_kde("shift+a"), "Shift+A");
    assert_eq!(normalize_shortcut_for_kde("Super+Section"), "Meta+§");
  }

  #[test]
  fn test_map_qt_key() {
    // Modifiers
    assert_eq!(map_qt_key("meta"), 0x10000000);
    assert_eq!(map_qt_key("ctrl"), 0x04000000);
    assert_eq!(map_qt_key("alt"), 0x08000000);
    assert_eq!(map_qt_key("shift"), 0x02000000);

    // Special keys
    assert_eq!(map_qt_key("grave"), 0x60);
    assert_eq!(map_qt_key("`"), 0x60);
    assert_eq!(map_qt_key("section"), 0xa7);

    // Function keys
    assert_eq!(map_qt_key("f1"), 0x01000030);
    assert_eq!(map_qt_key("f12"), 0x0100003b);

    // Alphanumeric
    assert_eq!(map_qt_key("a"), 0x41);
    assert_eq!(map_qt_key("z"), 0x5a);
    assert_eq!(map_qt_key("1"), 0x31);
  }
}
