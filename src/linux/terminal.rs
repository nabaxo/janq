use std::fs;
use std::process::{Command, Stdio};
use std::thread;
use std::time::Duration;
use crate::config::Config;

pub async fn ensure_terminal_running(config: &Config) -> bool {
    // 1. Precise process + class check
    if check_process_running(&config.window_class) {
        return false;
    }

    if config.start_command.is_empty() {
        return false;
    }

    let mut full_cmd = config.start_command.clone();
    let lower_cmd = full_cmd.to_lowercase();

    // Logic to inject flags
    if lower_cmd.contains("wezterm") {
        let mut flags = String::new();
        if !full_cmd.contains("--class") {
            flags.push_str(&format!(" --class {}", config.window_class));
        }
        if config.width_cols > 0 {
            flags.push_str(&format!(" --config initial_cols={}", config.width_cols));
        }
        if config.height_rows > 0 {
            flags.push_str(&format!(" --config initial_rows={}", config.height_rows));
        }

        if !flags.is_empty() {
             // Try to insert after 'wezterm'
             if let Some(idx) = lower_cmd.find("wezterm") {
                 // Try to find the end of the word "wezterm" or "wezterm-gui"
                 // Simple approach: find next space after idx
                 let rest = &full_cmd[idx..];
                 if let Some(space_idx) = rest.find(' ') {
                     let insert_idx = idx + space_idx;
                     full_cmd.insert_str(insert_idx, &flags);
                 } else {
                     full_cmd.push_str(&flags);
                 }
             } else {
                 full_cmd.push_str(&flags);
             }
        }
    } else if !full_cmd.contains("--class") {
        full_cmd.push_str(&format!(" --class {}", config.window_class));
    }

    println!("Starting terminal: {}", full_cmd);

    // Use sh -c
    match Command::new("sh")
        .arg("-c")
        .arg(&full_cmd)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn() {
            Ok(_) => {},
            Err(e) => {
                println!("Failed to start terminal: {}", e);
                return false;
            }
        }

    // Wait for process to appear
    for _ in 0..20 {
        if check_process_running(&config.window_class) {
            println!("Terminal process detected.");
            // Give it time to map the window
            thread::sleep(Duration::from_secs(1));
            // Call ensure_grabbed (async)
            let _ = crate::linux::kwin::ensure_grabbed(config).await;
            return true;
        }
        thread::sleep(Duration::from_millis(300));
    }
    false
}

pub fn check_process_running(target_class: &str) -> bool {
    // Iterate /proc
    let procs = match fs::read_dir("/proc") {
        Ok(p) => p,
        Err(_) => return false,
    };

    let my_pid = std::process::id();

    for entry in procs.flatten() {
        if let Ok(name) = entry.file_name().into_string() {
            if let Ok(pid) = name.parse::<u32>() {
                if pid == my_pid { continue; }

                let cmdline_path = format!("/proc/{}/cmdline", pid);
                if let Ok(cmdline) = fs::read(cmdline_path) {
                    // Split by null byte
                    let parts: Vec<&[u8]> = cmdline.split(|&b| b == 0).collect();

                    for (i, part) in parts.iter().enumerate() {
                        let s = String::from_utf8_lossy(part);
                        // Match exact --class arg
                        if s == "--class" && i + 1 < parts.len() {
                            let next = String::from_utf8_lossy(parts[i+1]);
                            if next.eq_ignore_ascii_case(target_class) {
                                return true;
                            }
                        }
                        // Match --class=foo
                        if s.to_lowercase().starts_with("--class=")
                            && s[8..].eq_ignore_ascii_case(target_class) {
                            return true;
                        }
                    }

                    // Fallback logic
                    let full_cmd_binding = cmdline.iter().map(|&b| if b == 0 { 32 } else { b }).collect::<Vec<u8>>();
                    let full_cmd = String::from_utf8_lossy(&full_cmd_binding);

                    if full_cmd.contains(target_class) {
                        // Check exe
                        if let Ok(exe) = fs::read_link(format!("/proc/{}/exe", pid)) {
                            let exe_str = exe.to_string_lossy().to_lowercase();
                            if exe_str.contains("wezterm") || exe_str.contains("alacritty") || exe_str.contains("kitty") {
                                return true;
                            }
                        }
                    }
                }
            }
        }
    }
    false
}
