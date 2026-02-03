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

use rustc_hash::{FxHashMap, FxHashSet};
use std::{env::current_exe, fmt, fs, path::PathBuf};

use crate::paths::{config_dir, home_dir};
use indexmap::IndexMap; // Preserves insertion order for deterministic app iteration

/// Specialized IndexMap that uses FxHash for maximum performance.
type FxIndexMap<K, V> = IndexMap<K, V, std::hash::BuildHasherDefault<rustc_hash::FxHasher>>;
use serde::{
  de::{self, value::MapAccessDeserializer, Deserializer, Visitor},
  Deserialize,
};

// Re-export matching types so other modules can import from config
pub use crate::matching::{fuzzy_match_window, FoundWindow};

// Re-export ConfigError for callers
pub use crate::error::ConfigError;

// Import validation functions used internally
use crate::error::format_error_with_span;
use crate::validation::validate_hotkey;

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
    let lower = s.trim().to_lowercase();
    if lower == "0" || lower == "unset" {
      return Ok(Dimension::Unset);
    }

    match parse_unit_value(&s) {
      Ok((val, true)) => Ok(Dimension::Percent(val)),
      Ok((val, false)) => Ok(Dimension::Pixels(val as i32)),
      Err(e) => Err(serde::de::Error::custom(format!(
        "Invalid dimension format: {}",
        e
      ))),
    }
  }
}

/// Direction from which the window slides in.
///
/// Parse formats: `"top"`, `"bottom"`, `"left"`, `"right"` (case-insensitive)
#[derive(Clone, Debug, PartialEq, Default)]
pub enum SlideDirection {
  #[default]
  Top,
  Bottom,
  Left,
  Right,
}

impl SlideDirection {
  /// Valid string values for this enum (single source of truth).
  pub const VALID_VALUES: &'static [&'static str] = &["top", "bottom", "left", "right"];
}

impl<'de> serde::Deserialize<'de> for SlideDirection {
  fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
  where
    D: serde::Deserializer<'de>,
  {
    let s = String::deserialize(deserializer)?;
    let lower = s.trim().to_lowercase();
    match lower.as_str() {
      "top" => Ok(SlideDirection::Top),
      "bottom" => Ok(SlideDirection::Bottom),
      "left" => Ok(SlideDirection::Left),
      "right" => Ok(SlideDirection::Right),
      other => {
        let hint = crate::matching::suggest_similar(other, Self::VALID_VALUES)
          .map(|s| format!(" Did you mean '{}'?", s))
          .unwrap_or_else(|| format!(" Valid values: {}.", Self::VALID_VALUES.join(", ")));
        Err(serde::de::Error::custom(format!(
          "Invalid slide_from value: '{}'.{}",
          s, hint
        )))
      }
    }
  }
}

/// Position offset along the edge from which the window slides.
///
/// Parse formats:
/// - `"center"` → centered on the edge (default)
/// - `"50%"` → 50% from left/top of edge
/// - `"-10%"` → 10% from right/bottom of edge
/// - `"100px"` → 100 pixels from left/top of edge
/// - `"-50px"` → 50 pixels from right/bottom of edge
#[derive(Clone, Debug, PartialEq, Default)]
pub enum PositionOffset {
  #[default]
  Center,
  Pixels(i32),
  Percent(f64),
}

impl<'de> serde::Deserialize<'de> for PositionOffset {
  fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
  where
    D: serde::Deserializer<'de>,
  {
    let s = String::deserialize(deserializer)?;
    let lower = s.trim().to_lowercase();
    if lower == "center" || lower == "0" {
      return Ok(PositionOffset::Center);
    }

    match parse_unit_value(&s) {
      Ok((val, true)) => Ok(PositionOffset::Percent(val)),
      Ok((val, false)) => Ok(PositionOffset::Pixels(val as i32)),
      Err(e) => {
        let hint = if lower.chars().all(|c| c.is_alphabetic()) && !lower.is_empty() {
          " Did you mean 'center'?"
        } else {
          ""
        };
        Err(serde::de::Error::custom(format!(
          "Invalid position_offset format: {}{}",
          e, hint
        )))
      }
    }
  }
}

/// A platform-agnostic rectangle for position calculations.
#[cfg(target_os = "windows")]
#[derive(Clone, Copy, Debug)]
pub struct WorkArea {
  pub left: i32,
  pub top: i32,
  pub right: i32,
  pub bottom: i32,
}

#[cfg(target_os = "windows")]
impl WorkArea {
  #[inline]
  pub fn width(&self) -> i32 {
    self.right - self.left
  }
  #[inline]
  pub fn height(&self) -> i32 {
    self.bottom - self.top
  }
}

/// Computed positions for window animation.
#[cfg(target_os = "windows")]
#[derive(Clone, Copy, Debug)]
pub struct SlidePositions {
  pub shown_x: i32,
  pub shown_y: i32,
  pub hidden_x: i32,
  pub hidden_y: i32,
}

