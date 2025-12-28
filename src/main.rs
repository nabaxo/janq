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

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    let (config, config_path) = config::load_config();

    if args.daemon {
        daemon::run_daemon(config, config_path, false).await?;
        return Ok(());
    }

    // Smart Mode
    // Try to connect to existing daemon
    if daemon::send_toggle().await.is_ok() {
        // Success! Daemon was running and we toggled it.
        return Ok(());
    }

    // Fallback: Start Daemon
    println!("Daemon not running (or reachable). Starting new daemon instance...");
    daemon::run_daemon(config, config_path, true).await?;

    Ok(())
}
