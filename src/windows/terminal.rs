use crate::config::AppConfig;
use std::process::{Command, Stdio};
use std::time::Duration;
use crate::windows::window::find_window_by_process;
use std::sync::atomic::{AtomicBool, Ordering};

// Global guard for preventing multiple spawns
static IS_SPAWNING: AtomicBool = AtomicBool::new(false);

pub async fn ensure_terminal_running(app_cfg: &AppConfig) -> bool {
    // Loop to acquire lock or check existing window
    loop {
        // 1. Check if window already exists
        if find_window_by_process(&app_cfg.window_class).is_some() {
            return false; // Already running and has window
        }

        // 2. Try to take the spawn lock
        if IS_SPAWNING.compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst).is_ok() {
             break; // Got lock, proceed to spawn
        }

        tokio::time::sleep(Duration::from_millis(200)).await;
    }

    // -- CRITICAL SECTION --
    // We own the spawn lock.

    // Double check (in case window appeared just before lock)
    if find_window_by_process(&app_cfg.window_class).is_some() {
         IS_SPAWNING.store(false, Ordering::SeqCst);
         return false;
    }

    if app_cfg.start_command.is_empty() {
        IS_SPAWNING.store(false, Ordering::SeqCst);
        return false;
    }

    println!("Starting terminal: {}", app_cfg.start_command);

    // On Windows, start_command might need cmd /C or just running executable
    // Split command
    let parts: Vec<&str> = app_cfg.start_command.split_whitespace().collect();
    if parts.is_empty() {
        IS_SPAWNING.store(false, Ordering::SeqCst);
        return false;
    }

    let cmd = parts[0];
    let final_args = &parts[1..];

    use std::os::windows::process::CommandExt;
    const DETACHED_PROCESS: u32 = 0x00000008;

    let spawn_result = Command::new(cmd)
        .args(final_args)
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
        if find_window_by_process(&app_cfg.window_class).is_some() {
            found = true;
            break;
        }
    }

    IS_SPAWNING.store(false, Ordering::SeqCst);
    found
}