/// Computes shown and hidden positions for a sliding window.
///
/// This is the position calculation logic used by the Windows animation engine.
/// Linux uses equivalent JavaScript logic in KWin scripts.
///
/// # Arguments
/// * `slide_from` - Edge from which the window slides in
/// * `position_offset` - Position along that edge (center, pixels, or percent)
/// * `work_area` - Monitor work area bounds
/// * `window_w` - Window width
/// * `window_h` - Window height
#[cfg(target_os = "windows")]
pub fn compute_slide_positions(
  slide_from: &SlideDirection,
  position_offset: &PositionOffset,
  work_area: WorkArea,
  window_w: i32,
  window_h: i32,
) -> SlidePositions {
  let screen_w = work_area.width();
  let screen_h = work_area.height();
  let is_horizontal = matches!(slide_from, SlideDirection::Top | SlideDirection::Bottom);

  // Calculate position along the edge (perpendicular to slide direction)
  let along_pos = if is_horizontal {
    match position_offset {
      PositionOffset::Center => work_area.left + (screen_w - window_w) / 2,
      PositionOffset::Pixels(px) => {
        if *px >= 0 {
          work_area.left + *px
        } else {
          work_area.right - window_w + *px
        }
      }
      PositionOffset::Percent(pct) => {
        if *pct >= 0.0 {
          work_area.left + (screen_w as f64 * *pct) as i32
        } else {
          work_area.right - window_w - (screen_w as f64 * pct.abs()) as i32
        }
      }
    }
  } else {
    match position_offset {
      PositionOffset::Center => work_area.top + (screen_h - window_h) / 2,
      PositionOffset::Pixels(px) => {
        if *px >= 0 {
          work_area.top + *px
        } else {
          work_area.bottom - window_h + *px
        }
      }
      PositionOffset::Percent(pct) => {
        if *pct >= 0.0 {
          work_area.top + (screen_h as f64 * *pct) as i32
        } else {
          work_area.bottom - window_h - (screen_h as f64 * pct.abs()) as i32
        }
      }
    }
  };

  // Calculate shown/hidden positions based on slide direction
  // Fixed: Added 10px buffer to hidden positions to ensure shadows are fully hidden (aligned with parking.rs)
  let (shown_x, shown_y, hidden_x, hidden_y) = match slide_from {
    SlideDirection::Top => (
      along_pos,
      work_area.top,
      along_pos,
      work_area.top - window_h - 10,
    ),
    SlideDirection::Bottom => (
      along_pos,
      work_area.bottom - window_h,
      along_pos,
      work_area.bottom + 10,
    ),
    SlideDirection::Left => (
      work_area.left,
      along_pos,
      work_area.left - window_w - 10,
      along_pos,
    ),
    SlideDirection::Right => (
      work_area.right - window_w,
      along_pos,
      work_area.right + 10,
      along_pos,
    ),
  };

  SlidePositions {
    shown_x,
    shown_y,
    hidden_x,
    hidden_y,
  }
}

// =============================================================================
// Helper Functions for Parsing
// =============================================================================

fn parse_unit_value(s: &str) -> Result<(f64, bool), String> {
  let s = s.trim().to_lowercase();
  if let Some(rest) = s.strip_suffix('%') {
    let val = rest
      .trim()
      .parse::<f64>()
      .map_err(|e| format!("Invalid percentage: {}", e))?;
    Ok((val / 100.0, true))
  } else if let Some(rest) = s.strip_suffix("px") {
    let val = rest
      .trim()
      .parse::<f64>() // Use f64 for parsing to handle potential decimals even for px
      .map_err(|e| format!("Invalid pixels: {}", e))?;
    Ok((val, false))
  } else {
    Err(format!("'{}' must end with '%' or 'px'.", s))
  }
}

/// A dimension value that can be specified as percent, pixels, or unset.

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
#[serde(deny_unknown_fields)]
pub struct Config {
  /// App definitions. Uses IndexMap to preserve TOML declaration order.
  #[serde(
    default,
    alias = "apps",
    alias = "general",
    deserialize_with = "deserialize_app"
  )]
  pub app: FxIndexMap<String, AppConfig>,
  /// Global window positioning and display settings.
  #[serde(default)]
  pub window: WindowConfig,
  /// Animation timing and easing configuration.
  #[serde(default)]
  pub animation: AnimationConfig,
}

