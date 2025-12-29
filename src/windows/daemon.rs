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
use tray_icon::{TrayIconBuilder, menu::{Menu, MenuItem, MenuEvent}};
use winit::event_loop::{ControlFlow, EventLoop};
use winit::event::{Event, StartCause};
use tokio::runtime::Runtime;

const PIPE_NAME: &str = r"\\.\pipe\ruake";

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

    // 2. Setup Winit EventLoop (Must be on main thread)
    let event_loop = EventLoop::new()?;

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

    // 4. Hotkey Manager
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

    // 5. Tray Icon
    let tray_menu = Menu::new();
    let quit_i = MenuItem::new("Quit", true, None);
    let toggle_i = MenuItem::new("Toggle", true, None);
    let _ = tray_menu.append(&toggle_i);
    let _ = tray_menu.append(&quit_i);

    let _tray_icon = TrayIconBuilder::new()
        .with_menu(Box::new(tray_menu))
        .with_tooltip("Ruake")
        .with_icon(load_icon())
        .build()
        .unwrap();

    // 6. Config Watcher (Spawned task or thread)
    let config_clone_watcher = config.clone();
    let path_to_watch = config_path.clone();

    // Watcher logic can run on a separate thread, but updating config needs lock
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
                },
                Err(e) => println!("Watch error: {:?}", e),
             }
        }
    });

    println!("Ruake (Windows) daemon running...");

    // 7. Event Loop (Main Thread)
    let hotkey_receiver = global_hotkey::GlobalHotKeyEvent::receiver();
    let menu_receiver = MenuEvent::receiver();
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

    event_loop.run(move |event, elwt| {
        // Keep manager alive by capturing it
        let _ = &manager;

        // Poll for events every ~16ms (60hz check for channels)
        elwt.set_control_flow(ControlFlow::WaitUntil(std::time::Instant::now() + std::time::Duration::from_millis(16)));

        match event {
            Event::NewEvents(StartCause::ResumeTimeReached { .. }) | Event::AboutToWait => {
                // Check Hotkeys
                if let Ok(event) = hotkey_receiver.try_recv() {
                    if event.state == global_hotkey::HotKeyState::Released {
                         if current_hotkeys.iter().any(|hk| hk.id() == event.id) {
                              // Check if we are already busy to avoid spam
                              {
                                  // Borrow IS_ANIMATING from window module via static, or better:
                                  // Just assume if toggle task is spawned, we shouldn't spawn another immediately.
                                  // A simple primitive valid atomic/mutex here would work.
                              }
                              // We'll use a local atomic in the loop if we could, but closure captures.
                              // Let's use a static since this is a singleton daemon.
                              static IS_BUSY: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
                              if !IS_BUSY.load(std::sync::atomic::Ordering::Relaxed) {
                                  IS_BUSY.store(true, std::sync::atomic::Ordering::Relaxed);
                                  let cfg = config_clone_loop.read().unwrap().clone();
                                  rt.spawn(async move {
                                      crate::windows::terminal::ensure_terminal_running(&cfg).await;
                                      toggle_window(&cfg).await;
                                      IS_BUSY.store(false, std::sync::atomic::Ordering::Relaxed);
                                  });
                              }
                         }
                    }
                }

                // Check Menu
                if let Ok(event) = menu_receiver.try_recv() {
                     if event.id == quit_i.id() {
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
