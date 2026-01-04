use crate::config::{Config, load_config};
use crate::windows::window::toggle_window;
use crate::hotkey::parse_hotkey;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use tokio::net::windows::named_pipe::{ServerOptions, ClientOptions};
use tokio::io::AsyncWriteExt;
use anyhow::Result;
use global_hotkey::{GlobalHotKeyManager, hotkey::HotKey};
use notify::{Watcher, RecursiveMode, RecommendedWatcher, Config as NotifyConfig};
use tray_icon::{TrayIconBuilder, menu::{Menu, MenuItem, MenuEvent, PredefinedMenuItem}, TrayIconEvent, MouseButton, MouseButtonState};
use winit::event_loop::{ControlFlow, EventLoopBuilder};
use winit::event::Event;
use tokio::runtime::Runtime;
use fs2::FileExt;

const PIPE_NAME: &str = r"\\.\pipe\ruake";

#[derive(Debug)]
enum DaemonEvent {
    Hotkey(global_hotkey::GlobalHotKeyEvent),
    TrayPoll,
    ReloadHotkeys,
    Exit,
}

fn load_icon() -> tray_icon::Icon {
    let bytes = include_bytes!("../../icon.ico");
    let image = image::load_from_memory(bytes).expect("Failed to load icon.ico").to_rgba8();
    let (width, height) = image.dimensions();
    let rgba = image.into_raw();
    tray_icon::Icon::from_rgba(rgba, width, height).expect("Failed to create tray icon")
}

