use std::{
  collections::{HashMap, HashSet},
  env::current_exe,
  fs,
  path::PathBuf,
};

use dirs::{config_dir, home_dir};
use indexmap::IndexMap;
use serde::Deserialize;

#[derive(Clone, Debug, Default)]
pub struct FoundWindow {
  pub id: String,
  pub class_name: String,
  pub proc_name: String,
  #[allow(dead_code)]
  pub pid: u32,
  pub is_visible: bool,
}

pub fn fuzzy_match_window(
  target: &str,
  candidates: &[FoundWindow],
  managed_ids: &[String],
) -> Option<FoundWindow> {
  let lower_target = target.to_lowercase();
  if lower_target.is_empty() {
    return None;
  }

  let mut best_score = 500; // Lower baseline threshold for the new algorithm
  let mut best_win = None;

  for win in candidates {
    let mut score = 0;

    // 1. Check class_name and proc_name
    for haystack in &[&win.class_name, &win.proc_name] {
      if haystack.is_empty() {
        continue;
      }

      let mut current_haystack_score = 0;

      if **haystack == lower_target {
        current_haystack_score = 10000;
      } else if haystack.contains(&lower_target) {
        current_haystack_score = 5000;
      } else {
        // Advanced Fuzzy Subsequence with Boundary/Gap penalties
        let mut h_idx = 0;
        let mut last_match_idx = -1;
        let mut consecutive_count = 0;
        let mut matches = 0;

        for n_char in lower_target.chars() {
          let mut found = false;
          let search_slice = &haystack[h_idx..];
          for (rel_idx, h_char) in search_slice.char_indices() {
            if h_char == n_char {
              let abs_idx = h_idx + rel_idx;
              matches += 1;

              // Bonus: Boundary (start of string or follows separator)
              if abs_idx == 0 {
                current_haystack_score += 300;
              } else {
                let prev_char = haystack.as_bytes().get(abs_idx - 1).copied().unwrap_or(0);
                if prev_char == b'.' || prev_char == b'-' || prev_char == b'_' || prev_char == b' '
                {
                  current_haystack_score += 250;
                }
              }

              // Bonus: Consecutive
              if last_match_idx != -1 && abs_idx == (last_match_idx as usize) + 1 {
                consecutive_count += 1;
                current_haystack_score += 100 * consecutive_count;
              } else {
                consecutive_count = 0;
                // Penalty: Gap
                if last_match_idx != -1 {
                  let gap = abs_idx - (last_match_idx as usize) - 1;
                  current_haystack_score -= (gap as i32) * 50;
                }
              }

              last_match_idx = abs_idx as i32;
              h_idx = abs_idx + h_char.len_utf8();
              found = true;
              break;
            }
          }
          if !found {
            // Entire needle must be found as subsequence
            current_haystack_score = 0;
            break;
          }
        }

        // Final Subsequence Polish: Base score for matching all letters
        if matches == lower_target.chars().count() {
          current_haystack_score += 1000;
        }
      }
      score = score.max(current_haystack_score);
    }

    if score <= 0 {
      continue;
    }

    // 2. Priority Boosts
    if win.is_visible {
      score += 2000; // Reduced visibility boost so it doesn't overwhelm the match score
    }
    if managed_ids.contains(&win.id) {
      score += 1000;
    }

    if score > best_score {
      best_score = score;
      best_win = Some(win.clone());
    }
  }

  best_win
}

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

#[derive(Clone, Debug, Deserialize, Default)]
pub struct Config {
  #[serde(
    default,
    alias = "apps",
    alias = "general",
    deserialize_with = "deserialize_app"
  )]
  pub app: IndexMap<String, AppConfig>,
  #[serde(default)]
  pub window: WindowConfig,
  #[serde(default)]
  pub animation: AnimationConfig,
}

impl Config {
  pub fn validate(&self) -> Result<(), String> {
    let mut seen_hotkeys = HashMap::new();
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

fn default_hotkey() -> String {
  "Meta+Grave".to_string()
}

fn validate_hotkey(s: &str) -> Result<(), String> {
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
          return Err(format!(
            "Multiple base keys found: use only one base key (e.g., 'F1') per shortcut."
          ));
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

fn is_valid_base_key(s: &str) -> bool {
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

fn is_valid_easing(s: &str) -> bool {
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
    "No config file found. Create ~/.janq.toml or ~/.config/janq/janq.toml with at least one [app] section."
      .to_string(),
  )
}

use serde::de::{self, Deserializer, Visitor};
use std::fmt;

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
      use serde::de::value::MapAccessDeserializer;

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

  #[test]
  fn test_parse_bezier() {
    assert_eq!(
      parse_bezier("cubic-bezier(0, 0.5, 0.5, 1)"),
      Some((0.0, 0.5, 0.5, 1.0))
    );
    assert_eq!(
      parse_bezier("bezier(0, 0.5, 0.5, 1)"),
      Some((0.0, 0.5, 0.5, 1.0))
    );
    assert_eq!(parse_bezier("(0, 1, 1, 0)"), Some((0.0, 1.0, 1.0, 0.0)));
    assert_eq!(
      parse_bezier(" ( 0.1 , 0.2 , 0.3 , 0.4 ) "),
      Some((0.1, 0.2, 0.3, 0.4))
    );
    assert_eq!(parse_bezier("linear"), None);
  }

  #[test]
  fn test_is_valid_easing() {
    assert!(is_valid_easing("ease"));
    assert!(is_valid_easing("windows"));
    assert!(is_valid_easing("back-out"));
    assert!(is_valid_easing("cubic-bezier(0, 1, 1, 0)"));
    assert!(is_valid_easing("bezier(0, 1, 1, 0)"));
    assert!(is_valid_easing("(0, 1, 1, 0)"));
    assert!(!is_valid_easing("invalid"));
    assert!(!is_valid_easing("cubic-bezier(1, 2)"));
  }
}
