use crate::egui_app::controller::hotkeys;
use crate::egui_app::state::FocusContext;
use crate::egui_app::ui::EguiApp;
use eframe::egui;
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug)]
pub(super) struct PendingChord {
    pub first: hotkeys::KeyPress,
    pub started_at: Instant,
}

pub(super) const CHORD_TIMEOUT: Duration = Duration::from_millis(900);

#[derive(Default)]
pub(super) struct KeyFeedback {
    pub last_key: Option<hotkeys::KeyPress>,
    pub pending_root: Option<hotkeys::KeyPress>,
    pub last_chord: Option<(hotkeys::KeyPress, hotkeys::KeyPress)>,
}

#[inline]
pub(super) fn format_keypress(press: &Option<hotkeys::KeyPress>) -> String {
    press
        .as_ref()
        .map(hotkeys::format_keypress)
        .unwrap_or_else(|| "—".to_string())
}

impl EguiApp {
    pub(super) fn process_hotkeys(&mut self, ctx: &egui::Context, focus: FocusContext) {
        let overlay_open = self.controller.ui.hotkeys.overlay_visible;
        let wants_text_input = ctx.wants_keyboard_input();
        let actions: Vec<_> = hotkeys::iter_actions()
            .filter(|action| (!overlay_open || action.is_global()) && action.is_active(focus))
            .collect();
        if actions.is_empty() {
            self.pending_chord = None;
            self.key_feedback.pending_root = None;
            return;
        }
        let now = Instant::now();
        if let Some(pending) = self.pending_chord
            && now.saturating_duration_since(pending.started_at) > CHORD_TIMEOUT
        {
            self.pending_chord = None;
            self.key_feedback.pending_root = None;
        }
        let events = ctx.input(|i| i.events.clone());
        for event in events {
            let Some(press) = keypress_from_event(&event) else {
                continue;
            };
            self.key_feedback.last_key = Some(press);
            if wants_text_input && !press.command {
                continue;
            }
            if self.try_handle_chord(ctx, &actions, press, focus, now) {
                continue;
            }
            if self.try_start_chord(ctx, &actions, press, now, wants_text_input) {
                continue;
            }
            if let Some(action) = actions
                .iter()
                .find(|action| {
                    action.gesture.chord.is_none() && press_matches(&press, &action.gesture.first)
                })
                .copied()
            {
                self.controller.handle_hotkey(action, focus);
                consume_press(ctx, press);
                continue;
            }
        }
    }

    fn try_handle_chord(
        &mut self,
        ctx: &egui::Context,
        actions: &[hotkeys::HotkeyAction],
        press: hotkeys::KeyPress,
        focus: FocusContext,
        now: Instant,
    ) -> bool {
        let Some(pending) = self.pending_chord else {
            return false;
        };
        if now.saturating_duration_since(pending.started_at) > CHORD_TIMEOUT {
            self.pending_chord = None;
            return false;
        }
        if let Some(action) = actions
            .iter()
            .find(|action| {
                action
                    .gesture
                    .chord
                    .is_some_and(|second| press_matches(&press, &second))
                    && press_matches(&pending.first, &action.gesture.first)
            })
            .copied()
        {
            self.pending_chord = None;
            self.key_feedback.last_chord = Some((pending.first, press));
            self.key_feedback.pending_root = None;
            consume_press(ctx, pending.first);
            consume_press(ctx, press);
            self.controller.handle_hotkey(action, focus);
            return true;
        }
        self.pending_chord = None;
        self.key_feedback.pending_root = None;
        false
    }

    fn try_start_chord(
        &mut self,
        ctx: &egui::Context,
        actions: &[hotkeys::HotkeyAction],
        press: hotkeys::KeyPress,
        now: Instant,
        wants_text_input: bool,
    ) -> bool {
        if wants_text_input {
            return false;
        }
        let starts_chord = actions.iter().any(|action| {
            action
                .gesture
                .chord
                .is_some_and(|_| press_matches(&press, &action.gesture.first))
        });
        if starts_chord {
            self.pending_chord = Some(PendingChord {
                first: press,
                started_at: now,
            });
            self.key_feedback.pending_root = Some(press);
            consume_press(ctx, press);
            return true;
        }
        false
    }
}

