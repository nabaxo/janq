use std::{env::current_exe, fs, os::unix::fs::symlink, path::PathBuf, process::Command};

use janq::paths::{config_dir, data_local_dir};

use janq::config::Config;

// ─────────────────────────────────────────────────────────────────────────────
// Public API
// ─────────────────────────────────────────────────────────────────────────────

pub fn get_desktop_path() -> PathBuf {
  data_local_dir()
    .expect("No XDG data directory found - is $HOME set?")
    .join("applications/dev.nabaxo.janq.desktop")
}

pub fn enable_autostart(config: &Config) -> janq::error::Result<()> {
  let desktop = get_desktop_path();
  if !desktop.exists() {
    println!("Desktop file missing at {:?}, generating it...", desktop);
    generate_desktop_file_headless(config)?;
  }

  let autostart = config_dir()
    .expect("No XDG config directory found - is $HOME set?")
    .join("autostart");
  let link = autostart.join("dev.nabaxo.janq.desktop");

  fs::create_dir_all(&autostart)?;
  let _ = fs::remove_file(&link);
  symlink(&desktop, &link)
    .map_err(|e| janq::format_error_boxed!("Failed to create autostart symlink: {}", e))?;

  println!("✓ Autostart enabled: {:?} -> {:?}", link, desktop);
  Ok(())
}

pub fn disable_autostart() -> janq::error::Result<()> {
  let link = config_dir()
    .expect("No XDG config directory found - is $HOME set?")
    .join("autostart/dev.nabaxo.janq.desktop");

  if link.exists() || link.is_symlink() {
    fs::remove_file(&link)?;
    println!("✓ Autostart disabled: removed {:?}", link);
  } else {
    println!("Autostart is not enabled (no symlink at {:?})", link);
  }
  Ok(())
}

pub fn generate_desktop_file(config: &Config) -> janq::error::Result<()> {
  let _ = generate_desktop_file_impl(config, true)?;
  Ok(())
}

pub fn generate_desktop_file_headless(config: &Config) -> janq::error::Result<bool> {
  generate_desktop_file_impl(config, false)
}

fn generate_desktop_file_impl(config: &Config, run_kbuild: bool) -> janq::error::Result<bool> {
  let current_exe = current_exe()
    .and_then(|p| p.canonicalize())
    .unwrap_or_else(|_| PathBuf::from("janq"));
  let exe_path_raw = current_exe.to_string_lossy();
  let exe_path = format!("\"{}\"", exe_path_raw);

  // 1. Install Icon
  install_icon()?;

  // Determine the Exec command for the main entry
  let exec_cmd = format!("{} --daemon", exe_path);

  // 2. Desktop File
  let desktop_path = get_desktop_path();
  fs::create_dir_all(desktop_path.parent().unwrap())?;

  // Build content in memory to check for changes
  let mut desktop_content = String::new();
  desktop_content.push_str("[Desktop Entry]\n");
  desktop_content.push_str("Name=janq\n");
  desktop_content.push_str("Comment=Quake-style terminal manager\n");
  desktop_content.push_str(&format!("Exec={}\n", exec_cmd));
  desktop_content.push_str("Icon=janq\n");
  desktop_content.push_str("Terminal=false\n");
  desktop_content.push_str("Type=Application\n");
  desktop_content.push_str("DBusActivatable=true\n");
  desktop_content.push_str("Categories=System;TerminalEmulator;\n");
  desktop_content.push_str("X-KDE-StartupNotify=false\n");
  desktop_content.push_str("X-Color-Scheme-Preference=Dark\n");
  desktop_content.push_str("X-DBus-ServiceName=dev.nabaxo.janq\n");
  desktop_content.push_str("X-DBus-StartupType=Unique\n");
  desktop_content.push_str("X-DBus-ObjectPath=/dev/nabaxo/janq\n");

  if !config.app.is_empty() {
    let mut keys: Vec<_> = config.app.keys().collect();
    keys.sort();

    desktop_content.push_str("Actions=");
    for (i, name) in keys.iter().enumerate() {
      if i > 0 {
        desktop_content.push(';');
      }
      desktop_content.push_str(name);
    }
    desktop_content.push_str(";\n");

    for name in &keys {
      let hotkey_str = config
        .app
        .get(*name)
        .map(|app| {
          app
            .hotkey
            .as_vec()
            .iter()
            .map(|hk| crate::linux::hotkey::normalize_shortcut_for_kde(hk))
            .collect::<Vec<_>>()
            .join(",")
        })
        .unwrap_or_default();

      desktop_content.push_str(&format!("\n[Desktop Action {}]\n", name));
      desktop_content.push_str(&format!("Name=Toggle {}\n", name));
      desktop_content.push_str(&format!("Exec={} --app {}\n", exe_path, name));
      desktop_content.push_str(&format!("X-KDE-Shortcuts={}\n", hotkey_str));
    }
  }

  let mut changed = true;
  if let Ok(existing) = fs::read_to_string(&desktop_path) {
    if existing == desktop_content {
      changed = false;
    } else if config.app.is_empty() && existing.contains("Actions=") {
      // Safeguard: Don't overwrite a functional desktop file with one that has no apps
      // This happens if janq is run from a location with a junk/empty config.
      janq::error::show_warning("Current config has no apps, but existing desktop file has actions. Skipping update to preserve shortcuts.");
      changed = false;
    }
  }

  if changed {
    fs::write(&desktop_path, desktop_content)?;
  }

  // 3. Service File (for DBusActivatable auto-start)
  let service_dir = data_local_dir()
    .expect("No XDG data directory found - is $HOME set?")
    .join("dbus-1/services");
  fs::create_dir_all(&service_dir)?;

  let service_path = service_dir.join("dev.nabaxo.janq.service");
  let service_content = format!(
    "[D-BUS Service]\nName=dev.nabaxo.janq\nExec={} --daemon\n",
    exe_path
  );

  let mut service_changed = true;
  if let Ok(existing) = fs::read_to_string(&service_path) {
    if existing == service_content {
      service_changed = false;
    }
  }

  if service_changed {
    fs::write(&service_path, service_content)?;
  }

  let modified = changed || service_changed;

  // 4. Update KDE Sycoca if anything changed
  if modified && run_kbuild {
    run_kbuildsycoca6();
  }

  Ok(modified)
}

fn run_kbuildsycoca6() {
  match Command::new("kbuildsycoca6")
    .arg("--noincremental")
    .status()
  {
    Ok(status) => {
      if !status.success() {
        janq::error::show_error(&format!("kbuildsycoca6 exited with status: {}", status));
      }
    }
    Err(e) => {
      janq::error::show_warning(&format!(
        "Failed to run kbuildsycoca6: {} (This is normal on non-KDE)",
        e
      ));
    }
  }
}

pub fn install_icon() -> janq::error::Result<()> {
  let icon_data = include_bytes!("../../icon.svg");
  let icon_dir = data_local_dir()
    .expect("No XDG data directory found - is $HOME set?")
    .join("icons/hicolor/scalable/apps");

  fs::create_dir_all(&icon_dir)?;
  let icon_path = icon_dir.join("janq.svg");

  // Compare contents to detect updates (not just existence)
  let needs_update = match fs::read(&icon_path) {
    Ok(existing) => {
      let changed = existing != icon_data.as_slice();
      if changed {
        println!("Icon changed, updating: {:?}", icon_path);
      }
      changed
    }
    Err(_) => {
      println!("Icon not found, installing: {:?}", icon_path);
      true
    }
  };

  if needs_update {
    fs::write(&icon_path, icon_data)?;
    println!("Icon written successfully");
  }

  Ok(())
}
