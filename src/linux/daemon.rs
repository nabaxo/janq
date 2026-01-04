use crate::config::{Config, load_config};
use crate::linux::kwin::{toggle_quake, restore_quake, ensure_grabbed};
use crate::terminal::{ensure_terminal_running};
use zbus::{interface, Connection};
use std::sync::{Arc, RwLock};
use tokio::time::{sleep, Duration};
use tokio::signal;
use notify::{Watcher, RecursiveMode, RecommendedWatcher, Config as NotifyConfig};
use image::GenericImageView;
use fs2::FileExt;

#[derive(Clone)]
struct QuakeApplication {
    config: Arc<RwLock<Arc<Config>>>,
    conn: Connection,
}

#[interface(name = "org.freedesktop.Application")]
impl QuakeApplication {
    async fn activate(&self, _platform_data: std::collections::HashMap<String, zbus::zvariant::OwnedValue>) {
        let daemon = QuakeDaemon { config: self.config.clone(), conn: self.conn.clone() };
        daemon.toggle().await;
    }

    async fn activate_action(
        &self,
        action_name: String,
        _parameter: Vec<zbus::zvariant::OwnedValue>,
        _platform_data: std::collections::HashMap<String, zbus::zvariant::OwnedValue>
    ) {
        println!("D-Bus: Action activated: {}", action_name);
        let daemon = QuakeDaemon { config: self.config.clone(), conn: self.conn.clone() };
        daemon.toggle_app(action_name).await;
    }

    fn open(
        &self,
        _uris: Vec<String>,
        _platform_data: std::collections::HashMap<String, zbus::zvariant::OwnedValue>
    ) {
        // Not used
    }
}

#[derive(Clone)]
struct QuakeDaemon {
    config: Arc<RwLock<Arc<Config>>>,
    conn: Connection,
}

#[interface(name = "dev.nabaxo.ruake")]
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
            let app_name = config.app_order.first();
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
    fn category(&self) -> String { "ApplicationStatus".to_string() }
    #[zbus(property)]
    fn id(&self) -> String { "ruake".to_string() }
    #[zbus(property)]
    fn title(&self) -> String { "Ruake".to_string() }
    #[zbus(property)]
    fn status(&self) -> String { "Active".to_string() }
    #[zbus(property)]
    fn icon_name(&self) -> String { "ruake".to_string() }
    #[zbus(property)]
    fn icon_pixmap(&self) -> IconPixmap { self.icon_cache.clone() }
    #[zbus(property)]
    fn item_is_menu(&self) -> bool { false }
}

