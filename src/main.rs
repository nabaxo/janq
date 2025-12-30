#![windows_subsystem = "windows"]

// Use jemalloc as global allocator on Linux when feature is enabled
// Improves memory performance for long-running daemons
#[cfg(all(target_os = "linux", feature = "jemalloc"))]
#[global_allocator]
static GLOBAL: tikv_jemallocator::Jemalloc = tikv_jemallocator::Jemalloc;

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
}

fn main() -> anyhow::Result<()> {
    #[cfg(target_os = "windows")]
    unsafe {
        use ::windows::Win32::System::Console::{AttachConsole, ATTACH_PARENT_PROCESS};
        let _ = AttachConsole(ATTACH_PARENT_PROCESS);
    }

    let args = Args::parse();
    let (config, config_path) = config::load_config();

    #[cfg(target_os = "linux")]
    {
        let rt = tokio::runtime::Runtime::new()?;
        rt.block_on(async {
            if args.daemon {
                daemon::run_daemon(config, config_path, false).await?;
                return Ok(());
            }

            if daemon::send_toggle().await.is_ok() {
                return Ok(());
            }

            println!("Daemon not running (or reachable). Starting new daemon instance...");
            daemon::run_daemon(config, config_path, true).await?;
            Ok(())
        })
    }

    #[cfg(target_os = "windows")]
    {
        if args.daemon {
            daemon::run_daemon(config, config_path, false)?;
            return Ok(());
        }

        // For Windows "Smart Mode", we need a temporary runtime to check IPC
        let rt = tokio::runtime::Runtime::new()?;
        let ipc_success = rt.block_on(async {
            // Add timeout to prevent hanging on zombie pipes
            match tokio::time::timeout(std::time::Duration::from_secs(1), daemon::send_toggle()).await {
                Ok(Ok(())) => true,
                _ => false,
            }
        });

        if ipc_success {
             return Ok(());
        }

        println!("Daemon not running (or reachable). Starting new daemon instance...");
        // This takes over the thread with Winit loop
        daemon::run_daemon(config, config_path, true)?;
        Ok(())
    }
}