impl Config {
  pub fn validate(&self, content: &str, path: &std::path::Path) -> Result<(), ConfigError> {
    let mut seen_hotkeys = FxHashMap::default();
    for (app_name, app_cfg) in &self.app {
      let hotkeys = app_cfg.hotkey.as_vec();

      // Find the app section span for error reporting
      let app_span = find_app_section_span(content, app_name);

      if hotkeys.len() > 4 {
        return Err(format_error_with_span(
          content,
          path,
          app_span.clone(),
          &format!(
            "App '{}' has {} hotkeys defined, but janq only supports a maximum of 4 hotkeys per application.",
            app_name,
            hotkeys.len()
          ),
        ).into());
      }

      for key in hotkeys {
        if !key.is_empty() {
          if let Err(e) = validate_hotkey(&key) {
            // Point to the hotkey field, not the section header
            let hotkey_span = find_field_in_app_section(content, app_name, "hotkey");
            return Err(
              format_error_with_span(
                content,
                path,
                hotkey_span,
                &format!("App [{}]: {} in hotkey '{}'", app_name, e, key),
              )
              .into(),
            );
          }

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
            // Point to the hotkey field for duplicate errors too
            let hotkey_span = find_field_in_app_section(content, app_name, "hotkey");
            return Err(
              format_error_with_span(
                content,
                path,
                hotkey_span,
                &format!(
                  "Duplicate hotkey '{}' (normalized: '{}') found in app '{}' and '{}'",
                  key, normalized, other_app, app_name
                ),
              )
              .into(),
            );
          }
        }
      }
    }

    for (app_name, app_cfg) in &self.app {
      let app_span = find_app_section_span(content, app_name);

      if app_cfg.window_class.is_empty() {
        return Err(
          format_error_with_span(
            content,
            path,
            app_span.clone(),
            &format!(
              "App '{}' is missing required field 'window_class'.",
              app_name
            ),
          )
          .into(),
        );
      }
      if app_cfg.window_class.len() < 3 {
        return Err(format_error_with_span(
          content,
          path,
          app_span.clone(),
          &format!(
            "App '{}' has a window_class '{}' that is too short. It must be at least 3 characters long for reliable fuzzy matching.",
            app_name, app_cfg.window_class
          ),
        ).into());
      }
      if app_cfg.start_command.is_empty() {
        return Err(
          format_error_with_span(
            content,
            path,
            app_span,
            &format!(
              "App [{}]: missing required field 'start_command'.",
              app_name
            ),
          )
          .into(),
        );
      }
    }

    // Note: Easing validation is now handled by the Easing enum's deserializer.

    if self.app.is_empty() {
      return Err(
        crate::error::format_error(
          "No app configured. Add at least one [app] or [app.name] section to your config.",
        )
        .into(),
      );
    }

    // Platform-specific validation
    #[cfg(target_os = "windows")]
    if self.window.all_desktops.is_some() || self.window.force_priority.is_some() {
      return Err(
        crate::error::format_error(
          "Linux-only settings (all_desktops, force_priority) are present in your config. These are not supported on Windows.",
        )
        .into(),
      );
    }

    Ok(())
  }
}

/// Finds the byte span of an [app.name] section header in the content.
fn find_app_section_span(content: &str, app_name: &str) -> std::ops::Range<usize> {
  // 1. Try exact [app.NAME]
  let pattern = "[app.".to_string() + app_name + "]";
  if let Some(pos) = content.find(&pattern) {
    return pos..pos + pattern.len();
  }

  // 2. Try generic [app]
  if let Some(pos) = content.find("[app]") {
    return pos..pos + 5;
  }

  // Fallback to start of file
  0..1
}

