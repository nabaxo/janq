use crate::config::Config;
use std::fs;
use std::path::PathBuf;
use anyhow::{Result, Context};

pub fn generate_desktop_file(config: &Config) -> Result<()> {
    let current_exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("ruake"));
    let exe_path = current_exe.to_string_lossy();

    // 1. Install Icon
    install_icon()?;

    // Determine the Exec command
    let exec_cmd = if config.app.len() == 1 {
        let first_app = config.app.keys().next().unwrap();
        format!("{} --app {}", exe_path, first_app)
    } else {
        exe_path.to_string()
    };

    // 2. Desktop File - write directly using BufWriter
    let xdg_data = dirs::data_local_dir().unwrap_or_else(|| {
        dirs::home_dir().unwrap().join(".local").join("share")
    });
    let app_dir = xdg_data.join("applications");
    fs::create_dir_all(&app_dir)?;

    let desktop_path = app_dir.join("dev.nabaxo.ruake.desktop");
    {
        use std::io::{BufWriter, Write};
        let file = fs::File::create(&desktop_path)?;
        let mut w = BufWriter::new(file);

        writeln!(w, "[Desktop Entry]")?;
        writeln!(w, "Name=Ruake")?;
        writeln!(w, "Comment=Quake-style terminal manager")?;
        writeln!(w, "Exec={}", exec_cmd)?;
        writeln!(w, "Icon=ruake")?;
        writeln!(w, "Terminal=false")?;
        writeln!(w, "Type=Application")?;
        writeln!(w, "DBusActivatable=true")?;
        writeln!(w, "Categories=System;TerminalEmulator;")?;
        writeln!(w, "X-KDE-StartupNotify=false")?;
        writeln!(w, "X-Color-Scheme-Preference=Dark")?;
        writeln!(w, "X-DBus-ServiceName=dev.nabaxo.ruake")?;
        writeln!(w, "X-DBus-StartupType=Unique")?;
        writeln!(w, "X-DBus-ObjectPath=/dev/nabaxo/ruake")?;

        // Add actions if not empty
        if !config.app.is_empty() {
            let mut keys: Vec<_> = config.app.keys().collect();
            keys.sort();

            // Write Actions line
            write!(w, "Actions=")?;
            for (i, name) in keys.iter().enumerate() {
                if i > 0 { write!(w, ";")?; }
                write!(w, "{}", name)?;
            }
            writeln!(w, ";")?;

            // Write action definitions
            for name in &keys {
                let hotkey_str = config.app.get(*name)
                    .map(|app| {
                        app.hotkey.as_vec().iter()
                            .map(|hk| crate::linux::hotkey::normalize_shortcut_for_kde(hk))
                            .collect::<Vec<_>>()
                            .join(",")
                    })
                    .unwrap_or_default();

                writeln!(w)?;
                writeln!(w, "[Desktop Action {}]", name)?;
                writeln!(w, "Name=Toggle {}", name)?;
                writeln!(w, "Exec={} --app {}", exe_path, name)?;
                writeln!(w, "X-KDE-Shortcuts={}", hotkey_str)?;
            }
        }
    }

    // 3. Service File (for DBusActivatable auto-start)
    let service_dir = xdg_data.join("dbus-1").join("services");
    fs::create_dir_all(&service_dir)?;

    let service_path = service_dir.join("dev.nabaxo.ruake.service");
    {
        use std::io::{BufWriter, Write};
        let file = fs::File::create(&service_path)?;
        let mut w = BufWriter::new(file);
        writeln!(w, "[D-BUS Service]")?;
        writeln!(w, "Name=dev.nabaxo.ruake")?;
        writeln!(w, "Exec={} --daemon", exe_path)?;
    }

    // 4. Update KDE Sycoca (System Configuration Cache)
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
