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
//! - `RespawnCheck` - Respawn managed apps on window destruction (destroy hook)
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
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::{
  HiDpi::{SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2},
  Input::KeyboardAndMouse::{GetKeyState, VK_SHIFT},
  WindowsAndMessaging::{
    AllowSetForegroundWindow, DispatchMessageW, GetMessageW, GetWindowThreadProcessId, IsWindow,
    SendMessageW, TranslateMessage, ASFW_ANY, MSG, WM_CANCELMODE, WM_USER,
  },
};

use crate::windows::hotkey::parse_hotkey;
use crate::windows::{
  show_warning,
  terminal::ensure_terminal_running,
  window::{
    fetch_system_windows, get_app_cache, init_destroy_hook, init_focus_hook, init_hidden_owner,
    post_wake_message, release_windows, reset_visible_app, restore_window_visibility,
    toggle_window, CachedWindow, BRIDGE_HWND,
  },
};
use janq::{
  config::Config,
  config_watcher, error, process,
  shutdown::{print_shutdown_message, print_termination_complete},
  spawn_guard::get_spawning_apps,
};

// =============================================================================
// IPC Constants
// =============================================================================

/// Named pipe path for IPC between janq client and daemon.
const PIPE_NAME: &str = r"\\.\pipe\janq";

// =============================================================================
// Bridge Window for Modal-Loop Safe Signaling
// =============================================================================

