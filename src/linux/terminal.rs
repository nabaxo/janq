use std::fs;
use std::process::{Command, Stdio};
use tokio;
use std::time::Duration;
use zbus::Connection;
use crate::config::{Config, AppConfig};

pub async fn ensure_terminal_running(app_cfg: &AppConfig, config: &Config, conn: &Connection) -> bool {
    let window_class = &app_cfg.window_class;
    let start_command = &app_cfg.start_command;

    // 1. Check if window already exists
    if check_window_exists(window_class).is_some() {
        return false;
    }

    // 2. Check if process is already running
    let process_running = check_process_running(window_class);

    if start_command.is_empty() {
        eprintln!("Ruake: No start_command for app with class '{}'", window_class);
        return false;
    }

    // If process is running but no window, we still want to try starting it
    // (e.g. for terminals that open new windows on command even if daemon is running)
    if process_running {
        println!("Ruake: Process for '{}' exists but no window found. Attempting to start/reanimate...", window_class);
    }

    let full_cmd = start_command.clone();

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

    // Wait for window to appear (more reliable than just process)
    for i in 0..20 {
        if let Some(_id) = check_window_exists(window_class) {
            // Give it a moment to finalize
            tokio::time::sleep(Duration::from_millis(500)).await;
            // Call ensure_grabbed (async)
            let _ = crate::linux::kwin::ensure_grabbed(app_cfg, config, conn).await;
            return true;
        }
        if i % 5 == 0 && i > 0 {
             println!("Ruake: Still waiting for window '{}' to appear (attempt {}/20)...", window_class, i);
        }
        tokio::time::sleep(Duration::from_millis(400)).await;
    }

    // Fallback: check if process is at least running
    if check_process_running(window_class) {
         println!("Ruake: Process for '{}' is running, but no window appeared after 8 seconds. This might be a configuration issue.", window_class);
         return true;
    }

    println!("Ruake: Failed to detect process or window for '{}' after spawning.", window_class);
    false
}

pub fn check_window_exists(target_class: &str) -> Option<String> {
    // We use kdotool to search for windows with this class
    let output = Command::new("sh")
        .arg("-c")
        .arg(format!("kdotool search --class '{}'", target_class))
        .output();

    match output {
        Ok(out) if out.status.success() => {
            let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if !s.is_empty() {
                return s.lines().next().map(|l| l.to_string());
            }
        }
        _ => {
            // Check if kdotool is installed
            if Command::new("kdotool").arg("--version").output().is_err() {
                 let msg = "CRITICAL: 'kdotool' is missing!\nRuake requires 'kdotool' for window detection and management.\nPlease install it (e.g. 'sudo pacman -S kdotool' or 'sudo apt install kdotool').";
                 eprintln!("Ruake: {}", msg);
                 crate::linux::show_error(msg);
            }
        }
    }
    None
}

use std::sync::OnceLock;
use std::collections::HashMap;

static PID_CACHE: OnceLock<std::sync::Mutex<HashMap<String, u32>>> = OnceLock::new();

fn get_pid_cache() -> &'static std::sync::Mutex<HashMap<String, u32>> {
    PID_CACHE.get_or_init(|| std::sync::Mutex::new(HashMap::new()))
}

pub fn check_process_running(target_class: &str) -> bool {
    let mut cache = get_pid_cache().lock().unwrap();

    // 1. Fast path: Check cached PID for this specific class
    if let Some(&pid) = cache.get(target_class) {
         // Fast liveness check: just check if the directory exists
         // This is much faster than reading cmdline every time.
         if std::path::Path::new(&format!("/proc/{}", pid)).exists() {
             return true;
         }
    }
    // If we're here, cache was invalid or empty for this class
    cache.remove(target_class);

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
                    cache.insert(target_class.to_string(), pid);
                    return true;
                }
            }
        }
    }
    false
}


fn verify_pid_matches(pid: u32, target_class: &str) -> bool {
    // Pre-compute lowercase target once
    let target_lower = target_class.to_lowercase();
    let target_dash_prefix = format!("{}-", target_lower);
    let target_dash_suffix = format!("-{}", target_lower);

    let mut path_buf = String::with_capacity(32);
    use std::fmt::Write;
    let _ = write!(path_buf, "/proc/{}/cmdline", pid);

    if let Ok(cmdline) = fs::read(&path_buf) {
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

        if full_cmd.to_lowercase().contains(&target_lower) {
            // Check exe
            path_buf.clear();
            let _ = write!(path_buf, "/proc/{}/exe", pid);
            if let Ok(exe) = fs::read_link(&path_buf) {
                let exe_str = exe.to_string_lossy().to_lowercase();

                // 1. General check: match filename against target class (Prefix/Suffix/Exact)
                let exe_name = std::path::Path::new(&exe_str)
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or(&exe_str)
                    .to_lowercase();

                if exe_name == target_lower
                   || exe_name.starts_with(&target_dash_prefix)
                   || exe_name.starts_with(&target_lower)
                   || exe_name.ends_with(&target_dash_suffix)
                {
                    return true;
                }
                // 2. Flatpak/Wrapper check
                if (exe_str.contains("flatpak") || exe_str.contains("bwrap") || exe_str.contains("snap"))
                   && !exe_str.contains("steam")
                {
                     return true;
                }
            }
        }
    }
    false
}
