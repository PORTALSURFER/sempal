use crate::egui_app::state::FocusContext;
use eframe::egui;

use super::super::EguiApp;
use super::super::input::{InputSnapshot, copy_shortcut_pressed, paste_shortcut_pressed};
use super::consume_keypress;

pub(super) struct FocusFlags {
    pub(super) browser: bool,
    pub(super) folder: bool,
    pub(super) waveform: bool,
    pub(super) sources: bool,
    pub(super) collection_sample: bool,
    pub(super) collections_list: bool,
}

impl FocusFlags {
    pub(super) fn from_context(context: FocusContext) -> Self {
        Self {
            browser: matches!(context, FocusContext::SampleBrowser | FocusContext::None),
            folder: matches!(context, FocusContext::SourceFolders),
            waveform: matches!(context, FocusContext::Waveform),
            sources: matches!(context, FocusContext::SourcesList),
            collection_sample: matches!(context, FocusContext::CollectionSample),
            collections_list: matches!(context, FocusContext::CollectionsList),
        }
    }
}

impl EguiApp {
    pub(super) fn handle_focus_side_effects(&mut self, focus: &FocusFlags) {
        if !focus.browser && !focus.waveform {
            self.controller.blur_browser_focus();
        }
        if focus.collection_sample {
            self.controller.ui.browser.autoscroll = false;
        }
    }

    pub(super) fn handle_space_shortcut(&mut self, ctx: &egui::Context, input: &InputSnapshot) {
        if !input.space {
            return;
        }
        if ctx.wants_keyboard_input() {
            return;
        }
        let ctrl_or_command = input.ctrl_or_command();
        if input.shift {
            let handled = self.controller.replay_from_last_start();
            if !handled {
                self.controller.toggle_play_pause();
            }
        } else if ctrl_or_command {
            let handled = self.controller.play_from_cursor();
            if !handled {
                self.controller.toggle_play_pause();
            }
        } else {
            self.controller.toggle_play_pause();
        }
        consume_keypress(ctx, input, egui::Key::Space);
    }

    pub(super) fn handle_copy_shortcut(&mut self, ctx: &egui::Context) {
        if copy_shortcut_pressed(ctx) {
            self.controller.copy_selection_to_clipboard();
        }
    }

    pub(super) fn handle_paste_shortcut(&mut self, ctx: &egui::Context) {
        if !paste_shortcut_pressed(ctx) {
            return;
        }
        if ctx.wants_keyboard_input() {
            return;
        }
        let handled = self.controller.paste_files_from_clipboard();
        if handled {
            ctx.input_mut(|state| {
                let mut modifiers = egui::Modifiers::default();
                if cfg!(target_os = "macos") {
                    modifiers.command = true;
                } else {
                    modifiers.ctrl = true;
                }
                state.consume_key(modifiers, egui::Key::V);
                state
                    .events
                    .retain(|event| !matches!(event, egui::Event::Paste(_)));
            });
        }
    }

    pub(super) fn handle_escape_shortcut(&mut self, ctx: &egui::Context, input: &InputSnapshot) {
        if !input.escape {
            return;
        }
        if self.controller.ui.progress.visible {
            self.controller.request_progress_cancel();
        }
        self.controller.handle_escape();
        if self.controller.ui.hotkeys.overlay_visible {
            self.controller.ui.hotkeys.overlay_visible = false;
            ctx.input_mut(|state| state.consume_key(egui::Modifiers::default(), egui::Key::Escape));
        }
        ctx.input_mut(|state| state.consume_key(egui::Modifiers::default(), egui::Key::Escape));
    }

    pub(super) fn handle_window_shortcuts(&self, ctx: &egui::Context) {
        if let Some(new_maximized) = ctx.input(|i| {
            if i.key_pressed(egui::Key::F11) {
                Some(!i.viewport().maximized.unwrap_or(false))
            } else {
                None
            }
        }) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(new_maximized));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::InputSnapshot;
    use crate::egui_app::controller::EguiController;
    use crate::egui_app::ui::EguiApp;
    use crate::egui_app::ui::hotkey_runtime::KeyFeedback;
    use crate::waveform::WaveformRenderer;
    use eframe::egui;

    fn test_app() -> EguiApp {
        let renderer = WaveformRenderer::new(8, 8);
        let controller = EguiController::new(renderer, None);
        EguiApp {
            controller,
            visuals_set: false,
            waveform_tex: None,
            last_viewport_log: None,
            sources_panel_rect: None,
            sources_panel_drop_hovered: false,
            sources_panel_drop_armed: false,
            selection_edge_offset: None,
            selection_edge_alt_scale: false,
            selection_slide: None,
            edit_selection_slide: None,
            edit_selection_gain_drag: None,
            slice_drag: None,
            slice_paint: None,
            pending_chord: None,
            key_feedback: KeyFeedback::default(),
            requested_initial_focus: false,
            external_drop_handled: false,
            external_drop_hover_pos: None,
        }
    }

    #[test]
    fn ctrl_shift_space_prefers_replay_from_last_start_over_cursor() {
        let ctx = egui::Context::default();
        let mut app = test_app();
        app.controller.ui.waveform.last_start_marker = Some(0.8);
        app.controller.ui.waveform.cursor = Some(0.2);

        app.handle_space_shortcut(
            &ctx,
            &InputSnapshot {
                space: true,
                ctrl: true,
                shift: true,
                ..InputSnapshot::default()
            },
        );

        assert_eq!(app.controller.ui.waveform.last_start_marker, Some(0.8));
    }
}