/// Creates a message-only window to receive wake-up messages.
/// This is necessary because PostThreadMessage signals are lost during modal loops
/// (e.g., when the tray menu is open), but PostMessage to a window handle works.
fn init_bridge_window() {
  use windows::core::w;
  // use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
  use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, RegisterClassW, HWND_MESSAGE, WINDOW_EX_STYLE, WNDCLASSW,
    WS_POPUP,
  };

  unsafe extern "system" fn bridge_wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
  ) -> LRESULT {
    use windows::Win32::UI::WindowsAndMessaging::{WM_SETTINGCHANGE, WM_USER};

    if msg == WM_SETTINGCHANGE {
      // Check if the change was to "ImmersiveColorSet" (the theme)
      let l_ptr = lparam.0 as *const u16;
      if !l_ptr.is_null() {
        let s = windows::core::PCWSTR(l_ptr).to_string().unwrap_or_default();
        if s == "ImmersiveColorSet" {
          // Wake the loop to re-apply themes
          post_wake_message(WM_USER + 1);
          // We'll reuse the ReloadHotkeys logic or a new ThemeChanged event
        }
      }
    }
    DefWindowProcW(hwnd, msg, wparam, lparam)
  }

  unsafe {
    let class_name = w!("janq_bridge");

    let wc = WNDCLASSW {
      lpfnWndProc: Some(bridge_wnd_proc),
      lpszClassName: class_name,
      ..Default::default()
    };
    RegisterClassW(&wc);

    let hwnd = CreateWindowExW(
      WINDOW_EX_STYLE::default(),
      class_name,
      w!("janq_bridge_window"),
      WS_POPUP,
      0,
      0,
      0,
      0,
      Some(HWND_MESSAGE),
      None,
      None,
      None,
    );

    if let Ok(h) = hwnd {
      let _ = BRIDGE_HWND.set(CachedWindow { hwnd: h });
    }
  }
}

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

  // Enable DPI Awareness
  unsafe {
    let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
  }

  // Initialize focus tracking hook
  let _focus_hook = init_focus_hook();

  // Initialize window-destroy hook for instant cache invalidation on managed window close
  let _destroy_hook = init_destroy_hook();

  // Initialize hidden owner window for taskbar hiding (skip_pager feature)
  init_hidden_owner();

  // Initialize bridge window for modal-loop safe message posting
  init_bridge_window();

  // Set tray menu theme
  apply_theme_preference();

  let config = Arc::new(RwLock::new(initial_config));

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
        Ok(mut server) => {
          first = false;
          if server.connect().await.is_ok() {
            use tokio::io::AsyncReadExt;
            let mut buf = [0u8; 1024];
            if let Ok(bytes_read) = server.read(&mut buf).await {
              if bytes_read > 0 {
                let msg = String::from_utf8_lossy(&buf[..bytes_read]);
                let cfg = config_ipc.read().unwrap().clone();
                let target_app = janq::resolve_target_app(&msg, &cfg);

                if let Some(target_name) = target_app {
                  if let Some(app_cfg) = cfg.app.get(&target_name) {
                    let app_cfg = app_cfg.clone();
                    let target_name_c = target_name.clone();
                    tokio::task::spawn_blocking(move || {
                      let cands = fetch_system_windows();
                      ensure_terminal_running(&target_name_c, &app_cfg, &cfg, Some(&cands[..]));
                      toggle_window(&target_name_c, &cfg);
                    });
                    post_wake_message(WM_USER + 1);
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
    post_wake_message(WM_USER + 1);
  });

  config_watcher::spawn_config_watcher(path_to_watch.clone(), move || {
    let path_to_watch = path_to_watch.clone();
    let config_clone_watcher = config_clone_watcher.clone();
    let event_tx_watcher = event_tx_watcher.clone();
    async move {
      let old_config =
        match config_watcher::reload_shared_config(path_to_watch.clone(), &*config_clone_watcher) {
          Some(old) => old,
          None => return,
        };

      let new_config = config_clone_watcher.read().unwrap().clone();
      {
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
                  if let Some(cw) = cache.remove(name.as_str()) {
                    to_restore.push(cw);
                  }
                }
              }
              None => {
                // App removed - collect cached HWND and clear cache
                if let Some(cw) = cache.remove(name.as_str()) {
                  to_restore.push(cw);
                }
              }
            }
          }
        }
        // Sync the lock-free HWND cache after reload
        crate::windows::window::update_managed_hwnds_cache();
        // Cancel any ongoing animation before restoring
        release_windows(to_restore);
      }
      let _ = event_tx_watcher.send(DaemonEvent::ReloadHotkeys);
      post_wake_message(WM_USER + 1);
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
    &*hotkey_map,
    &*current_hotkeys,
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
    let candidates = Arc::new(fetch_system_windows());
    let cfg = config.read().unwrap().clone();
    for (i, (name, app_cfg)) in cfg.app.iter().enumerate() {
      if i > 0 {
        tokio::time::sleep(Duration::from_millis(200)).await;
      }
      let name = name.clone();
      let app_cfg = app_cfg.clone();
      let cfg_copy = cfg.clone();
      let candidates_clone = candidates.clone();
      tokio::task::spawn_blocking(move || {
        ensure_terminal_running(&name, &app_cfg, &cfg_copy, Some(&candidates_clone[..]));
      })
      .await
      .ok();
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
          post_wake_message(WM_USER + 1);
        });
      }
    }
  }

  // Threads to bridge receivers to DaemonEvent channel
  let event_tx_hk = event_tx.clone();
  std::thread::spawn(move || {
    while let Ok(event) = hotkey_receiver.recv() {
      let _ = event_tx_hk.send(DaemonEvent::Hotkey(event));
      post_wake_message(WM_USER + 1);
    }
  });

  let event_tx_menu = event_tx.clone();
  std::thread::spawn(move || {
    while let Ok(event) = menu_receiver.recv() {
      let _ = event_tx_menu.send(DaemonEvent::Menu(event));
      post_wake_message(WM_USER + 1);
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
      post_wake_message(WM_USER + 1);
    }
  });

  // Main Win32 Message Loop
  let mut last_tray_toggle = Instant::now() - Duration::from_secs(1);
  unsafe {
    let mut msg = MSG::default();
    while GetMessageW(&mut msg, None, 0, 0).as_bool() {
      if msg.message == WM_USER + 2 {
        if let Some(bridge) = crate::windows::window::BRIDGE_HWND.get() {
          let _ = SendMessageW(bridge.hwnd, WM_CANCELMODE, Some(WPARAM(0)), Some(LPARAM(0)));
        }
        let _ = event_tx.send(DaemonEvent::FocusLost);
      }

      // WM_USER + 3: Immediate respawn request from destroy hook
      if msg.message == WM_USER + 3 {
        let _ = event_tx.send(DaemonEvent::RespawnCheck);
      }

      let _ = TranslateMessage(&msg);
      DispatchMessageW(&msg);

      // Force the theme to re-sync based on current registry state
      if msg.message == WM_USER + 1 {
        apply_theme_preference();
      }

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
              &*hotkey_map,
              &*current_hotkeys,
            );

            // Re-apply theme in case system settings changed
            apply_theme_preference();

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
              if let Some(visible_app) = crate::windows::window::get_visible_app() {
                let cfg_clone = cfg.clone();
                let app_name = visible_app.to_string();
                tokio::task::spawn_blocking(move || {
                  // Final safety check: Only toggle if the window is STILL the visible app.
                  // If the user hotkeyed to hide, VISIBLE_APP is already None.
                  if crate::windows::window::get_visible_app().as_deref() == Some(&app_name) {
                    println!("Focus Lost: Auto-hiding '{}'", app_name);
                    let _ = AllowSetForegroundWindow(ASFW_ANY);
                    toggle_window(&app_name, &cfg_clone);
                  }
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
              if let Some(app_cfg) = cfg.app.get(&app_name) {
                let app_cfg = app_cfg.clone();
                let app_name_spawn = app_name.clone();
                let cfg_spawn = cfg.clone();
                tokio::task::spawn_blocking(move || {
                  ensure_terminal_running(&app_name_spawn, &app_cfg, &cfg_spawn, None);
                  toggle_window(&app_name_spawn, &cfg_spawn);
                });
              }
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

              let is_shift_pressed = {
                let shift_state = GetKeyState(VK_SHIFT.0 as i32);
                (shift_state as u16 & 0x8000) != 0
              };

              let _ = AllowSetForegroundWindow(ASFW_ANY);
              let cfg = config.read().unwrap().clone();

              if is_shift_pressed {
                print_shutdown_message("Quit via Shift + Left-click");
                restore_window_visibility();
                print_termination_complete();
                exit(0);
              } else {
                let target = crate::windows::window::get_visible_app()
                  .map(|a| a.to_string())
                  .or_else(|| cfg.app.keys().next().cloned());

                if let Some(app_name) = target {
                  if let Some(app_cfg) = cfg.app.get(&app_name) {
                    let app_cfg = app_cfg.clone();
                    let cfg_spawn = cfg.clone();
                    tokio::task::spawn_blocking(move || {
                      ensure_terminal_running(&app_name, &app_cfg, &cfg_spawn, None);
                      toggle_window(&app_name, &cfg_spawn);
                    });
                  }
                }
              }
            }
          }
          DaemonEvent::RespawnCheck => {
            let cfg = config.read().unwrap().clone();
            let mut missing_apps = Vec::new();

            for name in cfg.app.keys() {
              let needs_check = {
                let cache = get_app_cache().read().unwrap();
                let already_spawning = {
                  let spawning = get_spawning_apps().lock().unwrap();
                  spawning.contains(name)
                };

                if already_spawning {
                  false
                } else if let Some(hwnd) = cache.get(name.as_str()) {
                  let is_alive = IsWindow(Some(hwnd.hwnd)).as_bool();
                  if is_alive {
                    let mut pid = 0;
                    GetWindowThreadProcessId(hwnd.hwnd, Some(&mut pid));
                    !process::is_process_running(pid, None)
                  } else {
                    true
                  }
                } else {
                  true
                }
              };

              if needs_check {
                missing_apps.push(name.clone());
              }
            }

            if !missing_apps.is_empty() {
              let cfg_spawn = cfg.clone();
              tokio::task::spawn_blocking(move || {
                let candidates = fetch_system_windows();
                for name in missing_apps {
                  if let Some(app_cfg) = cfg_spawn.app.get(&name) {
                    println!("App '{}' not managed. Checking/Respawning...", name);
                    ensure_terminal_running(&name, app_cfg, &cfg_spawn, Some(&candidates[..]));
                  }
                }
                reset_visible_app();
                post_wake_message(WM_USER + 1);
              });
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

pub async fn send_toggle(app_name: Option<String>) -> error::Result<()> {
  use tokio::io::AsyncWriteExt;
  use tokio::net::windows::named_pipe::ClientOptions;

  let mut attempts = 0;
  let mut client = loop {
    match ClientOptions::new().open(PIPE_NAME) {
      Ok(c) => break c,
      Err(e) if attempts < 10 && e.raw_os_error() == Some(231) => {
        attempts += 1;
        tokio::time::sleep(Duration::from_millis(20)).await;
      }
      Err(e) => return Err(e.into()),
    }
  };

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
  hotkey_map: &RwLock<FxHashMap<u32, String>>,
  current_hotkeys: &RwLock<Vec<HotKey>>,
) -> janq::error::Result<()> {
  // 1. Identify what we WANT to have registered
  // We track desired state as a mapping of HotKeyID -> (AppName, OriginalString)
  let mut desired: FxHashMap<u32, (String, String)> = FxHashMap::default();
  let mut signature = Vec::new();

  for (app_name, app_cfg) in &config.app {
    for hk_str in app_cfg.hotkey.as_vec() {
      if hk_str.is_empty() {
        continue;
      }
      if let Ok(key) = parse_hotkey(&hk_str) {
        let normalized = janq::validation::canonicalize_hotkey(&hk_str);
        signature.push(format!("{}:{}", app_name, normalized));

        // Detect collisions within the desired set
        if let Some(existing_app) = desired.get(&key.id()) {
          if *existing_app != (app_name.clone(), normalized.clone()) {
            println!(
              "Hotkey Collision: '{}' for app '{}' is already taken by '{}'",
              hk_str, app_name, existing_app.0
            );
            continue;
          }
        }
        desired.insert(key.id(), (app_name.clone(), normalized));
      }
    }
  }
  signature.sort();

  // 2. Short-circuit if nothing in the configuration has changed
  static LAST_SYNC_SIG: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());
  {
    let mut last = LAST_SYNC_SIG.lock().unwrap();
    if *last == signature && !last.is_empty() {
      return Ok(());
    }
    *last = signature;
  }

  println!("Hotkey: Synchronizing Windows hotkeys...");

  // 3. Surgical Diff: Unregister hotkeys that are no longer wanted
  {
    let mut current = current_hotkeys.write().unwrap();
    let mut map = hotkey_map.write().unwrap();

    let to_remove: Vec<HotKey> = current
      .iter()
      .filter(|hk| !desired.contains_key(&hk.id()))
      .cloned()
      .collect();

    for hk in to_remove {
      let _ = manager.unregister(hk);
      current.retain(|h| h.id() != hk.id());
      map.remove(&hk.id());
    }

    // 4. Register new hotkeys or fallback variants
    for (id, (app_name, _)) in &desired {
      if !map.contains_key(id) {
        // Find the original key struct
        // (We re-parse here for simplicity, or we could have stored it in 'desired')
        // Find the hk_str that generated this ID
        if let Some(hk_str) = config.app[app_name]
          .hotkey
          .as_vec()
          .iter()
          .find(|s| parse_hotkey(s).map(|k| k.id() == *id).unwrap_or(false))
        {
          if let Ok(key) = parse_hotkey(hk_str) {
            let key_code = key.key;
            match manager.register(key) {
              Ok(_) => {
                map.insert(key.id(), app_name.clone());
                current.push(key);
              }
              Err(_) => {
                // Fallback for Section key (IntlBackslash -> Backquote -> Backslash)
                if key_code == global_hotkey::hotkey::Code::IntlBackslash {
                  let fallback_key =
                    HotKey::new(Some(key.mods), global_hotkey::hotkey::Code::Backquote);
                  if !map.contains_key(&fallback_key.id()) && manager.register(fallback_key).is_ok()
                  {
                    map.insert(fallback_key.id(), app_name.clone());
                    current.push(fallback_key);
                  } else {
                    let fallback_key2 =
                      HotKey::new(Some(key.mods), global_hotkey::hotkey::Code::Backslash);
                    if !map.contains_key(&fallback_key2.id())
                      && manager.register(fallback_key2).is_ok()
                    {
                      map.insert(fallback_key2.id(), app_name.clone());
                      current.push(fallback_key2);
                    } else {
                      show_warning(&format!("  ✗ Failed to register hotkey: {}", hk_str));
                    }
                  }
                } else {
                  show_warning(&format!("  ✗ Failed to register hotkey: {}", hk_str));
                }
              }
            }
          }
        }
      }
    }
  }

  crate::windows::window::update_managed_hwnds_cache();
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

fn is_dark_mode() -> bool {
  use windows::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_DWORD};
  let mut data = 0u32;
  let mut size = std::mem::size_of::<u32>() as u32;
  unsafe {
    let path =
      windows::core::w!("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize");
    let name = windows::core::w!("AppsUseLightTheme");

    if RegGetValueW(
      HKEY_CURRENT_USER,
      path,
      name,
      RRF_RT_REG_DWORD,
      None,
      Some(&mut data as *mut _ as *mut _),
      Some(&mut size),
    )
    .is_ok()
    {
      return data == 0;
    }
  }
  false
}

fn apply_theme_preference() {
  use windows::core::PCSTR;
  use windows::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};

  unsafe {
    if let Ok(uxtheme) = GetModuleHandleW(windows::core::w!("uxtheme.dll")) {
      // SetPreferredAppMode (Ordinal 135)
      if let Some(set_preferred_ptr) = GetProcAddress(uxtheme, PCSTR(135 as *const u8)) {
        let set_preferred_app_mode: unsafe extern "system" fn(i32) -> i32 =
          std::mem::transmute(set_preferred_ptr);
        let mode = if is_dark_mode() { 2 } else { 3 };
        let _ = set_preferred_app_mode(mode);
      }

      // FlushMenuThemes (Ordinal 136)
      if let Some(flush_ptr) = GetProcAddress(uxtheme, PCSTR(136 as *const u8)) {
        let flush_menu_themes: unsafe extern "system" fn() -> i32 = std::mem::transmute(flush_ptr);
        let _ = flush_menu_themes();
      }
    }
  }
}
