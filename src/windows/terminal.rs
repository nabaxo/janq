use crate::config::Config;
use std::process::{Command, Stdio};
use std::thread;
use std::time::Duration;

use crate::windows::window::find_window_by_process;



pub async fn ensure_terminal_running(config: &Config) -> bool {
    if find_window_by_process(&config.window_class).is_some() {
        return false; // Already running and has window
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
    let original_args = &parts[1..];

    let mut final_args: Vec<String> = Vec::new();

    // Inject config dimensions if running wezterm
    // WezTerm global flags must appear BEFORE the subcommand (e.g. 'start')
    if cmd.contains("wezterm") {
        final_args.push("--config".to_string());
        final_args.push(format!("initial_rows={}", config.height_rows));
        final_args.push("--config".to_string());
        final_args.push(format!("initial_cols={}", config.width_cols));
    }

    // Append original arguments (e.g. "start --class wezquake")
    for arg in original_args {
        final_args.push(arg.to_string());
    }

    use std::os::windows::process::CommandExt;
    const DETACHED_PROCESS: u32 = 0x00000008;

    match Command::new(cmd)
        .args(&final_args)
        .creation_flags(DETACHED_PROCESS)
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

    // Wait for window to appear
    for _ in 0..20 {
        if find_window_by_process(&config.window_class).is_some() {
            println!("Terminal window detected.");
            thread::sleep(Duration::from_millis(200)); // Brief grace period
            return true;
        }
        thread::sleep(Duration::from_millis(300));
    }

    false
}
