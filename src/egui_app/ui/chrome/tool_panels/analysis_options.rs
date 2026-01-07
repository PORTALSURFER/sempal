use eframe::egui::{self, RichText};

use super::section_label;
use crate::egui_app::ui::EguiApp;
use crate::egui_app::ui::style;

impl EguiApp {
    pub(in crate::egui_app::ui::chrome) fn render_analysis_options_menu(
        &mut self,
        ui: &mut egui::Ui,
    ) {
        let palette = style::palette();
        section_label(ui, "Analysis");
        ui.label(
            RichText::new("Skip feature extraction for files longer than:")
                .color(palette.text_muted),
        );
        let mut seconds = self.controller.max_analysis_duration_seconds();
        let drag = egui::DragValue::new(&mut seconds)
            .speed(1.0)
            .range(1.0..=3600.0)
            .suffix(" s");
        let response = ui
            .add(drag)
            .on_hover_text("Long songs/loops can be expensive to decode and analyze");
        if response.changed() {
            self.controller.set_max_analysis_duration_seconds(seconds);
        }

        ui.add_space(ui.spacing().item_spacing.y);
        ui.label(RichText::new("Analysis workers (0 = auto):").color(palette.text_muted));
        let mut workers = self.controller.analysis_worker_count() as i64;
        let auto_workers = self.controller.analysis_auto_worker_count();
        let hover_text = if workers == 0 {
            format!(
                "Limit background CPU usage (change takes effect on next start). Auto = {} workers.",
                auto_workers
            )
        } else {
            "Limit background CPU usage (change takes effect on next start).".to_string()
        };
        let drag = egui::DragValue::new(&mut workers).range(0..=64);
        let response = ui.add(drag).on_hover_text(hover_text);
        if response.changed() {
            self.controller
                .set_analysis_worker_count(workers.max(0) as u32);
        }

        ui.add_space(ui.spacing().item_spacing.y);
        ui.separator();
        section_label(ui, "Similarity embeddings");
        ui.label(RichText::new("Backend: CPU (DSP)").color(palette.text_muted));
    }
}
