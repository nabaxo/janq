//! Configuration loading and validation for janq.
//!
//! This module handles:
//! - TOML config file loading from standard locations
//! - Input validation (hotkeys, easing curves, dimensions)
//! - Configuration structs and deserialization
//!
//! Config search priority:
//! 1. Executable directory (`./janq.toml`)
//! 2. XDG config directory (`~/.config/janq/janq.toml`)
//! 3. Home directory (`~/.janq.toml`)
//!
//! ## Re-exports
//!
//! For backward compatibility, `FoundWindow` and `fuzzy_match_window` are
//! re-exported from the [`crate::matching`] module.

use rustc_hash::FxHashMap;
use std::{collections::HashSet, env::current_exe, fmt, fs, path::PathBuf};

use dirs::{config_dir, home_dir};
use indexmap::IndexMap; // Preserves insertion order for deterministic app iteration
use serde::{
  de::{self, value::MapAccessDeserializer, Deserializer, Visitor},
  Deserialize,
};

// Re-export matching types so other modules can import from config
pub use crate::matching::{fuzzy_match_window, FoundWindow};

// Import validation functions used internally
use crate::validation::{is_valid_easing, validate_hotkey};

/// A dimension value that can be specified as percent, pixels, or unset.
///
/// Parse formats:
/// - `"50%"` → `Dimension::Percent(0.5)` (relative to screen dimension)
/// - `"800px"` → `Dimension::Pixels(800)` (absolute pixels)
/// - `"0"` or `"unset"` → `Dimension::Unset` (use window's natural size)
#[derive(Clone, Debug, PartialEq)]
pub enum Dimension {
  Percent(f64),
  Pixels(i32),
  Unset,
}

impl<'de> serde::Deserialize<'de> for Dimension {
  fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
  where
    D: serde::Deserializer<'de>,
  {
    let s = String::deserialize(deserializer)?;
    let s = s.trim().to_lowercase();
    if let Some(rest) = s.strip_suffix('%') {
      let val = rest
        .trim()
        .parse::<f64>()
        .map_err(serde::de::Error::custom)?;
      Ok(Dimension::Percent(val / 100.0))
    } else if let Some(rest) = s.strip_suffix("px") {
      let val = rest
        .trim()
        .parse::<i32>()
        .map_err(serde::de::Error::custom)?;
      Ok(Dimension::Pixels(val))
    } else if s == "0" || s == "unset" {
      Ok(Dimension::Unset)
    } else {
      Err(serde::de::Error::custom(format!(
        "Invalid dimension format: '{}'. Must end with '%' or 'px'.",
        s
      )))
    }
  }
}

// =============================================================================
// Configuration Structs
// =============================================================================

/// Root configuration structure loaded from janq.toml.
///
/// Supports both single-app and multi-app configurations:
/// ```toml
/// # Single app (implicit name from window_class)
/// [app]
/// window_class = "wezterm"
/// start_command = "wezterm"
///
/// # Multi-app (explicit names)
/// [app.terminal]
/// window_class = "wezterm"
/// [app.notes]
/// window_class = "obsidian"
/// ```
#[derive(Clone, Debug, Deserialize, Default)]
pub struct Config {
  /// App definitions. Uses IndexMap to preserve TOML declaration order.
  #[serde(
    default,
    alias = "apps",
    alias = "general",
    deserialize_with = "deserialize_app"
  )]
  pub app: IndexMap<String, AppConfig>,
  /// Global window positioning and display settings.
  #[serde(default)]
  pub window: WindowConfig,
  /// Animation timing and easing configuration.
  #[serde(default)]
  pub animation: AnimationConfig,
}

