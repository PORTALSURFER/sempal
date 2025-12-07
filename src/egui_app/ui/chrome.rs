use super::*;
use eframe::egui::{self, Color32, Frame, RichText};

impl EguiApp {
    pub(super) fn render_top_bar(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::TopBottomPanel::top("top_bar")
            .frame(Frame::none().fill(Color32::from_rgb(24, 24, 24)))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Sample Sources").color(Color32::WHITE));
                    ui.add_space(8.0);
                    ui.separator();
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .button(RichText::new("Close").color(Color32::WHITE))
                            .clicked()
                        {
                            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                    });
                });
            });
    }

    pub(super) fn render_status(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::bottom("status_bar")
            .frame(Frame::none().fill(Color32::from_rgb(0, 0, 0)))
            .show(ctx, |ui| {
                let status = self.controller.ui.status.clone();
                ui.columns(2, |columns| {
                    columns[0].horizontal(|ui| {
                        ui.add_space(8.0);
                        ui.painter().circle_filled(
                            ui.cursor().min + egui::vec2(9.0, 11.0),
                            9.0,
                            status.badge_color,
                        );
                        ui.add_space(8.0);
                        ui.label(RichText::new(&status.badge_label).color(Color32::WHITE));
                        ui.separator();
                        ui.label(RichText::new(&status.text).color(Color32::WHITE));
                    });
                    columns[1].with_layout(
                        egui::Layout::right_to_left(egui::Align::Center),
                        |ui| {
                            let mut volume = self.controller.ui.volume;
                            let slider = egui::Slider::new(&mut volume, 0.0..=1.0)
                                .text("Vol")
                                .clamp_to_range(true);
                            if ui.add(slider).changed() {
                                self.controller.set_volume(volume);
                            }
                        },
                    );
                });
            });
    }
}
