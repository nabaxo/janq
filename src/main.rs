#![windows_subsystem = "windows"]



use clap::Parser;

mod config;
#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "windows")]
mod windows;
mod terminal;
mod daemon;
#[cfg(target_os = "windows")]
mod hotkey;

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    /// Force run in daemon mode
    #[arg(long, default_value_t = false)]
    daemon: bool,

    /// Name of the app to toggle (from config)
    #[arg(long)]
    app: Option<String>,
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
        },
        None => {
            // Deterministic fallback: Use the first one from app_order if available, otherwise first alphabetically
            if !config.app_order.is_empty() {
                return Ok(Some(&config.app_order[0]));
            }
            // Only sort when needed
            let mut available: Vec<&str> = app.keys().map(|s| s.as_str()).collect();
            available.sort_unstable();
            Ok(available.first().copied())
        }
    }
}

fn main() -> anyhow::Result<()> {
    #[cfg(target_os = "windows")]
    unsafe {
        use ::windows::Win32::System::Console::{AttachConsole, ATTACH_PARENT_PROCESS};
        let _ = AttachConsole(ATTACH_PARENT_PROCESS);
    }

    let args = Args::parse();
    let (config, config_path) = match config::load_config() {
        Ok(c) => c,
        Err(e) => {
            #[cfg(target_os = "linux")]
            linux::show_error(&e);
            #[cfg(target_os = "windows")]
            windows::show_error(&e);
            std::process::exit(1);
        }
    };

    #[cfg(target_os = "linux")]
    {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;
        rt.block_on(async {
            let target_app = match resolve_app(&config, args.app.clone()) {
                Ok(a) => a,
                Err(e) => {
                    linux::show_error(&e);
                    std::process::exit(1);
                }
            };

            let target_app_owned = target_app.map(|s| s.to_string());
            if args.daemon {
                daemon::run_daemon(config, config_path, target_app_owned).await?;
                return Ok(());
            }

            if daemon::send_toggle(target_app_owned.clone()).await.is_ok() {
                return Ok(());
            }

            println!("Daemon not running (or reachable). Starting new daemon instance...");
            daemon::run_daemon(config, config_path, target_app_owned).await?;
            Ok(())
        })
    }

    #[cfg(target_os = "windows")]
    {
        let target_app = match resolve_app(&config, args.app.clone()) {
            Ok(a) => a,
            Err(e) => {
                windows::show_error(&e);
                std::process::exit(1);
            }
        };

        let target_app_owned = target_app.map(|s| s.to_string());
        if args.daemon {
            daemon::run_daemon(config, config_path, target_app_owned)?;
            return Ok(());
        }

        // For Windows "Smart Mode", we need a temporary runtime to check IPC
        let rt = tokio::runtime::Runtime::new()?;
        let ipc_success = rt.block_on(async {
            // Add timeout to prevent hanging on zombie pipes
            match tokio::time::timeout(std::time::Duration::from_secs(1), daemon::send_toggle(target_app_owned.clone())).await {
                Ok(Ok(())) => true,
                _ => false,
            }
        });

        if ipc_success {
             return Ok(());
        }

        println!("Daemon not running (or reachable). Starting new daemon instance...");
        // This takes over the thread with Winit loop
        daemon::run_daemon(config, config_path, target_app_owned)?;
        Ok(())
    }
}
