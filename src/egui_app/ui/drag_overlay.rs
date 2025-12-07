use super::*;
use super::style;
use eframe::egui::{self, Align2, Area, Color32, Frame, Order, RichText, Stroke, Vec2};

impl EguiApp {
    pub(super) fn render_drag_overlay(&mut self, ctx: &egui::Context) {
        if let Some(pos) = self.controller.ui.drag.position {
            let palette = style::palette();
            let label = if self.controller.ui.drag.label.is_empty() {
                "Sample".to_string()
            } else {
                self.controller.ui.drag.label.clone()
            };
            Area::new("drag_preview".into())
                .order(Order::Tooltip)
                .pivot(Align2::CENTER_CENTER)
                .current_pos(pos + Vec2::new(16.0, 16.0))
                .show(ctx, |ui| {
                    Frame::new()
                        .fill(Color32::from_rgba_unmultiplied(
                            palette.bg_tertiary.r(),
                            palette.bg_tertiary.g(),
                            palette.bg_tertiary.b(),
                            220,
                        ))
                        .stroke(Stroke::new(1.0, palette.accent_ice))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.add_space(8.0);
                                ui.colored_label(palette.accent_ice, "||");
                                ui.label(RichText::new(label).color(palette.text_primary));
                                ui.add_space(8.0);
                            });
                        });
                });
        }
        if self.controller.ui.drag.payload.is_some() {
            if ctx.input(|i| i.pointer.any_released()) {
                self.controller.finish_active_drag();
            } else if !ctx.input(|i| i.pointer.primary_down()) {
                // Safety net to clear drag visuals if a release was missed.
                self.controller.finish_active_drag();
            }
        }
    }
}
