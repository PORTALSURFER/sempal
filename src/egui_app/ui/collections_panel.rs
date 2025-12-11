use super::helpers::{
    NumberColumn, RowMarker, RowMetrics, clamp_label_for_width, list_row_height,
    number_column_width, render_list_row, scroll_offset_to_reveal_row,
};
use super::style;
use super::*;
use crate::egui_app::state::{CollectionRowView, CollectionSampleView, DragPayload, FocusContext};
use crate::egui_app::view_model;
use eframe::egui::{self, RichText, Stroke, StrokeKind, Ui};
use std::path::PathBuf;

impl EguiApp {
    pub(super) fn render_collections_panel(&mut self, ui: &mut Ui) {
        let palette = style::palette();
        let drag_active = self.controller.ui.drag.payload.is_some();
        let pointer_pos = ui
            .input(|i| i.pointer.hover_pos().or_else(|| i.pointer.interact_pos()))
            .or(self.controller.ui.drag.position);
        ui.vertical(|ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("Collections").color(palette.text_primary));
                let add_button = ui.add_enabled(
                    self.controller.ui.collections.enabled,
                    egui::Button::new(RichText::new("+").color(palette.text_primary)),
                );
                if add_button.clicked() {
                    self.controller.add_collection();
                }
                ui.add_space(4.0);
            });
            ui.add_space(6.0);
            let rows = self.controller.ui.collections.rows.clone();
            let list_response = egui::ScrollArea::vertical()
                .id_salt("collections_scroll")
                .show(ui, |ui| {
                    let row_height = list_row_height(ui);
                    for (index, collection) in rows.iter().enumerate() {
                        let selected = collection.selected;
                        let mut label = format!("{} ({})", collection.name, collection.count);
                        let (text_color, indicator) = if collection.missing {
                            (style::missing_text(), "! ")
                        } else if collection.export_path.is_none() {
                            (style::warning_soft_text(), "! ")
                        } else {
                            (style::high_contrast_text(), "")
                        };
                        if !indicator.is_empty() {
                            label.insert_str(0, indicator);
                        }
                        ui.push_id(&collection.id, |ui| {
                            let row_width = ui.available_width();
                            let padding = ui.spacing().button_padding.x * 2.0;
                            let label = clamp_label_for_width(&label, row_width - padding);
                            let bg = selected.then_some(style::row_selected_fill());
                            let response = render_list_row(
                                ui,
                                &label,
                                row_width,
                                row_height,
                                bg,
                                text_color,
                                egui::Sense::click(),
                                None,
                                None,
                            );
                            if response.clicked() {
                                self.controller.select_collection_by_index(Some(index));
                            }
                            self.collection_row_menu(&response, collection);
                            if drag_active
                                && let Some(pointer) = pointer_pos
                                && response.rect.contains(pointer)
                            {
                                self.controller.update_active_drag(
                                    pointer,
                                    Some(collection.id.clone()),
                                    false,
                                    None,
                                    None,
                                    false,
                                );
                            }
                        });
                    }
                });
            if matches!(
                self.controller.ui.focus.context,
                FocusContext::CollectionsList
            ) {
                ui.painter().rect_stroke(
                    list_response.inner_rect,
                    0.0,
                    style::focused_row_stroke(),
                    StrokeKind::Outside,
                );
            }
            ui.label(RichText::new("Collection items").color(palette.text_primary));
            self.render_collection_samples(ui, drag_active, pointer_pos);
        });
    }

    fn render_collection_samples(
        &mut self,
        ui: &mut Ui,
        drag_active: bool,
        pointer_pos: Option<egui::Pos2>,
    ) {
        let palette = style::palette();
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
                Some(DragPayload::Sample { relative_path, .. }) => Some(relative_path.clone()),
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
        let number_width = number_column_width(samples.len(), ui);
        let number_gap = ui.spacing().button_padding.x * 0.5;
        let available_height = ui.available_height();
        let frame = style::section_frame();
        let scroll_response = frame.show(ui, |ui| {
            ui.set_min_height(available_height);
            let scroll = egui::ScrollArea::vertical().id_salt("collection_items_scroll");
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
                        let mut label = format!("{} — {}", sample.source, sample.label);
                        if sample.missing {
                            label.insert_str(0, "! ");
                        }
                        let is_selected = Some(row) == selected_row;
                        let is_duplicate_hover =
                            drag_active && active_drag_path.as_ref().is_some_and(|p| p == &path);
                        let triage_marker =
                            style::triage_marker_color(sample.tag).map(|color| RowMarker {
                                width: style::triage_marker_width(),
                                color,
                            });
                        let trailing_space = triage_marker
                            .as_ref()
                            .map(|marker| marker.width + padding * 0.5)
                            .unwrap_or(0.0);
                        let bg = if is_duplicate_hover {
                            Some(style::duplicate_hover_fill())
                        } else if is_selected {
                            Some(style::row_selected_fill())
                        } else {
                            None
                        };
                        ui.push_id(
                            format!("{}:{}:{}", sample.source_id, sample.source, sample.label),
                            |ui| {
                                let label_width = row_width
                                    - padding
                                    - number_width
                                    - number_gap
                                    - trailing_space;
                                let number_text = format!("{}", row + 1);
                                let text_color = if sample.missing {
                                    style::missing_text()
                                } else {
                                    style::triage_label_color(sample.tag)
                                };
                                let response = render_list_row(
                                    ui,
                                    &clamp_label_for_width(&label, label_width),
                                    row_width,
                                    row_height,
                                    bg,
                                    text_color,
                                    egui::Sense::click_and_drag(),
                                    Some(NumberColumn {
                                        text: &number_text,
                                        width: number_width,
                                        color: palette.text_muted,
                                    }),
                                    triage_marker,
                                );
                                if response.clicked() {
                                    self.controller.select_collection_sample(row);
                                }
                                self.collection_sample_menu(&response, row, sample);
                                if is_duplicate_hover {
                                    ui.painter().rect_stroke(
                                        response.rect.expand(2.0),
                                        0.0,
                                        Stroke::new(2.0, style::duplicate_hover_stroke()),
                                        StrokeKind::Inside,
                                    );
                                }
                                if response.drag_started() {
                                    if let Some(pos) = response.interact_pointer_pos() {
                                        self.controller.start_sample_drag(
                                            sample.source_id.clone(),
                                            path.clone(),
                                            sample.label.clone(),
                                            pos,
                                        );
                                    }
                                } else if drag_active && response.dragged() {
                                    if let Some(pos) = response.interact_pointer_pos() {
                                        self.controller.update_active_drag(
                                            pos, None, false, None, None, false,
                                        );
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
        let focused = matches!(
            self.controller.ui.focus.context,
            FocusContext::CollectionSample
        );
        style::paint_section_border(ui, scroll_response.response.rect, focused);
        if drag_active
            && let Some(pointer) = pointer_pos
        {
            let target_rect = scroll_response.response.rect.expand2(egui::vec2(8.0, 0.0));
            if target_rect.contains(pointer) {
                self.controller.update_active_drag(
                    pointer,
                    current_collection_id.clone(),
                    true,
                    None,
                    None,
                    false,
                );
                ui.painter().rect_stroke(
                    target_rect,
                    6.0,
                    style::drag_target_stroke(),
                    StrokeKind::Inside,
                );
            }
        }
    }

    fn collection_sample_menu(
        &mut self,
        response: &egui::Response,
        row: usize,
        sample: &CollectionSampleView,
    ) {
        response.context_menu(|ui| {
            let mut close_menu = false;
            ui.label(RichText::new(sample.label.clone()).color(style::palette().text_primary));
            self.sample_tag_menu(ui, &mut close_menu, |app, tag| {
                app.controller.tag_collection_sample(row, tag).is_ok()
            });
            if ui
                .button("Normalize (overwrite)")
                .on_hover_text("Scale to full range and overwrite the wav")
                .clicked()
                && self.controller.normalize_collection_sample(row).is_ok()
            {
                close_menu = true;
            }
            ui.separator();
            let default_name = view_model::sample_display_label(&sample.path);
            let rename_id = ui.make_persistent_id(format!(
                "rename:sample:{}:{}",
                sample.source_id,
                sample.path.display()
            ));
            if self.sample_rename_controls(ui, rename_id, default_name.as_str(), |app, value| {
                app.controller.rename_collection_sample(row, value).is_ok()
            }) {
                close_menu = true;
            }
            let delete_btn = egui::Button::new(
                RichText::new("Delete from collection").color(style::destructive_text()),
            );
            if ui.add(delete_btn).clicked()
                && self.controller.delete_collection_sample(row).is_ok()
            {
                close_menu = true;
            }
            if close_menu {
                ui.close();
            }
        });
    }

    fn collection_row_menu(&mut self, response: &egui::Response, collection: &CollectionRowView) {
        response.context_menu(|ui| {
            if ui.button("Set export folder…").clicked() {
                self.controller.pick_collection_export_path(&collection.id);
                ui.close();
            }
            if ui.button("Clear export folder").clicked() {
                self.controller.clear_collection_export_path(&collection.id);
                ui.close();
            }
            let refresh_enabled = collection.export_path.is_some();
            if ui
                .add_enabled(refresh_enabled, egui::Button::new("Refresh export"))
                .clicked()
            {
                self.controller.refresh_collection_export(&collection.id);
                ui.close();
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
                ui.close();
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
                ui.close();
            }
            ui.separator();
            if ui
                .button(RichText::new("Delete collection").color(style::destructive_text()))
                .clicked()
            {
                let _ = self.controller.delete_collection(&collection.id);
                ui.close();
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
