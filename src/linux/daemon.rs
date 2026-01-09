use crate::config::{load_config, Config};
use crate::linux::kwin::{grab_apps, reset_visibility, restore_quake, toggle_quake};
use crate::terminal::ensure_terminal_running;
use fs2::FileExt;

use notify::{Config as NotifyConfig, RecommendedWatcher, RecursiveMode, Watcher};
use std::sync::{Arc, RwLock};
use tokio::signal;
use tokio::time::{sleep, Duration};
use zbus::{interface, Connection};

#[derive(Clone)]
struct QuakeApplication {
  config: Arc<RwLock<Arc<Config>>>,
  conn: Connection,
}

#[interface(name = "org.freedesktop.Application")]
impl QuakeApplication {
  async fn activate(&self, _platform_data: std::collections::HashMap<String, zbus::zvariant::OwnedValue>) {
    // No-op: Satisfaction of D-Bus Application activation.
    // Clicking the launcher icon should only start the background process, not toggle a window.
  }

  async fn activate_action(
    &self,
    action_name: String,
    _parameter: Vec<zbus::zvariant::OwnedValue>,
    _platform_data: std::collections::HashMap<String, zbus::zvariant::OwnedValue>,
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

  fn open(&self, _uris: Vec<String>, _platform_data: std::collections::HashMap<String, zbus::zvariant::OwnedValue>) {
    // Not used
  }
}

#[derive(Clone)]
struct QuakeDaemon {
  config: Arc<RwLock<Arc<Config>>>,
  conn: Connection,
}

#[interface(name = "dev.nabaxo.janq")]
impl QuakeDaemon {
  #[zbus(name = "Toggle")]
  async fn toggle(&self) {
    let config = { self.config.read().unwrap().clone() };
    let mut apps: Vec<_> = config.app.keys().collect();
    apps.sort_unstable();
    if let Some(app_name) = apps.first() {
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
}

struct StatusNotifierItem {
  config: Arc<RwLock<Arc<Config>>>,
  icon_cache: IconPixmap,
  conn: Connection,
}

type IconPixmap = Vec<(i32, i32, Vec<u8>)>;

#[interface(name = "org.kde.StatusNotifierItem")]
impl StatusNotifierItem {
  fn activate(&self, _x: i32, _y: i32) {
    let config = { self.config.read().unwrap().clone() };
    let conn = self.conn.clone();
    tokio::spawn(async move {
      let app_name = config.app.keys().next();
      if let Some(name) = app_name {
        let _ = toggle_quake(name, &config, &conn).await;
      }
    });
  }

  fn secondary_activate(&self, _x: i32, _y: i32) {
    let config = { self.config.read().unwrap().clone() };
    let conn = self.conn.clone();
    tokio::spawn(async move {
      let _ = restore_quake(&config, &conn).await;
      std::process::exit(0);
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

pub async fn run_daemon(
  initial_config: Config,
  config_path: Option<std::path::PathBuf>,
  target_app: Option<String>,
) -> anyhow::Result<()> {
  // 0. Acquire Lock File
  let lock_path = std::env::temp_dir().join("janq.lock");
  let lock_file = std::fs::File::create(&lock_path)?;
  if lock_file.try_lock_exclusive().is_err() {
    return Err(anyhow::anyhow!("janq is already running (lock file active)."));
  }

  println!("Starting janq daemon...");
  let config = Arc::new(RwLock::new(Arc::new(initial_config)));
  let conn = Connection::session().await?;

  let pid = std::process::id();
  let sni_name = format!("org.kde.StatusNotifierItem-janq-{}", pid);
  conn.request_name(sni_name.clone()).await?;

  // Empty pixmap forces KDE to use icon_name (SVG from icon theme)
  let icon_cache: IconPixmap = vec![];

  let sni = StatusNotifierItem {
    config: config.clone(),
    icon_cache,
    conn: conn.clone(),
  };
  conn.object_server().at("/StatusNotifierItem", sni).await?;

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
    let _ = conn.object_server().at(*path, app_instance.clone()).await;
    let _ = conn.object_server().at(*path, daemon_instance.clone()).await;
  }

  conn.request_name(activatable_bus).await?;
  let _ = conn.request_name("dev.nabaxo.janq.desktop").await;

  let watcher_proxy = zbus::Proxy::new(
    &conn,
    "org.kde.StatusNotifierWatcher",
    "/StatusNotifierWatcher",
    "org.kde.StatusNotifierWatcher",
  )
  .await?;
  let _ = watcher_proxy
    .call_method("RegisterStatusNotifierItem", &(sni_name))
    .await;

  // --- KDE Platform Integration ---
  {
    let cfg = config.read().unwrap().clone();
    let _ = crate::linux::desktop::generate_desktop_file(&cfg);
    tokio::spawn(async move {
      let _ = crate::linux::hotkey::sync_kde_shortcuts(&cfg, None).await;
    });
  }

  // Initial setup (Parallel)
  {
    let cfg = config.read().unwrap().clone();
    let mut terminal_tasks = Vec::new();
    let mut apps_for_grabbing = Vec::new();
    for name in cfg.app.keys() {
      if let Some(app_cfg) = cfg.app.get(name) {
        let app_cfg_owned = app_cfg.clone();
        let app_cfg_for_spawn = app_cfg_owned.clone();
        let cfg_clone = (*cfg).clone();
        let conn_clone = conn.clone();

        terminal_tasks.push(tokio::spawn(async move {
          let _ = ensure_terminal_running(&app_cfg_for_spawn, &cfg_clone, &conn_clone).await;
        }));
        apps_for_grabbing.push((app_cfg_owned, (*cfg).clone()));
      }
    }

    for task in terminal_tasks {
      let _ = task.await;
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
  let rt_handle = tokio::runtime::Handle::current();

  std::thread::spawn(move || {
    let (tx, rx) = std::sync::mpsc::channel();
    let mut watcher = RecommendedWatcher::new(tx, NotifyConfig::default()).unwrap();

    if let Some(path) = &path_to_watch {
      if let Ok(abs_path) = path.canonicalize() {
        println!("Watcher: Monitoring config file: {:?}", abs_path);
        if let Some(parent) = abs_path.parent() {
          let _ = watcher.watch(parent, RecursiveMode::NonRecursive);
        } else {
          let _ = watcher.watch(&abs_path, RecursiveMode::NonRecursive);
        }
      } else {
        let _ = watcher.watch(path, RecursiveMode::NonRecursive);
      }
    }

    let debounce_duration = Duration::from_millis(500);
    let mut last_event = std::time::Instant::now();
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
          }

          if is_config_file {
            last_event = std::time::Instant::now();
            pending = true;
          }
        }
        Ok(Err(e)) => println!("Watcher error: {:?}", e),
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
          if pending {
            pending = false;
            println!("Watcher: Debounced event triggered config reload...");

            let (new_config, _) = match load_config(path_to_watch.clone()) {
              Ok(c) => c,
              Err(e) => {
                let err_msg = format!("Config reload failed: {}", e);
                eprintln!("Watcher: {}", err_msg);

                // Restore all apps before shutting down
                let current_cfg = config_for_watcher.read().unwrap().clone();
                let conn_shutdown = conn_for_watcher.clone();
                rt_handle.block_on(async move {
                  println!("Watcher: Restoring all apps before shutdown...");
                  let _ = restore_quake(&current_cfg, &conn_shutdown).await;
                });

                crate::linux::show_error(&err_msg);
                std::process::exit(1);
              }
            };

            let old_config = {
              let mut w = config_for_watcher.write().unwrap();
              let old = (**w).clone();
              *w = Arc::new(new_config.clone());
              old
            };

            let conn_in_async = conn_for_watcher.clone();
            let new_config_in_async = new_config.clone();

            rt_handle.block_on(async move {
              println!("Watcher: Starting/Restoring apps as needed...");

              // 1. Restore removed apps
              for (name, app_cfg) in &old_config.app {
                if !new_config_in_async.app.contains_key(name) {
                  println!("Watcher: Restoring app '{}' (removed from config)", name);
                  let _ = crate::linux::kwin::restore_app(&app_cfg.window_class, &conn_in_async).await;
                }
              }

              reset_visibility(&new_config_in_async).await;

              // 2. Ensure all terminals are running and grabbed
              let mut apps_for_grabbing = Vec::new();
              for (name, app_cfg) in &new_config_in_async.app {
                if !old_config.app.contains_key(name) {
                  println!("Watcher: New app detected: {}. Starting terminal...", name);
                }
                // We ensure terminal is running for ALL apps (in case one crashed)
                let _ = ensure_terminal_running(app_cfg, &new_config_in_async, &conn_in_async).await;
                apps_for_grabbing.push((app_cfg.clone(), new_config_in_async.clone()));
              }
              let _ = grab_apps(&apps_for_grabbing, &conn_in_async).await;

              // 3. Update desktop file (don't run kbuild inside, we'll do it last)
              let desktop_changed = match crate::linux::desktop::generate_desktop_file_headless(&new_config_in_async) {
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
                if let Err(e) = crate::linux::hotkey::sync_kde_shortcuts(&new_config_in_async, Some(&old_config)).await
                {
                  eprintln!("Watcher: Failed to sync shortcuts: {}", e);
                }
              } else {
                println!("Config: No shortcut/desktop changes detected.");
              }
            });
          }
        }
        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
      }
    }
  });

  let config_for_signals = config.clone();
  let conn_for_signals = conn.clone();
  match signal::ctrl_c().await {
    Ok(()) => {
      let cfg = config_for_signals.read().unwrap().clone();
      let _ = restore_quake(&cfg, &conn_for_signals).await;
      println!("Quitting...");
    }
    Err(err) => eprintln!("Signal error: {}", err),
  }

  Ok(())
}

pub async fn send_toggle(app_name: Option<String>) -> anyhow::Result<()> {
  let conn = Connection::session().await?;
  let proxy = zbus::Proxy::new(
    &conn,
    "dev.nabaxo.janq.desktop",
    "/dev/nabaxo/janq/daemon",
    "dev.nabaxo.janq",
  )
  .await?;
  if let Some(name) = app_name {
    proxy.call_method("ToggleApp", &(name)).await?;
  } else {
    proxy.call_method("Toggle", &()).await?;
  }
  Ok(())
}
