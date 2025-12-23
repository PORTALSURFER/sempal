use eframe::egui::{self, Frame, Margin, RichText, StrokeKind};

use super::super::hotkey_runtime::format_keypress;
use super::super::style;
use super::super::EguiApp;

impl EguiApp {
    pub(crate) fn render_status(&mut self, ctx: &egui::Context) {
        self.log_viewport_info(ctx);
        let palette = style::palette();
        egui::TopBottomPanel::bottom("status_bar")
            .frame(
                Frame::new()
                    .fill(palette.bg_primary)
                    .stroke(style::section_stroke())
                    .inner_margin(Margin::symmetric(8, 4)),
            )
            .show(ctx, |ui| {
                let status = self.controller.ui.status.clone();
                let chord_label = self.chord_status_label();
                let key_label = format_keypress(&self.key_feedback.last_key);
                ui.columns(3, |columns| {
                    columns[0].vertical(|ui| {
                        ui.horizontal(|ui| {
                            ui.add_space(6.0);
                            let (badge_rect, _) = ui
                                .allocate_exact_size(egui::vec2(16.0, 16.0), egui::Sense::hover());
                            ui.painter()
                                .rect_filled(badge_rect, 0.0, status.badge_color);
                            ui.painter().rect_stroke(
                                badge_rect,
                                0.0,
                                style::inner_border(),
                                StrokeKind::Inside,
                            );
                            ui.add_space(8.0);
                            ui.label(
                                RichText::new(&status.badge_label).color(palette.text_primary),
                            );
                            ui.separator();
                            ui.label(RichText::new(&status.text).color(palette.text_primary));
                        });
                    });
                    columns[1].horizontal(|ui| {
                        ui.add_space(6.0);
                        ui.label(RichText::new("Key").color(palette.text_primary));
                        ui.separator();
                        ui.label(RichText::new(key_label).color(palette.text_primary));
                        ui.separator();
                        ui.label(RichText::new("Chord").color(palette.text_primary));
                        ui.separator();
                        ui.label(RichText::new(chord_label).color(palette.text_primary));
                    });
                    columns[2].with_layout(
                        egui::Layout::right_to_left(egui::Align::Center),
                        |ui| {
                            self.render_status_controls(ui);
                        },
                    );
                });
            });
        self.render_audio_settings_window(ctx);
    }

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

    fn chord_status_label(&self) -> String {
        if let Some(pending) = self.key_feedback.pending_root {
            return format!("{} …", format_keypress(&Some(pending)));
        }
        if let Some((first, second)) = self.key_feedback.last_chord {
            return format!(
                "{} + {}",
                format_keypress(&Some(first)),
                format_keypress(&Some(second))
            );
        }
        "—".to_string()
    }
}
