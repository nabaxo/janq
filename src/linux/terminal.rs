use std::fs;
use std::process::{Command, Stdio};
use std::thread;
use std::time::Duration;
use crate::config::Config;

pub async fn ensure_terminal_running(config: &Config) -> bool {
    // 1. Precise process + class check
    // Extract binary name from start command as a hint (e.g. "zed" from "zed --new")
    let cmd_parts: Vec<&str> = config.general.start_command.split_whitespace().collect();
    let process_hint = cmd_parts.first().copied();

    let process_match = if config.general.process_match.is_empty() {
        None
    } else {
        Some(config.general.process_match.as_str())
    };

    if check_process_running(&config.general.window_class, process_hint, process_match) {
        return false;
    }

    if config.general.start_command.is_empty() {
        return false;
    }

    let full_cmd = config.general.start_command.clone();

    // Logic to inject flags - REMOVED
    // We do NOT want to inject --class automatically because many apps (like Zed) don't support it.
    // If the user wants --class, they should add it to start_command.

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
        if check_process_running(&config.general.window_class, process_hint, process_match) {
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

use std::sync::OnceLock;

static CACHED_PID: OnceLock<std::sync::Mutex<Option<(u32, String)>>> = OnceLock::new();

fn get_cached_pid() -> &'static std::sync::Mutex<Option<(u32, String)>> {
    CACHED_PID.get_or_init(|| std::sync::Mutex::new(None))
}

pub fn check_process_running(target_class: &str, process_name_hint: Option<&str>, process_match: Option<&str>) -> bool {
    let mut cache = get_cached_pid().lock().unwrap();

    // 1. Fast path: Check cached PID
    if let Some((pid, class)) = &*cache {
         if class == target_class {
             // Verify if process still exists and verify identity
             if verify_pid_matches(*pid, target_class, process_name_hint, process_match) {
                 return true;
             }
         }
    }
    // If we're here, cache was invalid or empty
    *cache = None;

    // 2. Slow path: Iterate /proc
    let procs = match fs::read_dir("/proc") {
        Ok(p) => p,
        Err(_) => return false,
    };

    let my_pid = std::process::id();

    for entry in procs.flatten() {
        if let Ok(name) = entry.file_name().into_string() {
            if let Ok(pid) = name.parse::<u32>() {
                if pid == my_pid { continue; }

                if verify_pid_matches(pid, target_class, process_name_hint, process_match) {
                    *cache = Some((pid, target_class.to_string()));
                    return true;
                }
            }
        }
    }
    false
}

fn verify_pid_matches(pid: u32, target_class: &str, process_name_hint: Option<&str>, process_match: Option<&str>) -> bool {
    // Priority 0: Explicit Process Match (Configured Loophole)
    if let Some(pm) = process_match {
        if let Ok(exe) = fs::read_link(format!("/proc/{}/exe", pid)) {
            let exe_str = exe.to_string_lossy().to_lowercase();
            if exe_str.contains(&pm.to_lowercase()) {
                return true;
            }
        }
        // If process_match is set, we ONLY check that? Or do we allow fallbacks?
        // Let's allow fallbacks, but usually if this is set, it's the intended way.
        // Actually, if the user set this, they likely rely on it. But falling back corrects misconfiguration?
        // Let's safe fallback to other methods just in case.
    }

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

        // Check 1: If command line contains class, verify via EXE
        if full_cmd.contains(target_class) {
            // Check exe
            if let Ok(exe) = fs::read_link(format!("/proc/{}/exe", pid)) {
                let exe_str = exe.to_string_lossy().to_lowercase();

                // If the executable name contains the target class (e.g. "zed" contains "zed"), it's a match.
                if exe_str.contains(&target_class.to_lowercase())
                   || exe_str.contains("wezterm")
                   || exe_str.contains("alacritty")
                   || exe_str.contains("kitty") {
                    return true;
                }
            }
        }

        // Check 2: If we have a hint (binary name), check EXE directly
        // This handles cases where command line doesn't ANYWHERE mention the class (e.g. "zed" vs "dev.zed.zed")
        if let Some(hint) = process_name_hint {
            if let Ok(exe) = fs::read_link(format!("/proc/{}/exe", pid)) {
                 let exe_str = exe.to_string_lossy().to_lowercase();
                 // Matches if exe path contains the simplified hint (e.g. "/usr/bin/zed" contains "zed")
                 if exe_str.contains(&hint.to_lowercase()) {
                     return true;
                 }
            }
        }
    }
    false
}