/// Finds the byte span of a specific field within an app section.
/// Returns the span of the field name (e.g., "hotkey" in "hotkey = ...")
fn find_field_in_app_section(
  content: &str,
  app_name: &str,
  field_name: &str,
) -> std::ops::Range<usize> {
  let span = find_app_section_span(content, app_name);
  let section_start = span.start;

  // Find the end of this section (next [...] or end of file)
  let after_section = &content[section_start..];
  let section_end = after_section[1..] // Skip the '[' of the current section
    .find("\n[")
    .map(|p| section_start + 1 + p)
    .unwrap_or(content.len());

  // Search for the field within this section
  let section_content = &content[section_start..section_end];

  // Pattern match without formatting if possible:
  // We need to find "\n" + field_name
  for line in section_content.lines() {
    let trimmed = line.trim_start();
    if let Some(rest) = trimmed.strip_prefix(field_name) {
      if rest.is_empty() || rest.starts_with(' ') || rest.starts_with('=') || rest.starts_with('\t')
      {
        // Calculate byte offset of this line in the content
        // This is slightly complex with .lines() as it strips \n
        // Let's use string find instead but be careful about matches outside line starts.
      }
    }
  }

  // Fallback to the optimized find:
  let search_pattern = "\n".to_string() + field_name;
  if let Some(pos) = section_content.find(&search_pattern) {
    let field_start = section_start + pos + 1;
    return field_start..field_start + field_name.len();
  }

  span
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

#[derive(Clone, Debug, Deserialize, Default)]
#[serde(deny_unknown_fields, default)]
pub struct AppConfig {
  pub window_class: String,
  pub start_command: String,
  pub hotkey: HotkeyConfig,
  pub animate_opacity: Option<bool>,
  pub width: Option<Dimension>,
  pub height: Option<Dimension>,
  pub slide_from: Option<SlideDirection>,
  // Same alias here for individual app configs
  #[serde(alias = "offset")]
  pub position_offset: Option<PositionOffset>,
  pub no_borders: Option<bool>,
}

/// A resolved dimension value (calculated pixel value and whether it was a percentage).
#[derive(Clone, Copy, Debug)]
pub struct ResolvedDimension {
  pub val: f64,
  pub is_percent: bool,
}

impl AppConfig {
  pub fn get_animate_opacity(&self, default_val: bool) -> bool {
    self.animate_opacity.unwrap_or(default_val)
  }

  pub fn get_no_borders(&self, default_val: bool) -> bool {
    self.no_borders.unwrap_or(default_val)
  }

  pub fn resolve_dimensions(
    &self,
    global: &WindowConfig,
  ) -> (ResolvedDimension, ResolvedDimension) {
    let w = self.width.as_ref().or(global.width.as_ref());
    let h = self.height.as_ref().or(global.height.as_ref());

    let rw = match w {
      Some(Dimension::Percent(p)) => ResolvedDimension {
        val: *p,
        is_percent: true,
      },
      Some(Dimension::Pixels(px)) => ResolvedDimension {
        val: *px as f64,
        is_percent: false,
      },
      Some(Dimension::Unset) | None => ResolvedDimension {
        val: 0.0,
        is_percent: false,
      },
    };
    let rh = match h {
      Some(Dimension::Percent(p)) => ResolvedDimension {
        val: *p,
        is_percent: true,
      },
      Some(Dimension::Pixels(px)) => ResolvedDimension {
        val: *px as f64,
        is_percent: false,
      },
      Some(Dimension::Unset) | None => ResolvedDimension {
        val: 0.0,
        is_percent: false,
      },
    };
    (rw, rh)
  }

  /// Resolves slide direction and position offset with fallback to global config.
  pub fn resolve_slide_config(&self, global: &WindowConfig) -> (SlideDirection, PositionOffset) {
    let direction = self.slide_from.clone().unwrap_or(global.slide_from.clone());
    let offset = self
      .position_offset
      .clone()
      .unwrap_or(global.position_offset.clone());
    (direction, offset)
  }
}

/// A type-safe display mode for monitor selection.
#[derive(Clone, Debug, PartialEq, Default)]
pub enum DisplayMode {
  #[default]
  FollowMouse,
  Active,
  Specific,
}

impl DisplayMode {
  /// Valid string values for this enum (single source of truth).
  pub const VALID_VALUES: &'static [&'static str] = &["follow-mouse", "active", "specific"];
}

impl std::fmt::Display for DisplayMode {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    match self {
      DisplayMode::FollowMouse => write!(f, "follow-mouse"),
      DisplayMode::Active => write!(f, "active"),
      DisplayMode::Specific => write!(f, "specific"),
    }
  }
}

impl<'de> serde::Deserialize<'de> for DisplayMode {
  fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
  where
    D: serde::Deserializer<'de>,
  {
    let s = String::deserialize(deserializer)?;
    let s = s.trim().to_lowercase();
    match s.as_str() {
      "follow-mouse" => Ok(DisplayMode::FollowMouse),
      "active" => Ok(DisplayMode::Active),
      "specific" => Ok(DisplayMode::Specific),
      other => {
        let hint = crate::matching::suggest_similar(other, Self::VALID_VALUES)
          .map(|s| format!(" Did you mean '{}'?", s))
          .unwrap_or_else(|| format!(" Valid values: {}.", Self::VALID_VALUES.join(", ")));
        Err(serde::de::Error::custom(format!(
          "Invalid display_mode '{}'.{}",
          other, hint
        )))
      }
    }
  }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct WindowConfig {
  pub display_mode: DisplayMode,
  pub display_index: u8,
  pub width: Option<Dimension>,
  pub height: Option<Dimension>,
  pub keep_above: bool,
  pub no_borders: bool,
  pub skip_pager: bool,
  pub all_desktops: Option<bool>,
  pub force_priority: Option<bool>,
  pub auto_show: bool,
  pub auto_hide: bool,
  pub slide_from: SlideDirection,
  // This allows both "position_offset" and "offset" in TOML
  #[serde(alias = "offset")]
  pub position_offset: PositionOffset,
}

impl Default for WindowConfig {
  fn default() -> Self {
    Self {
      display_mode: DisplayMode::FollowMouse,
      display_index: 0,
      width: None,
      height: None,
      keep_above: false,
      no_borders: false,
      skip_pager: false,
      all_desktops: None,
      force_priority: None,
      auto_show: false,
      auto_hide: false,
      slide_from: SlideDirection::default(),
      position_offset: PositionOffset::default(),
    }
  }
}

/// A type-safe easing curve.
///
/// Supported values: linear, ease, ease-in, ease-out, ease-in-out,
/// sine, sine-in, sine-out, cubic, cubic-in, cubic-out,
/// quart, quart-in, quart-out, back, back-in, back-out,
/// expo, expo-in, expo-out, impulse, or cubic-bezier(x1,y1,x2,y2)
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub enum Easing {
  Linear,
  #[default]
  Ease,
  EaseIn,
  EaseOut,
  EaseInOut,
  SineIn,
  SineOut,
  SineInOut,
  CubicIn,
  CubicOut,
  CubicInOut,
  QuartIn,
  QuartOut,
  QuartInOut,
  BackIn,
  BackOut,
  BackInOut,
  ExpoIn,
  ExpoOut,
  ExpoInOut,
  Impulse,
  Custom(f64, f64, f64, f64),
}

impl Easing {
  /// Valid string values for this enum (single source of truth).
  pub const VALID_VALUES: &'static [&'static str] = &[
    "linear",
    "ease",
    "ease-in",
    "ease-out",
    "ease-in-out",
    "sine",
    "sine-in",
    "sine-out",
    "cubic",
    "cubic-in",
    "cubic-out",
    "quart",
    "quart-in",
    "quart-out",
    "back",
    "back-in",
    "back-out",
    "expo",
    "expo-in",
    "expo-out",
    "impulse",
  ];
}

