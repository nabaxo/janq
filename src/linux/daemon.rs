//! Linux daemon implementation using D-Bus for IPC and system integration.
//!
//! ## Architecture
//!
//! The janq daemon on Linux exposes multiple D-Bus interfaces:
//!
//! 1. **`org.freedesktop.Application`** (`QuakeApplication`)
//!    - Standard desktop activation interface
//!    - Receives hotkey triggers from KDE's KGlobalAccel via desktop actions
//!    - `activate_action(name)` is called when user presses registered hotkey
//!
//! 2. **`dev.nabaxo.janq`** (`QuakeDaemon`)
//!    - Custom janq interface for direct IPC
//!    - `Toggle()` / `ToggleApp(name)` - toggles window visibility
//!    - `ReportWindowMetadata()` / `ReportActiveWindow()` - callbacks from KWin scripts
//!
//! 3. **`org.kde.StatusNotifierItem`** (`StatusNotifierItem`)
//!    - System tray integration for KDE Plasma
//!    - Left-click: toggle first app
//!    - Middle-click: quit daemon
//!
//! ## Config Hot-Reloading
//!
//! A separate thread watches the config file using `notify` crate:
//! - Debounces rapid changes (500ms window)
//! - On valid reload: updates apps, re-grabs windows, syncs shortcuts
//! - On invalid config: restores all windows and exits (fail-fast)
//!
//! ## Signal Handling
//!
//! Gracefully handles SIGINT/SIGTERM by restoring all managed windows
//! to their normal state before exit.

use std::{
  collections::HashMap, // Required for D-Bus HashMap parameter types
  path::PathBuf,
  process::exit,
  sync::{Arc, RwLock},
};

use tokio::{
  signal::unix::{signal, SignalKind},
  time::{sleep, Duration},
};
use zbus::{interface, names::BusName, names::InterfaceName, zvariant::OwnedValue, Connection};

#[cfg(feature = "systray")]
use ksni::menu::*;
#[cfg(feature = "systray")]
use ksni::{self, MenuItem, Tray, TrayMethods};
#[cfg(not(feature = "systray"))]
use std::process::id;

use crate::linux::desktop::{generate_desktop_file, generate_desktop_file_headless};
use crate::linux::hotkey::sync_kde_shortcuts;
use crate::linux::kwin::{
  clear_removed_apps_from_cache, get_visible_app, grab_apps, init as init_kwin, reset_visibility,
  restore_app, restore_quake, toggle_quake,
};
use crate::linux::terminal::{
  ensure_terminal_running, ensure_terminal_running_with_candidates, fetch_system_windows_async,
};
use janq::config::Config;
use janq::error::show_error;
use janq::shutdown::{print_shutdown_message, print_termination_complete};

// =============================================================================
// D-Bus Interfaces
// =============================================================================

/// D-Bus Application interface for desktop activation.
///
/// Implements `org.freedesktop.Application` to receive activation signals
/// from KDE when the user triggers a registered shortcut.
#[derive(Clone)]
struct QuakeApplication {
  config: Arc<RwLock<Config>>,
  conn: Connection,
}

/// D-Bus interface for janq-specific IPC.
///
/// Exposes janq's toggle functionality to external callers and receives
/// callbacks from KWin scripts reporting window metadata.
#[derive(Clone)]
struct QuakeDaemon {
  config: Arc<RwLock<Config>>,
  conn: Connection,
}

#[interface(name = "org.freedesktop.Application")]
impl QuakeApplication {
  async fn activate(&self, _platform_data: HashMap<String, OwnedValue>) {
    // No-op: Satisfaction of D-Bus Application activation.
    // Clicking the launcher icon should only start the background process, not toggle a window.
  }

  async fn activate_action(
    &self,
    action_name: String,
    _parameter: Vec<OwnedValue>,
    _platform_data: HashMap<String, OwnedValue>,
  ) {
    // D-Bus activation log for visibility
    println!("D-Bus: Activating action '{}'", action_name);

    // Ensure the app is running before toggling (critical for D-Bus activation)
    let config = { self.config.read().unwrap().clone() };
    if let Some(app_cfg) = config.app.get(&action_name) {
      let _ = ensure_terminal_running(app_cfg, &config, &self.conn).await;
    }

    let daemon = QuakeDaemon {
      config: self.config.clone(),
      conn: self.conn.clone(),
    };
    daemon.toggle_app(action_name).await;
  }