impl Config {
  pub fn validate(&self) -> Result<(), String> {
    let mut seen_hotkeys = FxHashMap::default();
    for (app_name, app_cfg) in &self.app {
      let hotkeys = app_cfg.hotkey.as_vec();

      if hotkeys.len() > 4 {
        return Err(format!(
          "App '{}' has {} hotkeys defined, but janq only supports a maximum of 4 hotkeys per application.",
          app_name,
          hotkeys.len()
        ));
      }

      for key in hotkeys {
        if !key.is_empty() {
          validate_hotkey(&key)
            .map_err(|e| format!("Invalid hotkey for app '{}': {}", app_name, e))?;

          // Normalize for duplicate detection (sort modifiers, keep base key at the end)
          let mut mods = Vec::new();
          let mut base = String::new();
          for part in key.split('+').map(|s| s.trim().to_lowercase()) {
            match part.as_str() {
              "ctrl" | "control" | "alt" | "shift" | "meta" | "super" | "win" | "cmd" => {
                mods.push(part)
              }
              _ => base = part,
            }
          }
          mods.sort();
          let normalized = if mods.is_empty() {
            base
          } else {
            format!("{}+{}", mods.join("+"), base)
          };

          if let Some(other_app) = seen_hotkeys.insert(normalized.clone(), app_name.clone()) {
            return Err(format!(
              "Duplicate hotkey '{}' (normalized: '{}') found in app '{}' and '{}'",
              key, normalized, other_app, app_name
            ));
          }
        }
      }
    }

    for (app_name, app_cfg) in &self.app {
      if app_cfg.window_class.is_empty() {
        return Err(format!(
          "App '{}' is missing required field 'window_class'.",
          app_name
        ));
      }
      if app_cfg.window_class.len() < 3 {
        return Err(format!(
          "App '{}' has a window_class '{}' that is too short. It must be at least 3 characters long for reliable fuzzy matching.",
          app_name, app_cfg.window_class
        ));
      }
      if app_cfg.start_command.is_empty() {
        return Err(format!(
          "App '{}' is missing required field 'start_command'.",
          app_name
        ));
      }
    }

    // Validate Easing
    for (name, easing) in [
      ("show_easing", &self.animation.show_easing),
      ("hide_easing", &self.animation.hide_easing),
    ] {
      if !is_valid_easing(easing) {
        return Err(format!(
          "Invalid damping/easing curve '{}' for {}. Use a keyword (like 'ease', 'windows', 'back-out') or a custom cubic-bezier (like 'cubic-bezier(0, 1, 1, 0)' or '(0, 1, 1, 0)').",
          easing, name
        ));
      }
    }

    if self.app.is_empty() {
      return Err(
        "No app configured. Add at least one [app] or [app.name] section to your config."
          .to_string(),
      );
    }

    Ok(())
  }
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum HotkeyConfig {
  Single(String),
  Multiple(Vec<String>),
}

impl Default for HotkeyConfig {
  fn default() -> Self {
    HotkeyConfig::Single(default_hotkey())
  }
}

impl HotkeyConfig {
  pub fn as_vec(&self) -> Vec<String> {
    match self {
      HotkeyConfig::Single(s) => {
        if s.is_empty() {
          vec![]
        } else {
          vec![s.clone()]
        }
      }
      HotkeyConfig::Multiple(v) => v.clone(),
    }
  }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(default)]
pub struct AppConfig {
  pub window_class: String,
  pub start_command: String,
  pub hotkey: HotkeyConfig,
  pub animate_opacity: Option<bool>,
  pub width: Option<Dimension>,
  pub height: Option<Dimension>,
}

impl Default for AppConfig {
  fn default() -> Self {
    Self {
      window_class: String::new(),
      start_command: String::new(),
      hotkey: HotkeyConfig::default(),
      animate_opacity: None,
      width: None,
      height: None,
    }
  }
}

impl AppConfig {
  pub fn get_animate_opacity(&self, default_val: bool) -> bool {
    self.animate_opacity.unwrap_or(default_val)
  }

