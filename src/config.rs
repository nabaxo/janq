use serde::Deserialize;
use std::path::PathBuf;
use std::fs;

#[derive(Clone, Debug, Deserialize)]
#[allow(dead_code)]
pub struct Config {
    #[serde(default = "default_window_class")]
    pub window_class: String,
    #[serde(default = "default_start_command")]
    pub start_command: String,
    #[serde(default = "default_hotkeys", deserialize_with = "deserialize_hotkeys")]
    #[allow(dead_code)]
    pub hotkey: Vec<String>,
    #[serde(default = "default_display_mode")]
    pub display_mode: String,
    #[serde(default)]
    pub display_index: i32,
    #[serde(default = "default_width_percent")]
    pub width_percent: i32,
    #[serde(default = "default_height_percent")]
    pub height_percent: i32,
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
    #[serde(default = "default_width_cols")]
    pub width_cols: i32,
    #[serde(default = "default_height_rows")]
    pub height_rows: i32,
    #[serde(default)]
    pub keep_above: bool,
}

fn default_window_class() -> String { "wezquake".to_string() }
fn default_start_command() -> String { "wezterm-gui start".to_string() }
fn default_hotkeys() -> Vec<String> { vec!["Meta+Grave".to_string()] }
fn default_display_mode() -> String { "follow-mouse".to_string() }
fn default_width_percent() -> i32 { 40 }
fn default_height_percent() -> i32 { 40 }
fn default_duration() -> i32 { 350 }
fn default_show_easing() -> String { "ease-out-cubic".to_string() }
fn default_hide_easing() -> String { "ease-in-quart".to_string() }
fn default_true() -> bool { true }
fn default_show_opacity() -> f64 { 0.2 }
fn default_hide_opacity() -> f64 { 0.8 }
fn default_width_cols() -> i32 { 120 }
fn default_height_rows() -> i32 { 40 }

impl Default for Config {
    fn default() -> Self {
        Self {
            window_class: default_window_class(),
            start_command: default_start_command(),
            hotkey: default_hotkeys(),
            display_mode: default_display_mode(),
            display_index: 0,
            width_percent: default_width_percent(),
            height_percent: default_height_percent(),
            show_duration: default_duration(),
            hide_duration: default_duration(),
            show_easing: default_show_easing(),
            hide_easing: default_hide_easing(),
            animate_opacity: default_true(),
            show_opacity_point: default_show_opacity(),
            hide_opacity_point: default_hide_opacity(),
            width_cols: default_width_cols(),
            height_rows: default_height_rows(),
            keep_above: false,
        }
    }
}

pub fn load_config() -> (Config, Option<PathBuf>) {
    let mut config_paths = vec![PathBuf::from(".goake.toml")];

    if let Some(home) = dirs::home_dir() {
        config_paths.push(home.join(".goake.toml"));
        if let Some(xdg_config) = dirs::config_dir() {
             config_paths.push(xdg_config.join("rustake").join(".goake.toml"));
             config_paths.push(xdg_config.join("goake").join(".goake.toml"));
        }
    }

    for path in config_paths {
        if path.exists() {
            if let Ok(content) = fs::read_to_string(&path) {
                if let Ok(c) = toml::from_str(&content) {
                    println!("Loaded config from: {:?}", path);
                    return (c, Some(path));
                }
            }
        }
    }

    println!("No config file found. Using defaults.");
    (Config::default(), None)
}

use serde::de::{self, Deserializer, Visitor};
use std::fmt;

fn deserialize_hotkeys<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: Deserializer<'de>,
{
    struct StringOrVec;

    impl<'de> Visitor<'de> for StringOrVec {
        type Value = Vec<String>;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("string or list of strings")
        }

        fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(vec![value.to_owned()])
        }

        fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
        where
            A: de::SeqAccess<'de>,
        {
            let mut vec = Vec::new();
            while let Some(elem) = seq.next_element()? {
                vec.push(elem);
            }
            Ok(vec)
        }
    }

    deserializer.deserialize_any(StringOrVec)
}
