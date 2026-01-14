#![windows_subsystem = "windows"]

use std::process::exit;

use clap::Parser;
#[cfg(target_os = "linux")]
use tokio::runtime::Builder;

mod config;
mod daemon;
#[cfg(target_os = "windows")]
mod hotkey;
#[cfg(target_os = "linux")]
mod linux;
mod terminal;
#[cfg(target_os = "windows")]
mod windows;

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
  /// Run as a persistent process in the terminal (Server Mode)
  #[arg(long, default_value_t = false, aliases = ["demon", "deamon"])]
  daemon: bool,

  /// Name of the app to toggle (from config)
  #[arg(long)]
  app: Option<String>,

  /// Enable autostart on login (Linux only: creates symlink in ~/.config/autostart)
  #[cfg(target_os = "linux")]
  #[arg(long)]
  enable_autostart: bool,

  /// Disable autostart on login (Linux only: removes symlink from ~/.config/autostart)
  #[cfg(target_os = "linux")]
  #[arg(long)]
  disable_autostart: bool,
}

fn resolve_app(config: &config::Config, requested: Option<String>) -> Result<Option<&str>, String> {
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
        Err(format!(
          "External app '{}' not found in config.\nAvailable: {}",
          name,
          available.join(", ")
        ))
      }
    }
    None => {
      // Deterministic fallback: Use the first one defined in the config (IndexMap preserves order)
      Ok(app.keys().next().map(|s| s.as_str()))
    }
  }
}

fn main() -> anyhow::Result<()> {
  let args = Args::parse();

  let (config, config_path) = match config::load_config(None) {
    Ok(c) => c,
    Err(e) => {
      #[cfg(target_os = "linux")]
      linux::show_error(&e);
      #[cfg(target_os = "windows")]
      windows::show_error(&e);
      exit(1);
    }
  };

  // Handle autostart flags (Linux only) - these exit immediately
  #[cfg(target_os = "linux")]
  {
    if args.enable_autostart {
      if let Err(e) = linux::desktop::enable_autostart(&config) {
        linux::show_error(&e.to_string());
        exit(1);
      }
      return Ok(());
    }
    if args.disable_autostart {
      if let Err(e) = linux::desktop::disable_autostart() {
        linux::show_error(&e.to_string());
        exit(1);
      }
      return Ok(());
    }
  }

  #[cfg(target_os = "linux")]
  {
    let rt = Builder::new_current_thread().enable_all().build()?;
    rt.block_on(async {
      let target_app = match resolve_app(&config, args.app.clone()) {
        Ok(a) => a,
        Err(e) => {
          linux::show_error(&e);
          exit(1);
        }
      };

      let target_app_owned = target_app.map(|s| s.to_string());
      if args.daemon {
        if let Err(e) = daemon::run_daemon(config, config_path, target_app_owned).await {
          linux::show_error(&e.to_string());
          exit(1);
        }
        return Ok(());
      }

      if daemon::send_toggle(target_app_owned.clone()).await.is_ok() {
        return Ok(());
      }

      println!("Daemon not running (or reachable). Starting new daemon instance...");
      if let Err(e) = daemon::run_daemon(config, config_path, target_app_owned).await {
        linux::show_error(&e.to_string());
        exit(1);
      }
      Ok(())
    })
  }

  #[cfg(target_os = "windows")]
  {
    let target_app = match resolve_app(&config, args.app.clone()) {
      Ok(a) => a,
      Err(e) => {
        windows::show_error(&e);
        exit(1);
      }
    };

    let target_app_owned = target_app.map(|s| s.to_string());
    if args.daemon {
      if let Err(e) = daemon::run_daemon(config, config_path, target_app_owned) {
        windows::show_error(&e.to_string());
        exit(1);
      }
      return Ok(());
    }

    // Synchronous IPC check for Windows
    if daemon::send_toggle_sync(target_app_owned.clone()).is_ok() {
      return Ok(());
    }

    println!("Daemon not running (or reachable). Starting new daemon instance...");
    if let Err(e) = daemon::run_daemon(config, config_path, target_app_owned) {
      windows::show_error(&e.to_string());
      exit(1);
    }
    Ok(())
  }
}
