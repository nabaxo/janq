use std::fs;
use std::process::{Command, Stdio};
use std::thread;
use std::time::Duration;
use crate::config::Config;

pub async fn ensure_terminal_running(config: &Config) -> bool {
    // 1. Precise process + class check
    if check_process_running(&config.general.window_class) {
        return false;
    }

    if config.general.start_command.is_empty() {
        return false;
    }

    let mut full_cmd = config.general.start_command.clone();


    // Logic to inject flags
    if !full_cmd.contains("--class") {
        full_cmd.push_str(&format!(" --class {}", config.general.window_class));
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
        if check_process_running(&config.general.window_class) {
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

pub fn check_process_running(target_class: &str) -> bool {
    let mut cache = get_cached_pid().lock().unwrap();

    // 1. Fast path: Check cached PID
    if let Some((pid, class)) = &*cache {
         if class == target_class {
             // Verify if process still exists and verify identity
             if verify_pid_matches(*pid, target_class) {
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

                if verify_pid_matches(pid, target_class) {
                    *cache = Some((pid, target_class.to_string()));
                    return true;
                }
            }
        }
    }
    false
}

fn verify_pid_matches(pid: u32, target_class: &str) -> bool {
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
    false
}
