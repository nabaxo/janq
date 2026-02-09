// janq - Quake-style dropdown terminal manager for Linux (KDE) and Windows.
//
// This binary provides both daemon mode (persistent background service) and
// single-shot mode (sends toggle command to running daemon via IPC).
//
// ## Platform Architecture
//
// Platform-specific code is selected at compile time via `cfg` attributes:
// - **Linux**: Uses async Tokio runtime, D-Bus for IPC, KWin scripts for window control
// - **Windows**: Uses synchronous Win32 message loop, named pipes for IPC, direct Win32 API
//
// The `daemon` and `terminal` modules are facade modules that re-export the
// platform-specific implementations from `linux/` or `windows/` subdirectories.

#![windows_subsystem = "windows"]

// Memory Optimization Tip: For Linux environments with glibc, setting the
// environment variable MALLOC_ARENA_MAX=1 before starting janq can reduce
// baseline RSS by 1-2MB.

use std::process::exit;

use janq::config::{self, Config};
use janq::error::show_error;
use std::env;
use tokio::runtime::Builder;

#[cfg(target_os = "linux")]
mod daemon {
  pub use crate::linux::daemon::*;
}
#[cfg(target_os = "windows")]
mod daemon {
  pub use crate::windows::daemon::*;
}

#[cfg(target_os = "linux")]
mod linux;

#[cfg(target_os = "linux")]
#[allow(unused_imports)]
mod terminal {
  pub use crate::linux::terminal::*;
}
#[cfg(target_os = "windows")]
#[allow(unused_imports)]
mod terminal {
  pub use crate::windows::terminal::*;
}

#[cfg(target_os = "windows")]
mod windows;

// Attach to parent console if running from terminal (Windows only)
#[cfg(target_os = "windows")]
fn attach_parent_console() {
  unsafe {
    const ATTACH_PARENT_PROCESS: u32 = 0xFFFFFFFF;
    let _ = ::windows::Win32::System::Console::AttachConsole(ATTACH_PARENT_PROCESS);
  }
}

#[derive(Debug, Default)]
struct Args {
  daemon: bool,
  app: Option<String>,
  #[cfg(target_os = "linux")]
  enable_autostart: bool,
  #[cfg(target_os = "linux")]
  disable_autostart: bool,
  #[cfg(target_os = "linux")]
  setup: bool,
  #[cfg(target_os = "linux")]
  uninstall: bool,
}

fn print_help() {
  println!(
    "janq {} - Quake-style dropdown terminal manager

USAGE:
    janq [OPTIONS]

OPTIONS:
    --daemon            Run as a persistent process (Server Mode)
    --app <NAME>        Name of the app to toggle (from config)
    --help              Print help information
    --version           Print version information",
    env!("CARGO_PKG_VERSION")
  );

  #[cfg(target_os = "linux")]
  {
    println!(
      "    --enable-autostart  Enable autostart (creates symlink in ~/.config/autostart)
    --disable-autostart Disable autostart (removes symlink from ~/.config/autostart)
    --setup             Force refresh of desktop, icon, and D-Bus registration
    --uninstall         Remove all janq system integration (desktop, icons, rules)"
    );
  }

  println!("\nAliases: --demon, --deamon for --daemon");
}

fn parse_args() -> Args {
  let mut args = Args::default();
  let mut iter = env::args().skip(1);

  while let Some(arg) = iter.next() {
    match arg.as_str() {
      "--help" | "-h" => {
        print_help();
        exit(0);
      }
      "--version" | "-V" => {
        println!("janq {}", env!("CARGO_PKG_VERSION"));
        exit(0);
      }
      "--daemon" | "--demon" | "--deamon" => {
        args.daemon = true;
      }
      "--app" => {
        if let Some(val) = iter.next() {
          args.app = Some(val);
        } else {
          show_error("Error: --app requires a value");
          exit(1);
        }
      }
      #[cfg(target_os = "linux")]
      "--enable-autostart" => {
        args.enable_autostart = true;
      }
      #[cfg(target_os = "linux")]
      "--disable-autostart" => {
        args.disable_autostart = true;
      }
      #[cfg(target_os = "linux")]
      "--setup" => {
        args.setup = true;
      }
      #[cfg(target_os = "linux")]
      "--uninstall" => {
        args.uninstall = true;
      }
      _ => {
        if arg.starts_with("--app=") {
          args.app = Some(arg.trim_start_matches("--app=").to_string());
        } else {
          // Ignore unknown args to be more lax than clap
          eprintln!("Warning: Unknown argument '{}'", arg);
        }
      }
    }
  }
  args
}

