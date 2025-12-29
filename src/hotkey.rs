use global_hotkey::hotkey::{HotKey, Modifiers, Code};
use anyhow::{Context, Result};

pub fn parse_hotkey(hotkey_str: &str) -> Result<HotKey> {
    let parts: Vec<&str> = hotkey_str.split('+').collect();
    let mut mods = Modifiers::empty();
    let mut key_code: Option<Code> = None;

    for part in parts {
        let p = part.trim().to_lowercase();
        match p.as_str() {
            "ctrl" | "control" => mods |= Modifiers::CONTROL,
            "alt" => mods |= Modifiers::ALT,
            "shift" => mods |= Modifiers::SHIFT,
            "meta" | "super" | "win" | "cmd" => mods |= Modifiers::SUPER,
            _ => {
                // Parse code
                if let Some(code) = parse_code(p.as_str()) {
                    key_code = Some(code);
                } else {
                    return Err(anyhow::anyhow!("Unknown key: {}", part));
                }
            }
        }
    }

    let code = key_code.context("No key code specified")?;
    Ok(HotKey::new(Some(mods), code))
}

fn parse_code(s: &str) -> Option<Code> {
    match s {
        "`" | "grave" | "backtick" => Some(Code::Backquote),
        "1" => Some(Code::Digit1),
        "2" => Some(Code::Digit2),
        "3" => Some(Code::Digit3),
        "4" => Some(Code::Digit4),
        "5" => Some(Code::Digit5),
        "6" => Some(Code::Digit6),
        "7" => Some(Code::Digit7),
        "8" => Some(Code::Digit8),
        "9" => Some(Code::Digit9),
        "0" => Some(Code::Digit0),
        "-" | "minus" => Some(Code::Minus),
        "=" | "equal" => Some(Code::Equal),
        "q" => Some(Code::KeyQ),
        "w" => Some(Code::KeyW),
        "e" => Some(Code::KeyE),
        "r" => Some(Code::KeyR),
        "t" => Some(Code::KeyT),
        "y" => Some(Code::KeyY),
        "u" => Some(Code::KeyU),
        "i" => Some(Code::KeyI),
        "o" => Some(Code::KeyO),
        "p" => Some(Code::KeyP),
        "[" | "bracketleft" => Some(Code::BracketLeft),
        "]" | "bracketright" => Some(Code::BracketRight),
        "\\" | "backslash" => Some(Code::Backslash),
        "a" => Some(Code::KeyA),
        "s" => Some(Code::KeyS),
        "d" => Some(Code::KeyD),
        "f" => Some(Code::KeyF),
        "g" => Some(Code::KeyG),
        "h" => Some(Code::KeyH),
        "j" => Some(Code::KeyJ),
        "k" => Some(Code::KeyK),
        "l" => Some(Code::KeyL),
        ";" | "semicolon" => Some(Code::Semicolon),
        "'" | "quote" => Some(Code::Quote),
        "enter" | "return" => Some(Code::Enter),
        "z" => Some(Code::KeyZ),
        "x" => Some(Code::KeyX),
        "c" => Some(Code::KeyC),
        "v" => Some(Code::KeyV),
        "b" => Some(Code::KeyB),
        "n" => Some(Code::KeyN),
        "m" => Some(Code::KeyM),
        "," | "comma" => Some(Code::Comma),
        "." | "period" => Some(Code::Period),
        "/" | "slash" => Some(Code::Slash),
        "space" => Some(Code::Space),
        "esc" | "escape" => Some(Code::Escape),
        "tab" => Some(Code::Tab),
        "capslock" | "caps_lock" => Some(Code::CapsLock),
        "backspace" => Some(Code::Backspace),
        "up" | "arrowup" => Some(Code::ArrowUp),
        "down" | "arrowdown" => Some(Code::ArrowDown),
        "left" | "arrowleft" => Some(Code::ArrowLeft),
        "right" | "arrowright" => Some(Code::ArrowRight),
        "pgup" | "pageup" => Some(Code::PageUp),
        "pgdn" | "pagedown" => Some(Code::PageDown),
        "home" => Some(Code::Home),
        "end" => Some(Code::End),
        "insert" => Some(Code::Insert),
        "delete" | "del" => Some(Code::Delete),
        "f1" => Some(Code::F1),
        "f2" => Some(Code::F2),
        "f3" => Some(Code::F3),
        "f4" => Some(Code::F4),
        "f5" => Some(Code::F5),
        "f6" => Some(Code::F6),
        "f7" => Some(Code::F7),
        "f8" => Some(Code::F8),
        "f9" => Some(Code::F9),
        "f10" => Some(Code::F10),
        "f11" => Some(Code::F11),
        "f12" => Some(Code::F12),
        "§" | "section" => Some(Code::IntlBackslash),
        "±" | "plusminus" => Some(Code::IntlBackslash),
        "dead_grave" => Some(Code::Backquote),
        _ => None,
    }
}
