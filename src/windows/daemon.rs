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
use tray_icon::{TrayIconBuilder, menu::{Menu, MenuItem, MenuEvent}, TrayIconEvent, MouseButton, MouseButtonState};
use winit::event_loop::{ControlFlow, EventLoopBuilder};
use winit::event::Event;
use tokio::runtime::Runtime;

const PIPE_NAME: &str = r"\\.\pipe\ruake";

#[derive(Debug)]
enum DaemonEvent {
    Hotkey(global_hotkey::GlobalHotKeyEvent),
    TrayPoll,
    Exit,
}

fn load_icon() -> tray_icon::Icon {
    let bytes = include_bytes!("../../icon.ico");
    let image = image::load_from_memory(bytes).expect("Failed to load icon.ico").to_rgba8();
    let (width, height) = image.dimensions();
    let rgba = image.into_raw();
    tray_icon::Icon::from_rgba(rgba, width, height).expect("Failed to create tray icon")
}

pub fn run_daemon(initial_config: Config, config_path: Option<PathBuf>, auto_show: bool) -> Result<()> {
    // 1. Setup Runtime for async tasks (IPC, Animation, Watcher)
    let rt = Runtime::new()?;
    let _guard = rt.enter(); // Keep runtime context active for this thread

    // Enable DPI Awareness (Per Monitor V2) to ensure correct coordinates
    use windows::Win32::UI::HiDpi::{SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2};
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }

    let config = Arc::new(RwLock::new(initial_config));

    // 2. Setup Winit EventLoop with Custom Event Type
    let event_loop = EventLoopBuilder::<DaemonEvent>::with_user_event().build()?;

    // 3. Setup IPC Server (Spawned on Runtime)
    let config_clone = config.clone();
    let server = ServerOptions::new()
        .first_pipe_instance(true)
        .create(PIPE_NAME)?;

    rt.spawn(async move {
        // We need to keep the server alive
        let server = server;
        loop {
            if let Err(e) = server.connect().await {
                eprintln!("Pipe connection error: {}", e);
            } else {
                let cfg = config_clone.read().unwrap().clone();
                crate::windows::terminal::ensure_terminal_running(&cfg).await;
                toggle_window(&cfg).await;
            }
            if let Err(e) = server.disconnect() {
                 eprintln!("Pipe disconnect error: {}", e);
            }
        }
    });

    // 4. Hotkey Manager (Must be initialized on main thread)
    let manager = GlobalHotKeyManager::new().unwrap();
    let mut current_hotkeys: Vec<HotKey> = Vec::new();

    {
        let cfg = config.read().unwrap();
        println!("Attempting to register {} hotkey(s)...", cfg.general.hotkey.len());

        for hk_str in &cfg.general.hotkey {
            println!("Attempting to register hotkey: {}", hk_str);
            match parse_hotkey(hk_str) {
                Ok(key) => {
                    println!("  Parsed successfully: {:?}", key);
                    use std::io::Write;
                    let _ = std::io::stdout().flush();

                    let before_count = current_hotkeys.len();

                    match manager.register(key) {
                        Ok(_) => {
                            println!("  ✓ Registered: {}", hk_str);
                            current_hotkeys.push(key);
                        }
                        Err(e) => {
                            eprintln!("  ✗ Failed to register '{}': {}", hk_str, e);
                            eprintln!("    This key code is not supported on Windows.");
                        }
                    }

                    // Detect silent failures (register returned Ok but hotkey doesn't actually work)
                    if current_hotkeys.len() == before_count {
                        eprintln!("  ⚠ WARNING: '{}' may have failed to register (silent failure)", hk_str);
                        eprintln!("    The key code '{:?}' is likely not supported by Windows global hotkey system.", key.key);
                        eprintln!("    Please use an alternative hotkey.");
                    }

                    let _ = std::io::stdout().flush();
                },
                Err(e) => eprintln!("  ✗ Failed to parse '{}': {}", hk_str, e),
            }
        }
        println!("\nHotkey registration complete. Successfully registered: {}/{}",
                 current_hotkeys.len(), cfg.general.hotkey.len());

        if current_hotkeys.len() < cfg.general.hotkey.len() {
            eprintln!("\n❌ Some hotkeys failed to register. See warnings above.");
            eprintln!("   Known unsupported keys on Windows: Section (§), IntlBackslash");
            eprintln!("   Suggestion: Use function keys (F1-F12) or letter keys instead.");
        }
    }

    // 5. Tray Icon
    let tray_menu = Menu::new();
    let quit_i = MenuItem::new("Quit", true, None);
    let toggle_i = MenuItem::new("Toggle", true, None);
    let _ = tray_menu.append(&toggle_i);
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
                    let (new_config, _) = load_config();
                    {
                        let mut w = config_clone_watcher.write().unwrap();
                        *w = new_config.clone();
                    }
                    println!("NOTE: Hotkey changes require a daemon restart to take effect.");
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

    // Initial check (Start terminal if missing, optionally toggle)
    {
        let cfg = config_clone_loop.read().unwrap().clone();
        rt.spawn(async move {
            crate::windows::terminal::ensure_terminal_running(&cfg).await;
            if auto_show {
                 tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
                 toggle_window(&cfg).await;
            }
        });
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
                    DaemonEvent::Hotkey(event) => {
                        if event.state == global_hotkey::HotKeyState::Released {
                             if current_hotkeys.iter().any(|hk| hk.id() == event.id) {
                                  // println!("Hotkey Pressed! Toggling...");
                                  unsafe {
                                       use windows::Win32::UI::WindowsAndMessaging::{AllowSetForegroundWindow, ASFW_ANY};
                                       let _ = AllowSetForegroundWindow(ASFW_ANY);
                                  }
                                  let cfg = config_clone_loop.read().unwrap().clone();

                                  // Fast path: if window exists, toggle immediately without spawning check
                                  if crate::windows::window::find_window_by_process(&cfg.general.window_class).is_some() {
                                      rt.spawn(async move {
                                          toggle_window(&cfg).await;
                                      });
                                  } else {
                                      // Slow path: ensure terminal is running first
                                      rt.spawn(async move {
                                          crate::windows::terminal::ensure_terminal_running(&cfg).await;
                                          toggle_window(&cfg).await;
                                      });
                                  }
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
                                            unsafe {
                                                use windows::Win32::UI::WindowsAndMessaging::{AllowSetForegroundWindow, ASFW_ANY};
                                                let _ = AllowSetForegroundWindow(ASFW_ANY);
                                            }
                                            let cfg = config_clone_loop.read().unwrap().clone();

                                            // Fast path: if window exists, toggle immediately
                                            if crate::windows::window::find_window_by_process(&cfg.general.window_class).is_some() {
                                                rt.spawn(async move {
                                                    toggle_window(&cfg).await;
                                                });
                                            } else {
                                                // Slow path: ensure terminal is running first
                                                rt.spawn(async move {
                                                    crate::windows::terminal::ensure_terminal_running(&cfg).await;
                                                    toggle_window(&cfg).await;
                                                });
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
                            } else if event.id == toggle_i.id() {
                                let cfg = config_clone_loop.read().unwrap().clone();
                                rt.spawn(async move {
                                    crate::windows::terminal::ensure_terminal_running(&cfg).await;
                                    toggle_window(&cfg).await;
                                });
                            }
                        }
                    }
                }
            }
            _ => ()
        }
    })?;

    Ok(())
}

pub async fn send_toggle() -> Result<()> {
    let mut client = ClientOptions::new().open(PIPE_NAME)?;
    client.write_all(b"toggle").await?;
    Ok(())
}
