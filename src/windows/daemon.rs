//! Windows daemon implementation using Win32 message loop and named pipe IPC.
//!
//! ## Architecture
//!
//! Unlike Linux's async D-Bus approach, Windows uses a synchronous event loop:
//!
//! 1. **Win32 Message Loop** - Main thread runs `GetMessageW`/`DispatchMessageW`
//! 2. **Named Pipe IPC** - Receives toggle commands from janq client instances
//! 3. **Global Hotkey** - Registers system-wide hotkeys via `global-hotkey` crate
//! 4. **System Tray** - Shows tray icon for quick access and quit
//!
//! ## Event Handling
//!
//! Events from different sources are unified into `DaemonEvent` enum:
//! - `Hotkey(event)` - Global hotkey pressed
//! - `TrayPoll` - Check for tray/menu events
//! - `ReloadHotkeys` - Config changed, re-register hotkeys
//! - `RespawnCheck` - Periodic check for crashed apps
//! - `Exit` - Graceful shutdown requested
//!
//! Background threads post `WM_USER+1` to wake the main message loop
//! when they have events ready.
//!
//! ## Hotkey Fallback
//!
//! For European keyboards, IntlBackslash (§) key may fail to register.
//! The daemon automatically tries Backquote then Backslash as fallbacks.

use rustc_hash::FxHashMap;
use std::{
  path::PathBuf,
  process::exit,
  sync::{mpsc::channel, mpsc::Receiver, mpsc::Sender, Arc, RwLock},
  time::{Duration, Instant},
};

use global_hotkey::{hotkey::HotKey, GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};
use tokio::net::windows::named_pipe::ServerOptions;
use tray_icon::{
  menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem},
  MouseButton, TrayIconBuilder, TrayIconEvent,
};
use windows::Win32::Foundation::{LPARAM, WPARAM};
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::{
  HiDpi::{SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2},
  WindowsAndMessaging::{
    AllowSetForegroundWindow, DispatchMessageW, GetMessageW, IsWindow, PostThreadMessageW,
    TranslateMessage, ASFW_ANY, MSG, WM_USER,
  },
};

use crate::windows::hotkey::parse_hotkey;
use crate::windows::{
  show_error,
  terminal::ensure_terminal_running,
  window::{
    fetch_system_windows, get_app_cache, init_focus_hook, init_hidden_owner, release_windows,
    reset_visible_app, restore_window_visibility, toggle_window,
  },
};
use janq::config::{load_config, Config};
use janq::shutdown::{print_shutdown_message, print_termination_complete};
use janq::spawn_guard::get_spawning_apps;

// =============================================================================
// IPC Constants
// =============================================================================

/// Named pipe path for IPC between janq client and daemon.
const PIPE_NAME: &str = r"\\.\pipe\janq";

// =============================================================================
// Daemon Event Loop
// =============================================================================

