use crate::config::{load_config, Config};
use crate::hotkey::parse_hotkey;
use crate::windows::window::{get_hwnd_cache, toggle_window};
use anyhow::Result;
use fs2::FileExt;
use global_hotkey::{hotkey::HotKey, GlobalHotKeyManager};
use notify::{Config as NotifyConfig, RecommendedWatcher, RecursiveMode, Watcher};
use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use tokio::io::AsyncWriteExt;
use tokio::net::windows::named_pipe::{ClientOptions, ServerOptions};
use tokio::runtime::Runtime;
use tray_icon::{
  menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem},
  MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent,
};
use winit::event::Event;
use winit::event_loop::{ControlFlow, EventLoopBuilder};

const PIPE_NAME: &str = r"\\.\pipe\janq";

#[derive(Debug)]
enum DaemonEvent {
  Hotkey(global_hotkey::GlobalHotKeyEvent),
  TrayPoll,
  ReloadHotkeys,
  Exit,
}

fn load_icon() -> tray_icon::Icon {
  let bytes = include_bytes!("../../icon.png");
  let image = image::load_from_memory(bytes)
    .expect("Failed to load icon.ico")
    .to_rgba8();
  let (width, height) = image.dimensions();
  let rgba = image.into_raw();
  let tray_icon =
    tray_icon::Icon::from_rgba(rgba, width, height).expect("Failed to create tray icon");
  tray_icon
}

