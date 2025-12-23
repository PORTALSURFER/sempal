use super::style;
use eframe::egui::{self, RichText, UiBuilder};

pub(crate) fn render_empty_state(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    palette: &style::Palette,
    busy: bool,
) -> bool {
    let mut build_clicked = false;
    let pulse = if busy {
        ui.ctx().request_repaint();
        let t = ui.ctx().input(|i| i.time) as f32;
        (t * 2.4_f32).sin() * 0.5_f32 + 0.5_f32
    } else {
        0.0
    };
    let pulse_alpha = (80.0_f32 + pulse * 160.0_f32)
        .round()
        .clamp(0.0_f32, 255.0_f32) as u8;
    let pulse_color = egui::Color32::from_rgba_unmultiplied(
        palette.accent_mint.r(),
        palette.accent_mint.g(),
        palette.accent_mint.b(),
        pulse_alpha,
    );
    let _ = ui.scope_builder(UiBuilder::new().max_rect(rect), |ui| {
        ui.with_layout(
            egui::Layout::top_down(egui::Align::Center),
            |ui| {
                ui.add_space(rect.height() * 0.35);
                if busy {
                    ui.label(
                        RichText::new("Preparing similarity map…")
                            .color(palette.text_primary)
                            .strong(),
                    );
                    ui.label(
                        RichText::new("This can take a minute for new sources.")
                            .color(palette.text_muted),
                    );
                    let center = rect.center();
                    ui.painter()
                        .circle_filled(center + egui::vec2(0.0, 50.0), 8.0, pulse_color);
                } else {
                    ui.label(
                        RichText::new("No map layout yet.")
                            .color(palette.text_primary)
                            .strong(),
                    );
                    ui.label(
                        RichText::new("Preparing similarity data will build the map.")
                            .color(palette.text_muted),
                    );
                    if ui.button("Prepare similarity map").clicked() {
                        build_clicked = true;
                    }
                }
            },
        );
    });
    build_clicked
}
