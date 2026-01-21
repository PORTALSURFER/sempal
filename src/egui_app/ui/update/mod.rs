use eframe::egui;

use super::EguiApp;
use super::input::{InputSnapshot, user_activity_detected};
use update_prompt::FocusFlags;

mod release_notes;
mod update_progress;
mod update_prompt;

impl eframe::App for EguiApp {
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        self.prepare_frame(ctx, frame);
        let focus_context = self.controller.ui.focus.context;
        let focus_flags = FocusFlags::from_context(focus_context);
        self.handle_focus_side_effects(&focus_flags);
        let input = InputSnapshot::capture(ctx);
        self.controller
            .update_performance_governor(user_activity_detected(ctx));
        let feedback_modal_open = self.controller.ui.feedback_issue.open;
        if !feedback_modal_open {
            self.handle_space_shortcut(ctx, &input);
            self.handle_copy_shortcut(ctx);
            self.handle_paste_shortcut(ctx);
            self.handle_escape_shortcut(ctx, &input);
            self.handle_window_shortcuts(ctx);
            self.handle_arrow_keys(ctx, &focus_flags, &input);
            self.process_hotkeys(ctx, focus_context);
        }
        self.render_ui(ctx, &input, focus_context);
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.controller.commit_pending_age_update();
        self.controller.shutdown();
    }
}

pub(super) fn consume_keypress(ctx: &egui::Context, input: &InputSnapshot, key: egui::Key) {
    let mut modifiers = egui::Modifiers::default();
    modifiers.shift = input.shift;
    modifiers.alt = input.alt;
    modifiers.ctrl = input.ctrl;
    modifiers.command = input.command;
    ctx.input_mut(|state| state.consume_key(modifiers, key));
}
