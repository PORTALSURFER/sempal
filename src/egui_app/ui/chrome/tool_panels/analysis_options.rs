use eframe::egui::{self, RichText};

use super::super::EguiApp;
use super::super::style;
use super::super::buttons;
use super::section_label;

impl EguiApp {
    pub(super) fn render_analysis_options_menu(&mut self, ui: &mut egui::Ui) {
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
        let drag = egui::DragValue::new(&mut workers).range(0..=64);
        let response = ui
            .add(drag)
            .on_hover_text("Limit background CPU usage (change takes effect on next start)");
        if response.changed() {
            self.controller
                .set_analysis_worker_count(workers.max(0) as u32);
        }

        ui.add_space(ui.spacing().item_spacing.y);
        ui.separator();
        section_label(ui, "GPU embeddings");
        let backend_label = match self.controller.panns_backend() {
            crate::sample_sources::config::PannsBackendChoice::Wgpu => "WGPU (Vulkan)",
            crate::sample_sources::config::PannsBackendChoice::Cuda => "CUDA",
        };
        ui.label(
            RichText::new(format!("Backend: {}", backend_label)).color(palette.text_muted),
        );
        if ui
            .add(buttons::action_button("Open GPU embedding options…"))
            .clicked()
        {
            self.controller.ui.audio.panel_open = true;
            self.controller.refresh_audio_options();
        }
    }
}