  fn open(&self, _uris: Vec<String>, _platform_data: HashMap<String, OwnedValue>) {
    // Not used
  }
}

#[interface(name = "dev.nabaxo.janq")]
impl QuakeDaemon {
  #[zbus(name = "Toggle")]
  async fn toggle(&self) {
    let config = { self.config.read().unwrap().clone() };
    if let Some(app_name) = config.app.keys().next() {
      let _ = toggle_quake(app_name, &config, &self.conn).await;
    }
  }

  #[zbus(name = "ToggleApp")]
  async fn toggle_app(&self, app_name: String) {
    let config = { self.config.read().unwrap().clone() };
    let apps = &config.app;

    let target = if apps.len() == 1 {
      apps.keys().next().cloned()
    } else if apps.contains_key(&app_name) {
      Some(app_name)
    } else {
      None
    };

    if let Some(target_name) = target {
      let _ = toggle_quake(&target_name, &config, &self.conn).await;
    }
  }

  #[zbus(name = "ReportWindowMetadata")]
  async fn report_window_metadata(&self, payload: String) {
    crate::linux::terminal::report_metadata(payload).await;
  }

  #[zbus(name = "ReportActiveWindow")]
  async fn report_active_window(&self, payload: String) {
    crate::linux::kwin::report_active_window(payload).await;
  }
}

#[cfg(not(feature = "systray"))]
struct StatusNotifierItem {
  config: Arc<RwLock<Config>>,
  icon_cache: IconPixmap,
  conn: Connection,
}

#[cfg(not(feature = "systray"))]
type IconPixmap = Vec<(i32, i32, Vec<u8>)>;

#[cfg(not(feature = "systray"))]
#[interface(name = "org.kde.StatusNotifierItem")]
impl StatusNotifierItem {
  fn activate(&self, _x: i32, _y: i32) {
    let config = self.config.read().unwrap().clone();
    let conn = self.conn.clone();
    tokio::spawn(async move {
      let app_name = config.app.keys().next();
      if let Some(name) = app_name {
        let _ = toggle_quake(name, &config, &conn).await;
      }
    });
  }

  fn secondary_activate(&self, _x: i32, _y: i32) {
    let config = self.config.read().unwrap().clone();
    let conn = self.conn.clone();
    tokio::spawn(async move {
      print_shutdown_message("Quit via systray");
      let _ = restore_quake(&config, &conn).await;
      print_termination_complete();
      exit(0);
    });
  }

  #[zbus(property)]
  fn category(&self) -> String {
    "ApplicationStatus".to_string()
  }
  #[zbus(property)]
  fn id(&self) -> String {
    "janq".to_string()
  }
  #[zbus(property)]
  fn title(&self) -> String {
    "janq".to_string()
  }
  #[zbus(property)]
  fn status(&self) -> String {
    "Active".to_string()
  }
  #[zbus(property)]
  fn icon_name(&self) -> String {
    "janq".to_string()
  }
  #[zbus(property)]
  fn icon_pixmap(&self) -> IconPixmap {
    self.icon_cache.clone()
  }
  #[zbus(property)]
  fn item_is_menu(&self) -> bool {
    false
  }
}

#[cfg(feature = "systray")]
struct JanqTray {
  config: Arc<RwLock<Config>>,
  conn: Connection,
}

#[cfg(feature = "systray")]
impl Tray for JanqTray {
  fn activate(&mut self, _x: i32, _y: i32) {
    let config = self.config.read().unwrap().clone();
    let conn = self.conn.clone();
    tokio::spawn(async move {
      let target = get_visible_app()
        .await
        .or_else(|| config.app.keys().next().cloned());

      if let Some(app_name) = target {
        let _ = toggle_quake(&app_name, &config, &conn).await;
      }
    });
  }

  fn secondary_activate(&mut self, _x: i32, _y: i32) {
    let config = self.config.read().unwrap().clone();
    let conn = self.conn.clone();
    tokio::spawn(async move {
      print_shutdown_message("Quit via middle-click");
      let _ = restore_quake(&config, &conn).await;
      print_termination_complete();
      exit(0);
    });
  }

  fn category(&self) -> ksni::Category {
    ksni::Category::ApplicationStatus
  }

  fn id(&self) -> String {
    "janq".into()
  }

  fn title(&self) -> String {
    "janq".into()
  }

  fn icon_name(&self) -> String {
    "janq".into()
  }

