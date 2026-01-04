use serde::Deserialize;
use std::path::PathBuf;
use std::fs;
use std::collections::HashMap;

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
        if s.ends_with('%') {
            let val = s.trim_end_matches('%').parse::<f64>().map_err(serde::de::Error::custom)?;
            Ok(Dimension::Percent(val / 100.0))
        } else if s.ends_with("px") {
            let val = s.trim_end_matches("px").parse::<i32>().map_err(serde::de::Error::custom)?;
            Ok(Dimension::Pixels(val))
        } else if s == "0" || s == "unset" {
            Ok(Dimension::Pixels(0))
        } else {
            Err(serde::de::Error::custom(format!("Invalid dimension format: '{}'. Must end with '%' or 'px'.", s)))
        }
    }
}

#[derive(Clone, Debug, Deserialize, Default)]
pub struct Config {
    #[serde(default, alias = "apps", alias = "general", deserialize_with = "deserialize_app")]
    pub app: HashMap<String, AppConfig>,
    #[serde(default)]
    pub window: WindowConfig,
    #[serde(default)]
    pub animation: AnimationConfig,
    #[serde(skip)]
    pub app_order: Vec<String>,
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
            HotkeyConfig::Single(s) => if s.is_empty() { vec![] } else { vec![s.clone()] },
            HotkeyConfig::Multiple(v) => v.clone(),
        }
    }

    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        match self {
            HotkeyConfig::Single(s) => s.is_empty(),
            HotkeyConfig::Multiple(v) => v.is_empty(),
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
pub struct AppConfig {
    #[serde(default = "default_window_class")]
    pub window_class: String,
    #[serde(default = "default_start_command")]
    pub start_command: String,
    #[serde(default)]
    pub hotkey: HotkeyConfig,
    #[allow(dead_code)]
    #[serde(default)]
    pub disable_window_class_arg: bool,
    pub animate_opacity: Option<bool>,
    pub width: Option<Dimension>,
    pub height: Option<Dimension>,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            window_class: default_window_class(),
            start_command: default_start_command(),
            hotkey: HotkeyConfig::default(),
            disable_window_class_arg: false,
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

    pub fn resolve_dimensions(&self, global: &WindowConfig) -> (f64, f64) {
        let w = self.width.as_ref().or(global.width.as_ref());
        let h = self.height.as_ref().or(global.height.as_ref());

        let rw = match w {
            Some(Dimension::Percent(p)) => *p,
            Some(Dimension::Pixels(px)) => *px as f64,
            None => 0.0,
        };
        let rh = match h {
            Some(Dimension::Percent(p)) => *p,
            Some(Dimension::Pixels(px)) => *px as f64,
            None => 0.0,
        };
        (rw, rh)
    }
}

#[derive(Clone, Debug, Deserialize)]
pub struct WindowConfig {
    #[serde(default = "default_display_mode")]
    pub display_mode: String,
    #[serde(default)]
    pub display_index: i32,
    pub width: Option<Dimension>,
    pub height: Option<Dimension>,

    #[serde(default)]
    pub keep_above: bool,
}

impl Default for WindowConfig {
    fn default() -> Self {
        Self {
            display_mode: default_display_mode(),
            display_index: 0,
            width: None,
            height: None,
            keep_above: false,
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
pub struct AnimationConfig {
    #[serde(default = "default_duration")]
    pub show_duration: i32,
    #[serde(default = "default_duration")]
    pub hide_duration: i32,
    #[serde(default = "default_show_easing")]
    pub show_easing: String,
    #[serde(default = "default_hide_easing")]
    pub hide_easing: String,
    #[serde(default = "default_true")]
    pub animate_opacity: bool,
    #[serde(default = "default_show_opacity")]
    pub show_opacity_point: f64,
    #[serde(default = "default_hide_opacity")]
    pub hide_opacity_point: f64,
}

impl Default for AnimationConfig {
    fn default() -> Self {
        Self {
            show_duration: default_duration(),
            hide_duration: default_duration(),
            show_easing: default_show_easing(),
            hide_easing: default_hide_easing(),
            animate_opacity: default_true(),
            show_opacity_point: default_show_opacity(),
            hide_opacity_point: default_hide_opacity(),
        }
    }
}

fn default_window_class() -> String { "wezquake".to_string() }
fn default_start_command() -> String { "wezterm-gui start".to_string() }
fn default_hotkey() -> String { "Meta+Grave".to_string() }
fn default_display_mode() -> String { "follow-mouse".to_string() }
fn default_duration() -> i32 { 350 }
fn default_show_easing() -> String { "ease-out-cubic".to_string() }
fn default_hide_easing() -> String { "ease-in-quart".to_string() }
fn default_true() -> bool { true }
fn default_show_opacity() -> f64 { 0.2 }
fn default_hide_opacity() -> f64 { 0.8 }

pub fn load_config() -> Result<(Config, Option<PathBuf>), String> {
    let mut config_paths = Vec::new();

    if let Some(home) = dirs::home_dir() {
        config_paths.push(home.join(".ruake.toml"));
        config_paths.push(home.join(".goake.toml"));

        // Explicitly check ~/.config/ruake for cross-platform consistency
        config_paths.push(home.join(".config").join("ruake").join("ruake.toml"));
        config_paths.push(home.join(".config").join("ruake").join(".ruake.toml"));
        config_paths.push(home.join(".config").join("ruake").join(".goake.toml"));

        if let Some(xdg_config) = dirs::config_dir() {
             config_paths.push(xdg_config.join("ruake").join("ruake.toml"));
             config_paths.push(xdg_config.join("ruake").join(".ruake.toml"));
             config_paths.push(xdg_config.join("ruake").join(".goake.toml"));
             config_paths.push(xdg_config.join("goake").join(".goake.toml"));
        }
    }

    // Binary directory (Portable fallback)
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            config_paths.push(parent.join("ruake.toml"));
            config_paths.push(parent.join(".ruake.toml"));
            config_paths.push(parent.join(".goake.toml"));
        }
    }

    for path in config_paths {
        if path.exists() {
            match fs::read_to_string(&path) {
                Ok(content) => {
                    match toml::from_str::<Config>(&content) {
                        Ok(mut c) => {
                            println!("Loaded config from: {:?}", path);

                            // Heuristic to preserve order: Scan lines for [app.NAME], [apps.NAME] or [general.NAME]
                            let mut order = Vec::with_capacity(c.app.len());
                            for line in content.lines() {
                                let trimmed = line.trim();
                                if trimmed.is_empty() || trimmed.starts_with('#') { continue; }

                                let name = if trimmed == "[app]" || trimmed == "[apps]" || trimmed == "[general]" {
                                    if c.app.contains_key("default") {
                                        Some("default")
                                    } else {
                                        // Find first app not explicitly named in a subtable
                                        c.app.keys().find(|&k| {
                                            !content.contains(&format!("[app.{}", k)) &&
                                            !content.contains(&format!("[apps.{}", k)) &&
                                            !content.contains(&format!("[general.{}", k))
                                        }).map(|s| s.as_str())
                                    }
                                } else if trimmed.ends_with(']') {
                                     let content = trimmed.trim_start_matches('[').trim_end_matches(']');
                                     if let Some(rest) = content.strip_prefix("app.") {
                                         Some(rest)
                                     } else if let Some(rest) = content.strip_prefix("apps.") {
                                         Some(rest)
                                     } else if let Some(rest) = content.strip_prefix("general.") {
                                         Some(rest)
                                     } else {
                                         None
                                     }
                                } else {
                                    None
                                };

                                if let Some(name) = name {
                                    let name_s = name.to_string();
                                    if c.app.contains_key(&name_s) && !order.contains(&name_s) {
                                        order.push(name_s);
                                    }
                                }
                            }

                            // If we found ordered keys, use them.
                            // Any keys in c.app not found in scan (e.g. inline definitions) should be appended.
                            if order.len() < c.app.len() {
                                let mut remaining: Vec<_> = c.app.keys()
                                    .filter(|k| !order.contains(k))
                                    .cloned()
                                    .collect();
                                remaining.sort();
                                order.extend(remaining);
                            }

                            c.app_order = order;

                            // Duplicate hotkey check
                            let mut seen_hotkeys = HashMap::new();
                            for (app_name, app_cfg) in &c.app {
                                for key in app_cfg.hotkey.as_vec() {
                                    if !key.is_empty() {
                                        if let Some(other_app) = seen_hotkeys.insert(key.clone(), app_name.clone()) {
                                            return Err(format!("Duplicate hotkey '{}' found in app '{}' and '{}'", key, other_app, app_name));
                                        }
                                    }
                                }
                            }

                            return Ok((c, Some(path)));
                        }
                        Err(e) => {
                            return Err(format!("Malformed config file at {:?}: {}", path, e));
                        }
                    }
                }
                Err(e) => {
                    return Err(format!("Could not read config file at {:?}: {}", path, e));
                }
            }
        }
    }

    println!("No config file found. Using defaults.");
    Ok((Config::default(), None))
}

use serde::de::{self, Deserializer, Visitor};
use std::fmt;

fn deserialize_app<'de, D>(deserializer: D) -> Result<HashMap<String, AppConfig>, D::Error>
where
    D: Deserializer<'de>,
{
    struct AppVisitor;

    impl<'de> Visitor<'de> for AppVisitor {
        type Value = HashMap<String, AppConfig>;

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

            let raw_map = HashMap::<String, serde_json::Value>::deserialize(MapAccessDeserializer::new(map))?;

            if raw_map.is_empty() {
                return Ok(HashMap::new());
            }

            // Heuristic: Check if the map contains sub-tables (Objects), which implies a multi-app config.
            let has_subtables = raw_map.values().any(|v| v.is_object());
            let has_flat_keys = raw_map.contains_key("window_class") || raw_map.contains_key("start_command");

            if has_subtables {
                if has_flat_keys {
                   return Err(de::Error::custom("Config section '[app]' contains both app definitions (sub-tables like [app.myapp]) and direct keys (window_class). Choose one style."));
                }
                // Treat as a map of apps
                let mut result = HashMap::new();
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
                let mut result = HashMap::new();

                // Use window_class as a better name than "default" if available
                let name = if config.window_class.is_empty() {
                    "default".to_string()
                } else {
                    config.window_class.clone()
                };

                result.insert(name, config);
                Ok(result)
            } else {
                 // Empty or unknown structure, default to empty map
                 Ok(HashMap::new())
            }
        }

        fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
        where
            A: de::SeqAccess<'de>,
        {
            let mut result = HashMap::new();
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
        struct Test { d: Dimension }

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
