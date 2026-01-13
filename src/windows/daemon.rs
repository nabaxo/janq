use rustc_hash::FxHashMap;
use std::io::Write;
use std::{
  env::temp_dir,
  fs::File,
  path::PathBuf,
  sync::{
    mpsc::{channel, RecvTimeoutError},
    Arc, RwLock,
  },
  thread::{sleep as thread_sleep, Builder},
  time::{Duration, Instant},
};

use anyhow::Result;
use fs2::FileExt;
use global_hotkey::{hotkey::HotKey, GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};
use notify::{Config as NotifyConfig, RecommendedWatcher, RecursiveMode, Watcher};
use tray_icon::{
  menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem},
  MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent,
};
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::{
  HiDpi::{SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2},
  WindowsAndMessaging::{
    AllowSetForegroundWindow, DispatchMessageW, GetMessageW, PostThreadMessageW, TranslateMessage,
    ASFW_ANY, MSG, WM_USER,
  },
};

use crate::{
  config::{load_config, Config},
  hotkey::parse_hotkey,
  windows::window::get_hwnd_cache,
};

const PIPE_NAME: &str = r"\\.\pipe\janq";

#[derive(Debug)]
enum DaemonEvent {
  Hotkey(global_hotkey::GlobalHotKeyEvent),
  TrayPoll,
  ReloadHotkeys,
  RespawnCheck,
  Exit,
}

