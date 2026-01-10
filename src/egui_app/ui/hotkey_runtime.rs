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

impl EguiApp {
    pub(super) fn process_hotkeys(&mut self, ctx: &egui::Context, focus: FocusContext) {
        let overlay_open = self.controller.ui.hotkeys.overlay_visible;
        if self.controller.ui.hotkeys.suppress_for_bpm_input {
            return;
        }
        let collection_rename_active = matches!(
            self.controller.ui.collections.pending_action,
            Some(crate::egui_app::state::CollectionActionPrompt::Rename { .. })
        );
        let folder_rename_active = matches!(
            self.controller.ui.sources.folders.pending_action,
            Some(crate::egui_app::state::FolderActionPrompt::Rename { .. })
        );
        let folder_create_active = self.controller.ui.sources.folders.new_folder.is_some();
        let browser_rename_active = matches!(
            self.controller.ui.browser.pending_action,
            Some(crate::egui_app::state::SampleBrowserActionPrompt::Rename { .. })
        );
        if collection_rename_active
            || folder_rename_active
            || folder_create_active
            || browser_rename_active
        {
            return;
        }
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
            let Some(key_event) = keypress_from_event(&event) else {
                continue;
            };
            let press = key_event.press;
            self.key_feedback.last_key = Some(press);
            if wants_text_input && !press.command && !press_text_variants(&press).is_empty() {
                continue;
            }
            if key_event.repeat && self.pending_chord.is_some() {
                consume_press(ctx, press);
                continue;
            }
            if self.try_handle_chord(ctx, &actions, press, focus, now, key_event.repeat) {
                continue;
            }
            if self.try_start_chord(
                ctx,
                &actions,
                press,
                now,
                wants_text_input,
                key_event.repeat,
            ) {
                continue;
            }
            if self.try_handle_folder_hotkey(ctx, press, focus, key_event.repeat) {
                continue;
            }
            if let Some(action) = actions
                .iter()
                .find(|action| {
                    action.gesture.chord.is_none() && press_matches(&press, &action.gesture.first)
                })
                .copied()
            {
                consume_press(ctx, press);
                if !key_event.repeat {
                    self.controller.handle_hotkey(action, focus);
                }
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
        repeat: bool,
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
            if !repeat {
                self.controller.handle_hotkey(action, focus);
            }
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
        repeat: bool,
    ) -> bool {
        if wants_text_input || repeat {
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

    fn try_handle_folder_hotkey(
        &mut self,
        ctx: &egui::Context,
        press: hotkeys::KeyPress,
        focus: FocusContext,
        repeat: bool,
    ) -> bool {
        if repeat || press.command || press.shift || press.alt {
            return false;
        }
        let Some(hotkey) = hotkey_number_for_key(press.key) else {
            return false;
        };
        if self.controller.apply_folder_hotkey(hotkey, focus) {
            consume_press(ctx, press);
            return true;
        }
        false
    }
}

#[derive(Clone, Copy, Debug)]
struct KeyEventPress {
    press: hotkeys::KeyPress,
    repeat: bool,
}

fn keypress_from_event(event: &egui::Event) -> Option<KeyEventPress> {
    match event {
        egui::Event::Key {
            key,
            pressed: true,
            repeat,
            modifiers,
            ..
        } => {
            let command = if cfg!(target_os = "macos") {
                modifiers.command
            } else {
                modifiers.ctrl
            };
            Some(KeyEventPress {
                press: hotkeys::KeyPress {
                    key: *key,
                    command,
                    shift: modifiers.shift,
                    alt: modifiers.alt,
                },
                repeat: *repeat,
            })
        }
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
        egui::Key::Num0 => &["0"],
        egui::Key::Num1 => &["1"],
        egui::Key::Num2 => &["2"],
        egui::Key::Num3 => &["3"],
        egui::Key::Num4 => &["4"],
        egui::Key::Num5 => &["5"],
        egui::Key::Num6 => &["6"],
        egui::Key::Num7 => &["7"],
        egui::Key::Num8 => &["8"],
        egui::Key::Num9 => &["9"],
        egui::Key::X => &["x", "X"],
        egui::Key::N => &["n", "N"],
        egui::Key::D => &["d", "D"],
        egui::Key::C => &["c", "C"],
        egui::Key::M => &["m", "M"],
        egui::Key::B => &["b", "B"],
        egui::Key::T => &["t", "T"],
        egui::Key::U => &["u", "U"],
        egui::Key::Y => &["y", "Y"],
        egui::Key::Z => &["z", "Z"],
        egui::Key::F => &["f", "F"],
        egui::Key::Slash => &["/", "?"],
        egui::Key::Backslash => &["\\", "|"],
        egui::Key::Quote => &["'", "\""],
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
    let modifiers = keypress_modifiers(&press);
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

fn keypress_modifiers(press: &hotkeys::KeyPress) -> egui::Modifiers {
    let mut modifiers = egui::Modifiers::default();
    modifiers.alt = press.alt;
    modifiers.shift = press.shift;
    if cfg!(target_os = "macos") {
        modifiers.command = press.command;
    } else {
        modifiers.ctrl = press.command;
    }
    modifiers
}

fn hotkey_number_for_key(key: egui::Key) -> Option<u8> {
    match key {
        egui::Key::Num0 => Some(0),
        egui::Key::Num1 => Some(1),
        egui::Key::Num2 => Some(2),
        egui::Key::Num3 => Some(3),
        egui::Key::Num4 => Some(4),
        egui::Key::Num5 => Some(5),
        egui::Key::Num6 => Some(6),
        egui::Key::Num7 => Some(7),
        egui::Key::Num8 => Some(8),
        egui::Key::Num9 => Some(9),
        _ => None,
    }
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
