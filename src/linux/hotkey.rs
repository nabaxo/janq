use crate::config::Config;
use anyhow::Result;
// Convert KDE shortcut string (e.g. "Meta+Grave") to Qt keycode integer

/// Normalize a shortcut string to KDE's expected format for display in System Settings.
/// Converts internal names like "Grave", "Section" to the format KDE uses.
pub fn normalize_shortcut_for_kde(shortcut: &str) -> String {
    let mut normalized = String::with_capacity(shortcut.len());
    for (i, part) in shortcut.split('+').enumerate() {
        if i > 0 {
            normalized.push('+');
        }
        let p = part.trim();
        let p_lower = p.to_lowercase();

        match p_lower.as_str() {
            "meta" | "super" | "win" | "cmd" => normalized.push_str("Meta"),
            "ctrl" | "control" => normalized.push_str("Ctrl"),
            "alt" => normalized.push_str("Alt"),
            "shift" => normalized.push_str("Shift"),
            "`" | "grave" | "backtick" | "dead_grave" => normalized.push('`'),
            "§" | "section" => normalized.push('§'),
            "±" | "plusminus" => normalized.push('±'),
            "f1" | "f2" | "f3" | "f4" | "f5" | "f6" | "f7" | "f8" | "f9" | "f10" | "f11" | "f12" => {
                normalized.push('F');
                normalized.push_str(&p_lower[1..]);
            },
            "esc" | "escape" => normalized.push_str("Escape"),
            "tab" => normalized.push_str("Tab"),
            "space" => normalized.push_str("Space"),
            "enter" | "return" => normalized.push_str("Return"),
            "backspace" => normalized.push_str("Backspace"),
            "delete" | "del" => normalized.push_str("Delete"),
            "insert" => normalized.push_str("Insert"),
            "home" => normalized.push_str("Home"),
            "end" => normalized.push_str("End"),
            "pgup" | "pageup" => normalized.push_str("PgUp"),
            "pgdn" | "pagedown" => normalized.push_str("PgDown"),
            "up" | "arrowup" => normalized.push_str("Up"),
            "down" | "arrowdown" => normalized.push_str("Down"),
            "left" | "arrowleft" => normalized.push_str("Left"),
            "right" | "arrowright" => normalized.push_str("Right"),
            "capslock" | "caps_lock" => normalized.push_str("Caps Lock"),
            "-" | "minus" => normalized.push('-'),
            "=" | "equal" => normalized.push('='),
            "[" | "bracketleft" => normalized.push('['),
            "]" | "bracketright" => normalized.push(']'),
            "\\" | "backslash" => normalized.push('\\'),
            ";" | "semicolon" => normalized.push(';'),
            "'" | "quote" => normalized.push('\''),
            "," | "comma" => normalized.push(','),
            "." | "period" => normalized.push('.'),
            "/" | "slash" => normalized.push('/'),
            _ if p_lower.len() == 1 && p_lower.chars().next().unwrap().is_ascii_alphabetic() => {
                normalized.push(p_lower.chars().next().unwrap().to_ascii_uppercase());
            },
            _ => normalized.push_str(p),
        }
    }
    normalized
}

fn map_qt_key(s: &str) -> i32 {
    match s {
        "meta" | "super" | "win" | "cmd" => 0x10000000,
        "ctrl" | "control" => 0x04000000,
        "alt" => 0x08000000,
        "shift" => 0x02000000,
        "`" | "grave" | "backtick" | "dead_grave" => 0x60,
        "1" => 0x31, "2" => 0x32, "3" => 0x33, "4" => 0x34, "5" => 0x35,
        "6" => 0x36, "7" => 0x37, "8" => 0x38, "9" => 0x39, "0" => 0x30,
        "-" | "minus" => 0x2d, "=" | "equal" => 0x3d,
        "q" => 0x51, "w" => 0x57, "e" => 0x45, "r" => 0x52, "t" => 0x54,
        "y" => 0x59, "u" => 0x55, "i" => 0x49, "o" => 0x4f, "p" => 0x50,
        "[" | "bracketleft" => 0x5b, "]" | "bracketright" => 0x5d, "\\" | "backslash" => 0x5c,
        "a" => 0x41, "s" => 0x53, "d" => 0x44, "f" => 0x46, "g" => 0x47,
        "h" => 0x48, "j" => 0x4a, "k" => 0x4b, "l" => 0x4c,
        ";" | "semicolon" => 0x3b, "'" | "quote" => 0x27,
        "enter" | "return" => 0x01000004,
        "z" => 0x5a, "x" => 0x58, "c" => 0x43, "v" => 0x56, "b" => 0x42,
        "n" => 0x4e, "m" => 0x4d,
        "," | "comma" => 0x2c, "." | "period" => 0x2e, "/" | "slash" => 0x2f,
        "space" => 0x20, "esc" | "escape" => 0x01000000, "tab" => 0x01000001,
        "capslock" | "caps_lock" => 0x01000024, "backspace" => 0x01000003,
        "up" | "arrowup" => 0x01000013, "down" | "arrowdown" => 0x01000015,
        "left" | "arrowleft" => 0x01000012, "right" | "arrowright" => 0x01000014,
        "pgup" | "pageup" => 0x01000016, "pgdn" | "pagedown" => 0x01000017,
        "home" => 0x01000010, "end" => 0x01000011,
        "insert" => 0x01000006, "delete" | "del" => 0x01000007,
        "f1" => 0x01000030, "f2" => 0x01000031, "f3" => 0x01000032, "f4" => 0x01000033,
        "f5" => 0x01000034, "f6" => 0x01000035, "f7" => 0x01000036, "f8" => 0x01000037,
        "f9" => 0x01000038, "f10" => 0x01000039, "f11" => 0x0100003a, "f12" => 0x0100003b,
        "§" | "section" | "±" | "plusminus" => 0xa7,
        _ => {
            if s.len() == 1 {
                let ch = s.chars().next().unwrap().to_ascii_uppercase();
                if ch.is_ascii_alphanumeric() {
                    return ch as i32;
                }
            }
            0
        }
    }
}

