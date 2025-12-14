use super::types::KeyPress;
use egui::Key;

/// Render a keypress in a user-friendly format (e.g. "Ctrl + G").
pub(crate) fn format_keypress(press: &KeyPress) -> String {
    let mut parts: Vec<&'static str> = Vec::new();
    if press.command {
        parts.push(command_label());
    }
    if press.shift {
        parts.push("Shift");
    }
    if press.alt {
        parts.push("Alt");
    }
    parts.push(key_label(press.key));
    parts.join(" + ")
}

fn command_label() -> &'static str {
    if cfg!(target_os = "macos") {
        "Cmd"
    } else {
        "Ctrl"
    }
}

fn key_label(key: Key) -> &'static str {
    match key {
        egui::Key::X => "X",
        egui::Key::N => "N",
        egui::Key::D => "D",
        egui::Key::C => "C",
        egui::Key::R => "R",
        egui::Key::T => "T",
        egui::Key::U => "U",
        egui::Key::Y => "Y",
        egui::Key::Z => "Z",
        egui::Key::M => "M",
        egui::Key::Slash => "/",
        egui::Key::Backslash => "\\",
        egui::Key::Quote => "'",
        egui::Key::G => "G",
        egui::Key::S => "S",
        egui::Key::W => "W",
        egui::Key::L => "L",
        egui::Key::P => "P",
        egui::Key::F => "F",
        egui::Key::OpenBracket => "[",
        egui::Key::CloseBracket => "]",
        egui::Key::ArrowLeft => "Left",
        egui::Key::ArrowRight => "Right",
        egui::Key::ArrowUp => "Up",
        egui::Key::ArrowDown => "Down",
        _ => "Key",
    }
}

