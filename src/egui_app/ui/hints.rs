use super::EguiApp;
use super::style;
use eframe::egui::{self, Align2, RichText};

impl EguiApp {
    pub(super) fn render_hint_of_day_prompt(&mut self, ctx: &egui::Context) {
        if !self.controller.ui.hints.open {
            return;
        }
        if self.controller.ui.feedback_issue.open {
            return;
        }
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.controller.dismiss_hint_of_day();
            return;
        }

        self.render_hint_backdrop(ctx);

        let mut open = true;
        egui::Window::new("Hint of the day")
            .anchor(Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .order(egui::Order::Foreground)
            .collapsible(false)
            .resizable(false)
            .default_width(520.0)
            .open(&mut open)
            .show(ctx, |ui| {
                self.render_hint_of_day_body(ui);
            });

        if !open {
            self.controller.dismiss_hint_of_day();
        }
    }

    fn render_hint_backdrop(&mut self, ctx: &egui::Context) {
        let rect = ctx.viewport_rect();
        let painter = ctx.layer_painter(egui::LayerId::new(
            egui::Order::Background,
            egui::Id::new("hint_of_day_backdrop_paint"),
        ));
        painter.rect_filled(
            rect,
            0.0,
            egui::Color32::from_rgba_premultiplied(0, 0, 0, 160),
        );

        egui::Area::new(egui::Id::new("hint_of_day_backdrop_blocker"))
            .order(egui::Order::Middle)
            .fixed_pos(rect.min)
            .show(ctx, |ui| {
                let response = ui.allocate_rect(rect, egui::Sense::click_and_drag());
                if response.clicked() {
                    ui.ctx().request_repaint();
                }
            });
    }

    fn render_hint_of_day_body(&mut self, ui: &mut egui::Ui) {
        let palette = style::palette();
        let state = &mut self.controller.ui.hints;
        ui.set_min_width(520.0);
        ui.label(
            RichText::new(&state.title)
                .strong()
                .color(palette.text_primary),
        );
        ui.add_space(8.0);
        ui.label(RichText::new(&state.body).color(palette.text_primary));
        ui.add_space(12.0);

        let mut dont_show = !state.show_on_startup;
        if ui
            .checkbox(&mut dont_show, "Stop showing on launch")
            .changed()
        {
            self.controller.set_hint_on_startup(!dont_show);
        }
        ui.add_space(8.0);

        ui.horizontal(|ui| {
            if ui.button("Another hint").clicked() {
                self.controller.show_hint_of_day();
            }
            if ui.button("Close").clicked() {
                self.controller.dismiss_hint_of_day();
            }
        });
    }
}
