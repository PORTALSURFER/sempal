use eframe::egui::{self, RichText};

use super::section_label;
use crate::egui_app::ui::{EguiApp, helpers};
use crate::egui_app::ui::style;

impl EguiApp {
    pub(in crate::egui_app::ui::chrome) fn render_analysis_options_menu(
        &mut self,
        ui: &mut egui::Ui,
    ) {
        let palette = style::palette();
        let tooltip_mode = self.controller.ui.controls.tooltip_mode;
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
        let response = helpers::tooltip(
            ui.add(drag),
            "Max Analysis Duration",
            "Skip very long audio files during similarity and transient analysis to save system resources and time. Files longer than this threshold will still be playable but won't appear in the similarity map.",
            tooltip_mode,
        );
        if response.changed() {
            self.controller.set_max_analysis_duration_seconds(seconds);
        }

        ui.add_space(ui.spacing().item_spacing.y);
        ui.label(RichText::new("Analysis workers (0 = auto):").color(palette.text_muted));
        let mut workers = self.controller.analysis_worker_count() as i64;
        let auto_workers = self.controller.analysis_auto_worker_count();
        let drag = egui::DragValue::new(&mut workers).range(0..=64);
        let response = helpers::tooltip(
            ui.add(drag),
            "Analysis Workers",
            &format!("Number of background threads dedicated to audio analysis and feature extraction. Auto ({}) uses most available CPU cores without impacting UI responsiveness. Changes require a restart.", auto_workers),
            tooltip_mode,
        );
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