pub fn run_daemon(initial_config: Config, config_path: Option<PathBuf>, auto_show: bool, target_app: Option<String>) -> Result<()> {
    // 1. Setup Runtime for async tasks (IPC, Animation, Watcher)
    let rt = Runtime::new()?;
    let _guard = rt.enter(); // Keep runtime context active for this thread

    // 0. Acquire Lock File
    let lock_path = std::env::temp_dir().join("ruake.lock");
    let lock_file = std::fs::File::create(&lock_path)?;
    if lock_file.try_lock_exclusive().is_err() {
        return Err(anyhow::anyhow!("Ruake is already running (lock file active)."));
    }

    // Enable DPI Awareness (Per Monitor V2) to ensure correct coordinates
    use windows::Win32::UI::HiDpi::{SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2};
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }

    let config = Arc::new(RwLock::new(Arc::new(initial_config)));

    // 2. Setup Winit EventLoop with Custom Event Type
    let event_loop = EventLoopBuilder::<DaemonEvent>::with_user_event().build()?;

    // 3. Setup IPC Server (Spawned on Runtime)
    let config_clone = config.clone();
    let server = ServerOptions::new()
        .first_pipe_instance(true)
        .create(PIPE_NAME)?;

    rt.spawn(async move {
        let mut server = server;
        loop {
            if let Err(e) = server.connect().await {
                eprintln!("Pipe connection error: {}", e);
            } else {
                use tokio::io::AsyncReadExt;
                let mut buf = [0u8; 1024];
                let cfg = config_clone.read().unwrap().clone();
                if let Ok(n) = server.read(&mut buf).await {
                    let msg = String::from_utf8_lossy(&buf[..n]);
                    let app_name = if msg.starts_with("toggle:") {
                        msg.strip_prefix("toggle:").unwrap().trim().to_string()
                    } else {
                         // Default to first app alphabetically
                         let mut apps: Vec<_> = cfg.app.keys().collect();
                         apps.sort();
                         apps.first().cloned().map(|s| s.to_string()).unwrap_or_default()
                    };

                    let target_app = if cfg.app.len() == 1 {
                        cfg.app.keys().next().cloned()
                    } else if cfg.app.contains_key(&app_name) {
                        Some(app_name)
                    } else {
                        None
                    };

                    if let Some(target_name) = target_app {
                        if let Some(app_cfg) = cfg.app.get(&target_name) {
                            crate::windows::terminal::ensure_terminal_running(app_cfg).await;
                            toggle_window(&target_name, &cfg).await;
                        }
                    }
                }
            }
            if let Err(e) = server.disconnect() {
                 eprintln!("Pipe disconnect error: {}", e);
            }
        }
    });

    // 4. Hotkey Manager (Must be initialized on main thread)
    let manager = GlobalHotKeyManager::new().unwrap();
    let current_hotkeys = Arc::new(RwLock::new(Vec::<HotKey>::new()));
    let hotkey_map = Arc::new(RwLock::new(std::collections::HashMap::<u32, String>::new()));

    sync_hotkeys(&manager, &config.read().unwrap(), &hotkey_map, &current_hotkeys);

    // 5. Tray Icon
    let tray_menu = Menu::new();
    let mut app_menu_items = std::collections::HashMap::new();

    {
        let cfg = config.read().unwrap();
        // Sort keys for a consistent (alphabetical) order since HashMap is unordered
        let mut keys: Vec<_> = cfg.app.keys().collect();
        keys.sort();

        for name in keys {
            let item = MenuItem::new(name, true, None);
            let _ = tray_menu.append(&item);
            app_menu_items.insert(item.id().clone(), name.clone());
        }
    }

    let _ = tray_menu.append(&PredefinedMenuItem::separator());
    let quit_i = MenuItem::new("Quit", true, None);
    let _ = tray_menu.append(&quit_i);

    let tray_icon = TrayIconBuilder::new()
        .with_menu(Box::new(tray_menu))
        .with_tooltip("Ruake")
        .with_icon(load_icon())
        .build()
        .unwrap();

    // 6. Config Watcher (Spawned task or thread)
    let config_clone_watcher = config.clone();
    let path_to_watch = config_path.clone();

    // Watcher logic can run on a separate thread, but updating config needs lock
    // We'll pass the event proxy after event loop creation
    let watcher_proxy = event_loop.create_proxy();
    std::thread::spawn(move || {
        let (tx, rx) = std::sync::mpsc::channel();
        let mut watcher = RecommendedWatcher::new(tx, NotifyConfig::default()).unwrap();

        if let Some(path) = path_to_watch {
             if path.exists() {
                  let _ = watcher.watch(&path, RecursiveMode::NonRecursive);
             }
        } else if let Some(home) = dirs::home_dir() {
             // Fallbacks if no config found at startup
             let paths = vec![
                 home.join(".ruake.toml"),
                 home.join(".goake.toml"),
             ];
             for p in paths {
                 if p.exists() {
                      let _ = watcher.watch(&p, RecursiveMode::NonRecursive);
                      break;
                 }
             }
        }

        for res in rx {
             match res {
                Ok(_) => {
                    println!("Config change detected, reloading...");
                    let (new_config, _) = match load_config() {
                        Ok(c) => c,
                        Err(e) => {
                            crate::windows::show_error(&e);
                            continue;
                        }
                    };
                     {
                        let mut w = config_clone_watcher.write().unwrap();
                        let old_config = (**w).clone();
                        *w = Arc::new(new_config.clone());

                        // Identify and Release Removed Apps
                        for (name, app_cfg) in &old_config.app {
                            if !new_config.app.contains_key(name) {
                                crate::windows::window::restore_app_window(name, &app_cfg.window_class);
                            }
                        }
                    }
                    let _ = watcher_proxy.send_event(DaemonEvent::ReloadHotkeys);
                },
                Err(e) => println!("Watch error: {:?}", e),
             }
        }
    });

    println!("Ruake (Windows) daemon running...");

    // 7. Event Loop (Main Thread)
    let hotkey_receiver = global_hotkey::GlobalHotKeyEvent::receiver();
    let menu_receiver = MenuEvent::receiver();
    let tray_receiver = TrayIconEvent::receiver();
    let config_clone_loop = config.clone();

    // Initial setup (Parallel)
    {
        let cfg = config_clone_loop.read().unwrap().clone();
        for app_cfg in cfg.app.values() {
            let app_cfg = app_cfg.clone();
            rt.spawn(async move {
                let _ = crate::windows::terminal::ensure_terminal_running(&app_cfg).await;
            });
        }

        if auto_show {
            let app_to_show = target_app.as_ref();
            if let Some(app_name) = app_to_show {
                if let Some(_app_cfg) = cfg.app.get(app_name) {
                    let cfg = (*cfg).clone();
                    let app_name = app_name.clone();
                    rt.spawn(async move {
                         tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
                         toggle_window(&app_name, &cfg).await;
                    });
                }
            } else if let Some(first_app) = cfg.app_order.first() {
                 let cfg = (*cfg).clone();
                 let app_name = first_app.clone();
                 rt.spawn(async move {
                      tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
                      toggle_window(&app_name, &cfg).await;
                 });
            }
        }
    }

    // Setup EventLoopProxy for external signals (Ctrl+C, Hotkeys, and Tray polling)
    let proxy = event_loop.create_proxy();
    let hotkey_proxy = proxy.clone();
    let tray_proxy = proxy.clone();

    // Spawn Hotkey Listener Thread (instant wakeup)
    std::thread::spawn(move || {
        while let Ok(event) = hotkey_receiver.recv() {
            let _ = hotkey_proxy.send_event(DaemonEvent::Hotkey(event));
        }
    });

    // Spawn Tray Polling Thread (100ms interval)
    std::thread::spawn(move || {
        loop {
            std::thread::sleep(std::time::Duration::from_millis(100));
            let _ = tray_proxy.send_event(DaemonEvent::TrayPoll);
        }
    });

    // Watch for Ctrl+C
    rt.spawn(async move {
        if let Ok(_) = tokio::signal::ctrl_c().await {
            println!("Ctrl+C received. Sending exit signal...");
            let _ = proxy.send_event(DaemonEvent::Exit);
        }
    });

    event_loop.run(move |event, elwt| {
        // Capture tray icon explicitly to keep it alive
        let _ = &tray_icon;
        let _ = &manager;

        // Wait indefinitely for events - no polling!
        // Events wake us up instantly (hotkeys, tray timer, Ctrl+C)
        elwt.set_control_flow(ControlFlow::Wait);

        match event {
            Event::LoopExiting => {
                let cfg = config_clone_loop.read().unwrap().clone();
                crate::windows::window::restore_window_visibility(&cfg);
            }
            Event::UserEvent(daemon_event) => {
                match daemon_event {
                    DaemonEvent::Exit => {
                        elwt.exit();
                    },
                    DaemonEvent::ReloadHotkeys => {
                         let cfg = config_clone_loop.read().unwrap().clone();
                         sync_hotkeys(&manager, &cfg, &hotkey_map, &current_hotkeys);
                    },
                    DaemonEvent::Hotkey(event) => {
                        if event.state == global_hotkey::HotKeyState::Pressed {
                             let map = hotkey_map.read().unwrap();
                             if let Some(app_name) = map.get(&event.id) {
                                  unsafe {
                                       use windows::Win32::UI::WindowsAndMessaging::{AllowSetForegroundWindow, ASFW_ANY};
                                       let _ = AllowSetForegroundWindow(ASFW_ANY);
                                  }
                                  let cfg = config_clone_loop.read().unwrap().clone();
                                  let app_name = app_name.clone();

                                   rt.block_on(async {
                                       if let Some(app_cfg) = cfg.app.get(&app_name) {
                                           if !toggle_window(&app_name, &cfg).await {
                                               crate::windows::terminal::ensure_terminal_running(app_cfg).await;
                                               toggle_window(&app_name, &cfg).await;
                                           }
                                       }
                                   });
                             }
                        }
                    },
                    DaemonEvent::TrayPoll => {
                        // Check Tray Icon Events on 100ms timer
                        while let Ok(event) = tray_receiver.try_recv() {
                            match event {
                                TrayIconEvent::Click { button, button_state, .. } => {
                                    if button_state == MouseButtonState::Up {
                                        if button == MouseButton::Left {
                                            let cfg = config_clone_loop.read().unwrap().clone();
                                            let mut apps: Vec<_> = cfg.app.keys().cloned().collect();
                                            apps.sort();
                                            if let Some(app_name) = apps.first().cloned() {
                                                if let Some(app_cfg) = cfg.app.get(&app_name) {
                                                    let app_cfg = app_cfg.clone();
                                                    rt.spawn(async move {
                                                        crate::windows::terminal::ensure_terminal_running(&app_cfg).await;
                                                        toggle_window(&app_name, &cfg).await;
                                                    });
                                                }
                                            }
                                        }
                                    }
                                }
                                _ => {}
                            }
                        }

                        // Check Menu
                        while let Ok(event) = menu_receiver.try_recv() {
                            if event.id == quit_i.id() {
                                let cfg = config_clone_loop.read().unwrap().clone();
                                crate::windows::window::restore_window_visibility(&cfg);
                                std::process::exit(0);
                            } else if let Some(app_name) = app_menu_items.get(&event.id) {
                                let cfg = config_clone_loop.read().unwrap().clone();
                                let app_name = app_name.clone();
                                if let Some(app_cfg) = cfg.app.get(&app_name) {
                                    let app_cfg = app_cfg.clone();
                                    rt.spawn(async move {
                                        crate::windows::terminal::ensure_terminal_running(&app_cfg).await;
                                        toggle_window(&app_name, &cfg).await;
                                    });
                                }
                            }
                        }
                    }
                }
            }
            Event::AboutToWait => {
                // Respawn Loop check
                use std::sync::atomic::{AtomicU64, Ordering};
                static LAST_CHECK_SEC: AtomicU64 = AtomicU64::new(0);

                let now_sec = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs();

                if now_sec - LAST_CHECK_SEC.load(Ordering::Relaxed) >= 2 {
                    LAST_CHECK_SEC.store(now_sec, Ordering::Relaxed);
                    let cfg = config_clone_loop.read().unwrap().clone();
                    for (name, app_cfg) in &cfg.app {
                        if crate::windows::window::find_window_by_process(&app_cfg.window_class).is_none() {
                             // If this was the visible app, reset it
                             crate::windows::window::reset_visible_app();

                             println!("App '{}' closed. Respawning...", name);
                             let app_cfg = app_cfg.clone();
                             rt.spawn(async move {
                                 crate::windows::terminal::ensure_terminal_running(&app_cfg).await;
                             });
                        }
                    }
                }
            }
            _ => ()
        }
    })?;

    Ok(())
}