  fn menu(&self) -> Vec<MenuItem<Self>> {
    let config = self.config.read().unwrap().clone();
    let mut items = Vec::new();

    for name in config.app.keys() {
      let name = name.clone();
      let config = config.clone();
      let conn = self.conn.clone();

      // 1. Get and normalize the shortcut using hotkey.rs logic
      let hotkeys = config.app.get(&name).unwrap().hotkey.as_vec();
      let (shortcut_vec, normalized_str) = if !hotkeys.is_empty() {
        let normalized = crate::linux::hotkey::normalize_shortcut_for_kde(&hotkeys[0]);

        // Just split and trim, no more "Super" or "Control" hardcoding
        let parts: Vec<String> = normalized
          .split('+')
          .map(|part| part.trim().to_string())
          .collect();
        (vec![parts], normalized)
      } else {
        (vec![], String::new())
      };

      // 2. THE NBSP TRICK
      // Use normalized_str to calculate padding so the menu width is consistent.
      let name_len = name.chars().count();
      let shortcut_len = normalized_str.chars().count();

      // Calculate NBSP count to shove the greyed-out shortcut to the right.
      let padding_count = 20_usize.saturating_sub(name_len + shortcut_len).max(5);
      let label = format!("{}{}", name, "\u{00A0}".repeat(padding_count));

      items.push(
        StandardItem {
          label,
          shortcut: shortcut_vec, // Native greyed-out look
          activate: Box::new(move |_| {
            let name = name.clone();
            let config = config.clone();
            let conn = conn.clone();
            tokio::spawn(async move {
              let _ = toggle_quake(&name, &config, &conn).await;
            });
          }),
          ..Default::default()
        }
        .into(),
      );
    }

    items.push(MenuItem::Separator);

    let config = config.clone();
    let conn = self.conn.clone();
    items.push(
      StandardItem {
        label: "Quit".into(),
        activate: Box::new(move |_| {
          let config = config.clone();
          let conn = conn.clone();
          tokio::spawn(async move {
            print_shutdown_message("Quit via systray");
            let _ = restore_quake(&config, &conn).await;
            print_termination_complete();
            exit(0);
          });
        }),
        ..Default::default()
      }
      .into(),
    );

    items
  }
}

