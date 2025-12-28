use crate::config::Config;
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
};
use windows::Win32::Foundation::CloseHandle;
use std::process::{Command, Stdio};
use std::thread;
use std::time::Duration;

pub async fn ensure_terminal_running(config: &Config) -> bool {
    if check_process_running(&config.window_class) {
        return false; // Already running
    }

    if config.start_command.is_empty() {
        return false;
    }

    println!("Starting terminal: {}", config.start_command);

    // On Windows, start_command might need cmd /C or just running executable
    // Split command
    let parts: Vec<&str> = config.start_command.split_whitespace().collect();
    if parts.is_empty() { return false; }

    let cmd = parts[0];
    let args = &parts[1..];

    match Command::new(cmd)
        .args(args)
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

    // Wait for process
    for _ in 0..20 {
        if check_process_running(&config.window_class) {
            println!("Terminal process detected.");
            thread::sleep(Duration::from_secs(1));
            // Ensure window is created? toggle logic will handle it.
            return true;
        }
        thread::sleep(Duration::from_millis(300));
    }

    false
}

pub fn check_process_running(target_name: &str) -> bool {
    unsafe {
        let snapshot_result = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        let snapshot = match snapshot_result {
            Ok(handle) => handle,
            Err(_) => return false,
        };
        // Check for invalid handle if applicable (Windows crate handles are usually Result vs specific InvalidHandle value, but here Result handles errors)

        let mut entry = PROCESSENTRY32W {
            dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };

        if Process32FirstW(snapshot, &mut entry).is_ok() {
            loop {
                // szExeFile is [u16; 260]
                let name = String::from_utf16_lossy(&entry.szExeFile);
                let name_trimmed = name.trim_matches(char::from(0));

                if name_trimmed.to_lowercase().contains(&target_name.to_lowercase()) {
                    let _ = CloseHandle(snapshot);
                    return true;
                }

                if Process32NextW(snapshot, &mut entry).is_err() {
                    break;
                }
            }
        }
        let _ = CloseHandle(snapshot);
    }
    false
}