fn keypress_from_event(event: &egui::Event) -> Option<hotkeys::KeyPress> {
    match event {
        egui::Event::Key {
            key,
            pressed: true,
            repeat: false,
            modifiers,
            ..
        } => Some(hotkeys::KeyPress {
            key: *key,
            command: modifiers.command || modifiers.ctrl,
            shift: modifiers.shift,
            alt: modifiers.alt,
        }),
        _ => None,
    }
}

fn press_matches(press: &hotkeys::KeyPress, target: &hotkeys::KeyPress) -> bool {
    press.key == target.key
        && press.command == target.command
        && press.shift == target.shift
        && press.alt == target.alt
}

fn press_text_variants(press: &hotkeys::KeyPress) -> &'static [&'static str] {
    match press.key {
        egui::Key::X => &["x", "X"],
        egui::Key::N => &["n", "N"],
        egui::Key::D => &["d", "D"],
        egui::Key::C => &["c", "C"],
        egui::Key::T => &["t", "T"],
        egui::Key::Slash => &["/", "?"],
        egui::Key::Backslash => &["\\", "|"],
        egui::Key::G => &["g", "G"],
        egui::Key::S => &["s", "S"],
        egui::Key::W => &["w", "W"],
        egui::Key::L => &["l", "L"],
        egui::Key::P => &["p", "P"],
        egui::Key::OpenBracket => &["[", "{"],
        egui::Key::CloseBracket => &["]", "}"],
        _ => &[],
    }
}

fn consume_press(ctx: &egui::Context, press: hotkeys::KeyPress) {
    let modifiers = egui::Modifiers {
        alt: press.alt,
        shift: press.shift,
        command: press.command,
        ctrl: press.command,
        ..Default::default()
    };
    ctx.input_mut(|i| {
        i.consume_key(modifiers, press.key);
        let text_variants = press_text_variants(&press);
        if !text_variants.is_empty() {
            i.events.retain(|event| {
                !matches!(event, egui::Event::Text(text) if text_variants
                    .iter()
                    .any(|candidate| text.eq_ignore_ascii_case(candidate)))
            });
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn consume_press_drops_hotkey_events() {
        let ctx = egui::Context::default();
        let press = hotkeys::KeyPress::new(egui::Key::N);
        ctx.input_mut(|i| {
            i.events.push(egui::Event::Key {
                key: egui::Key::N,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::default(),
            });
            i.events.push(egui::Event::Text(String::from("n")));
            i.events.push(egui::Event::PointerGone);
        });

        consume_press(&ctx, press);

        let remaining = ctx.input(|i| i.events.clone());
        assert_eq!(remaining.len(), 1);
        assert!(matches!(remaining[0], egui::Event::PointerGone));
    }

    #[test]
    fn consume_press_removes_uppercase_text() {
        let ctx = egui::Context::default();
        let press = hotkeys::KeyPress::new(egui::Key::C);
        ctx.input_mut(|i| {
            i.events.push(egui::Event::Key {
                key: egui::Key::C,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::default(),
            });
            i.events.push(egui::Event::Text(String::from("C")));
        });

        consume_press(&ctx, press);

        let remaining = ctx.input(|i| i.events.clone());
        assert!(remaining.is_empty());
    }

    #[test]
    fn consume_press_removes_backslash_text() {
        let ctx = egui::Context::default();
        let press = hotkeys::KeyPress::new(egui::Key::Backslash);
        ctx.input_mut(|i| {
            i.events.push(egui::Event::Key {
                key: egui::Key::Backslash,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::default(),
            });
            i.events.push(egui::Event::Text(String::from("\\")));
        });

        consume_press(&ctx, press);

        let remaining = ctx.input(|i| i.events.clone());
        assert!(remaining.is_empty());
    }
}