pub fn run_daemon(
  initial_config: Config,
  config_path: Option<PathBuf>,
  target_app: Option<String>,
) -> Result<()> {
  // 1. Setup Runtime for async tasks (IPC, Animation, Watcher)
  let rt = Runtime::new()?;
  let _guard = rt.enter(); // Keep runtime context active for this thread

  // 0. Acquire Lock File
  let lock_path = std::env::temp_dir().join("janq.lock");
  let lock_file = std::fs::File::create(&lock_path)?;
  if lock_file.try_lock_exclusive().is_err() {
    return Err(anyhow::anyhow!(
      "janq is already running (lock file active)."
    ));
  }

  // Enable DPI Awareness (Per Monitor V2) to ensure correct coordinates
  use windows::Win32::UI::HiDpi::{
    SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
  };
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
            cfg.app.keys().next().cloned().unwrap_or_default()
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
              crate::windows::terminal::ensure_terminal_running(&target_name, app_cfg, &cfg).await;
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

  // 4. Shared Hotkey State
  let current_hotkeys = Arc::new(RwLock::new(Vec::<HotKey>::new()));
  let hotkey_map = Arc::new(RwLock::new(std::collections::HashMap::<u32, String>::new()));

  // 5. Config Watcher (Spawned task or thread)
  let config_clone_watcher = config.clone();
  let path_to_watch = config_path.clone();
  let watcher_proxy = event_loop.create_proxy();
  std::thread::spawn(move || {
    let (tx, rx) = std::sync::mpsc::channel();
    let mut watcher = RecommendedWatcher::new(tx, NotifyConfig::default()).unwrap();

    if let Some(path) = &path_to_watch {
      if path.exists() {
        let _ = watcher.watch(&path, RecursiveMode::NonRecursive);
      }
    } else if let Some(home) = dirs::home_dir() {
      let paths = vec![home.join(".janq.toml")];
      for p in paths {
        if p.exists() {
          let _ = watcher.watch(&p, RecursiveMode::NonRecursive);
          break;
        }
      }
    }

    let debounce_duration = std::time::Duration::from_millis(500);
    let mut last_event = std::time::Instant::now();
    let mut pending = false;

    loop {
      let timeout = if pending {
        debounce_duration.saturating_sub(last_event.elapsed())
      } else {
        std::time::Duration::from_secs(60)
      };

      match rx.recv_timeout(timeout) {
        Ok(_) => {
          last_event = std::time::Instant::now();
          pending = true;
        }
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
          if pending {
            pending = false;
            println!("Config change detected, reloading...");
            let (new_config, _) = match load_config(path_to_watch.clone()) {
              Ok(c) => c,
              Err(e) => {
                // Restore all apps from current config before shutting down
                let current_cfg = config_clone_watcher.read().unwrap().clone();
                for (_name, app_cfg) in &current_cfg.app {
                  crate::windows::window::restore_app_window("", &app_cfg.window_class);
                }

                crate::windows::show_error(&e);
                let _ = watcher_proxy.send_event(DaemonEvent::Exit);
                return;
              }
            };
            {
              let mut w = config_clone_watcher.write().unwrap();
              let old_config = (**w).clone();
              *w = Arc::new(new_config.clone());

              for (name, app_cfg) in &old_config.app {
                let still_managed = new_config
                  .app
                  .values()
                  .any(|new_app_cfg| new_app_cfg.window_class == app_cfg.window_class);
                if !still_managed {
                  crate::windows::window::restore_app_window(name, &app_cfg.window_class);
                }
              }
              // NOTE: Don't clear HWND cache here - let the respawn loop handle cache
              // invalidation naturally via IsWindow() checks. Clearing causes races.
            }
            let _ = watcher_proxy.send_event(DaemonEvent::ReloadHotkeys);
          }
        }
        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
      }
    }
  });

  // 6. Event Loop (Main Thread)
  let hotkey_receiver = global_hotkey::GlobalHotKeyEvent::receiver();
  let menu_receiver = MenuEvent::receiver();
  let tray_receiver = TrayIconEvent::receiver();
  let config_clone_loop = config.clone();

  let manager_arc = Arc::new(GlobalHotKeyManager::new().expect("Failed to create HotKeyManager"));
  sync_hotkeys(
    Arc::clone(&manager_arc),
    &config.read().unwrap(),
    &hotkey_map,
    &current_hotkeys,
  );

  let manager = Some(manager_arc);
  let mut tray_icon: Option<tray_icon::TrayIcon> = None;
  let mut app_menu_items = std::collections::HashMap::new();
  let mut quit_item_id = tray_icon::menu::MenuId::new("quit");

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
  std::thread::spawn(move || loop {
    std::thread::sleep(std::time::Duration::from_millis(100));
    let _ = tray_proxy.send_event(DaemonEvent::TrayPoll);
  });

  // Watch for Ctrl+C
  rt.spawn(async move {
    if let Ok(_) = tokio::signal::ctrl_c().await {
      println!("Ctrl+C received. Sending exit signal...");
      let _ = proxy.send_event(DaemonEvent::Exit);
    }
  });

  event_loop.run(move |event, elwt| {
    // Keep these alive by capturing them in the closure
    let _ = &tray_icon;
    let _ = &manager;

    elwt.set_control_flow(ControlFlow::Wait);

    match event {
      Event::NewEvents(winit::event::StartCause::Init) => {
        // 3. Tray Icon
        let tray_menu = Menu::new();
        {
          let cfg = config_clone_loop.read().unwrap();
          let keys = cfg.app.keys().cloned().collect::<Vec<_>>();

          for name in keys {
            if !cfg.app.contains_key(&name) {
              continue;
            }
            let item = MenuItem::new(&name, true, None);
            let _ = tray_menu.append(&item);
            app_menu_items.insert(item.id().clone(), name.clone());
          }
        }

        let _ = tray_menu.append(&PredefinedMenuItem::separator());
        let quit_i = MenuItem::new("Quit", true, None);
        quit_item_id = quit_i.id().clone();
        let _ = tray_menu.append(&quit_i);

        let icon = load_icon();
        match TrayIconBuilder::new()
          .with_menu(Box::new(tray_menu))
          .with_tooltip("janq")
          .with_icon(icon)
          .build()
        {
          Ok(ti) => {
            tray_icon = Some(ti);
          }
          Err(e) => eprintln!("    ✗ Failed to create tray icon: {}", e),
        }

        // 4. Initial app spawning
        {
          let cfg = config_clone_loop.read().unwrap().clone();
          let mut spawn_tasks = Vec::new();

          for (name, app_cfg) in &cfg.app {
            let name = name.clone();
            let app_cfg = app_cfg.clone();
            let cfg = cfg.clone();
            spawn_tasks.push(rt.spawn(async move {
              let _ =
                crate::windows::terminal::ensure_terminal_running(&name, &app_cfg, &cfg).await;
            }));
          }

          let target_app_clone = target_app.clone();
          let rt_clone = rt.handle().clone();
          rt.spawn(async move {
            for task in spawn_tasks {
              let _ = task.await;
            }

            if cfg.window.auto_show {
              let app_to_show = target_app_clone.as_ref();
              if let Some(app_name) = app_to_show {
                if let Some(_app_cfg) = cfg.app.get(app_name) {
                  let cfg = (*cfg).clone();
                  let app_name = app_name.clone();
                  rt_clone.spawn(async move {
                    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
                    toggle_window(&app_name, &cfg).await;
                  });
                }
              } else if let Some(first_app) = cfg.app.keys().next() {
                let cfg = (*cfg).clone();
                let app_name = first_app.clone();
                rt_clone.spawn(async move {
                  tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
                  toggle_window(&app_name, &cfg).await;
                });
              }
            }
          });
        }
      }
      Event::LoopExiting => {
        let cfg = config_clone_loop.read().unwrap().clone();
        crate::windows::window::restore_window_visibility(&cfg);
      }
      Event::UserEvent(daemon_event) => {
        match daemon_event {
          DaemonEvent::Exit => {
            elwt.exit();
          }
          DaemonEvent::ReloadHotkeys => {
            println!("Reloading config (hotkeys & tray menu)...");
            let cfg = config_clone_loop.read().unwrap().clone();

            // 1. Sync Hotkeys
            if let Some(m) = &manager {
              sync_hotkeys(Arc::clone(m), &cfg, &hotkey_map, &current_hotkeys);
            }

            // 2. Refresh Tray Menu
            let tray_menu = Menu::new();
            app_menu_items.clear();
            let keys = cfg.app.keys().cloned().collect::<Vec<_>>();

            for name in keys {
              if !cfg.app.contains_key(&name) {
                continue;
              }
              let item = MenuItem::new(&name, true, None);
              let _ = tray_menu.append(&item);
              app_menu_items.insert(item.id().clone(), name.clone());
            }

            let _ = tray_menu.append(&PredefinedMenuItem::separator());
            let quit_i = MenuItem::new("Quit", true, None);
            quit_item_id = quit_i.id().clone();
            let _ = tray_menu.append(&quit_i);

            if let Some(ti) = &tray_icon {
              let _ = ti.set_menu(Some(Box::new(tray_menu)));
            }
          }
          DaemonEvent::Hotkey(event) => {
            if event.state == global_hotkey::HotKeyState::Pressed {
              let map = hotkey_map.read().unwrap();
              if let Some(app_name) = map.get(&event.id) {
                unsafe {
                  use windows::Win32::UI::WindowsAndMessaging::{
                    AllowSetForegroundWindow, ASFW_ANY,
                  };
                  let _ = AllowSetForegroundWindow(ASFW_ANY);
                }
                let cfg = config_clone_loop.read().unwrap().clone();
                let app_name = app_name.clone();

                rt.spawn(async move {
                  if let Some(app_cfg) = cfg.app.get(&app_name) {
                    if !toggle_window(&app_name, &cfg).await {
                      crate::windows::terminal::ensure_terminal_running(&app_name, app_cfg, &cfg)
                        .await;
                      toggle_window(&app_name, &cfg).await;
                    }
                  }
                });
              }
            }
          }
          DaemonEvent::TrayPoll => {
            // Check Tray Icon Events on 100ms timer
            while let Ok(event) = tray_receiver.try_recv() {
              match event {
                TrayIconEvent::Click {
                  button,
                  button_state,
                  ..
                } => {
                  if button_state == MouseButtonState::Up {
                    if button == MouseButton::Left {
                      let cfg = config_clone_loop.read().unwrap().clone();
                      let app_to_toggle = cfg.app.keys().next().cloned();

                      if let Some(app_name) = app_to_toggle {
                        if let Some(app_cfg) = cfg.app.get(&app_name) {
                          let app_cfg = app_cfg.clone();
                          let cfg_spawn = cfg.clone();
                          let name_clone = app_name.clone();
                          rt.spawn(async move {
                            crate::windows::terminal::ensure_terminal_running(
                              &name_clone,
                              &app_cfg,
                              &cfg_spawn,
                            )
                            .await;
                            toggle_window(&app_name, &cfg_spawn).await;
                          });
                        }
                      }
                    } else if button == MouseButton::Middle {
                      let cfg = config_clone_loop.read().unwrap().clone();
                      crate::windows::window::restore_window_visibility(&cfg);
                      std::process::exit(0);
                    }
                  }
                }
                _ => {}
              }
            }

            // Check Menu
            while let Ok(event) = menu_receiver.try_recv() {
              if event.id == quit_item_id {
                let cfg = config_clone_loop.read().unwrap().clone();
                crate::windows::window::restore_window_visibility(&cfg);
                std::process::exit(0);
              } else if let Some(app_name) = app_menu_items.get(&event.id) {
                let cfg = config_clone_loop.read().unwrap().clone();
                let app_name = app_name.clone();
                if let Some(app_cfg) = cfg.app.get(&app_name) {
                  let app_cfg = app_cfg.clone();
                  let cfg_spawn = cfg.clone();
                  let name_clone = app_name.clone();
                  rt.spawn(async move {
                    crate::windows::terminal::ensure_terminal_running(
                      &name_clone,
                      &app_cfg,
                      &cfg_spawn,
                    )
                    .await;
                    toggle_window(&app_name, &cfg_spawn).await;
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
            // Only check if NOT in cache or invalid
            let needs_check = {
              let cache = get_hwnd_cache().read().unwrap();
              if let Some(hwnd) = cache.get(name) {
                unsafe { !windows::Win32::UI::WindowsAndMessaging::IsWindow(hwnd.0).as_bool() }
              } else {
                true
              }
            };

            if needs_check {
              // If this was the visible app, reset it
              crate::windows::window::reset_visible_app();

              println!("App '{}' not managed. Checking/Respawning...", name);
              let app_cfg = app_cfg.clone();
              let cfg_spawn = cfg.clone();
              let name_clone = name.clone();
              rt.spawn(async move {
                crate::windows::terminal::ensure_terminal_running(
                  &name_clone,
                  &app_cfg,
                  &cfg_spawn,
                )
                .await;
              });
            }
          }
        }
      }
      _ => (),
    }
  })?;

  Ok(())
}

pub async fn send_toggle(app_name: Option<String>) -> Result<()> {
  let mut client = ClientOptions::new().open(PIPE_NAME)?;
  if let Some(name) = app_name {
    client
      .write_all(format!("toggle:{}", name).as_bytes())
      .await?;
  } else {
    client.write_all(b"toggle").await?;
  }
  Ok(())
}

pub fn sync_hotkeys(
  manager: Arc<GlobalHotKeyManager>,
  config: &Config,
  hotkey_map: &Arc<RwLock<std::collections::HashMap<u32, String>>>,
  current_hotkeys: &Arc<RwLock<Vec<HotKey>>>,
) {
  // 0. Check if we actually need to sync
  let mut desired_map = std::collections::HashMap::new();
  let mut desired_hks = Vec::new();
  for (app_name, app_cfg) in &config.app {
    for hk_str in app_cfg.hotkey.as_vec() {
      if hk_str.is_empty() {
        continue;
      }
      if let Ok(key) = parse_hotkey(&hk_str) {
        desired_map.insert(key.id(), app_name.clone());
        desired_hks.push(key);
      }
    }
  }

  {
    let current_map = hotkey_map.read().unwrap();
    let current_hks = current_hotkeys.read().unwrap();

    if current_map.len() == desired_map.len() && current_hks.len() == desired_hks.len() {
      let mut identical = true;
      for (id, app_name) in &desired_map {
        if current_map.get(id) != Some(app_name) {
          identical = false;
          break;
        }
      }
      if identical {
        for hk in &desired_hks {
          if !current_hks.iter().any(|c| c.id() == hk.id()) {
            identical = false;
            break;
          }
        }
      }

      if identical {
        println!("Hotkey: Windows hotkeys already correct. Skipping sync.");
        return;
      }
    }
  }

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
      match parse_hotkey(&hk_str) {
        Ok(key) => {
          let key_code = key.key;
          match manager.register(key) {
            Ok(_) => {
              new_map.insert(key.id(), app_name.clone());
              new_hks.push(key);
            }
            Err(e) => {
              // Try fallback for section key variants if it failed
              if key_code == global_hotkey::hotkey::Code::IntlBackslash {
                let fallback_key =
                  HotKey::new(Some(key.mods), global_hotkey::hotkey::Code::Backquote);
                if let Ok(_) = manager.register(fallback_key) {
                  new_map.insert(fallback_key.id(), app_name.clone());
                  new_hks.push(fallback_key);
                } else {
                  let fallback_key2 =
                    HotKey::new(Some(key.mods), global_hotkey::hotkey::Code::Backslash);
                  if let Ok(_) = manager.register(fallback_key2) {
                    new_map.insert(fallback_key2.id(), app_name.clone());
                    new_hks.push(fallback_key2);
                  } else {
                    eprintln!("  ✗ Failed to register {}: {}", hk_str, e);
                  }
                }
              } else {
                eprintln!("  ✗ Failed to register {}: {}", hk_str, e);
              }
            }
          }
        }
        Err(e) => eprintln!("  ✗ Failed to parse {}: {}", hk_str, e),
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
