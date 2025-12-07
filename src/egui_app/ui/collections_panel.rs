use super::helpers::{
    clamp_label_for_width, list_row_height, render_list_row, scroll_offset_to_reveal_row,
    RowMetrics,
};
use super::*;
use crate::egui_app::state::{CollectionRowView, DragPayload};
use eframe::egui::{self, Color32, RichText, Stroke, Ui};
use std::path::PathBuf;

impl EguiApp {
    pub(super) fn render_collections_panel(&mut self, ui: &mut Ui) {
        let drag_active = self.controller.ui.drag.payload.is_some();
        let pointer_pos = ui
            .input(|i| i.pointer.hover_pos().or_else(|| i.pointer.interact_pos()))
            .or(self.controller.ui.drag.position);
        ui.vertical(|ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("Collections").color(Color32::WHITE));
                let add_button = ui.add_enabled(
                    self.controller.ui.collections.enabled,
                    egui::Button::new(RichText::new("+").color(Color32::WHITE)),
                );
                if add_button.clicked() {
                    self.controller.add_collection();
                }
                ui.add_space(4.0);
            });
            ui.add_space(6.0);
            let rows = self.controller.ui.collections.rows.clone();
            egui::ScrollArea::vertical()
                .id_source("collections_scroll")
                .show(ui, |ui| {
                    let row_height = list_row_height(ui);
                    for (index, collection) in rows.iter().enumerate() {
                        let selected = collection.selected;
                        let mut label = format!("{} ({})", collection.name, collection.count);
                        let (text_color, indicator) = if collection.export_path.is_none() {
                            (Color32::from_rgb(255, 200, 120), "! ")
                        } else {
                            (Color32::WHITE, "")
                        };
                        if !indicator.is_empty() {
                            label.insert_str(0, indicator);
                        }
                        ui.push_id(&collection.id, |ui| {
                            let row_width = ui.available_width();
                            let padding = ui.spacing().button_padding.x * 2.0;
                            let label = clamp_label_for_width(&label, row_width - padding);
                            let bg = selected.then_some(Color32::from_rgb(30, 30, 30));
                            let response = render_list_row(
                                ui,
                                &label,
                                row_width,
                                row_height,
                                bg,
                                text_color,
                                egui::Sense::click(),
                            );
                            if response.clicked() {
                                self.controller.select_collection_by_index(Some(index));
                            }
                            self.collection_row_menu(&response, collection);
                            if drag_active {
                                if let Some(pointer) = pointer_pos {
                                    if response.rect.contains(pointer) {
                                        self.controller.update_active_drag(
                                            pointer,
                                            Some(collection.id.clone()),
                                            false,
                                            None,
                                        );
                                    }
                                }
                            }
                        });
                    }
                });
            ui.label(RichText::new("Collection items").color(Color32::WHITE));
            self.render_collection_samples(ui, drag_active, pointer_pos);
        });
    }

    fn render_collection_samples(
        &mut self,
        ui: &mut Ui,
        drag_active: bool,
        pointer_pos: Option<egui::Pos2>,
    ) {
        let samples = self.controller.ui.collections.samples.clone();
        let selected_row = self.controller.ui.collections.selected_sample;
        let current_collection_id = self.controller.current_collection_id();
        let hovering_collection =
            self.controller
                .ui
                .drag
                .hovering_collection
                .clone()
                .or_else(|| {
                    if self.controller.ui.drag.hovering_drop_zone {
                        current_collection_id.clone()
                    } else {
                        None
                    }
                });
        let active_drag_path = if drag_active {
            match &self.controller.ui.drag.payload {
                Some(DragPayload::Sample { path }) => Some(path.clone()),
                _ => None,
            }
        } else {
            None
        };
        let duplicate_row = if drag_active
            && hovering_collection
                .as_ref()
                .is_some_and(|id| Some(id) == current_collection_id.as_ref())
        {
            active_drag_path
                .as_ref()
                .and_then(|p| samples.iter().position(|s| &s.path == p))
        } else {
            None
        };
        let row_height = list_row_height(ui);
        let row_metrics = RowMetrics {
            height: row_height,
            spacing: ui.spacing().item_spacing.y,
        };
        let available_height = ui.available_height();
        let frame = egui::Frame::none().fill(Color32::from_rgb(16, 16, 16));
        let scroll_response = frame.show(ui, |ui| {
            ui.set_min_height(available_height);
            let scroll = egui::ScrollArea::vertical().id_source("collection_items_scroll");
            if samples.is_empty() {
                scroll.show(ui, |ui| {
                    let height = ui.available_height().max(available_height);
                    ui.allocate_exact_size(
                        egui::vec2(ui.available_width(), height),
                        egui::Sense::hover(),
                    );
                })
            } else {
                scroll.show_rows(ui, row_height, samples.len(), |ui, row_range| {
                    for row in row_range {
                        let Some(sample) = samples.get(row) else {
                            continue;
                        };
                        let row_width = ui.available_width();
                        let padding = ui.spacing().button_padding.x * 2.0;
                        let path = sample.path.clone();
                        let label = format!("{} — {}", sample.source, sample.label);
                        let label = clamp_label_for_width(&label, row_width - padding);
                        let is_selected = Some(row) == selected_row;
                        let is_duplicate_hover =
                            drag_active && active_drag_path.as_ref().is_some_and(|p| p == &path);
                        let bg = if is_selected {
                            Some(Color32::from_rgb(30, 30, 30))
                        } else if is_duplicate_hover {
                            Some(Color32::from_rgb(90, 60, 24))
                        } else {
                            None
                        };
                        ui.push_id(
                            format!("{}:{}:{}", sample.source_id, sample.source, sample.label),
                            |ui| {
                                let response = render_list_row(
                                    ui,
                                    &label,
                                    row_width,
                                    row_height,
                                    bg,
                                    Color32::LIGHT_GRAY,
                                    egui::Sense::click_and_drag(),
                                );
                                if response.clicked() {
                                    self.controller.select_collection_sample(row);
                                }
                                if is_duplicate_hover {
                                    ui.painter().rect_stroke(
                                        response.rect.expand(2.0),
                                        4.0,
                                        Stroke::new(2.0, Color32::from_rgb(255, 170, 80)),
                                    );
                                }
                                if response.drag_started() {
                                    if let Some(pos) = response.interact_pointer_pos() {
                                        self.controller.start_sample_drag(
                                            path.clone(),
                                            sample.label.clone(),
                                            pos,
                                        );
                                    }
                                } else if drag_active && response.dragged() {
                                    if let Some(pos) = response.interact_pointer_pos() {
                                        self.controller
                                            .update_active_drag(pos, None, false, None);
                                    }
                                } else if response.drag_stopped() {
                                    self.controller.finish_active_drag();
                                }
                            },
                        );
                    }
                })
            }
        });
        let viewport_height = scroll_response.inner.inner_rect.height();
        let content_height = scroll_response.inner.content_size.y;
        let max_offset = (content_height - viewport_height).max(0.0);
        let mut desired_offset = scroll_response.inner.state.offset.y;
        if let Some(row) = duplicate_row {
            desired_offset =
                scroll_offset_to_reveal_row(desired_offset, row, row_metrics, viewport_height, 1.0);
        } else if let Some(row) = selected_row {
            desired_offset =
                scroll_offset_to_reveal_row(desired_offset, row, row_metrics, viewport_height, 1.0);
        }
        let mut state = scroll_response.inner.state;
        state.offset.y = desired_offset.clamp(0.0, max_offset);
        state.store(ui.ctx(), scroll_response.inner.id);
        if drag_active {
            if let Some(pointer) = pointer_pos {
                let target_rect = scroll_response.response.rect.expand2(egui::vec2(8.0, 0.0));
                if target_rect.contains(pointer) {
                    self.controller.update_active_drag(
                        pointer,
                        current_collection_id.clone(),
                        true,
                        None,
                    );
                    ui.painter().rect_stroke(
                        target_rect,
                        6.0,
                        Stroke::new(2.0, Color32::from_rgba_unmultiplied(80, 140, 200, 180)),
                    );
                }
            }
        }
    }

    fn collection_row_menu(&mut self, response: &egui::Response, collection: &CollectionRowView) {
        response.context_menu(|ui| {
            if ui.button("Set export folder…").clicked() {
                self.controller.pick_collection_export_path(&collection.id);
                ui.close_menu();
            }
            if ui.button("Clear export folder").clicked() {
                self.controller.clear_collection_export_path(&collection.id);
                ui.close_menu();
            }
            let refresh_enabled = collection.export_path.is_some();
            if ui
                .add_enabled(refresh_enabled, egui::Button::new("Refresh export"))
                .clicked()
            {
                self.controller.refresh_collection_export(&collection.id);
                ui.close_menu();
            }
            let export_dir = collection_export_dir(collection);
            if ui
                .add_enabled(
                    export_dir.is_some(),
                    egui::Button::new("Open export folder"),
                )
                .clicked()
            {
                self.controller
                    .open_collection_export_folder(&collection.id);
                ui.close_menu();
            }
            if let Some(path) = export_dir {
                ui.small(format!("Current export: {}", path.display()));
            } else {
                ui.small("No export folder set");
            }
            ui.separator();
            ui.label("Rename collection");
            let rename_id = ui.make_persistent_id(format!("rename:{}", collection.id.as_str()));
            let mut rename_value = ui.ctx().data_mut(|data| {
                let value = data.get_temp_mut_or_default::<String>(rename_id);
                if value.is_empty() {
                    *value = collection.name.clone();
                }
                value.clone()
            });
            let edit = ui.text_edit_singleline(&mut rename_value);
            ui.ctx()
                .data_mut(|data| data.insert_temp(rename_id, rename_value.clone()));
            let rename_requested =
                edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            if ui.button("Apply rename").clicked() || rename_requested {
                self.controller
                    .rename_collection(&collection.id, rename_value.clone());
                ui.ctx()
                    .data_mut(|data| data.insert_temp(rename_id, rename_value));
                ui.close_menu();
            }
        });
    }
}

fn collection_export_dir(collection: &CollectionRowView) -> Option<PathBuf> {
    collection
        .export_path
        .clone()
        .map(|base| base.join(sanitized_collection_name(&collection.name)))
}

fn sanitized_collection_name(name: &str) -> String {
    let mut cleaned: String = name
        .chars()
        .map(|c| {
            if matches!(c, '/' | '\\' | ':' | '*') {
                '_'
            } else {
                c
            }
        })
        .collect();
    if cleaned.is_empty() {
        cleaned.push_str("collection");
    }
    cleaned
}
