use crate::config::{Config, load_config};
use crate::windows::window::toggle_window;
use crate::hotkey::parse_hotkey;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use tokio::net::windows::named_pipe::{ServerOptions, ClientOptions};
use tokio::io::AsyncWriteExt;
use tokio::time::{sleep, Duration};
use anyhow::Result;
use global_hotkey::{GlobalHotKeyManager, hotkey::HotKey};
use notify::{Watcher, RecursiveMode, RecommendedWatcher, Config as NotifyConfig};
use tray_icon::{TrayIconBuilder, menu::{Menu, MenuItem, MenuEvent}};
// use image::GenericImageView;

const PIPE_NAME: &str = r"\\.\pipe\rustake";

fn load_icon() -> tray_icon::Icon {
    let bytes = include_bytes!("../../icon.ico");
    let image = image::load_from_memory(bytes).expect("Failed to load icon.ico").to_rgba8();
    let (width, height) = image.dimensions();
    let rgba = image.into_raw();
    tray_icon::Icon::from_rgba(rgba, width, height).expect("Failed to create tray icon")
}

pub async fn run_daemon(initial_config: Config, config_path: Option<PathBuf>, auto_show: bool) -> Result<()> {
    let config = Arc::new(RwLock::new(initial_config));

    // 0. Initial Setup
    {
        let cfg = config.read().unwrap().clone();
        if auto_show {
            crate::windows::terminal::ensure_terminal_running(&cfg).await;
            sleep(Duration::from_millis(500)).await;
            toggle_window(&cfg).await;
        }
    }

    // 1. Setup Named Pipe Server
    let server = ServerOptions::new()
        .first_pipe_instance(true)
        .create(PIPE_NAME)?;

    // Spawn IPC listener
    let config_clone = config.clone();
    tokio::spawn(async move {
        let server = server;
        loop {
            // Wait for connection
            if let Err(e) = server.connect().await {
                eprintln!("Pipe connection error: {}", e);
            } else {
                // Connected
                let cfg = config_clone.read().unwrap().clone();
                crate::windows::terminal::ensure_terminal_running(&cfg).await;
                toggle_window(&cfg).await;
            }

            // Disconnect to allow next client
            if let Err(e) = server.disconnect() {
                 eprintln!("Pipe disconnect error: {}", e);
            }
        }
    });

    println!("Rustake (Windows) daemon running...");

    // 2. Hotkey Manager
    let manager = GlobalHotKeyManager::new().unwrap();
    let mut current_hotkeys: Vec<HotKey> = Vec::new();

    {
        let cfg = config.read().unwrap();
        for hk_str in &cfg.hotkey {
            match parse_hotkey(hk_str) {
                Ok(key) => {
                    if let Err(e) = manager.register(key) {
                        eprintln!("Failed to register hotkey '{}': {}", hk_str, e);
                    } else {
                        println!("Registered hotkey: {}", hk_str);
                        current_hotkeys.push(key);
                    }
                },
                Err(e) => eprintln!("Failed to parse hotkey '{}': {}", hk_str, e),
            }
        }
    }

    // 3. Tray Icon
    let tray_menu = Menu::new();
    let quit_i = MenuItem::new("Quit", true, None);
    let toggle_i = MenuItem::new("Toggle", true, None);
    let _ = tray_menu.append(&toggle_i);
    let _ = tray_menu.append(&quit_i);

    let _tray_icon = TrayIconBuilder::new()
        .with_menu(Box::new(tray_menu))
        .with_tooltip("Rustake")
        .with_icon(load_icon())
        .build()
        .unwrap();

    // 4. Config Watcher (Thread)
    let config_clone_watcher = config.clone();
    let path_to_watch = config_path.clone();

    std::thread::spawn(move || {
        let (tx, rx) = std::sync::mpsc::channel();
        let mut watcher = RecommendedWatcher::new(tx, NotifyConfig::default()).unwrap();

        if let Some(path) = path_to_watch {
             if path.exists() {
                  let _ = watcher.watch(&path, RecursiveMode::NonRecursive);
             }
        } else if let Some(home) = dirs::home_dir() {
             let path = home.join(".goake.toml");
             if path.exists() {
                  let _ = watcher.watch(&path, RecursiveMode::NonRecursive);
             }
        }

        for res in rx {
             match res {
                Ok(_) => {
                    println!("Config change detected, reloading...");
                    let (new_config, _) = load_config();
                    {
                        let mut w = config_clone_watcher.write().unwrap();
                        *w = new_config.clone();
                    }
                },
                Err(e) => println!("Watch error: {:?}", e),
             }
        }
    });

    // 5. Main Loop
    let hotkey_receiver = global_hotkey::GlobalHotKeyEvent::receiver();
    let menu_receiver = MenuEvent::receiver();

    loop {
        // Hotkeys
        if let Ok(event) = hotkey_receiver.try_recv() {
            if event.state == global_hotkey::HotKeyState::Released {
                 // Check if ID matches any registered hotkey
                  if current_hotkeys.iter().any(|hk| hk.id() == event.id) {
                       let cfg = config.read().unwrap().clone();
                       crate::windows::terminal::ensure_terminal_running(&cfg).await;
                       toggle_window(&cfg).await;
                  }
            }
        }

        // Menu Events
        if let Ok(event) = menu_receiver.try_recv() {
             if event.id == quit_i.id() {
                  std::process::exit(0);
              } else if event.id == toggle_i.id() {
                   let cfg = config.read().unwrap().clone();
                   crate::windows::terminal::ensure_terminal_running(&cfg).await;
                   toggle_window(&cfg).await;
              }
        }

        sleep(Duration::from_millis(16)).await;
    }
}

pub async fn send_toggle() -> Result<()> {
    let mut client = ClientOptions::new().open(PIPE_NAME)?;
    client.write_all(b"toggle").await?;
    Ok(())
}