fn shortcut_to_keycode(shortcut: &str) -> i32 {
    shortcut.split('+')
        .map(|part| map_qt_key(part.trim().to_lowercase().as_str()))
        .sum::<i32>()
}

#[zbus::proxy(
    interface = "org.kde.KGlobalAccel",
    default_service = "org.kde.kglobalaccel",
    default_path = "/kglobalaccel"
)]
trait KGlobalAccel {
    #[zbus(name = "allActionsForComponent")]
    fn all_actions_for_component(&self, action_id: Vec<String>) -> zbus::Result<Vec<Vec<String>>>;

    #[zbus(name = "setShortcutKeys")]
    fn set_shortcut_keys(&self, action_id: Vec<String>, keys: Vec<(Vec<i32>,)>) -> zbus::Result<Vec<(Vec<i32>,)>>;

    #[zbus(name = "setShortcut")]
    fn set_shortcut(&self, action_id: Vec<String>, keys: Vec<i32>, flags: u32) -> zbus::Result<Vec<i32>>;

    #[zbus(name = "setForeignShortcutKeys")]
    fn set_foreign_shortcut_keys(&self, action_id: Vec<String>, keys: Vec<(Vec<i32>,)>) -> zbus::Result<()>;

    #[zbus(name = "doRegister")]
    fn do_register(&self, action_id: Vec<String>) -> zbus::Result<()>;

    #[zbus(name = "unregister")]
    fn unregister(&self, component_unique: &str, shortcut_unique: &str) -> zbus::Result<bool>;
}

struct ShortcutFile {
    path: std::path::PathBuf,
    lines: Vec<String>,
    modified: bool,
}

impl ShortcutFile {
    fn load() -> Result<Self> {
        let path = dirs::config_dir()
            .unwrap_or_else(|| dirs::home_dir().unwrap().join(".config"))
            .join("kglobalshortcutsrc");

        let content = if path.exists() {
            std::fs::read_to_string(&path)?
        } else {
            String::new()
        };

        let mut lines: Vec<String> = content.lines().map(|s| s.to_string()).collect();

        // Cleanup legacy section if exists
        if let Some(legacy_idx) = lines.iter().position(|l| l == "[dev.nabaxo.ruake.desktop]") {
            let mut end_idx = legacy_idx + 1;
            while end_idx < lines.len() && !lines[end_idx].starts_with('[') {
                end_idx += 1;
            }
            lines.drain(legacy_idx..end_idx);
        }

        Ok(Self { path, lines, modified: false })
    }

    fn upsert(&mut self, app_name: &str, value: &str) {
        let section_header = "[services][dev.nabaxo.ruake.desktop]";
        let key_line = format!("{}={}", app_name, value);

        let section_idx = self.lines.iter().position(|l| l == section_header);

        if let Some(sec_idx) = section_idx {
            let mut next_section_idx = self.lines.len();
            for i in (sec_idx + 1)..self.lines.len() {
                if self.lines[i].starts_with('[') {
                    next_section_idx = i;
                    break;
                }
            }

            let mut key_found = false;
            for i in (sec_idx + 1)..next_section_idx {
                if self.lines[i].starts_with(&format!("{}=", app_name)) {
                    if self.lines[i] != key_line {
                        self.lines[i] = key_line.clone();
                        self.modified = true;
                    }
                    key_found = true;
                    break;
                }
            }

            if !key_found {
                self.lines.insert(sec_idx + 1, key_line);
                self.modified = true;
            }
        } else {
            if !self.lines.is_empty() && !self.lines.last().is_none_or(|s| s.is_empty()) {
                self.lines.push(String::new());
            }
            self.lines.push(section_header.to_string());
            self.lines.push(key_line);
            self.modified = true;
        }
    }

