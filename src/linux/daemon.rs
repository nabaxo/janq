use crate::config::{Config, load_config};
use crate::linux::kwin::{toggle_quake, restore_quake, ensure_grabbed, reset_visibility};
use crate::terminal::{ensure_terminal_running, check_process_running};
use zbus::{interface, Connection};
use std::sync::{Arc, RwLock};
use tokio::time::{sleep, Duration};
use tokio::signal;
use notify::{Watcher, RecursiveMode, RecommendedWatcher, Config as NotifyConfig};
use image::GenericImageView;


struct QuakeDaemon {
    config: Arc<RwLock<Config>>,
}

#[interface(name = "dev.nabaxo.ruake")]
impl QuakeDaemon {
    async fn toggle(&self) {
        let config = { self.config.read().unwrap().clone() };
        let _ = toggle_quake(&config).await;
    }
}

struct StatusNotifierItem {
    config: Arc<RwLock<Config>>,
}

type IconPixmap = Vec<(i32, i32, Vec<u8>)>;

#[interface(name = "org.kde.StatusNotifierItem")]
impl StatusNotifierItem {
    fn activate(&self, _x: i32, _y: i32) {
        let config = { self.config.read().unwrap().clone() };
        tokio::spawn(async move {
            let _ = toggle_quake(&config).await;
        });
    }

    fn secondary_activate(&self, _x: i32, _y: i32) {
        println!("Quit requested via tray icon...");
        let config = { self.config.read().unwrap().clone() };
        tokio::spawn(async move {
            let _ = restore_quake(&config).await;
            std::process::exit(0);
        });
    }

    #[zbus(property)]
    fn category(&self) -> String { "ApplicationStatus".to_string() }
    #[zbus(property)]
    fn id(&self) -> String { "ruake".to_string() }
    #[zbus(property)]
    fn title(&self) -> String { "Ruake".to_string() }
    #[zbus(property)]
    fn status(&self) -> String { "Active".to_string() }
    #[zbus(property)]
    fn icon_name(&self) -> String { "ruake".to_string() }
    #[zbus(property)]
    fn icon_pixmap(&self) -> IconPixmap {
        if let Ok(img) = image::load_from_memory(include_bytes!("../../icon.png")) {
             let (w, h) = img.dimensions();
             let data = img.to_rgba8().into_raw();
             // Convert RGBA to ARGB
             let mut pixels = Vec::with_capacity(data.len());
             for chunk in data.chunks(4) {
                 pixels.push(chunk[3]); // A
                 pixels.push(chunk[0]); // R
                 pixels.push(chunk[1]); // G
                 pixels.push(chunk[2]); // B
             }
             vec![(w as i32, h as i32, pixels)]
        } else {
             vec![]
        }
    }
    #[zbus(property)]
    fn item_is_menu(&self) -> bool { false }
}

pub async fn run_daemon(initial_config: Config, config_path: Option<std::path::PathBuf>, auto_show: bool) -> anyhow::Result<()> {
    let config = Arc::new(RwLock::new(initial_config));

    let conn = Connection::session().await?;

    // Request name for SNI
    let pid = std::process::id();
    let sni_name = format!("org.kde.StatusNotifierItem-ruake-{}", pid);
    // Note: zbus request_name is usually done via connection builder or manually
    // But SNI protocol often requires unique name then registration
    conn.request_name(sni_name.clone()).await?;

    // Export interfaces
    let daemon = QuakeDaemon { config: config.clone() };
    conn.object_server().at("/dev/nabaxo/ruake", daemon).await?;
    conn.request_name("dev.nabaxo.ruake").await?;

    let sni = StatusNotifierItem { config: config.clone() };
    conn.object_server().at("/StatusNotifierItem", sni).await?;

    // Register with Watcher
    let watcher_proxy = zbus::Proxy::new(&conn, "org.kde.StatusNotifierWatcher", "/StatusNotifierWatcher", "org.kde.StatusNotifierWatcher").await?;
    let _ = watcher_proxy.call_method("RegisterStatusNotifierItem", &(sni_name)).await;

    println!("Rustake daemon running...");

    // Initial setup
    {
        let cfg = config.read().unwrap().clone();
        ensure_terminal_running(&cfg).await;
        let _ = ensure_grabbed(&cfg).await;

        if auto_show {
            sleep(Duration::from_millis(500)).await;
            let _ = toggle_quake(&cfg).await;
        }
    }

    // Config Watcher (Thread)
    let config_clone = config.clone();
    // Clone path for thread
    let path_to_watch = config_path.clone();

    std::thread::spawn(move || {
        let (tx, rx) = std::sync::mpsc::channel();
        let mut watcher = RecommendedWatcher::new(tx, NotifyConfig::default()).unwrap();

        // Watch the specific path if we have one
        if let Some(path) = path_to_watch {
            if path.exists() {
                println!("Watching config file: {:?}", path);
                let _ = watcher.watch(&path, RecursiveMode::NonRecursive);
            } else {
                println!("Config file path provided but not found: {:?}", path);
            }
        } else {
            // Fallback attempts if no config was loaded initially (using defaults)
            if let Some(home) = dirs::home_dir() {
                let paths = vec![
                    home.join(".ruake.toml"),
                    home.join(".goake.toml"),
                ];
                for path in paths {
                    if path.exists() {
                         println!("Watching default config file: {:?}", path);
                         let _ = watcher.watch(&path, RecursiveMode::NonRecursive);
                         break;
                    }
                }
            }
        }

        for res in rx {
            match res {
                Ok(_) => {
                    println!("Config change detected, reloading...");
                    // Reload config (tuple return)
                    let (new_config, _) = load_config();
                    {
                        let mut w = config_clone.write().unwrap();
                        *w = new_config.clone();
                    }
                    // Apply changes
                    let rt = tokio::runtime::Runtime::new().unwrap();
                    rt.block_on(async {
                        let _ = ensure_grabbed(&new_config).await;
                    });
                },
                Err(e) => println!("Watch error: {:?}", e),
            }
        }
    });

    // Respawn Loop
    let config_clone2 = config.clone();
    tokio::spawn(async move {
        loop {
            sleep(Duration::from_secs(2)).await;
            let (target_class, cfg_clone) = {
                let c = config_clone2.read().unwrap();
                (c.general.window_class.clone(), c.clone())
            };

            if !check_process_running(&target_class) {
                println!("Terminal process closed. Respawning...");
                if ensure_terminal_running(&cfg_clone).await {
                    println!("Respawn successful.");
                    reset_visibility().await;
                    sleep(Duration::from_millis(500)).await;
                    let _ = toggle_quake(&cfg_clone).await;
                }
            }
        }
    });

    // Wait for signal
    match signal::ctrl_c().await {
        Ok(()) => {
             println!("Shutting down...");
             let cfg = config.read().unwrap().clone();
             let _ = restore_quake(&cfg).await;
        },
        Err(err) => {
             eprintln!("Unable to listen for shutdown signal: {}", err);
        },
    }

    Ok(())
}

pub async fn send_toggle() -> anyhow::Result<()> {
    let conn = Connection::session().await?;
    let proxy = zbus::Proxy::new(
        &conn,
        "dev.nabaxo.ruake",
        "/dev/nabaxo/ruake",
        "dev.nabaxo.ruake"
    ).await?;

    proxy.call_method("Toggle", &()).await?;
    Ok(())
}