impl std::fmt::Display for Easing {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    let s = match self {
      Easing::Linear => "linear",
      Easing::Ease => "ease",
      Easing::EaseIn => "ease-in",
      Easing::EaseOut => "ease-out",
      Easing::EaseInOut => "ease-in-out",
      Easing::SineIn => "sine-in",
      Easing::SineOut => "sine-out",
      Easing::SineInOut => "sine",
      Easing::CubicIn => "cubic-in",
      Easing::CubicOut => "cubic-out",
      Easing::CubicInOut => "cubic",
      Easing::QuartIn => "quart-in",
      Easing::QuartOut => "quart-out",
      Easing::QuartInOut => "quart",
      Easing::BackIn => "back-in",
      Easing::BackOut => "back-out",
      Easing::BackInOut => "back",
      Easing::ExpoIn => "expo-in",
      Easing::ExpoOut => "expo-out",
      Easing::ExpoInOut => "expo",
      Easing::Impulse => "impulse",
      Easing::Custom(x1, y1, x2, y2) => {
        return write!(f, "cubic-bezier({}, {}, {}, {})", x1, y1, x2, y2);
      }
    };
    write!(f, "{}", s)
  }
}

impl<'de> serde::Deserialize<'de> for Easing {
  fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
  where
    D: serde::Deserializer<'de>,
  {
    let s = String::deserialize(deserializer)?;
    let s = s.trim().to_lowercase();
    let easing = match s.as_str() {
      "linear" => Easing::Linear,
      "ease" => Easing::Ease,
      "ease-in" => Easing::EaseIn,
      "ease-out" => Easing::EaseOut,
      "ease-in-out" => Easing::EaseInOut,
      "sine" => Easing::SineInOut,
      "sine-in" => Easing::SineIn,
      "sine-out" => Easing::SineOut,
      "cubic" => Easing::CubicInOut,
      "cubic-in" => Easing::CubicIn,
      "cubic-out" => Easing::CubicOut,
      "quart" => Easing::QuartInOut,
      "quart-in" => Easing::QuartIn,
      "quart-out" => Easing::QuartOut,
      "back" => Easing::BackInOut,
      "back-in" => Easing::BackIn,
      "back-out" => Easing::BackOut,
      "expo" => Easing::ExpoInOut,
      "expo-in" => Easing::ExpoIn,
      "expo-out" => Easing::ExpoOut,
      "impulse" => Easing::Impulse,
      // Custom Bezier
      other => {
        if let Some((x1, y1, x2, y2)) = crate::validation::parse_bezier(other) {
          Easing::Custom(x1, y1, x2, y2)
        } else {
          let hint = crate::matching::suggest_similar(other, Self::VALID_VALUES)
            .map(|s| format!(" Did you mean '{}'?", s))
            .unwrap_or_else(|| format!(" Use a keyword (like 'ease', 'impulse', 'back-out') or a custom cubic-bezier. Valid keywords: {}.", Self::VALID_VALUES.join(", ")));
          return Err(serde::de::Error::custom(format!(
            "Invalid easing curve '{}'.{}",
            other, hint
          )));
        }
      }
    };
    Ok(easing)
  }
}

/// Control the framerate of animations.
///
/// Can be:
/// - `"auto"` (default) - Uses VSync timing via `DwmFlush`
/// - A number `0-1000` - Fixed framerate (0 = disable animations)
#[derive(Clone, Debug, PartialEq, Default, Deserialize)]
#[serde(try_from = "FramerateRaw")]
pub enum Framerate {
  #[default]
  Auto,
  Specific(u16),
}