/// Internal event type for the daemon event loop.
///
/// Unifies events from different sources (hotkeys, tray, config watcher)
/// into a single enum for processing in the main message loop.
#[derive(Debug)]
enum DaemonEvent {
  Hotkey(GlobalHotKeyEvent),
  Menu(MenuEvent),
  Tray(TrayIconEvent),
  ReloadHotkeys,
  RespawnCheck,
  FocusLost,
  Exit(Option<&'static str>),
}

pub async fn run_daemon(
  initial_config: Config,
  config_path: Option<PathBuf>,
  target_app: Option<String>,
) -> janq::error::Result<()> {
  println!("Starting janq daemon...");
  let main_thread_id = unsafe { GetCurrentThreadId() };
  crate::windows::window::MAIN_THREAD_ID
    .set(main_thread_id)
    .unwrap();
  // 0. Acquire Lock File
  let _lock_file = janq::acquire_lock_file()?;

  // Enable DPI Awareness
  unsafe {
    let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
  }

  // Initialize focus tracking hook
  let _focus_hook = init_focus_hook();

  // Initialize hidden owner window for taskbar hiding (skip_pager feature)
  init_hidden_owner();

  let config = Arc::new(RwLock::new(Arc::new(initial_config)));

  // 3. Setup IPC Server (Tokio Task)
  let config_ipc = config.clone();
  tokio::spawn(async move {
    let mut first = true;
    loop {
      let mut options = ServerOptions::new();
      if first {
        options.first_pipe_instance(true);
      }
      let server_res = options.create(PIPE_NAME);

      match server_res {
        Ok(server) => {
          first = false;
          if server.connect().await.is_ok() {
            let mut buf = [0u8; 1024];
            if let Ok(bytes_read) = server.try_read(&mut buf) {
              if bytes_read > 0 {
                let msg = String::from_utf8_lossy(&buf[..bytes_read]);
                let cfg = config_ipc.read().unwrap().clone();
                let target_app = janq::resolve_target_app(&msg, &cfg);

                if let Some(target_name) = target_app {
                  if let Some(app_cfg) = cfg.app.get(&target_name) {
                    let app_cfg = app_cfg.clone();
                    let cfg = cfg.clone();
                    let target_name_c = target_name.clone();
                    tokio::task::spawn_blocking(move || {
                      ensure_terminal_running(&target_name_c, &app_cfg, &cfg, None);
                      toggle_window(&target_name_c, &cfg);
                    });
                    unsafe {
                      let _ = PostThreadMessageW(main_thread_id, WM_USER + 1, WPARAM(0), LPARAM(0));
                    }
                  }
                }
              }
            }
          }
        }
        Err(_) => {
          tokio::time::sleep(Duration::from_millis(500)).await;
        }
      }
    }
  });

  // 4. Hotkey State
  let current_hotkeys = Arc::new(RwLock::new(Vec::<HotKey>::new()));
  let hotkey_map = Arc::new(RwLock::new(FxHashMap::default()));

  // 5. Config Watcher
  let config_clone_watcher = config.clone();
  let path_to_watch = config_path.clone();

  // We need a way to send events back to the main loop.
  // Since we don't have Winit, we'll use a manual channel and wake up the message loop.
  let (event_tx, event_rx): (Sender<DaemonEvent>, Receiver<DaemonEvent>) = channel();
  let event_tx_watcher = event_tx.clone();

  // 6. Signal Handling (Graceful shutdown: Ctrl+C, Ctrl+Break, Console Close)
  let event_tx_signal = event_tx.clone();
  tokio::spawn(async move {
    use tokio::signal::windows::{ctrl_break, ctrl_c, ctrl_close};

    let mut sig_c = ctrl_c().expect("Failed to listen for Ctrl+C");
    let mut sig_break = ctrl_break().expect("Failed to listen for Ctrl+Break");
    let mut sig_close = ctrl_close().expect("Failed to listen for Ctrl+Close");

    let signal_name = tokio::select! {
        _ = sig_c.recv() => "Ctrl+C",
        _ = sig_break.recv() => "Ctrl+Break",
        _ = sig_close.recv() => "Console Close",
    };

    let _ = event_tx_signal.send(DaemonEvent::Exit(Some(signal_name)));
    unsafe {
      let _ = PostThreadMessageW(main_thread_id, WM_USER + 1, WPARAM(0), LPARAM(0));
    }
  });

  janq::config_watcher::spawn_config_watcher(path_to_watch.clone(), move || {
    let path_to_watch = path_to_watch.clone();
    let config_clone_watcher = config_clone_watcher.clone();
    let event_tx_watcher = event_tx_watcher.clone();
    let main_thread_id = main_thread_id;
    async move {
      let (new_config, _) = match load_config(path_to_watch.clone()) {
        Ok(c) => c,
        Err(e) => {
          let err_msg = format!(
            "Config reload failed: {}\nStaying with the last known good configuration.",
            e
          );
          show_error(&err_msg);
          return;
        }
      };
      {
        let mut w = config_clone_watcher.write().unwrap();
        let old_config = (**w).clone();
        *w = Arc::new(new_config.clone());

        // Handle removed or changed apps
        let mut to_restore = Vec::new();
        {
          let mut cache = get_app_cache().write().unwrap();
          for (name, old_app_cfg) in &old_config.app {
            match new_config.app.get(name) {
              Some(new_app_cfg) => {
                // App still exists, but check if class changed
                if new_app_cfg.window_class != old_app_cfg.window_class {
                  // Collect cached HWND before removing
                  if let Some(cw) = cache.remove(name) {
                    to_restore.push(cw);
                  }
                }
              }
              None => {
                // App removed - collect cached HWND and clear cache
                if let Some(cw) = cache.remove(name) {
                  to_restore.push(cw);
                }
              }
            }
          }
        }
        // Cancel any ongoing animation before restoring
        release_windows(to_restore);
      }
      let _ = event_tx_watcher.send(DaemonEvent::ReloadHotkeys);
      unsafe {
        let _ = PostThreadMessageW(main_thread_id, WM_USER + 1, WPARAM(0), LPARAM(0));
      }
    }
  });

  // 6. Event Loop (Main Thread)
  let hotkey_receiver = GlobalHotKeyEvent::receiver();
  let menu_receiver = MenuEvent::receiver();
  let tray_icon_receiver = TrayIconEvent::receiver();

  let manager_arc = Arc::new(GlobalHotKeyManager::new().expect("Failed to create HotKeyManager"));
  sync_hotkeys(
    Arc::clone(&manager_arc),
    &config.read().unwrap(),
    &hotkey_map,
    &current_hotkeys,
  )?;

  // Consolidated Initialization
  let (initial_menu, mut app_menu_items, mut quit_item_id) =
    build_tray_menu(&config.read().unwrap());

  let icon = tray_icon::Icon::from_resource(1, None).expect("Failed to load icon from resource");
  let tray_icon = Some(
    TrayIconBuilder::new()
      .with_menu(Box::new(initial_menu))
      .with_menu_on_left_click(false)
      .with_tooltip("janq")
      .with_icon(icon)
      .build()?,
  );

  // Initial app spawning
  {
    println!("janq: Yoinking apps...");
    let candidates = fetch_system_windows();
    let cfg = config.read().unwrap().clone();
    for (name, app_cfg) in &cfg.app {
      let name = name.clone();
      let app_cfg = app_cfg.clone();
      let cfg_copy = cfg.clone();
      let candidates_clone = candidates.clone();
      tokio::task::spawn_blocking(move || {
        ensure_terminal_running(&name, &app_cfg, &cfg_copy, Some(&candidates_clone));
      });
    }

    if cfg.window.auto_show {
      let app_name =
        target_app.unwrap_or_else(|| cfg.app.keys().next().cloned().unwrap_or_default());
      if !app_name.is_empty() {
        let app_name_c = app_name.clone();
        let cfg_c = cfg.clone();
        tokio::spawn(async move {
          tokio::time::sleep(Duration::from_millis(500)).await;
          let app_name_c2 = app_name_c.clone();
          let cfg_c2 = cfg_c.clone();
          tokio::task::spawn_blocking(move || {
            toggle_window(&app_name_c2, &cfg_c2);
          });
          unsafe {
            let _ = PostThreadMessageW(main_thread_id, WM_USER + 1, WPARAM(0), LPARAM(0));
          }
        });
      }
    }
  }

  // Threads to bridge receivers to DaemonEvent channel
  let event_tx_hk = event_tx.clone();
  std::thread::spawn(move || {
    while let Ok(event) = hotkey_receiver.recv() {
      let _ = event_tx_hk.send(DaemonEvent::Hotkey(event));
      unsafe {
        let _ = PostThreadMessageW(main_thread_id, WM_USER + 1, WPARAM(0), LPARAM(0));
      }
    }
  });

  let event_tx_menu = event_tx.clone();
  std::thread::spawn(move || {
    while let Ok(event) = menu_receiver.recv() {
      let _ = event_tx_menu.send(DaemonEvent::Menu(event));
      unsafe {
        let _ = PostThreadMessageW(main_thread_id, WM_USER + 1, WPARAM(0), LPARAM(0));
      }
    }
  });

  let event_tx_tray_icon = event_tx.clone();
  std::thread::spawn(move || {
    while let Ok(event) = tray_icon_receiver.recv() {
      // For left-clicks, claim foreground permission ASAP
      if matches!(
        event,
        TrayIconEvent::Click {
          button: MouseButton::Left,
          ..
        } | TrayIconEvent::DoubleClick {
          button: MouseButton::Left,
          ..
        }
      ) {
        unsafe {
          let _ = AllowSetForegroundWindow(ASFW_ANY);
        }
      }

      let _ = event_tx_tray_icon.send(DaemonEvent::Tray(event));
      unsafe {
        let _ = PostThreadMessageW(main_thread_id, WM_USER + 1, WPARAM(0), LPARAM(0));
      }
    }
  });

  let event_tx_heartbeat = event_tx.clone();
  tokio::spawn(async move {
    loop {
      tokio::time::sleep(Duration::from_secs(2)).await;
      let _ = event_tx_heartbeat.send(DaemonEvent::RespawnCheck);
      unsafe {
        let _ = PostThreadMessageW(main_thread_id, WM_USER + 1, WPARAM(0), LPARAM(0));
      }
    }
  });

  // Main Win32 Message Loop
  let mut last_tray_toggle = Instant::now() - Duration::from_secs(1);
  unsafe {
    let mut msg = MSG::default();
    while GetMessageW(&mut msg, None, 0, 0).as_bool() {
      if msg.message == WM_USER + 2 {
        let _ = event_tx.send(DaemonEvent::FocusLost);
      }
      let _ = TranslateMessage(&msg);
      DispatchMessageW(&msg);

      // Process our internal events
      while let Ok(daemon_event) = event_rx.try_recv() {
        match daemon_event {
          DaemonEvent::Exit(signal) => {
            print_shutdown_message(signal.unwrap_or("Quit requested"));
            restore_window_visibility();
            print_termination_complete();
            exit(0);
          }
          DaemonEvent::ReloadHotkeys => {
            let cfg = config.read().unwrap().clone();
            let _ = sync_hotkeys(
              Arc::clone(&manager_arc),
              &cfg,
              &hotkey_map,
              &current_hotkeys,
            );

            // Rebuild and update the tray menu
            let (new_menu, items, q_id) = build_tray_menu(&cfg);
            app_menu_items = items;
            quit_item_id = q_id;

            if let Some(ti) = &tray_icon {
              let _ = ti.set_menu(Some(Box::new(new_menu)));
            }
          }
          DaemonEvent::FocusLost => {
            let cfg = config.read().unwrap().clone();
            if cfg.window.auto_hide {
              if let Some(visible_app) = crate::windows::window::get_visible_app()
                .read()
                .unwrap()
                .clone()
              {
                println!("Focus Lost: Auto-hiding '{}'", visible_app);
                let _ = AllowSetForegroundWindow(ASFW_ANY);
                tokio::task::spawn_blocking(move || {
                  toggle_window(&visible_app, &cfg);
                });
              }
            }
          }
          DaemonEvent::Hotkey(event) => {
            if event.state == HotKeyState::Pressed {
              let map = hotkey_map.read().unwrap();
              if let Some(app_name) = map.get(&event.id) {
                println!("Hotkey: Activating action '{}'", app_name);
                let _ = AllowSetForegroundWindow(ASFW_ANY);
                let cfg = config.read().unwrap().clone();
                let app_name = app_name.clone();
                tokio::task::spawn_blocking(move || {
                  if let Some(app_cfg) = cfg.app.get(&app_name) {
                    if !toggle_window(&app_name, &cfg) {
                      ensure_terminal_running(&app_name, app_cfg, &cfg, None);
                      toggle_window(&app_name, &cfg);
                    }
                  }
                });
              }
            }
          }
          DaemonEvent::Menu(event) => {
            if event.id == quit_item_id {
              print_shutdown_message("Quit via tray menu");
              restore_window_visibility();
              print_termination_complete();
              exit(0);
            } else if let Some(app_name) = app_menu_items.get(&event.id) {
              let _ = AllowSetForegroundWindow(ASFW_ANY);
              let cfg = config.read().unwrap().clone();
              let app_name = app_name.clone();
              let app_cfg = cfg.app.get(&app_name).unwrap().clone();
              let app_name_spawn = app_name.clone();
              let cfg_spawn = cfg.clone();
              tokio::task::spawn_blocking(move || {
                ensure_terminal_running(&app_name_spawn, &app_cfg, &cfg_spawn, None);
                toggle_window(&app_name_spawn, &cfg_spawn);
              });
            }
          }
          DaemonEvent::Tray(event) => {
            if matches!(
              event,
              TrayIconEvent::Click {
                button: MouseButton::Left,
                ..
              } | TrayIconEvent::DoubleClick {
                button: MouseButton::Left,
                ..
              }
            ) {
              if last_tray_toggle.elapsed() < Duration::from_millis(100) {
                continue;
              }
              last_tray_toggle = Instant::now();

              let _ = AllowSetForegroundWindow(ASFW_ANY);
              let cfg = config.read().unwrap().clone();
              if let Some(app_name) = cfg.app.keys().next() {
                let app_name = app_name.clone();
                let app_cfg = cfg.app.get(&app_name).unwrap().clone();
                let cfg_spawn = cfg.clone();
                tokio::task::spawn_blocking(move || {
                  ensure_terminal_running(&app_name, &app_cfg, &cfg_spawn, None);
                  toggle_window(&app_name, &cfg_spawn);
                });
              }
            }
          }
          DaemonEvent::RespawnCheck => {
            let candidates = fetch_system_windows();
            let cfg = config.read().unwrap().clone();
            for (name, app_cfg) in &cfg.app {
              let needs_check = {
                let cache = get_app_cache().read().unwrap();
                let already_spawning = {
                  let spawning = get_spawning_apps().lock().unwrap();
                  spawning.contains(name)
                };

                if already_spawning {
                  false
                } else if let Some(hwnd) = cache.get(name) {
                  !IsWindow(Some(hwnd.hwnd)).as_bool()
                } else {
                  true
                }
              };

              if needs_check {
                reset_visible_app();
                println!("App '{}' not managed. Checking/Respawning...", name);
                let app_cfg = app_cfg.clone();
                let cfg_spawn = cfg.clone();
                let name_clone = name.clone();
                let candidates_clone = candidates.clone();
                tokio::task::spawn_blocking(move || {
                  ensure_terminal_running(
                    &name_clone,
                    &app_cfg,
                    &cfg_spawn,
                    Some(&candidates_clone),
                  );
                  let _ = PostThreadMessageW(main_thread_id, WM_USER + 1, WPARAM(0), LPARAM(0));
                });
              }
            }
          }
        }
      }
    }
  }

  println!("janq: Termination complete.");
  Ok(())
}

// =============================================================================
// IPC Client
// =============================================================================

pub async fn send_toggle(app_name: Option<String>) -> janq::error::Result<()> {
  use tokio::io::AsyncWriteExt;

  let mut client = tokio::net::windows::named_pipe::ClientOptions::new().open(PIPE_NAME)?;
  if let Some(name) = app_name {
    client
      .write_all(format!("toggle:{}", name).as_bytes())
      .await?;
  } else {
    client.write_all(b"toggle").await?;
  }
  Ok(())
}

// =============================================================================
// Hotkey Management
// =============================================================================

pub fn sync_hotkeys(
  manager: Arc<GlobalHotKeyManager>,
  config: &Config,
  hotkey_map: &Arc<RwLock<FxHashMap<u32, String>>>,
  current_hotkeys: &Arc<RwLock<Vec<HotKey>>>,
) -> janq::error::Result<()> {
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
                    show_error(&format!("  ✗ Failed to register {}: {}", hk_str, e));
                  }
                }
              } else {
                show_error(&format!("  ✗ Failed to register {}: {}", hk_str, e));
              }
            }
          }
        }
        Err(e) => {
          return Err(janq::format_error_boxed!(
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

fn build_tray_menu(
  cfg: &janq::config::Config,
) -> (
  Menu,
  FxHashMap<tray_icon::menu::MenuId, String>,
  tray_icon::menu::MenuId,
) {
  let tray_menu = Menu::new();
  let mut app_items = FxHashMap::default();

  for name in cfg.app.keys() {
    let hks = cfg.app[name].hotkey.as_vec();
    let hk_str = hks.first().cloned().unwrap_or_default();

    // Create the label with a Tab separator: "App Name\tCtrl+`"
    let label = if !hk_str.is_empty() {
      format!(
        "{}\t{}",
        name,
        crate::windows::hotkey::normalize_for_win(&hk_str)
      )
    } else {
      name.clone()
    };

    // We pass None for the accelerator because we've manually added it to the label
    let item = MenuItem::new(label, true, None);
    let _ = tray_menu.append(&item);
    app_items.insert(item.id().clone(), name.clone());
  }

  let _ = tray_menu.append(&PredefinedMenuItem::separator());
  let quit_i = MenuItem::new("Quit", true, None);
  let quit_id = quit_i.id().clone();
  let _ = tray_menu.append(&quit_i);

  (tray_menu, app_items, quit_id)
}
