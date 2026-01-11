use super::drag_targets;
use super::flat_items_list::{FlatItemsListConfig, render_flat_items_list};
use super::helpers::{
    NumberColumn, RowBackground, RowMarker, clamp_label_for_width, list_row_height,
    number_column_width, render_list_row,
};
use super::status_badges;
use super::style;
use super::*;
use crate::egui_app::state::{
    CollectionActionPrompt, DragPayload, DragSample, DragSource, DragTarget, FocusContext,
};
use eframe::egui::{self, RichText, Stroke, StrokeKind, Ui};
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
            let show_hotkey_column = rows.iter().any(|row| row.hotkey.is_some());
            let hotkey_width = if show_hotkey_column {
                number_column_width(9, ui)
            } else {
                0.0
            };
            let list_height = (ui.available_height() * 0.35).max(140.0);
            let list_response = egui::ScrollArea::vertical()
                .id_salt("collections_scroll")
                .max_height(list_height)
                .show(ui, |ui| {
                    let row_height = list_row_height(ui);
                    for (index, collection) in rows.iter().enumerate() {
                        let selected = collection.selected;
                        let is_drag_hover = drag_active
                            && matches!(
                                self.controller.ui.drag.active_target,
                                DragTarget::CollectionsRow(ref id) if id == &collection.id
                            );
                        let mut label = format!("{} ({})", collection.name, collection.count);
                        let (text_color, indicator) = if collection.missing {
                            (
                                status_badges::missing_text_color(),
                                status_badges::missing_prefix(),
                            )
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
                            let label_width = if show_hotkey_column {
                                row_width - padding - hotkey_width
                            } else {
                                row_width - padding
                            };
                            let label = if rename_match {
                                String::new()
                            } else {
                                clamp_label_for_width(&label, label_width)
                            };
                            let bg =
                                RowBackground::from_option(selected.then_some(style::row_selected_fill()));
                            let hotkey_text = collection.hotkey.map(|key| key.to_string()).unwrap_or_default();
                            let response = render_list_row(
                                ui,
                                super::helpers::ListRow {
                                    label: &label,
                                    row_width,
                                    row_height,
                                    background: bg,
                                    skip_hover: false,
                                    text_color,
                                    sense: if rename_match {
                                        egui::Sense::hover()
                                    } else {
                                        egui::Sense::click()
                                    },
                                    number: show_hotkey_column.then_some(NumberColumn {
                                        text: hotkey_text.as_str(),
                                        width: hotkey_width,
                                        color: palette.text_muted,
                                    }),
                                    marker: None,
                                    rating: None,
                                    looped: false,
                                    bpm_label: None,
                                },
                            );
                            if is_drag_hover {
                                ui.painter().rect_stroke(
                                    response.rect.expand(1.0),
                                    0.0,
                                    style::drag_target_stroke(),
                                    StrokeKind::Inside,
                                );
                            }
                            if response.clicked() && !rename_match {
                                if selected {
                                    self.controller.select_collection_by_index(None);
                                } else {
                                    self.controller.select_collection_by_index(Some(index));
                                }
                                self.controller.focus_collections_list_from_ui();
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
                                let alt_down = ui.input(|i| i.modifiers.alt);
                                self.controller.update_active_drag(
                                    pointer,
                                    DragSource::Collections,
                                    DragTarget::CollectionsRow(collection.id.clone()),
                                    shift_down,
                                    alt_down,
                                );
                            }
                        });
                    }
                });
            let min_focus_height = list_row_height(ui);
            let focus_height = list_response
                .content_size
                .y
                .max(min_focus_height)
                .min(list_response.inner_rect.height());
            let focus_rect = egui::Rect::from_min_size(
                list_response.inner_rect.min,
                egui::vec2(list_response.inner_rect.width(), focus_height),
            );
            if matches!(
                self.controller.ui.focus.context,
                FocusContext::CollectionsList
            ) {
                ui.painter().rect_stroke(
                    focus_rect,
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
        let now_epoch = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;
        let samples = self.controller.ui.collections.samples.clone();
        let selected_row = self.controller.ui.collections.selected_sample;
        let selected_paths = self.controller.ui.collections.selected_paths.clone();
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
        let active_drag_paths = if drag_active {
            match &self.controller.ui.drag.payload {
                Some(DragPayload::Sample { relative_path, .. }) => {
                    Some(vec![relative_path.clone()])
                }
                Some(DragPayload::Samples { samples }) => Some(
                    samples
                        .iter()
                        .map(|sample| sample.relative_path.clone())
                        .collect(),
                ),
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
            active_drag_paths.as_ref().and_then(|paths| {
                samples
                    .iter()
                    .position(|s| paths.iter().any(|p| p == &s.path))
            })
        } else {
            None
        };
        let selected_samples: Vec<DragSample> = if selected_paths.len() > 1 {
            samples
                .iter()
                .filter(|sample| selected_paths.iter().any(|p| p == &sample.path))
                .map(|sample| DragSample {
                    source_id: sample.source_id.clone(),
                    relative_path: sample.path.clone(),
                })
                .collect()
        } else {
            Vec::new()
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
                let base_label = format!("{} — {}", sample.source, sample.label);
                let is_selected = selected_paths.iter().any(|p| p == &path);
                let is_focused = Some(row) == selected_row;
                let is_duplicate_hover = drag_active
                    && active_drag_paths
                        .as_ref()
                        .is_some_and(|paths| paths.iter().any(|candidate| candidate == &path));
                let triage_marker = style::triage_marker_color(sample.tag).map(|color| RowMarker {
                    width: style::triage_marker_width(),
                    color,
                });
                let trailing_space = triage_marker
                    .as_ref()
                    .map(|marker| marker.width + metrics.padding * 0.5)
                    .unwrap_or(0.0);
                let bg = RowBackground::from_option(if is_duplicate_hover {
                    Some(style::duplicate_hover_fill())
                } else if is_focused {
                    Some(style::row_selected_fill())
                } else {
                    None
                });
                ui.push_id(
                    format!("{}:{}:{}", sample.source_id, sample.source, sample.label),
                    |ui| {
                        let label_width = row_width
                            - metrics.padding
                            - metrics.number_width
                            - metrics.number_gap
                            - trailing_space;
                        let number_text = format!("{}", row + 1);
                        let status_label = status_badges::apply_sample_status(
                            base_label,
                            style::playback_age_label_color(sample.last_played_at, now_epoch),
                            sample.missing,
                            None,
                        );
                        let text_color = status_label.text_color;
                        let clamped_label = clamp_label_for_width(&status_label.label, label_width);
                        let response = render_list_row(
                            ui,
                            super::helpers::ListRow {
                                label: &clamped_label,
                                row_width,
                                row_height: metrics.row_height,
                                background: bg,
                                skip_hover: false,
                                text_color,
                                sense: egui::Sense::click_and_drag(),
                                number: Some(NumberColumn {
                                    text: &number_text,
                                    width: metrics.number_width,
                                    color: palette.text_muted,
                                }),
                                marker: triage_marker,
                                rating: Some(sample.tag),
                                looped: false,
                                bpm_label: None,
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
                        }
                        if is_focused {
                            ui.painter().rect_stroke(
                                response.rect,
                                0.0,
                                style::focused_row_stroke(),
                                StrokeKind::Inside,
                            );
                        }
                        if response.clicked() {
                            let modifiers = ui.input(|i| i.modifiers);
                            let ctrl = modifiers.command || modifiers.ctrl;
                            if modifiers.shift && ctrl {
                                self.controller.add_range_collection_sample_selection(row);
                            } else if modifiers.shift {
                                self.controller
                                    .extend_collection_sample_selection_to_row(row);
                            } else if ctrl {
                                self.controller.toggle_collection_sample_selection(row);
                            } else {
                                self.controller.clear_collection_sample_selection();
                                self.controller.focus_collection_sample_row(row);
                            }
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
                        let pending_selected = selected_samples.clone();
                        let is_multi_drag = selected_samples.len() > 1 && is_selected;
                        let drag_samples = pending_selected.clone();
                        let pending_samples_for_pending = pending_selected.clone();
                        let _pending_samples_for_match = pending_selected.clone();
                        drag_targets::handle_sample_row_drag(
                            ui,
                            &response,
                            drag_active,
                            &mut self.controller,
                            DragSource::Collections,
                            DragTarget::None,
                            move |pos, controller| {
                                if is_multi_drag {
                                    controller.start_samples_drag(
                                        drag_samples.clone(),
                                        format!("{} samples", drag_samples.len()),
                                        pos,
                                    );
                                } else {
                                    controller.start_sample_drag(
                                        drag_source_id,
                                        drag_path,
                                        drag_label,
                                        pos,
                                    );
                                }
                            },
                            move |pos, _controller| {
                                let payload = if pending_samples_for_pending.len() > 1
                                    && pending_samples_for_pending
                                        .iter()
                                        .any(|sample| sample.relative_path == pending_path)
                                {
                                    DragPayload::Samples {
                                        samples: pending_samples_for_pending.clone(),
                                    }
                                } else {
                                    DragPayload::Sample {
                                        source_id: pending_source_id,
                                        relative_path: pending_path,
                                    }
                                };
                                let label = if matches!(payload, DragPayload::Samples { .. }) {
                                    format!("{} samples", pending_samples_for_pending.len())
                                } else {
                                    pending_label
                                };
                                Some(crate::egui_app::state::PendingOsDragStart {
                                    payload,
                                    label,
                                    origin: pos,
                                })
                            },
                            move |pending| match &pending.payload {
                                DragPayload::Sample {
                                    source_id,
                                    relative_path,
                                } => *source_id == match_source_id && *relative_path == match_path,
                                DragPayload::Samples { samples } => samples.iter().any(|sample| {
                                    sample.source_id == match_source_id
                                        && sample.relative_path == match_path
                                }),
                                DragPayload::Folder { .. } => false,
                                DragPayload::Selection { .. } => false,
                                DragPayload::DropTargetReorder { .. } => false,
                            },
                        );
                    },
                );
            },
        );
        let target_rect = list_response.frame_rect.expand2(egui::vec2(8.0, 0.0));
        let hovered = drag_targets::handle_drop_zone(
            ui,
            &mut self.controller,
            drag_active,
            pointer_pos,
            target_rect,
            DragSource::Collections,
            DragTarget::CollectionsDropZone {
                collection_id: current_collection_id.clone(),
            },
            style::drag_target_stroke(),
            StrokeKind::Inside,
        );
        if hovered {
            debug!(
                "Collections drop zone hover: pointer={:?} rect={:?} current_collection_id={:?}",
                pointer_pos, target_rect, current_collection_id
            );
        }
    }
}