pub async fn send_toggle(app_name: Option<String>) -> Result<()> {
    let mut client = ClientOptions::new().open(PIPE_NAME)?;
    if let Some(name) = app_name {
        client.write_all(format!("toggle:{}", name).as_bytes()).await?;
    } else {
        client.write_all(b"toggle").await?;
    }
    Ok(())
}

pub fn sync_hotkeys(
    manager: &GlobalHotKeyManager,
    config: &Config,
    hotkey_map: &Arc<RwLock<std::collections::HashMap<u32, String>>>,
    current_hotkeys: &Arc<RwLock<Vec<HotKey>>>
) {
    println!("Syncing hotkeys...");

    // 1. Unregister all existing hotkeys
    {
        let mut hks = current_hotkeys.write().unwrap();
        if !hks.is_empty() {
             let _ = manager.unregister_all(&hks);
             hks.clear();
        }
    }
    {
        let mut map = hotkey_map.write().unwrap();
        map.clear();
    }

    // 2. Register from current config
    let mut new_hks = Vec::new();
    let mut new_map = std::collections::HashMap::new();

    for (app_name, app_cfg) in &config.app {
        for hk_str in app_cfg.hotkey.as_vec() {
            if hk_str.is_empty() {
                continue;
            }
            println!("App '{}' attempting to register hotkey: {}", app_name, hk_str);
            match parse_hotkey(&hk_str) {
                Ok(key) => {
                    match manager.register(key) {
                        Ok(_) => {
                            println!("  ✓ Registered: {}", hk_str);
                            new_map.insert(key.id(), app_name.clone());
                            new_hks.push(key);
                        }
                        Err(e) => {
                            eprintln!("  ✗ Failed to register '{}': {}", hk_str, e);
                        }
                    }
                },
                Err(e) => eprintln!("  ✗ Failed to parse '{}': {}", hk_str, e),
            }
        }
    }

    // 3. Update shared state
    {
        let mut hks = current_hotkeys.write().unwrap();
        *hks = new_hks;
    }
    {
        let mut map = hotkey_map.write().unwrap();
        *map = new_map;
    }
}
