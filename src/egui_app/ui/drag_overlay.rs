use super::*;
use eframe::egui::{self, Align2, Area, Color32, Frame, Order, RichText, Stroke, Vec2};

impl EguiApp {
    pub(super) fn render_drag_overlay(&mut self, ctx: &egui::Context) {
        if let Some(pos) = self.controller.ui.drag.position {
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
                        .fill(Color32::from_rgba_unmultiplied(26, 39, 51, 220))
                        .stroke(Stroke::new(1.0, Color32::from_rgb(47, 111, 177)))
                        .corner_radius(6.0)
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.add_space(8.0);
                                ui.colored_label(Color32::from_rgb(90, 176, 255), "●");
                                ui.label(RichText::new(label).color(Color32::WHITE));
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
