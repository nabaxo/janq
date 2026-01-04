use crate::config::Config;
use std::fs;
use std::path::PathBuf;
use anyhow::{Result, Context};

pub fn generate_desktop_file(config: &Config) -> Result<()> {
    let current_exe = std::env::current_exe()
        .and_then(|p| p.canonicalize())
        .unwrap_or_else(|_| PathBuf::from("ruake"));
    let exe_path_raw = current_exe.to_string_lossy();
    let exe_path = format!("\"{}\"", exe_path_raw);

    // 1. Install Icon
    install_icon()?;

    // Determine the Exec command
    let exec_cmd = if config.app.len() == 1 {
        let first_app = config.app.keys().next().unwrap();
        format!("{} --app {}", exe_path, first_app)
    } else {
        exe_path.clone()
    };

    // 2. Desktop File
    let xdg_data = dirs::data_local_dir().unwrap_or_else(|| {
        dirs::home_dir().unwrap().join(".local").join("share")
    });
    let app_dir = xdg_data.join("applications");
    fs::create_dir_all(&app_dir)?;

    let desktop_path = app_dir.join("dev.nabaxo.ruake.desktop");

    // Build content in memory to check for changes
    let mut desktop_content = String::new();
    desktop_content.push_str("[Desktop Entry]\n");
    desktop_content.push_str("Name=Ruake\n");
    desktop_content.push_str("Comment=Quake-style terminal manager\n");
    desktop_content.push_str(&format!("Exec={}\n", exec_cmd));
    desktop_content.push_str("Icon=ruake\n");
    desktop_content.push_str("Terminal=false\n");
    desktop_content.push_str("Type=Application\n");
    desktop_content.push_str("DBusActivatable=true\n");
    desktop_content.push_str("Categories=System;TerminalEmulator;\n");
    desktop_content.push_str("X-KDE-StartupNotify=false\n");
    desktop_content.push_str("X-Color-Scheme-Preference=Dark\n");
    desktop_content.push_str("X-DBus-ServiceName=dev.nabaxo.ruake\n");
    desktop_content.push_str("X-DBus-StartupType=Unique\n");
    desktop_content.push_str(&format!("X-DBus-ObjectPath=/dev/nabaxo/ruake\n"));

    if !config.app.is_empty() {
        let mut keys: Vec<_> = config.app.keys().collect();
        keys.sort();

        desktop_content.push_str("Actions=");
        for (i, name) in keys.iter().enumerate() {
            if i > 0 { desktop_content.push_str(";"); }
            desktop_content.push_str(name);
        }
        desktop_content.push_str(";\n");

        for name in &keys {
            let hotkey_str = config.app.get(*name)
                .map(|app| {
                    app.hotkey.as_vec().iter()
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
            // This happens if Ruake is run from a location with a junk/empty config.
            eprintln!("Warning: Current config has no apps, but existing desktop file has actions. Skipping update to preserve shortcuts.");
            changed = false;
        }
    }

    if changed {
        fs::write(&desktop_path, desktop_content)?;
    }

    // 3. Service File (for DBusActivatable auto-start)
    let service_dir = xdg_data.join("dbus-1").join("services");
    fs::create_dir_all(&service_dir)?;

    let service_path = service_dir.join("dev.nabaxo.ruake.service");
    let service_content = format!(
        "[D-BUS Service]\nName=dev.nabaxo.ruake\nExec={} --daemon\n",
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

    // 4. Update KDE Sycoca if anything changed
    if changed || service_changed {
        match std::process::Command::new("kbuildsycoca6").arg("--noincremental").status() {
            Ok(status) => {
                if !status.success() {
                    eprintln!("kbuildsycoca6 exited with status: {}", status);
                }
            }
            Err(e) => {
                eprintln!("Failed to run kbuildsycoca6: {} (This is normal on non-KDE)", e);
            }
        }
    }

    Ok(())
}

fn install_icon() -> Result<()> {
    let icon_data = include_bytes!("../../icon.png");
    let icon_dir = dirs::data_local_dir()
        .context("Failed to get local data dir")?
        .join("icons")
        .join("hicolor")
        .join("512x512")
        .join("apps");

    fs::create_dir_all(&icon_dir)?;
    let icon_path = icon_dir.join("ruake.png");
    if !icon_path.exists() {
        fs::write(&icon_path, icon_data)?;
    }

    Ok(())
}