pub async fn run_daemon(
  initial_config: Config,
  config_path: Option<PathBuf>,
  target_app: Option<String>,
) -> janq::error::Result<()> {
  // 0. Acquire Lock File
  let _lock_file = janq::acquire_lock_file()?;

  println!("Starting janq daemon...");
  init_kwin().await;
  let config = Arc::new(RwLock::new(initial_config));
  let conn = zbus::connection::Builder::session()?
    .internal_executor(false)
    .build()
    .await?;

  #[cfg(not(feature = "systray"))]
  let pid = id();
  #[cfg(not(feature = "systray"))]
  let sni_name = format!("org.kde.StatusNotifierItem-janq-{}", pid);
  #[cfg(not(feature = "systray"))]
  conn.request_name(sni_name.clone()).await?;

  #[cfg(not(feature = "systray"))]
  // Empty pixmap forces KDE to use icon_name (SVG from icon theme)
  let icon_cache: IconPixmap = vec![];

  #[cfg(not(feature = "systray"))]
  let sni = StatusNotifierItem {
    config: config.clone(),
    icon_cache,
    conn: conn.clone(),
  };
  #[cfg(not(feature = "systray"))]
  conn.object_server().at("/StatusNotifierItem", sni).await?;

  #[cfg(feature = "systray")]
  let tray_handle = JanqTray {
    config: config.clone(),
    conn: conn.clone(),
  }
  .spawn()
  .await
  .expect("Failed to spawn tray");

  let activatable_bus = "dev.nabaxo.janq";
  let activatable_path = "/dev/nabaxo/janq";
  let xdg_path = "/org/freedesktop/Application/dev/nabaxo/janq";
  let daemon_path = "/dev/nabaxo/janq/daemon";

  let app_instance = QuakeApplication {
    config: config.clone(),
    conn: conn.clone(),
  };
  let daemon_instance = QuakeDaemon {
    config: config.clone(),
    conn: conn.clone(),
  };

  for path in &[activatable_path, xdg_path, daemon_path] {
    let r1 = conn.object_server().at(*path, app_instance.clone()).await;
    let r2 = conn
      .object_server()
      .at(*path, daemon_instance.clone())
      .await;

    if let Err(e) = r1 {
      eprintln!(
        "janq: Failed to register Application interface at {}: {}",
        path, e
      );
    }
    if let Err(e) = r2 {
      eprintln!(
        "janq: Failed to register Daemon interface at {}: {}",
        path, e
      );
    }
  }

  conn.request_name(activatable_bus).await?;
  let _ = conn.request_name("dev.nabaxo.janq.desktop").await;

  #[cfg(not(feature = "systray"))]
  let _ = conn
    .call_method(
      Some(BusName::try_from("org.kde.StatusNotifierWatcher").unwrap()),
      "/StatusNotifierWatcher",
      Some(InterfaceName::try_from("org.kde.StatusNotifierWatcher").unwrap()),
      "RegisterStatusNotifierItem",
      &(sni_name),
    )
    .await;

  // --- KDE Platform Integration ---
  {
    let cfg = config.read().unwrap().clone();
    let _ = generate_desktop_file(&cfg);
    let _ = crate::linux::kwin::sync_kwin_rules(&cfg);
    tokio::spawn(async move {
      let _ = sync_kde_shortcuts(&cfg, None).await;
    });
  }

  // Small delay to ensure D-Bus service is fully registered before KWin scripts call back
  sleep(Duration::from_millis(100)).await;

  // Initial setup (Parallel)
  {
    let cfg = config.read().unwrap().clone();
    let initial_candidates = Arc::new(fetch_system_windows_async().await);

    let mut terminal_tasks = Vec::new();
    for name in cfg.app.keys() {
      if let Some(app_cfg) = cfg.app.get(name) {
        let app_cfg_owned = app_cfg.clone();
        let cfg_clone = cfg.clone();
        let conn_clone = conn.clone();
        let candidates_clone = initial_candidates.clone();

        terminal_tasks.push(tokio::spawn(async move {
          let _ = ensure_terminal_running_with_candidates(
            &app_cfg_owned,
            &cfg_clone,
            &conn_clone,
            Some(&candidates_clone[..]),
          )
          .await;
        }));
      }
    }

    for task in terminal_tasks {
      let _ = task.await;
    }

    // Grabbing apps (now using the pre-fetched list is too old, grab_apps will do its own scan if needed)
    let mut apps_for_grabbing = Vec::new();
    for (_name, app_cfg) in &cfg.app {
      apps_for_grabbing.push((app_cfg, &cfg));
    }
    let _ = grab_apps(&apps_for_grabbing, &conn).await;

    if cfg.window.auto_show {
      let app_to_show = target_app.as_ref();
      if let Some(app_name) = app_to_show {
        if let Some(_app_cfg) = cfg.app.get(app_name) {
          println!("janq: Auto-showing requested app: {}", app_name);
          sleep(Duration::from_millis(500)).await;
          let _ = toggle_quake(app_name, &cfg, &conn).await;
        }
      } else if let Some(first_app) = cfg.app.keys().next() {
        println!("janq: Auto-showing first app: {}", first_app);
        sleep(Duration::from_millis(500)).await;
        let _ = toggle_quake(first_app, &cfg, &conn).await;
      }
    }
  }

  // Config Watcher (Thread)
  let config_for_watcher = config.clone();
  let conn_for_watcher = conn.clone();
  let path_to_watch = config_path.clone();
  #[cfg(feature = "systray")]
  let tray_handle_for_watcher = tray_handle.clone();

  janq::config_watcher::spawn_config_watcher(path_to_watch.clone(), move || {
    let path_to_watch = path_to_watch.clone();
    let config_for_watcher = config_for_watcher.clone();
    let conn_for_watcher = conn_for_watcher.clone();
    #[cfg(feature = "systray")]
    let tray_handle = tray_handle_for_watcher.clone();

    async move {
      let old_config = match janq::config_watcher::reload_shared_config(
        path_to_watch.clone(),
        &*config_for_watcher,
      ) {
        Some(old) => old,
        None => return,
      };

      let new_config = config_for_watcher.read().unwrap().clone();

      // Notify tray immediately that config state changed
      #[cfg(feature = "systray")]
      let _ = tray_handle.update(|_| {}).await;

      let conn_in_async = conn_for_watcher.clone();
      let new_config_in_async = new_config; // Removed .clone() since we already cloned from read()

      println!("Watcher: Starting/Restoring apps as needed...");

      // 1. Restore removed or changed apps
      for (name, old_app_cfg) in &old_config.app {
        match new_config_in_async.app.get(name) {
          Some(new_app_cfg) => {
            if new_app_cfg.window_class != old_app_cfg.window_class {
              println!("Watcher: Restoring app '{}' (class changed)", name);
              let _ = restore_app(name, &old_app_cfg.window_class, &conn_in_async).await;
            }
          }
          None => {
            println!("Watcher: Restoring app '{}' (removed from config)", name);
            let _ = restore_app(name, &old_app_cfg.window_class, &conn_in_async).await;
          }
        }
      }

      // 1.5. Clear cache entries for removed apps
      clear_removed_apps_from_cache(&old_config, &new_config_in_async);

      reset_visibility(&new_config_in_async).await;

      // Sync KWin Rules ALWAYS on any valid reload to ensure icons stay fresh
      let _ = crate::linux::kwin::sync_kwin_rules(&new_config_in_async);

      // 2. Ensure all terminals are running and grabbed
      let mut apps_for_grabbing = Vec::new();
      for (name, app_cfg) in &new_config_in_async.app {
        if !old_config.app.contains_key(name) {
          println!("Watcher: New app detected: {}. Starting terminal...", name);
        }
        // We ensure terminal is running for ALL apps (in case one crashed)
        let _ = ensure_terminal_running(app_cfg, &new_config_in_async, &conn_in_async).await;
        apps_for_grabbing.push((app_cfg, &new_config_in_async));
      }
      let _ = grab_apps(&apps_for_grabbing, &conn_in_async).await;

      // 3. Update desktop file (don't run kbuild inside, we'll do it last)
      let desktop_changed = match generate_desktop_file_headless(&new_config_in_async) {
        Ok(changed) => changed,
        Err(e) => {
          eprintln!("Watcher: Desktop file generation failed: {}", e);
          false
        }
      };

      // 4. Check if hotkeys changed
      let mut hotkeys_changed = false;
      if old_config.app.len() != new_config_in_async.app.len()
        || old_config.app.keys().ne(new_config_in_async.app.keys())
      {
        hotkeys_changed = true;
      } else {
        for (name, old_app) in &old_config.app {
          if let Some(new_app) = new_config_in_async.app.get(name) {
            if old_app.hotkey != new_app.hotkey || old_app.window_class != new_app.window_class {
              hotkeys_changed = true;
              break;
            }
          } else {
            hotkeys_changed = true;
            break;
          }
        }
      }

      if hotkeys_changed || desktop_changed {
        println!("Config: Shortcuts or Desktop entries changed, synchronizing with KDE...");
        let _ = crate::linux::kwin::sync_kwin_rules(&new_config_in_async);
        if let Err(e) = sync_kde_shortcuts(&new_config_in_async, Some(&old_config)).await {
          show_error(&format!("Watcher: Failed to sync shortcuts: {}", e));
        }
      } else {
        println!("Config: No shortcut/desktop changes detected.");
      }
    }
  });

  let config_for_signals = config.clone();
  let conn_for_signals = conn.clone();

  let mut sigint = signal(SignalKind::interrupt())?;
  let mut sigterm = signal(SignalKind::terminate())?;

  tokio::select! {
      _ = sigint.recv() => print_shutdown_message("Received SIGINT"),
      _ = sigterm.recv() => print_shutdown_message("Received SIGTERM"),
  }

  let cfg = config_for_signals.read().unwrap().clone();
  let _ = restore_quake(&cfg, &conn_for_signals).await;
  // Ensure scripts have time to finish before process exit
  tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
  print_termination_complete();

  Ok(())
}

pub async fn send_toggle(app_name: Option<String>) -> janq::error::Result<()> {
  let conn = zbus::connection::Builder::session()?
    .internal_executor(false)
    .build()
    .await?;
  if let Some(name) = app_name {
    conn
      .call_method(
        Some(BusName::try_from("dev.nabaxo.janq.desktop").unwrap()),
        "/dev/nabaxo/janq/daemon",
        Some(InterfaceName::try_from("dev.nabaxo.janq").unwrap()),
        "ToggleApp",
        &(name),
      )
      .await?;
  } else {
    conn
      .call_method(
        Some(BusName::try_from("dev.nabaxo.janq.desktop").unwrap()),
        "/dev/nabaxo/janq/daemon",
        Some(InterfaceName::try_from("dev.nabaxo.janq").unwrap()),
        "Toggle",
        &(),
      )
      .await?;
  }
  Ok(())
}
