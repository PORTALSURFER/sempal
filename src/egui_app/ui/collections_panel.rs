use super::drag_targets;
use super::flat_items_list::{FlatItemsListConfig, render_flat_items_list};
use super::helpers::{
    InlineTextEditAction, NumberColumn, RowMarker, clamp_label_for_width, list_row_height,
    render_inline_text_edit, render_list_row,
};
use super::style;
use super::*;
use crate::egui_app::state::{
    CollectionActionPrompt, CollectionRowView, CollectionSampleView, DragPayload, DragSource,
    DragTarget, FocusContext,
};
use crate::egui_app::view_model;
use eframe::egui::{self, RichText, Stroke, StrokeKind, Ui};
use std::path::PathBuf;
use tracing::debug;

impl EguiApp {
    pub(super) fn render_collections_panel(&mut self, ui: &mut Ui) {
        let palette = style::palette();
        let drag_active = self.controller.ui.drag.payload.is_some();
        let pointer_pos = drag_targets::pointer_pos_for_drag(ui, self.controller.ui.drag.position);
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
                        let rename_match = matches!(
                            self.controller.ui.collections.pending_action,
                            Some(CollectionActionPrompt::Rename { ref target, .. })
                                if target == &collection.id
                        );
                        ui.push_id(&collection.id, |ui| {
                            let row_width = ui.available_width();
                            let padding = ui.spacing().button_padding.x * 2.0;
                            let label = if rename_match {
                                String::new()
                            } else {
                                clamp_label_for_width(&label, row_width - padding)
                            };
                            let bg = selected.then_some(style::row_selected_fill());
                            let response = render_list_row(
                                ui,
                                super::helpers::ListRow {
                                    label: &label,
                                    row_width,
                                    row_height,
                                    bg,
                                    text_color,
                                    sense: if rename_match {
                                        egui::Sense::hover()
                                    } else {
                                        egui::Sense::click()
                                    },
                                    number: None,
                                    marker: None,
                                },
                            );
                            if response.clicked() && !rename_match {
                                if selected {
                                    self.controller.select_collection_by_index(None);
                                } else {
                                    self.controller.select_collection_by_index(Some(index));
                                }
                            }
                            if rename_match {
                                self.render_collection_rename_editor(ui, &response);
                            } else {
                                self.collection_row_menu(&response, collection);
                            }
                            if drag_active
                                && let Some(pointer) = pointer_pos
                                && response.rect.contains(pointer)
                            {
                                let shift_down = ui.input(|i| i.modifiers.shift);
                                self.controller.update_active_drag(
                                    pointer,
                                    DragSource::Collections,
                                    DragTarget::CollectionsRow(collection.id.clone()),
                                    shift_down,
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

    fn render_collection_rename_editor(&mut self, ui: &mut Ui, row_response: &egui::Response) {
        let Some(prompt) = self.controller.ui.collections.pending_action.as_mut() else {
            return;
        };
        let name = match prompt {
            CollectionActionPrompt::Rename { name, .. } => name,
        };
        let padding = ui.spacing().button_padding.x;
        let mut edit_rect = row_response.rect;
        edit_rect.min.x += padding;
        edit_rect.max.x -= padding;
        edit_rect.min.y += 2.0;
        edit_rect.max.y -= 2.0;
        match render_inline_text_edit(
            ui,
            edit_rect,
            name,
            "Rename collection",
            &mut self.controller.ui.collections.rename_focus_requested,
        ) {
            InlineTextEditAction::Submit => self.controller.apply_pending_collection_rename(),
            InlineTextEditAction::Cancel => self.controller.cancel_collection_rename(),
            InlineTextEditAction::None => {}
        }
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
        let hovering_collection = match &self.controller.ui.drag.active_target {
            DragTarget::CollectionsRow(id) => Some(id.clone()),
            DragTarget::CollectionsDropZone { collection_id } => {
                if collection_id.is_none() {
                    current_collection_id.clone()
                } else {
                    collection_id.clone()
                }
            }
            _ => None,
        };
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
        let available_height = ui.available_height();
        let focused_section = matches!(
            self.controller.ui.focus.context,
            FocusContext::CollectionSample
        );
        let scroll_to_sample = self.controller.ui.collections.scroll_to_sample.take();
        let autoscroll_to = duplicate_row.or(scroll_to_sample);
        let list_response = render_flat_items_list(
            ui,
            FlatItemsListConfig {
                scroll_id_salt: "collection_items_scroll",
                min_height: available_height,
                total_rows: samples.len(),
                focused_section,
                autoscroll_to,
                autoscroll_padding_rows: 1.0,
            },
            |ui, row, metrics| {
                let Some(sample) = samples.get(row) else {
                    return;
                };
                let row_width = ui.available_width();
                let path = sample.path.clone();
                let mut label = format!("{} — {}", sample.source, sample.label);
                if sample.missing {
                    label.insert_str(0, "! ");
                }
                let is_selected = Some(row) == selected_row;
                let is_duplicate_hover =
                    drag_active && active_drag_path.as_ref().is_some_and(|p| p == &path);
                let triage_marker = style::triage_marker_color(sample.tag).map(|color| RowMarker {
                    width: style::triage_marker_width(),
                    color,
                });
                let trailing_space = triage_marker
                    .as_ref()
                    .map(|marker| marker.width + metrics.padding * 0.5)
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
                            - metrics.padding
                            - metrics.number_width
                            - metrics.number_gap
                            - trailing_space;
                        let number_text = format!("{}", row + 1);
                        let text_color = if sample.missing {
                            style::missing_text()
                        } else {
                            style::triage_label_color(sample.tag)
                        };
                        let clamped_label = clamp_label_for_width(&label, label_width);
                        let response = render_list_row(
                            ui,
                            super::helpers::ListRow {
                                label: &clamped_label,
                                row_width,
                                row_height: metrics.row_height,
                                bg,
                                text_color,
                                sense: egui::Sense::click_and_drag(),
                                number: Some(NumberColumn {
                                    text: &number_text,
                                    width: metrics.number_width,
                                    color: palette.text_muted,
                                }),
                                marker: triage_marker,
                            },
                        );
                        if is_selected {
                            let marker_width = 4.0;
                            let marker_rect = egui::Rect::from_min_max(
                                response.rect.left_top(),
                                response.rect.left_top()
                                    + egui::vec2(marker_width, metrics.row_height),
                            );
                            ui.painter().rect_filled(
                                marker_rect,
                                0.0,
                                style::selection_marker_fill(),
                            );
                            ui.painter().rect_stroke(
                                response.rect,
                                0.0,
                                style::focused_row_stroke(),
                                StrokeKind::Inside,
                            );
                        }
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
                        let drag_source_id = sample.source_id.clone();
                        let drag_path = path.clone();
                        let drag_label = sample.label.clone();
                        let pending_source_id = sample.source_id.clone();
                        let pending_path = path.clone();
                        let pending_label = sample.label.clone();
                        let match_source_id = sample.source_id.clone();
                        let match_path = path.clone();
                        drag_targets::handle_sample_row_drag(
                            ui,
                            &response,
                            drag_active,
                            &mut self.controller,
                            DragSource::Collections,
                            DragTarget::None,
                            move |pos, controller| {
                                controller.start_sample_drag(
                                    drag_source_id,
                                    drag_path,
                                    drag_label,
                                    pos,
                                );
                            },
                            move |pos, _controller| {
                                Some(crate::egui_app::state::PendingOsDragStart {
                                    payload: DragPayload::Sample {
                                        source_id: pending_source_id,
                                        relative_path: pending_path,
                                    },
                                    label: pending_label,
                                    origin: pos,
                                })
                            },
                            move |pending| {
                                matches!(
                                    &pending.payload,
                                    DragPayload::Sample {
                                        source_id,
                                        relative_path
                                    } if *source_id == match_source_id && *relative_path == match_path
                                )
                            },
                        );
                    },
                );
            },
        );
        if drag_active && let Some(pointer) = pointer_pos {
            let target_rect = list_response.frame_rect.expand2(egui::vec2(8.0, 0.0));
            if target_rect.contains(pointer) {
                debug!(
                    "Collections drop zone hover: pointer={:?} rect={:?} current_collection_id={:?}",
                    pointer, target_rect, current_collection_id
                );
                let shift_down = ui.input(|i| i.modifiers.shift);
                self.controller.update_active_drag(
                    pointer,
                    DragSource::Collections,
                    DragTarget::CollectionsDropZone {
                        collection_id: current_collection_id.clone(),
                    },
                    shift_down,
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
            if ui.add(delete_btn).clicked() && self.controller.delete_collection_sample(row).is_ok()
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
