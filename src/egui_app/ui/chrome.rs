use super::*;
use super::style;
use eframe::egui::{self, Frame, RichText, SliderClamping, StrokeKind};

impl EguiApp {
    fn log_viewport_info(&mut self, ctx: &egui::Context) {
        let (inner, monitor, fullscreen, maximized) = ctx.input(|i| {
            (
                i.viewport().inner_rect,
                i.viewport().monitor_size,
                i.viewport().fullscreen,
                i.viewport().maximized,
            )
        });
        if let (Some(inner), Some(mon)) = (inner, monitor) {
            let mode = if fullscreen == Some(true) {
                "fullscreen"
            } else if maximized == Some(true) {
                "maximized"
            } else {
                "windowed"
            };
            let dims = (
                inner.width().round() as u32,
                inner.height().round() as u32,
                mon.x.round() as u32,
                mon.y.round() as u32,
                mode,
            );
            if Some(dims) != self.last_viewport_log {
                println!(
                    "mode: {:<10} | viewport: {} x {} | monitor: {} x {}",
                    dims.4, dims.0, dims.1, dims.2, dims.3
                );
                self.last_viewport_log = Some(dims);
            }
        }
    }

    pub(super) fn render_status(&mut self, ctx: &egui::Context) {
        self.log_viewport_info(ctx);
        let palette = style::palette();
        egui::TopBottomPanel::bottom("status_bar")
            .frame(
                Frame::new()
                    .fill(palette.bg_primary)
                    .stroke(style::outer_border()),
            )
            .show(ctx, |ui| {
                let status = self.controller.ui.status.clone();
                ui.columns(2, |columns| {
                    columns[0].horizontal(|ui| {
                        ui.add_space(8.0);
                        let (badge_rect, _) = ui.allocate_exact_size(
                            egui::vec2(18.0, 18.0),
                            egui::Sense::hover(),
                        );
                        ui.painter()
                            .rect_filled(badge_rect, 0.0, status.badge_color);
                        ui.painter()
                            .rect_stroke(badge_rect, 0.0, style::inner_border(), StrokeKind::Inside);
                        ui.add_space(10.0);
                        ui.label(RichText::new(&status.badge_label).color(palette.text_primary));
                        ui.separator();
                        ui.label(RichText::new(&status.text).color(palette.text_primary));
                    });
                    columns[1].with_layout(
                        egui::Layout::right_to_left(egui::Align::Center),
                        |ui| {
                            let mut volume = self.controller.ui.volume;
                            let slider = egui::Slider::new(&mut volume, 0.0..=1.0)
                                .text("Vol")
                                .clamping(SliderClamping::Always);
                            if ui.add(slider).changed() {
                                self.controller.set_volume(volume);
                            }
                        },
                    );
                });
            });
    }
}