pub async fn run_daemon(initial_config: Config, config_path: Option<std::path::PathBuf>, auto_show: bool, target_app: Option<String>) -> anyhow::Result<()> {
    // 0. Acquire Lock File
    let lock_path = std::env::temp_dir().join("ruake.lock");
    let lock_file = std::fs::File::create(&lock_path)?;
    if lock_file.try_lock_exclusive().is_err() {
        return Err(anyhow::anyhow!("Ruake is already running (lock file active)."));
    }

    println!("Starting Ruake daemon...");
    let config = Arc::new(RwLock::new(Arc::new(initial_config)));
    let conn = Connection::session().await?;

    let pid = std::process::id();
    let sni_name = format!("org.kde.StatusNotifierItem-ruake-{}", pid);
    conn.request_name(sni_name.clone()).await?;

    // Precompute icon
    let icon_cache = if let Ok(img) = image::load_from_memory(include_bytes!("../../icon.png")) {
         let mut pixmaps = Vec::new();
         for size in [64, 32, 22] {
             let resized = img.resize(size, size, image::imageops::FilterType::Lanczos3);
             let (w, h) = resized.dimensions();
             let data = resized.to_rgba8().into_raw();
             let mut pixels = Vec::with_capacity(data.len());
             for chunk in data.chunks(4) {
                 pixels.push(chunk[3]); pixels.push(chunk[0]); pixels.push(chunk[1]); pixels.push(chunk[2]);
             }
             pixmaps.push((w as i32, h as i32, pixels));
         }
         pixmaps
    } else {
         vec![]
    };

    let sni = StatusNotifierItem { config: config.clone(), icon_cache, conn: conn.clone() };
    conn.object_server().at("/StatusNotifierItem", sni).await?;

    let activatable_bus = "dev.nabaxo.ruake";
    let activatable_path = "/dev/nabaxo/ruake";
    let xdg_path = "/org/freedesktop/Application/dev/nabaxo/ruake";
    let daemon_path = "/dev/nabaxo/ruake/daemon";
    let root_path = "/";

    let app_instance = QuakeApplication { config: config.clone(), conn: conn.clone() };
    let daemon_instance = QuakeDaemon { config: config.clone(), conn: conn.clone() };

    for path in &[activatable_path, xdg_path, daemon_path, root_path] {
        let _ = conn.object_server().at(*path, app_instance.clone()).await;
        let _ = conn.object_server().at(*path, daemon_instance.clone()).await;
    }

    conn.request_name(activatable_bus).await?;
    let _ = conn.request_name("dev.nabaxo.ruake.desktop").await;

    let watcher_proxy = zbus::Proxy::new(&conn, "org.kde.StatusNotifierWatcher", "/StatusNotifierWatcher", "org.kde.StatusNotifierWatcher").await?;
    let _ = watcher_proxy.call_method("RegisterStatusNotifierItem", &(sni_name)).await;

    // --- KDE Platform Integration ---
    {
        let cfg = config.read().unwrap().clone();
        let _ = crate::linux::desktop::generate_desktop_file(&cfg);
        let _ = crate::linux::hotkey::sync_kde_shortcuts(&cfg, None).await;
    }


    // Initial setup (Parallel)
    {
        let cfg = config.read().unwrap().clone();
        let app_names: Vec<_> = cfg.app.keys().cloned().collect();
        let app_order = cfg.app_order.clone();
        println!("Ruake: Found {} apps in config: {}", cfg.app.len(), app_names.join(", "));

        let mut startup_tasks = Vec::new();

        for name in app_order {
            if let Some(app_cfg) = cfg.app.get(&name) {
                let app_cfg = app_cfg.clone();
                let cfg_clone = (*cfg).clone();
                let conn_clone = conn.clone();

                startup_tasks.push(tokio::spawn(async move {
                    let _ = ensure_terminal_running(&app_cfg, &cfg_clone, &conn_clone).await;
                    let _ = crate::linux::kwin::ensure_grabbed(&app_cfg, &cfg_clone, &conn_clone).await;
                }));
            }
        }

        // We don't necessarily need to wait for all of them to finish before starting the watcher,
        // but it's cleaner to wait for the initial batch.
        for task in startup_tasks {
            let _ = task.await;
        }

        if auto_show {
            let app_to_show = target_app.as_ref();
            if let Some(app_name) = app_to_show {
                if let Some(_app_cfg) = cfg.app.get(app_name) {
                    println!("Ruake: Auto-showing requested app: {}", app_name);
                    sleep(Duration::from_millis(500)).await;
                    let _ = toggle_quake(app_name, &cfg, &conn).await;
                }
            } else if let Some(first_app) = cfg.app_order.first() {
                 println!("Ruake: Auto-showing first app: {}", first_app);
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

                        let (new_config, _) = match load_config() {
                            Ok(c) => c,
                            Err(e) => {
                                let err_msg = format!("Config reload failed: {}", e);
                                eprintln!("Watcher: {}", err_msg);
                                crate::linux::show_error(&err_msg);
                                continue;
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
                            for (name, app_cfg) in &old_config.app {
                                if !new_config_in_async.app.contains_key(name) {
                                    let _ = crate::linux::kwin::restore_app(&app_cfg.window_class, &conn_in_async).await;
                                }
                            }

                            crate::linux::kwin::reset_visibility().await;
                            for app_cfg in new_config_in_async.app.values() {
                                let _ = ensure_grabbed(app_cfg, &new_config_in_async, &conn_in_async).await;
                            }

                            let _ = crate::linux::desktop::generate_desktop_file(&new_config_in_async);
                            tokio::time::sleep(Duration::from_millis(300)).await;

                            // SAFEGUARD: Only sync shortcuts if they actually changed
                            let mut hotkeys_changed = false;
                            if old_config.app.len() != new_config_in_async.app.len() {
                                hotkeys_changed = true;
                            } else {
                                for (name, old_app) in &old_config.app {
                                    if let Some(new_app) = new_config_in_async.app.get(name) {
                                        if old_app.hotkey != new_app.hotkey {
                                            hotkeys_changed = true;
                                            break;
                                        }
                                    } else {
                                        hotkeys_changed = true;
                                        break;
                                    }
                                }
                                if !hotkeys_changed && old_config.app.len() < new_config_in_async.app.len() {
                                     hotkeys_changed = true;
                                }
                            }

                            if hotkeys_changed {
                                println!("Config: Hotkeys changed, synchronizing with KDE...");
                                if let Err(e) = crate::linux::hotkey::sync_kde_shortcuts(&new_config_in_async, Some(&old_config)).await {
                                     eprintln!("Watcher: Failed to sync shortcuts: {}", e);
                                }
                            } else {
                                println!("Config: Hotkeys unchanged, skipping shortcut sync.");
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
        },
        Err(err) => eprintln!("Signal error: {}", err),
    }

    Ok(())
}

pub async fn send_toggle(app_name: Option<String>) -> anyhow::Result<()> {
    let conn = Connection::session().await?;
    let proxy = zbus::Proxy::new(&conn, "dev.nabaxo.ruake.desktop", "/dev/nabaxo/ruake/daemon", "dev.nabaxo.ruake").await?;
    if let Some(name) = app_name {
        proxy.call_method("ToggleApp", &(name)).await?;
    } else {
        proxy.call_method("Toggle", &()).await?;
    }
    Ok(())
}
