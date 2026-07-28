//! Theme detection for the StatusNotifierItem tray icon.
//!
//! ## Strategy
//!
//! The tray icon is served entirely via `IconName` over D-Bus:
//!
//! - `mono_icon = false` → `IconName = "janq-color"` (colored SVG from hicolor theme)
//! - `mono_icon = true`  → `IconName = "janq-symbolic"` (symbolic SVG recolored natively by Plasma)
//!
//! Dark/light detection (used by `mono_icon_dark` / `mono_icon_light`) is driven
//! by `[Colors:Window] BackgroundNormal` luminance — the background directly
//! encodes the mode, avoiding false positives when a scheme customises foreground
//! but not background. Falls back to `ForegroundNormal` and then to the
//! `[General] ColorScheme` name (`*Dark*`).

use std::sync::atomic::{AtomicBool, Ordering};

use tokio::sync::mpsc as tokio_mpsc;

// Cached theme state — refreshed only when the kdeglobals watcher fires (or
// the first time `is_dark_theme()` is queried).
static CACHED_IS_DARK: AtomicBool = AtomicBool::new(true);
static CACHE_INITIALIZED: AtomicBool = AtomicBool::new(false);

/// Returns true when Plasma's active color scheme is dark.
pub fn is_dark_theme() -> bool {
  if !CACHE_INITIALIZED.load(Ordering::Relaxed) {
    refresh_theme_cache();
  }
  CACHED_IS_DARK.load(Ordering::Relaxed)
}

/// Re-reads kdeglobals and updates the cached is-dark value. Returns `true`
/// when the resolved value actually changed (or the cache was uninitialised),
/// so the caller can skip a redundant NewIcon emission.
pub fn refresh_theme_cache() -> bool {
  let mut bg = None;
  let mut fg = None;
  let mut scheme = None;

  if let Some(content) = read_kdeglobals() {
    let mut section = String::new();
    for line in content.lines() {
      let line = line.split('#').next().unwrap_or("").trim();
      if line.is_empty() {
        continue;
      }

      if line.starts_with('[') && line.ends_with(']') {
        section = line.to_lowercase();
        continue;
      }

      if let Some(pos) = line.find('=') {
        let key = line[..pos].trim().to_lowercase();
        let val = line[pos + 1..].trim();

        if section == "[colors:window]" {
          if key == "backgroundnormal" {
            bg = parse_rgb(val);
          } else if key == "foregroundnormal" {
            fg = parse_rgb(val);
          }
        } else if section == "[general]" && key == "colorscheme" {
          scheme = Some(val.to_string());
        }
      }
    }
  }

  let is_dark = if let Some((r, g, b)) = bg {
    luminance(r, g, b) < 128.0
  } else if let Some((r, g, b)) = fg {
    luminance(r, g, b) > 128.0
  } else if let Some(s) = scheme {
    s.to_lowercase().contains("dark")
  } else {
    true
  };

  let was_initialized = CACHE_INITIALIZED.swap(true, Ordering::Relaxed);
  let prev_dark = CACHED_IS_DARK.swap(is_dark, Ordering::Relaxed);

  !was_initialized || prev_dark != is_dark
}

fn parse_rgb(s: &str) -> Option<(u8, u8, u8)> {
  let mut parts = s.split(',').map(|p| p.trim().parse::<u8>().ok());
  Some((
    parts.next().flatten()?,
    parts.next().flatten()?,
    parts.next().flatten()?,
  ))
}

fn luminance(r: u8, g: u8, b: u8) -> f32 {
  0.299 * r as f32 + 0.587 * g as f32 + 0.114 * b as f32
}

fn read_kdeglobals() -> Option<String> {
  let home = std::env::var_os("HOME")?;
  let path = std::path::PathBuf::from(home).join(".config/kdeglobals");
  std::fs::read_to_string(path).ok()
}

/// Watches `~/.config/kdeglobals` for changes. The returned receiver fires `()`
/// whenever the file is modified — caller should re-emit NewIcon so Plasma
/// re-queries `IconName` and picks up the correct symbolic/color icon.
pub fn watch_kdeglobals() -> Option<tokio_mpsc::UnboundedReceiver<()>> {
  let home = std::env::var_os("HOME")?;
  let path = std::path::PathBuf::from(home).join(".config/kdeglobals");
  janq::inotify::watch_file_changes(path)
}
