use super::style;
use eframe::egui::{self, RichText, UiBuilder};

pub(crate) fn render_empty_state(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    palette: &style::Palette,
) -> bool {
    let mut build_clicked = false;
    let _ = ui.scope_builder(UiBuilder::new().max_rect(rect), |ui| {
        ui.with_layout(
            egui::Layout::top_down(egui::Align::Center),
            |ui| {
                ui.add_space(rect.height() * 0.35);
                ui.label(
                    RichText::new("No map layout yet.")
                        .color(palette.text_primary)
                        .strong(),
                );
                ui.label(
                    RichText::new("Generate it with sempal-umap or click below.")
                        .color(palette.text_muted),
                );
                if ui.button("Build map layout").clicked() {
                    build_clicked = true;
                }
            },
        );
    });
    build_clicked
}