fn resolve_app(config: &Config, requested: Option<String>) -> Result<Option<&str>, String> {
  let app = &config.app;
  if app.is_empty() {
    return Ok(None);
  }

  // Single app mode: always use the only app available
  if app.len() == 1 {
    return Ok(app.keys().next().map(|s| s.as_str()));
  }

  // Multi app mode
  match requested {
    Some(name) => {
      if app.contains_key(&name) {
        // Find the key in the map to return a reference with the lifetime of config
        Ok(app.get_key_value(&name).map(|(k, _)| k.as_str()))
      } else {
        // Build error message only on failure path
        let mut available: Vec<&str> = app.keys().map(|s| s.as_str()).collect();
        available.sort_unstable();
        Err(janq::error::format_error(&format!(
          "App '{}' not found in config.\nAvailable: {}",
          name,
          available.join(", ")
        )))
      }
    }
    None => {
      // Deterministic fallback: Use the first one defined in the config (IndexMap preserves order)
      Ok(app.keys().next().map(|s| s.as_str()))
    }
  }
}

fn main() -> janq::error::Result<()> {
  #[cfg(target_os = "windows")]
  attach_parent_console();

  let args = parse_args();

  let (config, config_path) = match config::load_config(None) {
    Ok(c) => c,
    Err(e) => {
      show_error(&e.to_string());
      exit(1);
    }
  };

  // Handle autostart flags (Linux only) - these exit immediately
  #[cfg(target_os = "linux")]
  {
    if args.enable_autostart {
      if let Err(e) = linux::desktop::enable_autostart(&config) {
        show_error(&e.to_string());
        exit(1);
      }
      return Ok(());
    }
    if args.disable_autostart {
      if let Err(e) = linux::desktop::disable_autostart() {
        show_error(&e.to_string());
        exit(1);
      }
      return Ok(());
    }
    if args.setup {
      println!("Forcing system integration refresh...");
      if let Err(e) = linux::desktop::generate_desktop_file_force(&config) {
        show_error(&e.to_string());
        exit(1);
      }
      if let Err(e) = linux::kwin::sync_kwin_rules(&config) {
        eprintln!("Warning: Failed to sync KWin rules: {}", e);
      } else {
        println!("✓ KWin window rules synchronized.");
      }
      println!("✓ Refresh complete.");
      return Ok(());
    }

    if args.uninstall {
      if let Err(e) = linux::desktop::purge_system_integration() {
        show_error(&e.to_string());
        exit(1);
      }
      return Ok(());
    }
  }

  // Setup Tokio Runtime
  let mut builder = if cfg!(target_os = "windows") {
    let mut b = Builder::new_multi_thread();
    b.worker_threads(2).thread_stack_size(256 * 1024);
    b
  } else {
    let mut b = Builder::new_current_thread();
    b.thread_stack_size(512 * 1024);
    b
  };

  let rt = builder.enable_all().build()?;

  rt.block_on(async {
    let target_app = match resolve_app(&config, args.app.clone()) {
      Ok(a) => a,
      Err(e) => {
        show_error(&e);
        exit(1);
      }
    };

    let target_app_owned = target_app.map(|s| s.to_string());
    if args.daemon {
      if let Err(e) = daemon::run_daemon(config, config_path, target_app_owned).await {
        show_error(&e.to_string());
        exit(1);
      }
      return Ok(());
    }

    if daemon::send_toggle(target_app_owned.clone()).await.is_ok() {
      return Ok(());
    }

    println!("Daemon not running (or reachable). Starting new daemon instance...");
    if let Err(e) = daemon::run_daemon(config, config_path, target_app_owned).await {
      #[cfg(target_os = "windows")]
      windows::show_error(&e.to_string());
      #[cfg(target_os = "linux")]
      show_error(&e.to_string());
      exit(1);
    }
    Ok(())
  })
}
