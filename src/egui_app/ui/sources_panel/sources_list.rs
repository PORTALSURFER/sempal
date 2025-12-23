use super::helpers::{clamp_label_for_width, list_row_height, render_list_row};
use super::style;
use super::EguiApp;
use crate::egui_app::state::FocusContext;
use eframe::egui::{self, RichText, Ui};

impl EguiApp {
    pub(super) fn render_sources_list(&mut self, ui: &mut Ui, height: f32) -> egui::Rect {
        let output = egui::ScrollArea::vertical()
            .id_salt("sources_scroll")
            .max_height(height)
            .show(ui, |ui| {
                let rows = self.controller.ui.sources.rows.clone();
                let selected = self.controller.ui.sources.selected;
                let row_height = list_row_height(ui);
                for (index, row) in rows.iter().enumerate() {
                    let is_selected = Some(index) == selected;
                    ui.push_id(&row.id, |ui| {
                        let row_width = ui.available_width();
                        let padding = ui.spacing().button_padding.x * 2.0;
                        let base_label = clamp_label_for_width(&row.name, row_width - padding);
                        let label = if row.missing {
                            format!("! {base_label}")
                        } else {
                            base_label
                        };
                        let text_color = if row.missing {
                            style::missing_text()
                        } else {
                            style::high_contrast_text()
                        };
                        let bg = is_selected.then_some(style::row_selected_fill());
                        let response = render_list_row(
                            ui,
                            super::helpers::ListRow {
                                label: &label,
                                row_width,
                                row_height,
                                bg,
                                text_color,
                                sense: egui::Sense::click(),
                                number: None,
                                marker: None,
                            },
                        )
                        .on_hover_text(&row.path);
                        if response.clicked() {
                            self.controller.select_source_by_index(index);
                            self.controller
                                .focus_context_from_ui(FocusContext::SourcesList);
                        }
                        self.source_row_menu(&response, index, row);
                    });
                }
            });
        let min_focus_height = list_row_height(ui);
        let focus_height = output
            .content_size
            .y
            .max(min_focus_height)
            .min(output.inner_rect.height());
        let focus_rect = egui::Rect::from_min_size(
            output.inner_rect.min,
            egui::vec2(output.inner_rect.width(), focus_height),
        );
        focus_rect
    }

    fn source_row_menu(
        &mut self,
        response: &egui::Response,
        index: usize,
        row: &crate::egui_app::state::SourceRowView,
    ) {
        response.context_menu(|ui| {
            let palette = style::palette();
            ui.label(RichText::new(row.name.clone()).color(palette.text_primary));
            let mut close_menu = false;
            if ui.button("Quick sync").clicked() {
                self.controller.select_source_by_index(index);
                self.controller.request_quick_sync();
                close_menu = true;
            }
            if ui
                .button("Hard sync (full rescan)")
                .on_hover_text("Prune missing rows and rebuild from disk")
                .clicked()
            {
                self.controller.select_source_by_index(index);
                self.controller.request_hard_sync();
                close_menu = true;
            }
            if ui
                .button("Remove dead links")
                .on_hover_text("Remove missing rows from the library")
                .clicked()
            {
                self.controller.remove_dead_links_for_source(index);
                close_menu = true;
            }
            if ui
                .button("Prepare similarity search")
                .on_hover_text("Scan, embed, build ANN, t-SNE, and cluster for this source")
                .clicked()
            {
                self.controller.select_source_by_index(index);
                self.controller.prepare_similarity_for_selected_source();
                close_menu = true;
            }
            ui.separator();
            if ui.button("Open in file explorer").clicked() {
                self.controller.select_source_by_index(index);
                self.controller.open_source_folder(index);
                close_menu = true;
            }
            if ui.button("Remap source…").clicked() {
                self.controller.select_source_by_index(index);
                self.controller.remap_source_via_dialog(index);
                close_menu = true;
            }
            let remove_btn = egui::Button::new(
                RichText::new("Remove source")
                    .color(style::destructive_text())
                    .strong(),
            );
            if ui.add(remove_btn).clicked() {
                self.controller.remove_source(index);
                close_menu = true;
            }
            if close_menu {
                ui.close();
            }
        });
    }
}
