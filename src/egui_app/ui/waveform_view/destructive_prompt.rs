use super::style;
use super::*;
use eframe::egui::{self, Align2, RichText};

impl EguiApp {
    pub(super) fn render_destructive_edit_prompt(
        &mut self,
        ctx: &egui::Context,
        prompt: crate::egui_app::state::DestructiveEditPrompt,
    ) {
        let mut open = true;
        let mut apply = false;
        let mut close_prompt = false;
        egui::Window::new("Confirm destructive edit")
            .anchor(Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .collapsible(false)
            .resizable(false)
            .auto_sized()
            .open(&mut open)
            .show(ctx, |ui| {
                self.render_destructive_prompt_body(ui, &prompt, &mut apply, &mut close_prompt);
            });
        if apply {
            self.controller
                .apply_confirmed_destructive_edit(prompt.edit);
            return;
        }
        if close_prompt {
            open = false;
        }
        if !open {
            self.controller.clear_destructive_prompt();
        }
    }

    fn render_destructive_prompt_body(
        &mut self,
        ui: &mut egui::Ui,
        prompt: &crate::egui_app::state::DestructiveEditPrompt,
        apply: &mut bool,
        close_prompt: &mut bool,
    ) {
        let palette = style::palette();
        ui.set_min_width(340.0);
        self.render_destructive_prompt_copy(ui, prompt, &palette);
        ui.add_space(8.0);
        self.render_destructive_prompt_yolo(ui, apply, close_prompt);
        ui.add_space(8.0);
        self.render_destructive_prompt_buttons(ui, apply, close_prompt);
    }

    fn render_destructive_prompt_copy(
        &self,
        ui: &mut egui::Ui,
        prompt: &crate::egui_app::state::DestructiveEditPrompt,
        palette: &style::Palette,
    ) {
        ui.label(
            RichText::new(prompt.title.clone())
                .strong()
                .color(style::destructive_text()),
        );
        ui.label(
            RichText::new(prompt.message.clone())
                .color(style::status_badge_color(style::StatusTone::Warning)),
        );
        ui.label(
            RichText::new("This will overwrite the source file on disk.")
                .color(palette.text_primary),
        );
    }

    fn render_destructive_prompt_yolo(
        &mut self,
        ui: &mut egui::Ui,
        apply: &mut bool,
        close_prompt: &mut bool,
    ) {
        let mut yolo_mode = self.controller.ui.controls.destructive_yolo_mode;
        let label = RichText::new("Enable yolo mode (apply destructive edits without prompting)")
            .color(style::destructive_text());
        if ui.checkbox(&mut yolo_mode, label).changed() {
            self.controller.set_destructive_yolo_mode(yolo_mode);
            if yolo_mode {
                *apply = true;
                *close_prompt = true;
            }
        }
    }

    fn render_destructive_prompt_buttons(
        &mut self,
        ui: &mut egui::Ui,
        apply: &mut bool,
        close_prompt: &mut bool,
    ) {
        ui.horizontal(|ui| {
            if ui.button("Cancel").clicked() {
                *close_prompt = true;
            }
            let apply_btn =
                egui::Button::new(RichText::new("Apply edit").color(style::destructive_text()));
            if ui.add(apply_btn).clicked() {
                *apply = true;
            }
        });
    }
}