#[derive(Deserialize)]
#[serde(untagged)]
enum FramerateRaw {
  Num(i16),
  Str(String),
}

impl TryFrom<FramerateRaw> for Framerate {
  type Error = String;

  fn try_from(raw: FramerateRaw) -> Result<Self, Self::Error> {
    match raw {
      FramerateRaw::Num(n) if n >= 0 && n <= 1000 => Ok(Framerate::Specific(n as u16)),
      FramerateRaw::Num(n) => Err(format!("Invalid framerate: {}. Must be between 0-1000.", n)),
      FramerateRaw::Str(s) if s.trim().to_lowercase() == "auto" => Ok(Framerate::Auto),
      FramerateRaw::Str(s) => Err(format!(
        "Invalid framerate: '{}'. Must be a number between 0-1000 or the string 'auto'.",
        s
      )),
    }
  }
}

#[derive(Clone, Debug)]
pub struct AnimationConfig {
  pub show_duration: u16,
  pub hide_duration: u16,
  pub show_easing: Easing,
  pub hide_easing: Easing,
  pub animate_opacity: bool,
  pub show_opacity_point: f64,
  pub hide_opacity_point: f64,
  pub framerate: Framerate,
}

impl<'de> Deserialize<'de> for AnimationConfig {
  fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
  where
    D: Deserializer<'de>,
  {
    #[derive(Deserialize, Default)]
    #[serde(deny_unknown_fields, default)]
    struct Shadow {
      duration: Option<u16>,
      show_duration: Option<u16>,
      hide_duration: Option<u16>,
      easing: Option<Easing>,
      show_easing: Option<Easing>,
      hide_easing: Option<Easing>,
      animate_opacity: bool,
      show_opacity_point: f64,
      hide_opacity_point: f64,
      framerate: Option<Framerate>,
    }

    let s = Shadow::deserialize(deserializer)?;
    let d = AnimationConfig::default();

    Ok(AnimationConfig {
      show_duration: s.show_duration.or(s.duration).unwrap_or(d.show_duration),
      hide_duration: s.hide_duration.or(s.duration).unwrap_or(d.hide_duration),
      show_easing: s.show_easing.or(s.easing).unwrap_or(d.show_easing),
      hide_easing: s.hide_easing.or(s.easing).unwrap_or(d.hide_easing),
      animate_opacity: s.animate_opacity,
      show_opacity_point: s.show_opacity_point,
      hide_opacity_point: s.hide_opacity_point,
      framerate: s.framerate.unwrap_or(d.framerate),
    })
  }
}

impl Default for AnimationConfig {
  fn default() -> Self {
    Self {
      show_duration: 350,
      hide_duration: 350,
      show_easing: Easing::Ease,
      hide_easing: Easing::Ease,
      animate_opacity: false,
      show_opacity_point: 0.2,
      hide_opacity_point: 0.8,
      framerate: Framerate::Auto,
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

pub fn load_config(target_path: Option<PathBuf>) -> Result<(Config, Option<PathBuf>), ConfigError> {
  if let Some(path) = target_path {
    if path.exists() {
      match fs::read_to_string(&path) {
        Ok(content) => match toml::from_str::<Config>(&content) {
          Ok(c) => {
            println!("Loaded config from (cached): {:?}", path);
            c.validate(&content, &path)?;
            return Ok((c, Some(path)));
          }
          Err(e) => {
            return Err(format_toml_error(&content, &path, e).into());
          }
        },
        Err(e) => {
          return Err(
            crate::error::format_error(&format!("Could not read config file at {:?}: {}", path, e))
              .into(),
          );
        }
      }
    } else {
      return Err(
        crate::error::format_error(&format!("Config file no longer exists at: {:?}", path)).into(),
      );
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
    config_paths.extend([home.join("janq.toml"), home.join(".janq.toml")]);
  }

  // De-duplicate while preserving order
  let mut unique_paths = Vec::new();
  let mut seen = FxHashSet::default();
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
            c.validate(&content, &path)?;
            return Ok((c, Some(path)));
          }
          Err(e) => {
            return Err(format_toml_error(&content, &path, e).into());
          }
        },
        Err(e) => {
          return Err(
            crate::error::format_error(&format!("Could not read config file at {:?}: {}", path, e))
              .into(),
          );
        }
      }
    }
  }

  let msg = if cfg!(target_os = "windows") {
    "No config file found. Create %APPDATA%\\janq\\janq.toml with at least one [app] section."
  } else {
    "No config file found. Create ./janq.toml or ~/.config/janq/janq.toml with at least one [app] section."
  };

  Err(crate::error::format_error(msg).into())
}

