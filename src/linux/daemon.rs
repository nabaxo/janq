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

use std::process::id;

use crate::linux::desktop::{generate_desktop_file, generate_desktop_file_headless};
use crate::linux::hotkey::sync_kde_shortcuts;

use crate::linux::kwin::{
  clear_removed_apps_from_cache, detect_refresh_rate, grab_apps, init as init_kwin,
  purge_stale_scripts, recover_all, report_active_window as kwin_report_active, reset_state,
  reset_visibility, restore_app, restore_quake, sync_kwin_rules, toggle_quake,
};
use crate::linux::terminal::{
  ensure_terminal_running, report_metadata as terminal_report_metadata,
};

use janq::config::Config;
use janq::config_watcher;
use janq::error::{self, show_error};
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

const RE_GRAB_TIME: u64 = 2000;

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
      let _ = ensure_terminal_running(&action_name, app_cfg, &config, &self.conn).await;
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
    terminal_report_metadata(payload).await;
  }

  #[zbus(name = "ReportActiveWindow")]
  async fn report_active_window(&self, payload: String) {
    kwin_report_active(payload).await;
  }

  #[zbus(name = "Recover")]
  async fn recover(&self) {
    let config = { self.config.read().unwrap().clone() };
    recover_all(&config, &self.conn).await;
  }

  #[zbus(name = "Quit")]
  async fn quit(&self) {
    print_shutdown_message("Quit via IPC");
    let config = { self.config.read().unwrap().clone() };
    let _ = restore_quake(&config, &self.conn).await;
    print_termination_complete();
    exit(0);
  }
}

struct StatusNotifierItem {
  config: Arc<RwLock<Config>>,
  conn: Connection,
}

#[interface(name = "org.kde.StatusNotifierItem")]
impl StatusNotifierItem {
  fn activate(&self, _x: i32, _y: i32) {
    let config = self.config.read().unwrap().clone();
    let conn = self.conn.clone();
    tokio::spawn(async move {
      let target = crate::linux::kwin::get_visible_app()
        .await
        .map(|a| a.to_string())
        .or_else(|| config.app.keys().next().cloned());

      if let Some(name) = target {
        let _ = toggle_quake(&name, &config, &conn).await;
      }
    });
  }

  fn secondary_activate(&self, _x: i32, _y: i32) {
    let config = self.config.read().unwrap().clone();
    let conn = self.conn.clone();
    tokio::spawn(async move {
      print_shutdown_message("Quit via middle-click");
      let _ = restore_quake(&config, &conn).await;
      print_termination_complete();
      exit(0);
    });
  }

  #[zbus(property)]
  fn category(&self) -> String {
    "ApplicationStatus".into()
  }
  #[zbus(property)]
  fn id(&self) -> String {
    "janq".into()
  }
  #[zbus(property)]
  fn title(&self) -> String {
    "janq".into()
  }
  #[zbus(property)]
  fn status(&self) -> String {
    "Active".into()
  }
  #[zbus(property)]
  fn icon_name(&self) -> String {
    // Route to the symbolic or color SVG by name — Plasma resolves these from
    // the hicolor theme installed by install_icon().
    // "janq-symbolic" ends in -symbolic so Plasma applies CSS recoloring.
    // "janq-color" has no -symbolic counterpart so Plasma never auto-substitutes.
    let dark = crate::linux::icon::is_dark_theme();
    if self.config.read().unwrap().window.wants_mono(dark) {
      "janq-symbolic".into()
    } else {
      "janq-color".into()
    }
  }
  #[zbus(property)]
  fn icon_pixmap(&self) -> Vec<(i32, i32, Vec<u8>)> {
    // Icon is served entirely via IconName — pixmap is always empty.
    Vec::new()
  }
  #[zbus(property)]
  fn item_is_menu(&self) -> bool {
    false
  }
  #[zbus(property)]
  fn menu(&self) -> zbus::zvariant::ObjectPath<'_> {
    zbus::zvariant::ObjectPath::try_from("/MenuBar").unwrap()
  }

  /// Signal Plasma to re-query the icon from the theme.
  #[zbus(signal)]
  async fn new_icon(ctxt: &zbus::object_server::SignalEmitter<'_>) -> zbus::Result<()>;
}

