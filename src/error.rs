use std::fmt;
use std::path::Path;
#[cfg(target_os = "linux")]
use std::process::Command;

/// Project-wide Result type that replaces anyhow::Result.
pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

/// Replacement for anyhow::anyhow! macro for creating simple string errors.
#[macro_export]
macro_rules! format_error_boxed {
  ($($arg:tt)*) => {
    Box::<dyn std::error::Error + Send + Sync>::from(format!($($arg)*))
  };
}

// =============================================================================
// ConfigError - Wrapper for rich config error messages
// =============================================================================

/// A configuration error that preserves rich formatting while implementing std::error::Error.
///
/// This allows config errors to be used with `?` propagation while still providing
/// the detailed, colorized error messages users expect.
#[derive(Debug, Clone)]
pub struct ConfigError {
  /// The pre-formatted error message (may contain ANSI codes).
  pub message: String,
}

impl ConfigError {
  pub fn new(message: impl Into<String>) -> Self {
    Self {
      message: message.into(),
    }
  }
}

impl fmt::Display for ConfigError {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    write!(f, "{}", self.message)
  }
}

impl std::error::Error for ConfigError {}

impl From<String> for ConfigError {
  fn from(s: String) -> Self {
    Self::new(s)
  }
}

impl From<&str> for ConfigError {
  fn from(s: &str) -> Self {
    Self::new(s)
  }
}

/// Formats an error with line context and a visual pointer.
pub fn format_error_with_span(
  content: &str,
  path: &Path,
  span: std::ops::Range<usize>,
  message: &str,
) -> String {
  let (line_no, col_no, line_content) = get_line_context(content, span.start);
  let visual_width = calculate_visual_width(&content[span.clone()]);

  let mut pointer = " ".repeat(col_no.saturating_sub(1));
  pointer.push('^');
  for _ in 1..visual_width.saturating_sub(1) {
    pointer.push('~');
  }

  let colored_msg = colorize_message(message);

  format!(
    "\x1b[1;31merror\x1b[0m: {}\n \x1b[1;34m-->\x1b[0m \x1b[4m{}\x1b[0m:{}:{}\n\x1b[1;34m      |\n{:>5} | \x1b[0m{}\n\x1b[1;34m      | \x1b[1;31m{}\x1b[0m",
    colored_msg,
    path.display(),
    line_no,
    col_no,
    line_no,
    line_content,
    pointer
  )
}

/// Formats an error that references multiple locations (e.g., duplicate definitions).
pub fn format_error_with_multi_span(
  content: &str,
  path: &Path,
  spans: Vec<(std::ops::Range<usize>, String)>,
) -> String {
  let mut result = String::new();

  for (i, (span, message)) in spans.into_iter().enumerate() {
    let (line_no, col_no, line_content) = get_line_context(content, span.start);
    let visual_width = calculate_visual_width(&content[span.clone()]);

    let mut pointer = " ".repeat(col_no.saturating_sub(1));
    pointer.push('^');
    for _ in 1..visual_width.saturating_sub(1) {
      pointer.push('~');
    }

    let colored_msg = colorize_message(&message);

    if i == 0 {
      result.push_str(&format!(
        "\x1b[1;31merror\x1b[0m: {}\n \x1b[1;34m-->\x1b[0m \x1b[4m{}\x1b[0m:{}:{}\n\x1b[1;34m      |\n{:>5} | \x1b[0m{}\n\x1b[1;34m      | \x1b[1;31m{}\x1b[0m",
        colored_msg, path.display(), line_no, col_no, line_no, line_content, pointer
      ));
    } else {
      result.push_str(&format!(
        "\n \x1b[1;34m=\x1b[0m \x1b[1m{}\x1b[0m:\n \x1b[1;34m-->\x1b[0m \x1b[4m{}\x1b[0m:{}:{}\n\x1b[1;34m      |\n{:>5} | \x1b[0m{}\n\x1b[1;34m      | \x1b[1;31m{}\x1b[0m",
        colored_msg, path.display(), line_no, col_no, line_no, line_content, pointer
      ));
    }
  }

  result
}

/// Helper to get line number, visual column, and line content for a byte offset.
fn get_line_context(content: &str, span_start: usize) -> (usize, usize, String) {
  let mut line_no = 1;
  let mut col_no = 1;

  for (i, c) in content.char_indices() {
    if i >= span_start {
      break;
    }
    if c == '\n' {
      line_no += 1;
      col_no = 1;
    } else if c == '\t' {
      col_no += 4; // Tab expansion (match replace below)
    } else if c != '\r' {
      // Treat other characters as 1 visual column (close enough for CLI)
      col_no += 1;
    }
  }

  let line_content = content
    .lines()
    .nth(line_no - 1)
    .unwrap_or("")
    .replace('\t', "    ");

  (line_no, col_no, line_content)
}

/// Calculates visual width of a string segment, accounting for tabs.
fn calculate_visual_width(s: &str) -> usize {
  let mut width = 0;
  for c in s.chars() {
    if c == '\t' {
      width += 4;
    } else if c != '\r' && c != '\n' {
      width += 1;
    }
  }
  width.max(1)
}

/// Colorizes quoted values and key names in error messages.
fn colorize_message(message: &str) -> String {
  let mut result = String::with_capacity(message.len() + 50);
  let mut chars = message.chars().peekable();

  while let Some(c) = chars.next() {
    if c == '\'' {
      // Single-quoted value - cyan
      result.push_str("\x1b[1;36m'");
      while let Some(&inner) = chars.peek() {
        chars.next();
        result.push(inner);
        if inner == '\'' {
          break;
        }
      }
      result.push_str("\x1b[0m");
    } else if c == '`' {
      // Backtick-quoted value - cyan
      result.push_str("\x1b[1;36m`");
      while let Some(&inner) = chars.peek() {
        chars.next();
        result.push(inner);
        if inner == '`' {
          break;
        }
      }
      result.push_str("\x1b[0m");
    } else if c == '[' {
      // Bracketed app name - yellow
      result.push_str("\x1b[1;33m[");
      while let Some(&inner) = chars.peek() {
        chars.next();
        result.push(inner);
        if inner == ']' {
          break;
        }
      }
      result.push_str("\x1b[0m");
    } else {
      result.push(c);
    }
  }
  result
}