/// Formats a TOML error with line context and a visual pointer.
fn format_toml_error(content: &str, path: &std::path::Path, err: toml::de::Error) -> String {
  let message = err.message().to_string();

  // Try to extract field name from error message to find actual line
  let field_name = extract_field_from_error(&message);

  // Get TOML's span as a hint for which section the error is in
  let toml_span = err.span();

  // If we found a field name and have a section hint, find the field near that section
  if let (Some(ref field), Some(ref hint_span)) = (&field_name, &toml_span) {
    if let Some(better_span) = find_field_near_span(content, field, hint_span.clone()) {
      return format_error_with_span(content, path, better_span, &message);
    }
  }

  // Fall back to TOML's span if available
  let span = match toml_span {
    Some(s) if s.start < content.len() => s,
    _ => return crate::error::format_error(&format!("{} in {:?}", message, path)),
  };

  format_error_with_span(content, path, span, &message)
}

/// Extracts field name from common TOML error message patterns.
/// Only extracts from semantic errors (missing/unknown field), not syntax errors.
fn extract_field_from_error(message: &str) -> Option<String> {
  // Only extract field names from semantic errors, not syntax errors like "expected newline"
  // Pattern: "missing field `fieldname`" or "unknown field `fieldname`"
  if message.contains("missing field") || message.contains("unknown field") {
    if let Some(start) = message.find('`') {
      if let Some(end) = message[start + 1..].find('`') {
        return Some(message[start + 1..start + 1 + end].to_string());
      }
    }
  }
  // Pattern: 'invalid type: string "fieldname"' (bare key interpreted as string)
  if message.contains("invalid type") {
    if let Some(start) = message.find('"') {
      if let Some(end) = message[start + 1..].find('"') {
        return Some(message[start + 1..start + 1 + end].to_string());
      }
    }
  }
  None
}

/// Finds the span of a field, preferring the occurrence closest to hint_span.
/// If only one occurrence exists, returns it regardless of hint.
fn find_field_near_span(
  content: &str,
  field: &str,
  hint_span: std::ops::Range<usize>,
) -> Option<std::ops::Range<usize>> {
  // Find ALL occurrences of the field
  let mut occurrences = Vec::new();
  let mut byte_offset = 0;

  for line in content.lines() {
    let trimmed = line.trim_start();
    let leading_ws = line.len() - trimmed.len();

    if let Some(after_field) = trimmed.strip_prefix(field) {
      if after_field.is_empty()
        || after_field.starts_with(' ')
        || after_field.starts_with('=')
        || after_field.starts_with('\t')
      {
        let field_start = byte_offset + leading_ws;
        occurrences.push(field_start..field_start + field.len());
      }
    }

    byte_offset += line.len() + 1; // +1 for newline
  }

  if occurrences.is_empty() {
    return None;
  }

  // If only one occurrence, return it directly (no need for hint)
  if occurrences.len() == 1 {
    return occurrences.into_iter().next();
  }

  // Pick the occurrence closest to hint_span.start
  occurrences.into_iter().min_by_key(|span| {
    let dist_to_start = (span.start as isize - hint_span.start as isize).abs();
    let dist_to_end = (span.start as isize - hint_span.end as isize).abs();
    dist_to_start.min(dist_to_end)
  })
}

// =============================================================================
// Custom Deserialization
// =============================================================================

