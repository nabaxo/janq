use clap::Parser;
use zbus::Connection;

mod config;
mod kwin;
mod terminal;
mod daemon;

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    /// Force run in daemon mode
    #[arg(long, default_value_t = false)]
    daemon: bool,
}

#[tokio::main]
async fn main() -> zbus::Result<()> {
    let args = Args::parse();
    let (config, config_path) = config::load_config();

    if args.daemon {
        daemon::run_daemon(config, config_path, false).await?;
        return Ok(());
    }

    // Smart Mode
    // Try to connect to existing daemon
    let conn_result = Connection::session().await;

    if let Ok(conn) = conn_result {
        // We try to call Toggle on dev.nabaxo.rustake
        // If the service is not active, this call will fail quickly
        // We don't need a full Proxy struct, just a quick call
        let proxy_result = zbus::Proxy::new(
            &conn,
            "dev.nabaxo.rustake",
            "/dev/nabaxo/rustake",
            "dev.nabaxo.rustake"
        ).await;

        if let Ok(proxy) = proxy_result {
            // "Toggle" is the method name (PascalCase)
            if let Ok(_) = proxy.call_method("Toggle", &()).await {
                 // Success! Daemon was running and we toggled it.
                 return Ok(());
            }
        }
        // If we got here, connection worked but maybe service not found or call failed
    }

    // Fallback: Start Daemon
    println!("Daemon not running (or reachable). Starting new daemon instance...");
    daemon::run_daemon(config, config_path, true).await?;

    Ok(())
}