    fn remove(&mut self, app_name: &str) {
        let section_header = "[services][dev.nabaxo.ruake.desktop]";
        if let Some(sec_idx) = self.lines.iter().position(|l| l == section_header) {
            let mut i = sec_idx + 1;
            while i < self.lines.len() && !self.lines[i].starts_with('[') {
                if self.lines[i].starts_with(&format!("{}=", app_name)) {
                    self.lines.remove(i);
                    self.modified = true;
                } else {
                    i += 1;
                }
            }
        }
    }

    fn save(&self) -> Result<()> {
        if !self.modified { return Ok(()); }
        use std::io::Write;
        let mut file = std::fs::File::create(&self.path)?;
        for line in &self.lines {
            writeln!(file, "{}", line)?;
        }
        Ok(())
    }
}

pub async fn register_via_dbus(config: &Config, old_config: Option<&Config>) -> Result<()> {
    let component = "dev.nabaxo.ruake.desktop";
    let conn = zbus::Connection::session().await?;
    let proxy = KGlobalAccelProxy::new(&conn).await?;

    let mut sf = ShortcutFile::load()?;

    // SAFEGUARD: Collect actions to unregister first, then process with delays
    // This avoids rapid-fire D-Bus calls that can trigger KWin race conditions
    let mut actions_to_remove = Vec::new();

    if let Ok(all_actions) = proxy.all_actions_for_component(vec![component.to_string()]).await {
        for action_info in all_actions {
            if action_info.len() >= 2 {
                let action_name = &action_info[1];
                if !config.app.contains_key(action_name) {
                    actions_to_remove.push(action_name.clone());
                }
            }
        }
    }

    // SAFEGUARD: Process removals with delays between each operation
    for action_name in &actions_to_remove {
        println!("✓ Releasing shortcut for removed app: {}", action_name);
        // Delay BEFORE unregister to let KWin stabilize
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        let _ = proxy.unregister(component, action_name).await;
        sf.remove(action_name);
    }

    // SAFEGUARD: Wait after all removals before proceeding
    if !actions_to_remove.is_empty() {
        tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;
    }

    for app_name in &config.app_order {
        if let Some(app_cfg) = config.app.get(app_name) {
            let hotkeys = app_cfg.hotkey.as_vec();
            let mut normalized_parts = Vec::with_capacity(hotkeys.len());
            for hk in &hotkeys {
                normalized_parts.push(normalize_shortcut_for_kde(hk));
            }
            let normalized_joined = normalized_parts.join("\t");
            let val = format!("{},{},Toggle {}", normalized_joined, normalized_joined, app_name);
            sf.upsert(app_name, &val);
        }
    }

    sf.save()?;

    // Sync only once after all file updates
    let _ = tokio::process::Command::new("kbuildsycoca6").arg("--noincremental").status().await;

    // SAFEGUARD: Longer delay after kbuildsycoca6 to let KDE fully process changes
    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

    for app_name in &config.app_order {
        if let Some(app_cfg) = config.app.get(app_name) {
            let hotkeys = app_cfg.hotkey.as_vec();

            // SAFEGUARD: Check if we even need to touch this app's registration
            let mut needs_registration = true;
            let mut needs_shortcut_update = true;

            if let Some(old) = old_config {
                if let Some(old_app) = old.app.get(app_name) {
                    needs_registration = false; // Already registered
                    if old_app.hotkey == app_cfg.hotkey {
                        needs_shortcut_update = false; // Same keys
                    }
                }
            }

            if !needs_registration && !needs_shortcut_update {
                continue; // Skip entirely for this app
            }

            let display_name = format!("Toggle {}", app_name);
            let action_id = vec![
                component.to_string(),
                app_name.to_string(),
                "Ruake".to_string(), // Group/Category?
                display_name.clone(), // Display Name
            ];

            if needs_registration || needs_shortcut_update {
                let _ = proxy.unregister(component, app_name).await;
                tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
                let _ = proxy.do_register(action_id.clone()).await;
            }

            if needs_shortcut_update {
                let mut key_seq = vec![0; 4];
                for (i, hk) in hotkeys.iter().enumerate().take(4) {
                    key_seq[i] = shortcut_to_keycode(hk);
                }

                if key_seq[0] != 0 {
                    println!("Hotkey: Updating shortcuts for '{}'...", app_name);
                    tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
                    match proxy.set_shortcut(action_id, key_seq.clone(), 3).await {
                        Ok(res) => {
                            if res.len() < hotkeys.len() || (res.len() >= hotkeys.len() && res != key_seq) {
                                 eprintln!("WARN: KGlobalAccel rejected some shortcuts for '{}'. Conflict or KDE limit.", app_name);
                            }
                        },
                        Err(e) => eprintln!("WARN: setShortcut failed for '{}': {}", app_name, e),
                    }
                }
            }
        }
    }

    // SAFEGUARD: Final delay to let KWin fully stabilize before returning
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    Ok(())
}

pub async fn sync_kde_shortcuts(config: &Config, old_config: Option<&Config>) -> Result<()> {
    // SAFEGUARD: Debounce delay before syncing shortcuts
    // This helps avoid rapid successive calls from config reloads
    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
    register_via_dbus(config, old_config).await
}