impl StatusNotifierItem {
  /// Emit NewIcon on the SNI object so Plasma refreshes the tray icon.
  ///
  /// We also explicitly emit PropertiesChanged for both IconName and IconPixmap
  /// because the `mono_icon = true` path keeps IconName empty on both sides of
  /// a theme change — without a pixmap_changed nudge, Plasma has no reason to
  /// re-read the property and the recolor stays stale.
  pub async fn emit_new_icon(conn: &Connection) {
    let iface_ref = conn
      .object_server()
      .interface::<_, StatusNotifierItem>("/StatusNotifierItem")
      .await;
    if let Ok(iface) = iface_ref {
      let ctxt = iface.signal_emitter();
      let sni = iface.get().await;
      let _ = sni.icon_name_changed(ctxt).await;
      let _ = sni.icon_pixmap_changed(ctxt).await;
      let _ = StatusNotifierItem::new_icon(&ctxt).await;
    }
  }
}

/// Register our StatusNotifierItem with KDE's StatusNotifierWatcher.
/// Logs the result rather than crashing — the watcher may not be available yet.
async fn register_sni(conn: &Connection, sni_name: &str) {
  match conn
    .call_method(
      Some(BusName::try_from("org.kde.StatusNotifierWatcher").unwrap()),
      "/StatusNotifierWatcher",
      Some(InterfaceName::try_from("org.kde.StatusNotifierWatcher").unwrap()),
      "RegisterStatusNotifierItem",
      &(sni_name),
    )
    .await
  {
    Ok(_) => println!("Tray: Registered with StatusNotifierWatcher"),
    Err(e) => eprintln!("Tray: Failed to register with StatusNotifierWatcher: {}", e),
  }
}

fn spawn_theme_watcher(conn: &Connection) {
  let conn_for_theme = conn.clone();
  tokio::spawn(async move {
    let mut rx = match crate::linux::icon::watch_kdeglobals() {
      Some(r) => r,
      None => return,
    };
    while rx.recv().await.is_some() {
      if crate::linux::icon::refresh_theme_cache() {
        StatusNotifierItem::emit_new_icon(&conn_for_theme).await;
      }
    }
  });
}

