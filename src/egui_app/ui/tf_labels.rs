use super::*;
use eframe::egui;

impl EguiApp {
    pub(super) fn render_tf_label_windows(&mut self, ctx: &egui::Context) {
        self.render_tf_label_create_prompt(ctx);
        self.render_tf_label_auto_tag_prompt(ctx);
        self.render_tf_label_calibration_window(ctx);
        self.render_tf_label_editor(ctx);
    }

    pub(super) fn open_tf_label_editor(&mut self) {
        self.controller.ui.tf_labels.editor_open = true;
    }
}
