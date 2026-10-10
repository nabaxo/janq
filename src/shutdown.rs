//! Shared shutdown messaging for graceful daemon termination.
//!
//! Provides consistent shutdown messages across platforms.

use std::io::Write;

// Write errors are ignored: after a SIGHUP the terminal may be gone, and a
// panicking println! would abort before managed windows are restored.

/// Prints shutdown message with the source/signal that triggered it.
pub fn print_shutdown_message(source: &str) {
  let _ = writeln!(std::io::stdout(), "\n{}, shutting down...", source);
}

/// Prints the final termination complete message.
pub fn print_termination_complete() {
  let _ = writeln!(std::io::stdout(), "janq: Termination complete.");
}