  pub fn resolve_dimensions(&self, global: &WindowConfig) -> ((f64, bool), (f64, bool)) {
    let w = self.width.as_ref().or(global.width.as_ref());
    let h = self.height.as_ref().or(global.height.as_ref());

    let rw = match w {
      Some(Dimension::Percent(p)) => (*p, true),
      Some(Dimension::Pixels(px)) => (*px as f64, false),
      Some(Dimension::Unset) | None => (0.0, false),
    };
    let rh = match h {
      Some(Dimension::Percent(p)) => (*p, true),
      Some(Dimension::Pixels(px)) => (*px as f64, false),
      Some(Dimension::Unset) | None => (0.0, false),
    };
    (rw, rh)
  }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(default)]
pub struct WindowConfig {
  pub display_mode: String,
  pub display_index: i32,
  pub width: Option<Dimension>,
  pub height: Option<Dimension>,
  pub keep_above: bool,
  pub force_priority: bool,
  pub auto_show: bool,
}

impl Default for WindowConfig {
  fn default() -> Self {
    Self {
      display_mode: "follow-mouse".to_string(),
      display_index: 0,
      width: None,
      height: None,
      keep_above: false,
      force_priority: false,
      auto_show: false,
    }
  }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(default)]
pub struct AnimationConfig {
  pub show_duration: i32,
  pub hide_duration: i32,
  pub show_easing: String,
  pub hide_easing: String,
  pub animate_opacity: bool,
  pub show_opacity_point: f64,
  pub hide_opacity_point: f64,
}

impl Default for AnimationConfig {
  fn default() -> Self {
    Self {
      show_duration: 350,
      hide_duration: 350,
      show_easing: "ease".to_string(),
      hide_easing: "ease".to_string(),
      animate_opacity: false,
      show_opacity_point: 0.2,
      hide_opacity_point: 0.8,
    }
  }
}

// =============================================================================
// Defaults
// =============================================================================

fn default_hotkey() -> String {
  "Meta+Grave".to_string()
}

// =============================================================================
// Config Loading
// =============================================================================

pub fn load_config(target_path: Option<PathBuf>) -> Result<(Config, Option<PathBuf>), String> {
  if let Some(path) = target_path {
    if path.exists() {
      match fs::read_to_string(&path) {
        Ok(content) => match toml::from_str::<Config>(&content) {
          Ok(c) => {
            println!("Loaded config from (cached): {:?}", path);
            c.validate()?;
            return Ok((c, Some(path)));
          }
          Err(e) => {
            return Err(format!("Malformed config file at {:?}: {}", path, e));
          }
        },
        Err(e) => {
          return Err(format!("Could not read config file at {:?}: {}", path, e));
        }
      }
    } else {
      return Err(format!("Config file no longer exists at: {:?}", path));
    }
  }

  let mut config_paths = Vec::new();

  // 1. Current EXE Directory
  if let Ok(exe) = current_exe() {
    if let Some(parent) = exe.parent() {
      config_paths.extend([parent.join("janq.toml"), parent.join(".janq.toml")]);
    }
  }

  if let Some(home) = home_dir() {
    // 2. XDG Config Directory (~/.config/janq/)
    if let Some(xdg_config) = config_dir() {
      let janq_dir = xdg_config.join("janq");
      config_paths.extend([janq_dir.join("janq.toml"), janq_dir.join(".janq.toml")]);
    }

    // 3. Home Directory
    config_paths.push(home.join(".janq.toml"));
  }

  // De-duplicate while preserving order
  let mut unique_paths = Vec::new();
  let mut seen = HashSet::new();
  for path in config_paths {
    if seen.insert(path.clone()) {
      unique_paths.push(path);
    }
  }
  let config_paths = unique_paths;

  for path in config_paths {
    if path.exists() {
      match fs::read_to_string(&path) {
        Ok(content) => match toml::from_str::<Config>(&content) {
          Ok(c) => {
            println!("Loaded config from: {:?}", path);
            c.validate()?;
            return Ok((c, Some(path)));
          }
          Err(e) => {
            return Err(format!("Malformed config file at {:?}: {}", path, e));
          }
        },
        Err(e) => {
          return Err(format!("Could not read config file at {:?}: {}", path, e));
        }
      }
    }
  }

  Err(
    "No config file found. Create ./janq.toml or ~/.config/janq/janq.toml with at least one [app] section."
      .to_string(),
  )
}

// =============================================================================
// Custom Deserialization
// =============================================================================

fn deserialize_app<'de, D>(deserializer: D) -> Result<IndexMap<String, AppConfig>, D::Error>
where
  D: Deserializer<'de>,
{
  struct AppVisitor;

  impl<'de> Visitor<'de> for AppVisitor {
    type Value = IndexMap<String, AppConfig>;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
      formatter.write_str("a map of apps or a single app configuration")
    }

    fn visit_map<M>(self, map: M) -> Result<Self::Value, M::Error>
    where
      M: de::MapAccess<'de>,
    {
      // We need to peek or try both ways.
      // A simple trick: try to deserialize as HashMap<String, AppConfig>.
      // If it has keys like "window_class", it will fail if AppConfig doesn't match a map value.

      let raw_map = IndexMap::<String, toml::Value>::deserialize(MapAccessDeserializer::new(map))?;

      if raw_map.is_empty() {
        return Ok(IndexMap::new());
      }

      // Heuristic: Check if the map contains sub-tables (Objects), which implies a multi-app config.
      let has_subtables = raw_map.values().any(|v| v.is_table());
      let has_flat_keys =
        raw_map.contains_key("window_class") || raw_map.contains_key("start_command");

      if has_subtables {
        if has_flat_keys {
          return Err(de::Error::custom("Config section '[app]' contains both app definitions (sub-tables like [app.myapp]) and direct keys (window_class). Choose one style."));
        }
        // Treat as a map of apps
        let mut result = IndexMap::new();
        for (name, value) in raw_map {
          if value.is_table() {
            let config = AppConfig::deserialize(value).map_err(de::Error::custom)?;
            result.insert(name, config);
          }
        }
        Ok(result)
      } else if has_flat_keys {
        let config = AppConfig::deserialize(toml::Value::Table(raw_map.into_iter().collect()))
          .map_err(de::Error::custom)?;
        let mut result = IndexMap::new();

        // Require window_class for single-app mode
        if config.window_class.is_empty() {
          return Err(de::Error::custom(
            "[app] section requires 'window_class' field",
          ));
        }
        let name = config.window_class.clone();

        result.insert(name, config);
        Ok(result)
      } else {
        // Empty or unknown structure, default to empty map
        Ok(IndexMap::new())
      }
    }

    fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
    where
      A: de::SeqAccess<'de>,
    {
      let mut result = IndexMap::new();
      let mut i = 1;
      while let Some(value) = seq.next_element::<toml::Value>()? {
        let config = AppConfig::deserialize(value).map_err(de::Error::custom)?;
        result.insert(format!("app{}", i), config);
        i += 1;
      }
      Ok(result)
    }
  }

