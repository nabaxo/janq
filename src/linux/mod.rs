pub mod kwin;
pub mod terminal;
pub mod daemon;
pub mod desktop;
pub mod hotkey;

pub fn show_error(message: &str) {
    eprintln!("{}", message);

    // Escape single quotes for shell
    let escaped_message = message.replace('\'', "'\\''");
    let shell_cmd = format!("echo '{}'; echo; echo 'Press Enter to exit...'; read", escaped_message);
    let simple_echo = format!("echo '{}'", escaped_message);

    // Attempt to show in a terminal window as requested
    // We'll try some common terminals
    let terminals = vec![
        ("wezterm", vec!["start", "--", "bash", "-c", &shell_cmd]),
        ("kitty", vec!["sh", "-c", &shell_cmd]),
        ("alacritty", vec!["-e", "sh", "-c", &shell_cmd]),
        ("konsole", vec!["--noclose", "-e", "sh", "-c", &simple_echo]),
        ("gnome-terminal", vec!["--", "sh", "-c", &shell_cmd]),
        ("xterm", vec!["-hold", "-e", "sh", "-c", &simple_echo]),
    ];

    for (cmd, args) in terminals {
        if std::process::Command::new(cmd)
            .args(args)
            .spawn()
            .is_ok() {
                // Wait a bit to ensure it stays open
                std::thread::sleep(std::time::Duration::from_millis(500));
                return;
            }
    }

    // Fallback to a desktop alert if possible (zenity/kdialog)
    let _ = std::process::Command::new("zenity").args(["--error", "--text", message]).spawn();
    let _ = std::process::Command::new("kdialog").args(["--error", message]).spawn();
}
