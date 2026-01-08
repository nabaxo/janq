use indexmap::IndexMap;
use serde::Deserialize;
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

#[derive(Clone, Debug, PartialEq)]
pub enum Dimension {
  Percent(f64),
  Pixels(i32),
}

impl<'de> serde::Deserialize<'de> for Dimension {
  fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
  where
    D: serde::Deserializer<'de>,
  {
    let s = String::deserialize(deserializer)?;
    let s = s.trim().to_lowercase();
    if let Some(rest) = s.strip_suffix('%') {
      let val = rest.trim().parse::<f64>().map_err(serde::de::Error::custom)?;
      Ok(Dimension::Percent(val / 100.0))
    } else if let Some(rest) = s.strip_suffix("px") {
      let val = rest.trim().parse::<i32>().map_err(serde::de::Error::custom)?;
      Ok(Dimension::Pixels(val))
    } else if s == "0" || s == "unset" {
      Ok(Dimension::Pixels(0))
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
  #[serde(default, alias = "apps", alias = "general", deserialize_with = "deserialize_app")]
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
      for key in app_cfg.hotkey.as_vec() {
        if !key.is_empty() {
          let lower_key = key.to_lowercase();
          if let Some(other_app) = seen_hotkeys.insert(lower_key.clone(), app_name.clone()) {
            return Err(format!(
              "Duplicate hotkey '{}' found in app '{}' and '{}'",
              key, other_app, app_name
            ));
          }
        }
      }
    }

    if self.app.is_empty() {
      return Err("No app configured. Add at least one [app] or [app.name] section to your config.".to_string());
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
      None => (0.0, false),
    };
    let rh = match h {
      Some(Dimension::Percent(p)) => (*p, true),
      Some(Dimension::Pixels(px)) => (*px as f64, false),
      None => (0.0, false),
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
      show_opacity_point: 0.0,
      hide_opacity_point: 0.0,
    }
  }
}

fn default_hotkey() -> String {
  "Meta+Grave".to_string()
}

pub fn load_config() -> Result<(Config, Option<PathBuf>), String> {
  let mut config_paths = Vec::new();

  if let Some(home) = dirs::home_dir() {
    // 1. Home Directory
    config_paths.push(home.join(".janq.toml"));

    // 2. XDG Config Directory (~/.config/janq/)
    if let Some(xdg_config) = dirs::config_dir() {
      let janq_dir = xdg_config.join("janq");
      config_paths.extend([janq_dir.join("janq.toml"), janq_dir.join(".janq.toml")]);
    }
  }

  // 4. Current EXE Directory
  if let Ok(exe) = std::env::current_exe() {
    if let Some(parent) = exe.parent() {
      config_paths.extend([parent.join("janq.toml"), parent.join(".janq.toml")]);
    }
  }

  // De-duplicate while preserving order
  let mut unique_paths = Vec::new();
  let mut seen = std::collections::HashSet::new();
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

      let raw_map = IndexMap::<String, serde_json::Value>::deserialize(MapAccessDeserializer::new(map))?;

      if raw_map.is_empty() {
        return Ok(IndexMap::new());
      }

      // Heuristic: Check if the map contains sub-tables (Objects), which implies a multi-app config.
      let has_subtables = raw_map.values().any(|v| v.is_object());
      let has_flat_keys = raw_map.contains_key("window_class") || raw_map.contains_key("start_command");

      if has_subtables {
        if has_flat_keys {
          return Err(de::Error::custom("Config section '[app]' contains both app definitions (sub-tables like [app.myapp]) and direct keys (window_class). Choose one style."));
        }
        // Treat as a map of apps
        let mut result = IndexMap::new();
        for (name, value) in raw_map {
          if value.is_object() {
            let config = AppConfig::deserialize(value).map_err(de::Error::custom)?;
            result.insert(name, config);
          }
        }
        Ok(result)
      } else if has_flat_keys {
        let config = AppConfig::deserialize(serde_json::Value::Object(raw_map.into_iter().collect()))
          .map_err(de::Error::custom)?;
        let mut result = IndexMap::new();

        // Require window_class for single-app mode
        if config.window_class.is_empty() {
          return Err(de::Error::custom("[app] section requires 'window_class' field"));
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
      while let Some(value) = seq.next_element::<serde_json::Value>()? {
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
    assert_eq!(t.d, Dimension::Pixels(0));

    let err = toml::from_str::<Test>("d = \"50\"");
    assert!(err.is_err());
    assert!(err.unwrap_err().to_string().contains("Must end with '%' or 'px'"));
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

    let (w, h) = app.resolve_dimensions(&global);
    assert_eq!(w, 800.0);
    assert_eq!(h, 600.0);

    let app2 = AppConfig {
      width: Some(Dimension::Pixels(0)),
      ..Default::default()
    };
    let (w2, h2) = app2.resolve_dimensions(&global);
    assert_eq!(w2, 0.0); // Explicit 0 should override global
    assert_eq!(h2, 600.0);
  }
}
