use crate::config::Config;
use std::process::{Command, Stdio};
use std::time::Duration;
use crate::windows::window::find_window_by_process;
use std::sync::atomic::{AtomicBool, Ordering};

// Global guard for preventing multiple spawns
static IS_SPAWNING: AtomicBool = AtomicBool::new(false);

pub async fn ensure_terminal_running(config: &Config) -> bool {
    // Loop to acquire lock or check existing window
    loop {
        // 1. Check if window already exists
        if find_window_by_process(&config.window_class).is_some() {
            return false; // Already running and has window
        }

        // 2. Try to take the spawn lock
        if IS_SPAWNING.compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst).is_ok() {
             break; // Got lock, proceed to spawn
        }

        // 3. Wait if someone else is spawning
        println!("DEBUG: Spawn already in progress, waiting...");
        tokio::time::sleep(Duration::from_millis(200)).await;
    }

    // -- CRITICAL SECTION --
    // We own the spawn lock.
    println!("DEBUG: Acquired spawn lock. spawning...");

    // Double check (in case window appeared just before lock)
    if find_window_by_process(&config.window_class).is_some() {
         IS_SPAWNING.store(false, Ordering::SeqCst);
         return false;
    }

    if config.start_command.is_empty() {
        IS_SPAWNING.store(false, Ordering::SeqCst);
        return false;
    }

    println!("Starting terminal: {}", config.start_command);

    // On Windows, start_command might need cmd /C or just running executable
    // Split command
    let parts: Vec<&str> = config.start_command.split_whitespace().collect();
    if parts.is_empty() {
        IS_SPAWNING.store(false, Ordering::SeqCst);
        return false;
    }

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

    let spawn_result = Command::new(cmd)
        .args(&final_args)
        .creation_flags(DETACHED_PROCESS)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();

    match spawn_result {
            Ok(_) => {},
            Err(e) => {
                println!("Failed to start terminal: {}", e);
                IS_SPAWNING.store(false, Ordering::SeqCst);
                return false;
            }
    }

    // Wait for window to appear
    let mut found = false;
    for _ in 0..40 { // Wait up to 8s (200ms * 40)
        tokio::time::sleep(Duration::from_millis(200)).await;
        if find_window_by_process(&config.window_class).is_some() {
            println!("Terminal window detected.");
            found = true;
            break;
        }
    }

    IS_SPAWNING.store(false, Ordering::SeqCst);
    found
}
