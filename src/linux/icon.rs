//! Symbolic tray icon pixmap data for StatusNotifierItem.
//!
//! ## Strategy
//!
//! Only the `mono_icon = true` path goes through here — the colored case is
//! served via `IconName = "janq"` and resolved by Plasma from the hicolor
//! theme (`apps/janq.svg`), so none of the ARGB blobs below get paged in.
//!
//! At build time, the symbolic SVG is rendered to ARGB in white + alpha.
//! At runtime, RGB is swapped to the active Plasma foreground color (read
//! from `[Colors:Window] ForegroundNormal` in `kdeglobals`), with alpha
//! preserved. This mirrors SVG's `currentColor` semantics without a runtime
//! SVG parser: the symbolic source is single-color, so every pre-rendered
//! pixel is `(255, 255, 255, α)` after demultiplication — replacing RGB
//! with any theme color leaves the anti-aliased silhouette intact.
//!
//! Dark/light detection (used by per-theme mono flags) is driven primarily
//! by `[Colors:Window] BackgroundNormal` — the background directly encodes
//! the mode, avoiding false positives when a scheme customises foreground
//! but not background. Falls back to `ForegroundNormal` and then to the
//! `[General] ColorScheme` name (`*Dark*`).
//!
//! We serve via `IconPixmap` with empty `IconName` for this case because
//! Plasma's tray would otherwise auto-substitute `<name>-symbolic` theme
//! icons for items with `category = "ApplicationStatus"`.

use std::sync::atomic::{AtomicBool, Ordering};

use tokio::sync::mpsc as tokio_mpsc;

pub type IconPixmap = Vec<(i32, i32, Vec<u8>)>;

// Cached theme state — refreshed only when the kdeglobals watcher fires (or
// the first time `is_dark_theme()` is queried). Plasma and any secondary SNI
// hosts poll icon properties aggressively; recomputing from disk on each
// getter call burns syscalls and spams logs with redundant reads.
//
// The extra AtomicBool gates the first-call initialization so we don't have
// to store the computed value behind a Mutex.
static CACHED_IS_DARK: AtomicBool = AtomicBool::new(true);
static CACHED_FG_R: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(255);
static CACHED_FG_G: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(255);
static CACHED_FG_B: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(255);
static CACHE_INITIALIZED: AtomicBool = AtomicBool::new(false);

/// Source the cache consumers should treat as authoritative until the next
/// `refresh_theme_cache()` call. Cheap — one atomic load.

const SYMBOLIC_ALPHA: [(i32, &[u8]); 4] = [
  (
    22,
    include_bytes!(concat!(env!("OUT_DIR"), "/symbolic_22.alpha")),
  ),
  (
    32,
    include_bytes!(concat!(env!("OUT_DIR"), "/symbolic_32.alpha")),
  ),
  (
    48,
    include_bytes!(concat!(env!("OUT_DIR"), "/symbolic_48.alpha")),
  ),
  (
    64,
    include_bytes!(concat!(env!("OUT_DIR"), "/symbolic_64.alpha")),
  ),
];

/// Default foreground used when kdeglobals can't be parsed — matches Breeze Dark
/// (KDE's default dark scheme). Picked so a misparse still produces a visible icon.
const DEFAULT_FG: (u8, u8, u8) = (239, 240, 241);

/// Returns the symbolic tray pixmap set, retinted to the active Plasma foreground
/// color. Ascending size; Plasma picks the best match for the panel dimension.
pub fn symbolic_pixmap() -> IconPixmap {
  if !CACHE_INITIALIZED.load(Ordering::Relaxed) {
    refresh_theme_cache();
  }
  let r = CACHED_FG_R.load(Ordering::Relaxed);
  let g = CACHED_FG_G.load(Ordering::Relaxed);
  let b = CACHED_FG_B.load(Ordering::Relaxed);

  SYMBOLIC_ALPHA
    .iter()
    .map(|(size, alpha)| {
      let mut argb = Vec::with_capacity(alpha.len() * 4);
      for &a in *alpha {
        argb.push(a); // A
        argb.push(r); // R
        argb.push(g); // G
        argb.push(b); // B
      }
      (*size, *size, argb)
    })
    .collect()
}

/// Returns true when Plasma's active color scheme is dark.
///
/// Uses `[Colors:Window] BackgroundNormal` luminance as the primary signal —
/// the background directly encodes dark vs. light, so a single threshold works
/// regardless of how saturated the foreground is. Falls back to
/// `ForegroundNormal` (inverted), then to the `[General] ColorScheme=` name
/// (matching `*Dark*`) if the colors aren't written to kdeglobals at all.
pub fn is_dark_theme() -> bool {
  if !CACHE_INITIALIZED.load(Ordering::Relaxed) {
    refresh_theme_cache();
  }
  CACHED_IS_DARK.load(Ordering::Relaxed)
}

/// Re-reads kdeglobals and updates the cached is-dark value. Returns `true`
/// when the resolved value actually changed (or the cache was uninitialised),
/// so the caller can skip a redundant NewIcon emission when KConfig's
/// secondary write burst doesn't actually flip the theme.
///
/// Call on startup (implicit via first `is_dark_theme()`) and after each
/// debounced kdeglobals event — never from the property getters, which are
/// on Plasma's hot path.
pub fn refresh_theme_cache() -> bool {
  let mut bg = None;
  let mut fg = None;
  let mut scheme = None;

  if let Some(content) = read_kdeglobals() {
    let mut section = String::new();
    for line in content.lines() {
      // Strip comments and trim
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

  let (fr, fg_g, fb) = fg.unwrap_or(DEFAULT_FG);

  let was_initialized = CACHE_INITIALIZED.swap(true, Ordering::Relaxed);
  let prev_dark = CACHED_IS_DARK.swap(is_dark, Ordering::Relaxed);
  let prev_r = CACHED_FG_R.swap(fr, Ordering::Relaxed);
  let prev_g = CACHED_FG_G.swap(fg_g, Ordering::Relaxed);
  let prev_b = CACHED_FG_B.swap(fb, Ordering::Relaxed);

  !was_initialized || prev_dark != is_dark || prev_r != fr || prev_g != fg_g || prev_b != fb
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
/// re-queries IconPixmap and picks up the retinted symbolic.
pub fn watch_kdeglobals() -> Option<tokio_mpsc::UnboundedReceiver<()>> {
  let home = std::env::var_os("HOME")?;
  let path = std::path::PathBuf::from(home).join(".config/kdeglobals");
  janq::inotify::watch_file_changes(path)
}
