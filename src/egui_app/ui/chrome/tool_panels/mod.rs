mod analysis_options;
mod audio_combos;
mod audio_settings;
mod gpu_embeddings;

use eframe::egui::{self, RichText};

use super::super::style;

fn section_label(ui: &mut egui::Ui, label: &str) {
    ui.label(
        RichText::new(label)
            .strong()
            .color(style::palette().text_primary),
    );
}
