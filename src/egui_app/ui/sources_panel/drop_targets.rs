use super::EguiApp;
use super::helpers::{RowBackground, clamp_label_for_width, list_row_height, render_list_row};
use super::style;
use crate::egui_app::state::{DragPayload, DragSource, DragTarget};
use crate::egui_app::ui::drag_targets::{handle_drop_zone, pointer_pos_for_drag};
use eframe::egui::{self, Align2, RichText, TextStyle, Ui};

impl EguiApp {
    pub(super) fn render_drop_targets(&mut self, ui: &mut Ui, height: f32) {
        let palette = style::palette();
        ui.horizontal(|ui| {
            ui.label(RichText::new("Drop targets").color(palette.text_primary));
            if ui
                .button(RichText::new("+").color(palette.text_primary))
                .clicked()
            {
                self.controller.add_drop_target_via_dialog();
            }
        });
        ui.add_space(6.0);

        let drag_payload = self.controller.ui.drag.payload.clone();
        let drag_active = matches!(
            drag_payload,
            Some(DragPayload::Sample { .. } | DragPayload::Samples { .. })
        );
        let pointer_pos = pointer_pos_for_drag(ui, self.controller.ui.drag.position);
        let rows = self.controller.ui.sources.drop_targets.rows.clone();
        let selected = self.controller.ui.sources.drop_targets.selected;
        let frame = style::section_frame();
        let frame_response = frame.show(ui, |ui| {
            ui.set_min_height(height);
            ui.set_max_height(height);
            let row_height = list_row_height(ui);
            egui::ScrollArea::vertical()
                .id_salt("drop_targets_scroll")
                .max_height(height)
                .show(ui, |ui| {
                    if rows.is_empty() {
                        let (rect, _) = ui.allocate_exact_size(
                            egui::vec2(ui.available_width(), row_height),
                            egui::Sense::hover(),
                        );
                        ui.painter().text(
                            rect.left_center(),
                            Align2::LEFT_CENTER,
                            "Add a folder to create a drop target",
                            TextStyle::Body.resolve(ui.style()),
                            palette.text_muted,
                        );
                        return;
                    }
                    for (index, row) in rows.iter().enumerate() {
                        let is_selected = Some(index) == selected;
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
                        let bg = RowBackground::from_option(
                            is_selected.then_some(style::row_selected_fill()),
                        );
                        let response = render_list_row(
                            ui,
                            super::helpers::ListRow {
                                label: &label,
                                row_width,
                                row_height,
                                background: bg,
                                skip_hover: false,
                                text_color,
                                sense: egui::Sense::click(),
                                number: None,
                                marker: None,
                                rating: None,
                            },
                        )
                        .on_hover_text(row.path.display().to_string());
                        if response.clicked() {
                            self.controller.select_drop_target_by_index(index);
                        }
                        handle_drop_zone(
                            ui,
                            &mut self.controller,
                            drag_active,
                            pointer_pos,
                            response.rect,
                            DragSource::DropTargets,
                            DragTarget::DropTarget {
                                path: row.path.clone(),
                            },
                            style::drag_target_stroke(),
                            egui::StrokeKind::Inside,
                        );
                        self.drop_target_row_menu(&response, index, row);
                    }
                });
        });
        style::paint_section_border(ui, frame_response.response.rect, false);
    }

    fn drop_target_row_menu(
        &mut self,
        response: &egui::Response,
        index: usize,
        row: &crate::egui_app::state::DropTargetRowView,
    ) {
        response.context_menu(|ui| {
            let palette = style::palette();
            ui.label(RichText::new(row.name.clone()).color(palette.text_primary));
            let mut close_menu = false;
            if ui.button("Remove drop target").clicked() {
                self.controller.remove_drop_target(index);
                close_menu = true;
            }
            if close_menu {
                ui.close();
            }
        });
    }
}