  deserializer.deserialize_any(AppVisitor)
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn test_dimension_parsing() {
    #[derive(Deserialize, Debug)]
    struct Test {
      d: Dimension,
    }

    let t: Test = toml::from_str("d = \"50%\"").unwrap();
    assert_eq!(t.d, Dimension::Percent(0.5));

    let t: Test = toml::from_str("d = \"800px\"").unwrap();
    assert_eq!(t.d, Dimension::Pixels(800));

    let t: Test = toml::from_str("d = \"0\"").unwrap();
    assert_eq!(t.d, Dimension::Unset);

    let err = toml::from_str::<Test>("d = \"50\"");
    assert!(err.is_err());
    assert!(err
      .unwrap_err()
      .to_string()
      .contains("Must end with '%' or 'px'"));
  }

  #[test]
  fn test_app_config_resolve() {
    let global = WindowConfig {
      width: Some(Dimension::Percent(0.4)),
      height: Some(Dimension::Pixels(600)),
      ..Default::default()
    };

    let app = AppConfig {
      width: Some(Dimension::Pixels(800)),
      ..Default::default()
    };

    let ((w, _), (h, _)) = app.resolve_dimensions(&global);
    assert_eq!(w, 800.0);
    assert_eq!(h, 600.0);

    let app2 = AppConfig {
      width: Some(Dimension::Unset), // "0" or "unset" means skip resizing
      ..Default::default()
    };
    let ((w2, w2_is_pct), (h2, _)) = app2.resolve_dimensions(&global);
    assert_eq!(w2, 0.0); // Unset means no resize (0.0)
    assert!(!w2_is_pct);
    assert_eq!(h2, 600.0); // Height still inherits from global
  }
}
