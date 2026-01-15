//! Shared shutdown messaging for graceful daemon termination.
//!
//! Provides consistent shutdown messages across platforms.

/// Prints shutdown message with the source/signal that triggered it.
pub fn print_shutdown_message(source: &str) {
  println!("\n{}, shutting down...", source);
}

/// Prints the final termination complete message.
pub fn print_termination_complete() {
  println!("janq: Termination complete.");
}