pub async fn run_daemon(
  initial_config: Config,
  config_path: Option<PathBuf>,
  target_app: Option<String>,
) -> error::Result<()> {
  println!("Starting janq daemon (PID {})...", std::process::id());
  init_kwin(&initial_config).await;
  let config = Arc::new(RwLock::new(initial_config));
  let conn = zbus::connection::Builder::session()?
    .internal_executor(false)
    .build()
    .await?;

  // Install icon BEFORE registering SNI so Plasma's QIconLoader can
  // resolve icon names from the hicolor theme on its first query.
  // Without this, Plasma caches a negative lookup for the session.
  // If any icon file was newly written, flush KDE's icon cache now so that
  // the `janq-color` and `janq-symbolic` names are discoverable immediately.
  if crate::linux::desktop::install_icon().unwrap_or(false) {
    crate::linux::desktop::run_kbuildsycoca6();
  }

  let pid = id();
  let sni_name = format!("org.kde.StatusNotifierItem-janq-{}", pid);
  conn.request_name(sni_name.clone()).await?;

  let sni = StatusNotifierItem {
    config: config.clone(),
    conn: conn.clone(),
  };
  conn.object_server().at("/StatusNotifierItem", sni).await?;

  // Register dbusmenu service for right-click menu
  {
    use crate::linux::tray::DbusmenuService;
    use std::sync::atomic::AtomicU32;
    let dbusmenu = DbusmenuService {
      config: config.clone(),
      conn: conn.clone(),
      revision: AtomicU32::new(1),
    };
    conn.object_server().at("/MenuBar", dbusmenu).await?;
  }

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
      show_error(&format!(
        "janq: Failed to register Application interface at {}: {}",
        path, e
      ));
    }
    if let Err(e) = r2 {
      show_error(&format!(
        "janq: Failed to register Daemon interface at {}: {}",
        path, e
      ));
    }
  }

  conn.request_name(activatable_bus).await?;
  let _ = conn.request_name("dev.nabaxo.janq.desktop").await;

  register_sni(&conn, &sni_name).await;

  // --- KDE Platform Integration ---
  {
    let cfg = config.read().unwrap().clone();
    let _ = generate_desktop_file(&cfg);
    let _ = sync_kwin_rules(&cfg);
    let conn_for_shortcuts = conn.clone();
    tokio::spawn(async move {
      let _ = sync_kde_shortcuts(&cfg, None, &conn_for_shortcuts).await;
    });
  }

  // Nudge Plasma to re-query the icon in case it resolved before the SVG
  // was on disk or its theme cache was stale from a prior negative lookup.
  StatusNotifierItem::emit_new_icon(&conn).await;

  // Monitor StatusNotifierWatcher for restarts and re-register our SNI.
  // When plasmashell restarts (crash, panel reconfigure, theme change, etc.),
  // the watcher forgets all registered items. We detect reappearance via the
  // NameOwnerChanged signal and re-register so the icon comes back automatically.
  {
    let conn_for_sni_watch = conn.clone();
    let sni_name_for_sni_watch = sni_name.clone();
    let config_for_sni_watch = config.clone();
    tokio::spawn(async move {
      use zbus::export::ordered_stream::OrderedStreamExt as _;
      use zbus::fdo::DBusProxy;

      let mut retry_count = 0;
      loop {
        let run_result: zbus::Result<()> = async {
          let dbus_proxy = DBusProxy::new(&conn_for_sni_watch).await?;

          // Pre-filtered stream: only wakes for org.kde.plasmashell name changes.
          // This name is more definitive for a shell restart than the SNI watcher itself.
          let mut stream = dbus_proxy
            .receive_name_owner_changed_with_args(&[(0, "org.kde.plasmashell")])
            .await?;

          while let Some(signal) = stream.next().await {
            if let Ok(args) = signal.args() {
              // new_owner is non-empty when the watcher has (re)appeared.
              let new_owner_present = args
                .new_owner()
                .as_ref()
                .map(|o| !o.as_str().is_empty())
                .unwrap_or(false);

              if new_owner_present {
                println!("Plasma: Shell restarted, re-registering tray and re-grabbing windows...");
                register_sni(&conn_for_sni_watch, &sni_name_for_sni_watch).await;
                StatusNotifierItem::emit_new_icon(&conn_for_sni_watch).await;

                // Plasmashell restart can reset window state — re-grab all managed windows.
                // We wait 2s to ensure the shell has finished its initial window configuration.
                tokio::time::sleep(tokio::time::Duration::from_millis(RE_GRAB_TIME)).await;
                println!("Plasma: Re-grabbing managed windows now...");
                reset_state().await;
                let cfg = config_for_sni_watch.read().unwrap().clone();
                let mut apps_for_grabbing = Vec::new();
                for (_name, app_cfg) in &cfg.app {
                  apps_for_grabbing.push((app_cfg, &cfg));
                }
                let _ = grab_apps(&apps_for_grabbing, &conn_for_sni_watch).await;
                println!("Plasma: Recovery complete.");
              }
            }
          }
          Ok(())
        }
        .await;

        if let Err(e) = run_result {
          if retry_count < janq::MAX_RETRY_COUNT {
            eprintln!(
              "Tray: StatusNotifierWatcher monitor failed: {}. Retrying in 5s...",
              e
            );
            retry_count += 1;
            tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
          } else {
            let msg = format!(
              "System Tray Monitor failed persistently: {}\n\njanq will now exit. Please try to manually restart janq.",
              e
            );
            crate::error::show_error(&msg);
            std::process::exit(1);
          }
        } else {
          // Normal exit (if stream ends unexpectedly but without error)
          break;
        }
      }
    });
  }

  // Monitor KWin for restarts (sleep/wake, manual kwin_wayland --replace, etc.)
  // When KWin restarts, all window state (opacity, blur, properties) is destroyed.
  // We detect reappearance via NameOwnerChanged and re-grab all managed windows.
  {
    let conn_for_kwin_watch = conn.clone();
    let config_for_kwin_watch = config.clone();
    tokio::spawn(async move {
      use zbus::export::ordered_stream::OrderedStreamExt as _;
      use zbus::fdo::DBusProxy;

      let mut retry_count = 0;
      loop {
        let run_result: zbus::Result<()> = async {
          let dbus_proxy = DBusProxy::new(&conn_for_kwin_watch).await?;

          let mut stream = dbus_proxy
            .receive_name_owner_changed_with_args(&[(0, "org.kde.KWin")])
            .await?;

          while let Some(signal) = stream.next().await {
            if let Ok(args) = signal.args() {
              let new_owner_present = args
                .new_owner()
                .as_ref()
                .map(|o| !o.as_str().is_empty())
                .unwrap_or(false);

              if new_owner_present {
                println!("KWin: Compositor restarted, re-grabbing all managed windows...");
                // Let KWin fully initialize its scripting engine
                tokio::time::sleep(tokio::time::Duration::from_millis(RE_GRAB_TIME)).await;

                reset_state().await;

                let cfg = config_for_kwin_watch.read().unwrap().clone();
                let mut apps_for_grabbing = Vec::new();
                for (_name, app_cfg) in &cfg.app {
                  apps_for_grabbing.push((app_cfg, &cfg));
                }
                let _ = grab_apps(&apps_for_grabbing, &conn_for_kwin_watch).await;
                println!("KWin: Re-grab complete.");
              }
            }
          }
          Ok(())
        }
        .await;

        if let Err(e) = run_result {
          if retry_count < janq::MAX_RETRY_COUNT {
            eprintln!("KWin: Compositor monitor failed: {}. Retrying in 5s...", e);
            retry_count += 1;
            tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
          } else {
            eprintln!(
              "KWin: Compositor monitor failed persistently: {}. Giving up.",
              e
            );
            break;
          }
        } else {
          break;
        }
      }
    });
  }

  // Monitor logind for sleep/wake cycles.
  // After resume, KWin may have reset window state — re-grab all managed windows.
  // Also re-register the SNI with the StatusNotifierWatcher, which may have dropped
  // our registration during the suspend/resume cycle.
  {
    let conn_for_sleep = conn.clone();
    let config_for_sleep = config.clone();
    let sni_name_for_sleep = sni_name.clone();
    tokio::spawn(async move {
      use zbus::export::ordered_stream::OrderedStreamExt as _;

      let system_conn = match zbus::connection::Builder::system().ok().map(|b| b.build()) {
        Some(fut) => match fut.await {
          Ok(c) => c,
          Err(e) => {
            eprintln!("Sleep: Failed to connect to system bus: {}", e);
            return;
          }
        },
        None => {
          eprintln!("Sleep: Failed to build system bus connection");
          return;
        }
      };

      let rule = "type='signal',sender='org.freedesktop.login1',\
        interface='org.freedesktop.login1.Manager',\
        member='PrepareForSleep'";

      if let Err(e) = system_conn
        .call_method(
          Some(zbus::names::BusName::try_from("org.freedesktop.DBus").unwrap()),
          "/org/freedesktop/DBus",
          Some(zbus::names::InterfaceName::try_from("org.freedesktop.DBus").unwrap()),
          "AddMatch",
          &(rule),
        )
        .await
      {
        eprintln!("Sleep: Failed to add match rule: {}", e);
        return;
      }

      use zbus::message::Type;
      let mut stream = zbus::MessageStream::from(&system_conn);

      while let Some(Ok(msg)) = stream.next().await {
        if msg.message_type() != Type::Signal {
          continue;
        }
        if msg.header().member().map(|m| m.as_str()) != Some("PrepareForSleep") {
          continue;
        }
        // PrepareForSleep(bool): true = going to sleep, false = waking up
        if let Ok(going_to_sleep) = msg.body().deserialize::<bool>() {
          if !going_to_sleep {
            println!(
              "Sleep: System resumed, re-registering tray and re-grabbing managed windows..."
            );
            tokio::time::sleep(tokio::time::Duration::from_millis(RE_GRAB_TIME)).await;
            register_sni(&conn_for_sleep, &sni_name_for_sleep).await;
            StatusNotifierItem::emit_new_icon(&conn_for_sleep).await;
            let cfg = config_for_sleep.read().unwrap().clone();
            detect_refresh_rate(&cfg).await;
            let mut apps_for_grabbing = Vec::new();
            for (_name, app_cfg) in &cfg.app {
              apps_for_grabbing.push((app_cfg, &cfg));
            }
            let _ = grab_apps(&apps_for_grabbing, &conn_for_sleep).await;
            println!("Sleep: Re-grab complete.");
          }
        }
      }
    });
  }

  // Monitor kdeglobals for color-scheme changes (mono_icon_dark / mono_icon_light).
  // Always spawned — if no mono setting is active the watcher just sleeps idle.
  spawn_theme_watcher(&conn);

  // Small delay to ensure D-Bus service is fully registered before KWin scripts call back
  sleep(Duration::from_millis(100)).await;

  // Purge stale KWin scripts from prior sessions (crash recovery)
  purge_stale_scripts(&conn).await;

  // Initial setup (Sequential with stagger to reduce KWin contention)
  {
    let cfg = config.read().unwrap().clone();
    for (i, name) in cfg.app.keys().enumerate() {
      if i > 0 {
        sleep(Duration::from_millis(200)).await;
      }
      if let Some(app_cfg) = cfg.app.get(name) {
        let _ = ensure_terminal_running(name, app_cfg, &cfg, &conn).await;
      }
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
          if crate::linux::kwin::get_visible_app().await.is_none() {
            let _ = toggle_quake(app_name, &cfg, &conn).await;
          }
        }
      } else if let Some(first_app) = cfg.app.keys().next() {
        println!("janq: Auto-showing first app: {}", first_app);
        sleep(Duration::from_millis(500)).await;
        if crate::linux::kwin::get_visible_app().await.is_none() {
          let _ = toggle_quake(first_app, &cfg, &conn).await;
        }
      }
    }
  }

  // Config Watcher (Thread)
  let config_for_watcher = config.clone();
  let conn_for_watcher = conn.clone();
  let path_to_watch = config_path.clone();

  config_watcher::spawn_config_watcher(path_to_watch.clone(), move || {
    let path_to_watch = path_to_watch.clone();
    let config_for_watcher = config_for_watcher.clone();
    let conn_for_watcher = conn_for_watcher.clone();

    async move {
      let old_config =
        match config_watcher::reload_shared_config(path_to_watch.clone(), &*config_for_watcher) {
          Some(old) => old,
          None => return,
        };

      let new_config = config_for_watcher.read().unwrap().clone();

      // Notify tray that menu layout changed and icon may have changed
      crate::linux::tray::DbusmenuService::notify_layout_changed(&conn_for_watcher).await;
      StatusNotifierItem::emit_new_icon(&conn_for_watcher).await;

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
      let _ = sync_kwin_rules(&new_config_in_async);

      // 2. Ensure all terminals are running and grabbed
      let mut apps_for_grabbing = Vec::new();
      for (name, app_cfg) in &new_config_in_async.app {
        if !old_config.app.contains_key(name) {
          println!("Watcher: New app detected: {}. Starting terminal...", name);
        }
        // We ensure terminal is running for ALL apps (in case one crashed)
        let _ = ensure_terminal_running(name, app_cfg, &new_config_in_async, &conn_in_async).await;
        apps_for_grabbing.push((app_cfg, &new_config_in_async));
      }
      let _ = grab_apps(&apps_for_grabbing, &conn_in_async).await;

      detect_refresh_rate(&new_config_in_async).await;

      // 3. Update desktop file (don't run kbuild inside, we'll do it last)
      let desktop_changed = match generate_desktop_file_headless(&new_config_in_async) {
        Ok(changed) => changed,
        Err(e) => {
          show_error(&format!("Watcher: Desktop file generation failed: {}", e));
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
        let _ = sync_kwin_rules(&new_config_in_async);
        if let Err(e) =
          sync_kde_shortcuts(&new_config_in_async, Some(&old_config), &conn_in_async).await
        {
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

pub async fn send_toggle(app_name: Option<String>) -> error::Result<()> {
  let conn = zbus::connection::Builder::session()?.build().await?;
  if let Some(name) = app_name {
    conn
      .call_method(
        Some(BusName::try_from("dev.nabaxo.janq.desktop").expect("valid D-Bus bus name")),
        "/dev/nabaxo/janq/daemon",
        Some(InterfaceName::try_from("dev.nabaxo.janq").expect("valid D-Bus interface name")),
        "ToggleApp",
        &(name),
      )
      .await?;
  } else {
    conn
      .call_method(
        Some(BusName::try_from("dev.nabaxo.janq.desktop").expect("valid D-Bus bus name")),
        "/dev/nabaxo/janq/daemon",
        Some(InterfaceName::try_from("dev.nabaxo.janq").expect("valid D-Bus interface name")),
        "Toggle",
        &(),
      )
      .await?;
  }
  Ok(())
}

pub async fn send_recover() -> error::Result<()> {
  let conn = zbus::connection::Builder::session()?.build().await?;
  conn
    .call_method(
      Some(BusName::try_from("dev.nabaxo.janq.desktop").expect("valid D-Bus bus name")),
      "/dev/nabaxo/janq/daemon",
      Some(InterfaceName::try_from("dev.nabaxo.janq").expect("valid D-Bus interface name")),
      "Recover",
      &(),
    )
    .await?;
  println!("janq: Recovery signal sent to daemon.");
  Ok(())
}

pub async fn send_quit() -> error::Result<()> {
  let conn = zbus::connection::Builder::session()?.build().await?;
  // The daemon calls exit(0) before it can reply, so the D-Bus call
  // always returns a "NoReply" / peer-disconnected error on success.
  // Treat any error from the Quit call as success.
  let _ = conn
    .call_method(
      Some(BusName::try_from("dev.nabaxo.janq.desktop").expect("valid D-Bus bus name")),
      "/dev/nabaxo/janq/daemon",
      Some(InterfaceName::try_from("dev.nabaxo.janq").expect("valid D-Bus interface name")),
      "Quit",
      &(),
    )
    .await;
  println!("janq: Quit signal sent to daemon.");
  Ok(())
}
