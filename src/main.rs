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

use janq::{
  acquire_lock_file,
  config::{self, Config},
  error::{self, show_error},
  matching::suggest_similar,
};
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
  quit: bool,
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
  const HDR: &str = "\x1b[1;93m"; // Bold, Bright Yellow
  const CMD: &str = "\x1b[36m"; //Cyan
  const ARG: &str = "\x1b[33m"; // Yellow
  const LGO: &str = "\x1b[1;91m"; // Bold, Bright Red
  const RST: &str = "\x1b[0m"; // Reset

  // Using numbered arguments {1}, {2}, etc. makes the template much cleaner
  println!(
    "{5}janq{2} {0} - A Quake-style dropdown terminal manager

{1}USAGE:{2}
  {5}janq{2} {3}[OPTION]{2}

{1}OPTIONS:{2}
  {4}-D, --daemon{2}          Run as a persistent process (Server Mode)
  {4}-a, --app{2} {3}[NAME]{2}      Name of the app to toggle (from config)
  {4}-q, --quit{2}            Gracefully stop the running daemon
  {4}-h, --help{2}            Print help information
  {4}-V, --version{2}         Print version information",
    env!("CARGO_PKG_VERSION"), // {0}
    HDR,                       // {1}
    RST,                       // {2}
    ARG,                       // {3}
    CMD,                       // {4}
    LGO                        // {5}
  );

  #[cfg(target_os = "linux")]
  println!(
    "\n  {0}-i, --setup{1}           Force refresh of system/desktop/D-Bus
  {0}-u, --cleanup{1}         Remove all janq system integration
  {0}--enable-autostart{1}    Enable autostart (creates symlink)
  {0}--disable-autostart{1}   Disable autostart (removes symlink)",
    CMD, RST
  );
}

macro_rules! define_flags {
  ($arg:expr, $args:expr, $iter:expr, [
    $($(#[$m:meta])* $long:literal $(| $short:literal)* => $logic:expr),* $(,)?
  ]) => {
    match $arg {
      $(
        $(#[$m])*
        $long $(| $short)* => {
          $logic
        }
      )*
      _ => {
        if $arg.starts_with("--app=") {
          $args.app = Some($arg.trim_start_matches("--app=").to_string());
        } else {
          let mut valid: Vec<&str> = Vec::new();
          $(
            $(#[$m])*
            valid.push($long);
          )*

          let mut msg = format!("Unknown argument '{}'", $arg);
          if let Some(suggestion) = suggest_similar($arg, &valid) {
            msg.push_str(&format!(". Did you mean '{}'?", suggestion));
          }
          show_error(&msg);
          exit(1);
        }
      }
    }
  }
}

fn parse_args() -> Args {
  let mut args = Args::default();
  let mut iter = env::args().skip(1);

  while let Some(arg) = iter.next() {
    define_flags!(arg.as_str(), args, iter, [
      "--help" | "-h" => {
        print_help();
        exit(0);
      },
      "--version" | "-V" => {
        println!("janq {}", env!("CARGO_PKG_VERSION"));
        exit(0);
      },
      "--daemon" | "--demon" | "--deamon" | "-D" => {
        args.daemon = true;
      },
      "--quit" | "--exit" | "-q" => {
        args.quit = true;
      },
      "--app" | "-a" => {
        if let Some(val) = iter.next() {
          args.app = Some(val);
        } else {
          show_error("Error: --app requires a value");
          exit(1);
        }
      },
      #[cfg(target_os = "linux")]
      "--enable-autostart" | "--enableautostart" => {
        args.enable_autostart = true;
      },
      #[cfg(target_os = "linux")]
      "--disable-autostart" | "--disableautostart" => {
        args.disable_autostart = true;
      },
      #[cfg(target_os = "linux")]
      "--setup" | "-i" => {
        args.setup = true;
      },
      #[cfg(target_os = "linux")]
      "--cleanup" | "-u" => {
        args.uninstall = true;
      },
    ]);
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
        let mut available: Vec<&str> = app.keys().map(|s| s.as_str()).collect();
        available.sort_unstable();

        let mut msg = format!("App '{}' not found in config.", name);
        if let Some(suggestion) = suggest_similar(&name, &available) {
          msg.push_str(&format!(" Did you mean '{}'?", suggestion));
        } else {
          msg.push_str(&format!("\nAvailable: {}", available.join(", ")));
        }
        Err(msg)
      }
    }
    None => {
      // Deterministic fallback: Use the first one defined in the config (IndexMap preserves order)
      Ok(app.keys().next().map(|s| s.as_str()))
    }
  }
}

/// Disable Transparent Huge Pages for this process.
///
/// The kernel's `khugepaged` can opportunistically promote a 2 MiB-aligned
/// anonymous region into a single Transparent Huge Page, making the entire
/// 2 MiB resident even when only a fraction contains data. For a long-lived
/// daemon with a small heap (~400 KiB anon), this causes an intermittent
/// and sticky ~2 MiB RSS inflation (2344 KiB → 4400 KiB).
///
/// `prctl(PR_SET_THP_DISABLE, 1)` tells the kernel to never promote this
/// process's pages to huge pages, keeping RSS proportional to actual usage.
#[cfg(target_os = "linux")]
fn disable_thp() {
  const PR_SET_THP_DISABLE: i32 = 41;
  extern "C" {
    fn prctl(option: i32, arg2: u64, arg3: u64, arg4: u64, arg5: u64) -> i32;
  }
  // Safety: prctl with PR_SET_THP_DISABLE is a simple flag-set with no
  // pointer arguments. The only effect is setting a per-task flag.
  unsafe {
    prctl(PR_SET_THP_DISABLE, 1, 0, 0, 0);
  }
}

fn main() -> error::Result<()> {
  #[cfg(target_os = "linux")]
  disable_thp();

  println!("janq (PID {}): Initializing...", std::process::id());

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
        error::show_warning(&format!("Failed to sync KWin rules: {}", e));
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
    b.thread_stack_size(512 * 1024).max_blocking_threads(4);
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
    if args.quit {
      if let Err(e) = daemon::send_quit().await {
        show_error(&format!("Failed to stop daemon: {}", e));
        exit(1);
      }
      return Ok(());
    }

    if args.daemon {
      if let Err(e) = acquire_lock_file() {
        show_error(&e.to_string());
        exit(1);
      }
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
    if let Err(e) = acquire_lock_file() {
      show_error(&e.to_string());
      exit(1);
    }
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