/// Formats a plain message with the styled error prefix.
///
/// Use this for errors that don't have span information (I/O errors, etc.)
pub fn format_error(message: &str) -> String {
  format!("\x1b[1;31merror\x1b[0m: {}", colorize_message(message))
}

/// Formats a plain message with the styled warning prefix.
pub fn format_warning(message: &str) -> String {
  format!("\x1b[1;33mwarning\x1b[0m: {}", colorize_message(message))
}

/// Strips ANSI escape codes from a string for use in GUI dialogs.
pub fn strip_ansi(s: &str) -> String {
  let mut result = String::with_capacity(s.len());
  let mut in_escape = false;
  for c in s.chars() {
    if c == '\x1b' {
      in_escape = true;
    } else if in_escape {
      if c == 'm' {
        in_escape = false;
      }
    } else {
      result.push(c);
    }
  }
  result
}

pub fn show_warning(message: &str) {
  let styled = if message.contains("\x1b[1;33mwarning\x1b[0m") {
    message.to_string()
  } else {
    format_warning(message)
  };
  eprintln!("{}", styled);
}

pub fn show_error(message: &str) {
  use std::io::IsTerminal;

  let styled = if message.contains("\x1b[1;31merror\x1b[0m") {
    message.to_string()
  } else {
    format_error(message)
  };
  eprintln!("{}", styled);

  // Only spawn a GUI error window if we're not already in an interactive terminal
  #[cfg(target_os = "linux")]
  if !std::io::stderr().is_terminal() {
    show_error_linux(&styled);
  }

  #[cfg(target_os = "windows")]
  if !std::io::stderr().is_terminal() {
    show_error_windows(&styled);
  }
}

#[cfg(target_os = "linux")]
fn show_error_linux(styled: &str) {
  use std::io::Write;
  use std::time::Duration;

  let clean_message = strip_ansi(styled);

  // Write message to temp file to preserve ANSI codes perfectly
  let temp_path = std::env::temp_dir().join("janq_error.txt");
  let file_written = (|| -> std::io::Result<()> {
    let mut file = std::fs::File::create(&temp_path)?;
    writeln!(file, "{}\n\nPress Enter to exit...", styled)?;
    file.sync_all()?; // Ensure content is flushed to disk
    Ok(())
  })()
  .is_ok();

  if !file_written {
    return;
  }

  let cat_cmd = format!("cat '{}'; read", temp_path.display());
  let cat_cmd_no_wait = format!("cat '{}'", temp_path.display());

  // Attempt to show in a terminal window and WAIT for it to close
  let terminals = vec![
    ("wezterm", vec!["start", "--", "bash", "-c", &cat_cmd]),
    ("kitty", vec!["sh", "-c", &cat_cmd]),
    ("alacritty", vec!["-e", "sh", "-c", &cat_cmd]),
    (
      "konsole",
      vec!["--noclose", "-e", "sh", "-c", &cat_cmd_no_wait],
    ),
    ("gnome-terminal", vec!["--wait", "--", "sh", "-c", &cat_cmd]),
    ("xterm", vec!["-hold", "-e", "sh", "-c", &cat_cmd_no_wait]),
  ];

  for (cmd, args) in terminals {
    if let Ok(mut child) = Command::new(cmd).args(args).spawn() {
      // Wait for terminal to close before continuing
      // Use a timeout to avoid hanging forever if something goes wrong
      let start = std::time::Instant::now();
      let timeout = Duration::from_secs(60); // 1 minute timeout
      loop {
        match child.try_wait() {
          Ok(Some(_)) => break, // Process exited
          Ok(None) => {
            if start.elapsed() > timeout {
              let _ = child.kill();
              break;
            }
            std::thread::sleep(Duration::from_millis(100));
          }
          Err(_) => break,
        }
      }
      // Clean up temp file
      let _ = std::fs::remove_file(&temp_path);
      return;
    }
  }

  // Fallback to a desktop alert if possible (zenity/kdialog)
  // These are modal dialogs so they block until closed
  if let Ok(mut child) = Command::new("zenity")
    .args(["--error", "--text", &clean_message])
    .spawn()
  {
    let _ = child.wait();
    let _ = std::fs::remove_file(&temp_path);
    return;
  }
  if let Ok(mut child) = Command::new("kdialog")
    .args(["--error", &clean_message])
    .spawn()
  {
    let _ = child.wait();
    let _ = std::fs::remove_file(&temp_path);
    return;
  }

  // If nothing worked, at least clean up
  let _ = std::fs::remove_file(&temp_path);
}

#[cfg(target_os = "windows")]
fn show_error_windows(styled: &str) {
  use windows::core::HSTRING;
  use windows::Win32::UI::WindowsAndMessaging::{
    MessageBoxW, MB_ICONERROR, MB_OK, MB_SETFOREGROUND, MB_TOPMOST,
  };

  let clean_message = strip_ansi(styled);

  let title = HSTRING::from("janq Error");
  let msg = HSTRING::from(clean_message);

  unsafe {
    MessageBoxW(
      None,
      &msg,
      &title,
      MB_OK | MB_ICONERROR | MB_SETFOREGROUND | MB_TOPMOST,
    );
  }
}