pub fn run_daemon(
  initial_config: Config,
  config_path: Option<PathBuf>,
  target_app: Option<String>,
) -> Result<()> {
  let main_thread_id = unsafe { GetCurrentThreadId() };
  // 0. Acquire Lock File
  let lock_path = temp_dir().join("janq.lock");
  let lock_file = File::create(&lock_path)?;
  if lock_file.try_lock_exclusive().is_err() {
    return Err(anyhow::anyhow!(
      "janq is already running (lock file active)."
    ));
  }

  // Enable DPI Awareness
  unsafe {
    let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
  }

  let config = Arc::new(RwLock::new(Arc::new(initial_config)));

  // 3. Setup IPC Server (Standard Thread)
  let config_clone = config.clone();
  Builder::new()
    .name("ipc-server".to_string())
    .spawn(move || loop {
      let handle = unsafe {
        windows::Win32::System::Pipes::CreateNamedPipeW(
          windows::core::w!(r"\\.\pipe\janq"),
          std::mem::transmute(3u32),
          std::mem::transmute(6u32),
          255,
          1024,
          1024,
          0,
          None,
        )
      };

      if !handle.is_invalid() {
        if unsafe { windows::Win32::System::Pipes::ConnectNamedPipe(handle, None).is_ok() } {
          let mut buf = [0u8; 1024];
          let mut bytes_read = 0;
          unsafe {
            let _ = windows::Win32::Storage::FileSystem::ReadFile(
              handle,
              Some(&mut buf),
              Some(&mut bytes_read),
              None,
            );
          }

          if bytes_read > 0 {
            let msg = String::from_utf8_lossy(&buf[..bytes_read as usize]);
            let cfg = config_clone.read().unwrap().clone();
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
                crate::windows::terminal::ensure_terminal_running(
                  &target_name,
                  app_cfg,
                  &cfg,
                  None,
                );
                crate::windows::window::toggle_window(&target_name, &cfg);
                unsafe {
                  let _ = windows::Win32::UI::WindowsAndMessaging::PostThreadMessageW(
                    main_thread_id,
                    windows::Win32::UI::WindowsAndMessaging::WM_USER + 1,
                    None,
                    None,
                  );
                }
              }
            }
          }
        }
        unsafe {
          let _ = windows::Win32::Foundation::CloseHandle(handle);
        }
      } else {
        thread_sleep(Duration::from_millis(500));
      }
    })?;

  // 4. Hotkey State
  let current_hotkeys = Arc::new(RwLock::new(Vec::<HotKey>::new()));
  let hotkey_map = Arc::new(RwLock::new(FxHashMap::default()));

  // 5. Config Watcher
  // (Watcher code remains mostly the same, but using std::thread and sync calls)
  let config_clone_watcher = config.clone();
  let path_to_watch = config_path.clone();

  // We need a way to send events back to the main loop.
  // Since we don't have Winit, we'll use a manual channel and wake up the message loop.
  let (event_tx, event_rx) = channel::<DaemonEvent>();
  let event_tx_watcher = event_tx.clone();

  Builder::new()
    .name("config-watcher".to_string())
    .stack_size(128 * 1024)
    .spawn(move || {
      let (tx, rx) = channel();
      let mut watcher = RecommendedWatcher::new(tx, NotifyConfig::default()).unwrap();

      if let Some(path) = &path_to_watch {
        if let Ok(abs_path) = path.canonicalize() {
          if let Some(parent) = abs_path.parent() {
            let _ = watcher.watch(parent, RecursiveMode::NonRecursive);
          } else {
            let _ = watcher.watch(&abs_path, RecursiveMode::NonRecursive);
          }
        } else {
          let _ = watcher.watch(path, RecursiveMode::NonRecursive);
        }
      } else if let Some(home) = dirs::home_dir() {
        let _ = watcher.watch(&home, RecursiveMode::NonRecursive);
      }

      let debounce_duration = Duration::from_millis(500);
      let mut last_event = Instant::now();
      let mut pending = false;

      loop {
        let timeout = if pending {
          debounce_duration.saturating_sub(last_event.elapsed())
        } else {
          Duration::from_secs(60)
        };

        match rx.recv_timeout(timeout) {
          Ok(Ok(event)) => {
            let mut is_config_file = false;
            if let Some(target_path) = &path_to_watch {
              let target_path_abs = target_path.canonicalize().unwrap_or(target_path.clone());
              for p in &event.paths {
                let p_abs = p.canonicalize().unwrap_or(p.clone());
                if p_abs == target_path_abs {
                  is_config_file = true;
                  break;
                }
              }
            } else if let Some(home) = dirs::home_dir() {
              let target = home.join(".janq.toml");
              for p in &event.paths {
                if p == &target {
                  is_config_file = true;
                  break;
                }
              }
            }

            if is_config_file {
              last_event = Instant::now();
              pending = true;
            }
          }
          Ok(Err(e)) => eprintln!("Watcher error: {:?}", e),
          Err(RecvTimeoutError::Timeout) => {
            if pending {
              pending = false;
              println!("Config change detected, reloading...");
              let (new_config, _) = match load_config(path_to_watch.clone()) {
                Ok(c) => c,
                Err(e) => {
                  // Restore all apps from current config before shutting down
                  let current_cfg = config_clone_watcher.read().unwrap().clone();
                  for app_cfg in current_cfg.app.values() {
                    crate::windows::window::restore_app_window(&app_cfg.window_class);
                  }
                  crate::windows::show_error(&e.to_string());
                  let _ = event_tx_watcher.send(DaemonEvent::Exit);
                  return;
                }
              };
              {
                let mut w = config_clone_watcher.write().unwrap();
                let old_config = (**w).clone();
                *w = Arc::new(new_config.clone());

                // 1. Handle removed or changed apps
                let mut to_restore = Vec::new();
                {
                  let mut cache = crate::windows::window::get_hwnd_cache().write().unwrap();
                  for (name, old_app_cfg) in &old_config.app {
                    match new_config.app.get(name) {
                      Some(new_app_cfg) => {
                        // App still exists, but check if class changed
                        if new_app_cfg.window_class != old_app_cfg.window_class {
                          cache.remove(name);
                        }
                      }
                      None => {
                        // App removed - restore window and clear cache
                        to_restore.push(old_app_cfg.window_class.clone());
                        cache.remove(name);
                      }
                    }
                  }
                }
                for class in to_restore {
                  crate::windows::window::restore_app_window(&class);
                }
              }
              let _ = event_tx_watcher.send(DaemonEvent::ReloadHotkeys);
              unsafe {
                let _ = PostThreadMessageW(main_thread_id, WM_USER + 1, None, None);
              }
            }
          }
          Err(RecvTimeoutError::Disconnected) => break,
        }
      }
    })?;

  // 6. Event Loop (Main Thread)
  let hotkey_receiver = GlobalHotKeyEvent::receiver();
  let menu_receiver = MenuEvent::receiver();
  let tray_receiver = TrayIconEvent::receiver();

  let manager_arc = Arc::new(GlobalHotKeyManager::new().expect("Failed to create HotKeyManager"));
  sync_hotkeys(
    Arc::clone(&manager_arc),
    &config.read().unwrap(),
    &hotkey_map,
    &current_hotkeys,
  )?;

  let tray_icon: Option<TrayIcon>;
  let mut app_menu_items = FxHashMap::default();
  let mut quit_item_id;

  // Initial Tray Setup
  {
    let tray_menu = Menu::new();
    let cfg = config.read().unwrap();
    for name in cfg.app.keys() {
      let item = MenuItem::new(name, true, None);
      let _ = tray_menu.append(&item);
      app_menu_items.insert(item.id().clone(), name.clone());
    }
    let _ = tray_menu.append(&PredefinedMenuItem::separator());
    let quit_i = MenuItem::new("Quit", true, None);
    quit_item_id = quit_i.id().clone();
    let _ = tray_menu.append(&quit_i);

    // Optimization 4: Native Resource Loading
    let icon = tray_icon::Icon::from_resource(1, None).expect("Failed to load icon from resource");
    let ti = TrayIconBuilder::new()
      .with_menu(Box::new(tray_menu))
      .with_tooltip("janq")
      .with_icon(icon)
      .build()?;
    tray_icon = Some(ti);
  }

  // Initial app spawning
  {
    let candidates = crate::windows::window::fetch_system_windows();
    let cfg = config.read().unwrap().clone();
    for (name, app_cfg) in &cfg.app {
      let name = name.clone();
      let app_cfg = app_cfg.clone();
      let cfg_copy = cfg.clone();
      let candidates_clone = candidates.clone();
      Builder::new().spawn(move || {
        crate::windows::terminal::ensure_terminal_running(
          &name,
          &app_cfg,
          &cfg_copy,
          Some(&candidates_clone),
        );
      })?;
    }

    if cfg.window.auto_show {
      let app_name =
        target_app.unwrap_or_else(|| cfg.app.keys().next().cloned().unwrap_or_default());
      if !app_name.is_empty() {
        let app_name_c = app_name.clone();
        let cfg_c = cfg.clone();
        Builder::new().spawn(move || {
          thread_sleep(Duration::from_millis(500));
          crate::windows::window::toggle_window(&app_name_c, &cfg_c);
          unsafe {
            let _ = PostThreadMessageW(main_thread_id, WM_USER + 1, None, None);
          }
        })?;
      }
    }
  }

  // Threads to bridge receivers to DaemonEvent channel
  let event_tx_hk = event_tx.clone();
  Builder::new().spawn(move || {
    while let Ok(event) = hotkey_receiver.recv() {
      let _ = event_tx_hk.send(DaemonEvent::Hotkey(event));
      unsafe {
        let _ = PostThreadMessageW(main_thread_id, WM_USER + 1, None, None);
      }
    }
  })?;

  let event_tx_tray = event_tx.clone();
  Builder::new().spawn(move || loop {
    thread_sleep(Duration::from_millis(100));
    let _ = event_tx_tray.send(DaemonEvent::TrayPoll);
    unsafe {
      let _ = PostThreadMessageW(main_thread_id, WM_USER + 1, None, None);
    }
  })?;

  let event_tx_heartbeat = event_tx.clone();
  Builder::new().spawn(move || loop {
    thread_sleep(Duration::from_secs(2));
    let _ = event_tx_heartbeat.send(DaemonEvent::RespawnCheck);
    unsafe {
      let _ = PostThreadMessageW(main_thread_id, WM_USER + 1, None, None);
    }
  })?;

  // Main Win32 Message Loop
  unsafe {
    let mut msg = MSG::default();
    while GetMessageW(&mut msg, None, 0, 0).as_bool() {
      let _ = TranslateMessage(&msg);
      DispatchMessageW(&msg);

      // Process our internal events
      while let Ok(daemon_event) = event_rx.try_recv() {
        match daemon_event {
          DaemonEvent::Exit => {
            crate::windows::window::restore_window_visibility();
            return Ok(());
          }
          DaemonEvent::ReloadHotkeys => {
            println!("Reloading config...");
            let cfg = config.read().unwrap().clone();
            let _ = sync_hotkeys(
              Arc::clone(&manager_arc),
              &cfg,
              &hotkey_map,
              &current_hotkeys,
            );

            // Refresh Tray
            let tray_menu = Menu::new();
            app_menu_items.clear();
            for name in cfg.app.keys() {
              let item = MenuItem::new(name, true, None);
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
            if event.state == HotKeyState::Pressed {
              let map = hotkey_map.read().unwrap();
              if let Some(app_name) = map.get(&event.id) {
                let _ = AllowSetForegroundWindow(ASFW_ANY);
                let cfg = config.read().unwrap().clone();
                let app_name = app_name.clone();
                Builder::new().spawn(move || {
                  if let Some(app_cfg) = cfg.app.get(&app_name) {
                    if !crate::windows::window::toggle_window(&app_name, &cfg) {
                      crate::windows::terminal::ensure_terminal_running(
                        &app_name, app_cfg, &cfg, None,
                      );
                      crate::windows::window::toggle_window(&app_name, &cfg);
                    }
                  }
                })?;
              }
            }
          }
          DaemonEvent::TrayPoll => {
            while let Ok(event) = tray_receiver.try_recv() {
              if let TrayIconEvent::Click {
                button,
                button_state,
                ..
              } = event
              {
                if button_state == MouseButtonState::Up && button == MouseButton::Left {
                  let cfg = config.read().unwrap().clone();
                  if let Some(app_name) = cfg.app.keys().next().cloned() {
                    let app_cfg = cfg.app.get(&app_name).unwrap().clone();
                    Builder::new().spawn(move || {
                      crate::windows::terminal::ensure_terminal_running(
                        &app_name, &app_cfg, &cfg, None,
                      );
                      crate::windows::window::toggle_window(&app_name, &cfg);
                    })?;
                  }
                }
              }
            }
            while let Ok(event) = menu_receiver.try_recv() {
              if event.id == quit_item_id {
                crate::windows::window::restore_window_visibility();
                return Ok(());
              } else if let Some(app_name) = app_menu_items.get(&event.id) {
                let cfg = config.read().unwrap().clone();
                let app_name = app_name.clone();
                let app_cfg = cfg.app.get(&app_name).unwrap().clone();
                Builder::new().spawn(move || {
                  crate::windows::terminal::ensure_terminal_running(
                    &app_name, &app_cfg, &cfg, None,
                  );
                  crate::windows::window::toggle_window(&app_name, &cfg);
                })?;
              }
            }
          }
          DaemonEvent::RespawnCheck => {
            let candidates = crate::windows::window::fetch_system_windows();
            let cfg = config.read().unwrap().clone();
            for (name, app_cfg) in &cfg.app {
              let needs_check = {
                let cache = get_hwnd_cache().read().unwrap();
                let already_spawning = {
                  let spawning = crate::windows::terminal::get_spawning_apps()
                    .lock()
                    .unwrap();
                  spawning.contains(name)
                };

                if already_spawning {
                  false
                } else if let Some(hwnd) = cache.get(name) {
                  !windows::Win32::UI::WindowsAndMessaging::IsWindow(hwnd.0).as_bool()
                } else {
                  true
                }
              };

              if needs_check {
                crate::windows::window::reset_visible_app();
                println!("App '{}' not managed. Checking/Respawning...", name);
                let app_cfg = app_cfg.clone();
                let cfg_spawn = cfg.clone();
                let name_clone = name.clone();
                let candidates_clone = candidates.clone();
                Builder::new().spawn(move || {
                  crate::windows::terminal::ensure_terminal_running(
                    &name_clone,
                    &app_cfg,
                    &cfg_spawn,
                    Some(&candidates_clone),
                  );
                  let _ = windows::Win32::UI::WindowsAndMessaging::PostThreadMessageW(
                    main_thread_id,
                    windows::Win32::UI::WindowsAndMessaging::WM_USER + 1,
                    None,
                    None,
                  );
                })?;
              }
            }
          }
        }
      }
    }
  }

  Ok(())
}