fn deserialize_app<'de, D>(deserializer: D) -> Result<FxIndexMap<String, AppConfig>, D::Error>
where
  D: Deserializer<'de>,
{
  struct AppVisitor;

  impl<'de> Visitor<'de> for AppVisitor {
    type Value = FxIndexMap<String, AppConfig>;

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

      let raw_map =
        FxIndexMap::<String, toml::Value>::deserialize(MapAccessDeserializer::new(map))?;

      if raw_map.is_empty() {
        return Ok(FxIndexMap::default());
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
        let mut result = FxIndexMap::default();
        for (name, value) in raw_map {
          if value.is_table() {
            let config = AppConfig::deserialize(value).map_err(de::Error::custom)?;
            result.insert(name, config);
          }
        }
        Ok(result)
      } else if has_flat_keys {
        let table = toml::Value::Table(raw_map.into_iter().collect());
        let config = AppConfig::deserialize(table).map_err(de::Error::custom)?;
        let mut result = FxIndexMap::default();

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
        Ok(FxIndexMap::default())
      }
    }

    fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
    where
      A: de::SeqAccess<'de>,
    {
      let mut result = FxIndexMap::default();
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
  fn test_animation_config_aliases() {
    #[derive(Deserialize, Debug)]
    struct TestConfig {
      animation: AnimationConfig,
    }

    // Test duration and easing fallbacks
    let toml_str = r#"
[animation]
duration = 500
easing = "back-out"
"#;
    let config: TestConfig = toml::from_str(toml_str).unwrap();
    assert_eq!(config.animation.show_duration, 500);
    assert_eq!(config.animation.hide_duration, 500);
    assert_eq!(config.animation.show_easing, Easing::BackOut);
    assert_eq!(config.animation.hide_easing, Easing::BackOut);

    // Test explicit override (show_duration overrides duration)
    // Note: Serde's behavior for multiple aliases/fields depends on order in the source or field declaration.
    // Usually, the last one seen wins if they map to the same field.
    let toml_str2 = r#"
[animation]
duration = 500
show_duration = 300
"#;
    let config2: TestConfig = toml::from_str(toml_str2).unwrap();
    assert_eq!(config2.animation.show_duration, 300);
    assert_eq!(config2.animation.hide_duration, 500);

    let toml_str3 = r#"
[animation]
show_duration = 300
duration = 500
"#;
    let config3: TestConfig = toml::from_str(toml_str3).unwrap();
    // With the refined implementation, explicit fields override the fallback regardless of order.
    assert_eq!(config3.animation.show_duration, 300);
    assert_eq!(config3.animation.hide_duration, 500);
  }

  #[test]
  fn test_strict_config() {
    // Root level unknown key
    let toml_str = r#"
unknown_global_key = "value"
[window]
display_mode = "active"
"#;
    let err = toml::from_str::<Config>(toml_str);
    assert!(err.is_err());
    assert!(err.unwrap_err().to_string().contains("unknown field"));

    // [window] level unknown key
    let toml_str = r#"
[window]
unknown_window_key = "value"
"#;
    let err = toml::from_str::<Config>(toml_str);
    assert!(err.is_err());
    assert!(err.unwrap_err().to_string().contains("unknown field"));

    // [app] level unknown key
    let toml_str = r#"
[app.terminal]
window_class = "wezterm"
start_command = "wezterm"
unknown_app_key = "value"
"#;
    let err = toml::from_str::<Config>(toml_str);
    assert!(err.is_err());
    assert!(err.unwrap_err().to_string().contains("unknown field"));

    // [animation] level unknown key
    let toml_str = r#"
[animation]
unknown_animation_key = "value"
"#;
    let err = toml::from_str::<Config>(toml_str);
    assert!(err.is_err());
    assert!(err.unwrap_err().to_string().contains("unknown field"));
  }

  #[test]
  fn test_error_formatting() {
    let content = "foo = \"bar\"\nbaz = 123";
    let path = std::path::Path::new("test.toml");

    #[derive(Deserialize, Debug)]
    #[serde(deny_unknown_fields)]
    struct LocalConfig {
      #[allow(dead_code)]
      foo: String,
    }

    let err = toml::from_str::<LocalConfig>(content).unwrap_err();
    let formatted = format_toml_error(content, path, err);

    println!("{}", formatted);
    // Note: formatted string contains ANSI color codes, so check for parts that aren't split
    assert!(formatted.contains("unknown field"));
    assert!(formatted.contains("baz"));
    assert!(formatted.contains("test.toml"));
    assert!(formatted.contains(":2:1"));
    assert!(formatted.contains("baz = 123"));
    assert!(formatted.contains("^~~~"));
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
    assert_eq!(w.val, 800.0);
    assert_eq!(h.val, 600.0);

    let app2 = AppConfig {
      width: Some(Dimension::Unset), // "0" or "unset" means skip resizing
      ..Default::default()
    };
    let (w2, h2) = app2.resolve_dimensions(&global);
    assert_eq!(w2.val, 0.0); // Unset means no resize (0.0)
    assert!(!w2.is_percent);
    assert_eq!(h2.val, 600.0); // Height still inherits from global
  }

  #[test]
  fn test_semantic_error_helpfulness() {
    let toml_str = r#"
[app.terminal]
window_class = "wezterm"
start_command = "wezterm"
hotkey = "Meta+Graveee" # Typo in key name
"#;
    let config: Config = toml::from_str(toml_str).unwrap();
    let err = config
      .validate(toml_str, std::path::Path::new("test.toml"))
      .unwrap_err();

    println!("Semantic Error: {}", err);
    // Note: error message contains ANSI color codes, check for parts that aren't split
    assert!(err.message.contains("App"));
    assert!(err.message.contains("terminal"));
    assert!(err.message.contains("Graveee"));
    assert!(err.message.contains("hotkey"));
    assert!(err.message.contains("Meta+Graveee"));
  }

  #[test]
  fn test_structural_error_helpfulness() {
    let toml_str = r#"
[window]
displa_mode = "active" # Typo: displa -> display
"#;
    let err = toml::from_str::<Config>(toml_str).unwrap_err();
    let formatted = format_toml_error(toml_str, std::path::Path::new("janq.toml"), err);

    println!("Structural Error:\n{}", formatted);
    // Note: formatted string contains ANSI color codes, so check for parts that aren't split
    assert!(formatted.contains("unknown field"));
    assert!(formatted.contains("displa_mode"));
    assert!(formatted.contains("janq.toml"));
    assert!(formatted.contains(":3:1"));
    assert!(formatted.contains("displa_mode = \"active\""));
    assert!(formatted.contains("^~~~"));
  }
}
