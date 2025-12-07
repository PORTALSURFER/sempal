use super::helpers::{clamp_label_for_width, list_row_height, render_list_row};
use super::*;
use eframe::egui::{self, Color32, RichText, Ui};

impl EguiApp {
    pub(super) fn render_sources_panel(&mut self, ui: &mut Ui) {
        ui.vertical(|ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("Sources").color(Color32::WHITE));
                if ui
                    .button(RichText::new("+").color(Color32::WHITE))
                    .clicked()
                {
                    self.controller.add_source_via_dialog();
                }
            });
            ui.add_space(6.0);
            egui::ScrollArea::vertical()
                .id_salt("sources_scroll")
                .show(ui, |ui| {
                    let rows = self.controller.ui.sources.rows.clone();
                    let selected = self.controller.ui.sources.selected;
                    let row_height = list_row_height(ui);
                    for (index, row) in rows.iter().enumerate() {
                        let is_selected = Some(index) == selected;
                        ui.push_id(&row.id, |ui| {
                            let row_width = ui.available_width();
                            let padding = ui.spacing().button_padding.x * 2.0;
                            let label = clamp_label_for_width(&row.name, row_width - padding);
                            let bg = is_selected.then_some(Color32::from_rgb(30, 30, 30));
                            let response = render_list_row(
                                ui,
                                &label,
                                row_width,
                                row_height,
                                bg,
                                Color32::WHITE,
                                egui::Sense::click(),
                            )
                            .on_hover_text(&row.path);
                            if response.clicked() {
                                self.controller.select_source_by_index(index);
                            }
                        });
                    }
                });
        });
    }
}