pub fn send_toggle_sync(app_name: Option<String>) -> Result<()> {
  use std::fs::OpenOptions;
  let mut file = OpenOptions::new().write(true).open(PIPE_NAME)?;
  if let Some(name) = app_name {
    file.write_all(format!("toggle:{}", name).as_bytes())?;
  } else {
    file.write_all(b"toggle")?;
  }
  Ok(())
}

pub fn sync_hotkeys(
  manager: Arc<GlobalHotKeyManager>,
  config: &Config,
  hotkey_map: &Arc<RwLock<FxHashMap<u32, String>>>,
  current_hotkeys: &Arc<RwLock<Vec<HotKey>>>,
) -> Result<()> {
  // 1. Check if we actually need to sync
  let mut desired_map = FxHashMap::default();
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
        return Ok(());
      }
    }
  }

  // 2. Unregister all existing hotkeys
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
  let mut new_map = FxHashMap::default();

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
                if manager.register(fallback_key).is_ok() {
                  new_map.insert(fallback_key.id(), app_name.clone());
                  new_hks.push(fallback_key);
                } else {
                  let fallback_key2 =
                    HotKey::new(Some(key.mods), global_hotkey::hotkey::Code::Backslash);
                  if manager.register(fallback_key2).is_ok() {
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
        Err(e) => {
          return Err(anyhow::anyhow!(
            "Failed to parse hotkey '{}': {}",
            hk_str,
            e
          ))
        }
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

  Ok(())
}
