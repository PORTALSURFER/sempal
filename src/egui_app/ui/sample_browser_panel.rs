use super::helpers::{
    NumberColumn, RowMarker, RowMetrics, clamp_label_for_width, list_row_height,
    number_column_width, render_list_row, scroll_offset_to_reveal_row,
};
use super::style;
use super::*;
use crate::egui_app::state::TriageFlagFilter;
use eframe::egui::{self, RichText, StrokeKind, Ui};
use std::path::Path;

impl EguiApp {
    pub(super) fn render_sample_browser(&mut self, ui: &mut Ui) {
        let palette = style::palette();
        let selected_row = self.controller.ui.browser.selected_visible;
        let loaded_row = self.controller.ui.browser.loaded_visible;
        let drop_target = self.controller.triage_flag_drop_target();
        self.render_sample_browser_filter(ui);
        ui.add_space(6.0);
        let list_height = ui.available_height().max(0.0);
        let drag_active = self.controller.ui.drag.payload.is_some();
        let pointer_pos = ui
            .input(|i| i.pointer.hover_pos().or_else(|| i.pointer.interact_pos()))
            .or(self.controller.ui.drag.position);
        let autoscroll_enabled = self.controller.ui.browser.autoscroll
            && self.controller.ui.collections.selected_sample.is_none();
        let row_height = list_row_height(ui);
        let row_metrics = RowMetrics {
            height: row_height,
            spacing: ui.spacing().item_spacing.y,
        };
        let total_rows = self.controller.visible_browser_indices().len();
        let bg_frame = egui::Frame::new()
            .fill(style::compartment_fill())
            .stroke(style::outer_border());
        let frame_response = bg_frame.show(ui, |ui| {
            let number_width = number_column_width(total_rows, ui);
            let number_gap = ui.spacing().button_padding.x * 0.5;
            let scroll_response = egui::ScrollArea::vertical()
                .id_salt("sample_browser_scroll")
                .max_height(list_height)
                .show_rows(ui, row_height, total_rows, |ui, row_range| {
                    for row in row_range {
                        let entry_index = {
                            let indices = self.controller.visible_browser_indices();
                            match indices.get(row) {
                                Some(index) => *index,
                                None => continue,
                            }
                        };
                        let (tag, path) = match self.controller.wav_entry(entry_index) {
                            Some(entry) => (entry.tag, entry.relative_path.clone()),
                            None => continue,
                        };
                        let is_focused = selected_row == Some(row);
                        let is_selected = self
                            .controller
                            .ui
                            .browser
                            .selected_paths
                            .iter()
                            .any(|p| p == &path);
                        let is_loaded = loaded_row == Some(row);
                        let row_width = ui.available_width();
                        let padding = ui.spacing().button_padding.x * 2.0;
                        let triage_marker =
                            style::triage_marker_color(tag).map(|color| RowMarker {
                                width: style::triage_marker_width(),
                                color,
                            });
                        let trailing_space = triage_marker
                            .as_ref()
                            .map(|marker| marker.width + padding * 0.5)
                            .unwrap_or(0.0);
                        let mut label = self
                            .controller
                            .wav_label(entry_index)
                            .unwrap_or_else(|| path.to_string_lossy().to_string());
                        if is_loaded {
                            label.push_str(" • loaded");
                        }
                        let label_width =
                            row_width - padding - number_width - number_gap - trailing_space;
                        let label = clamp_label_for_width(&label, label_width);
                        let bg = if is_selected || is_focused {
                            Some(style::row_selected_fill())
                        } else {
                            None
                        };
                        let number_text = format!("{}", row + 1);
                        ui.push_id(&path, |ui| {
                            let response = render_list_row(
                                ui,
                                &label,
                                row_width,
                                row_height,
                                bg,
                                palette.text_primary,
                                egui::Sense::click_and_drag(),
                                Some(NumberColumn {
                                    text: &number_text,
                                    width: number_width,
                                    color: palette.text_muted,
                                }),
                                triage_marker,
                            );
                            if is_selected {
                                let marker_width = 4.0;
                                let marker_rect = egui::Rect::from_min_max(
                                    response.rect.left_top(),
                                    response.rect.left_top() + egui::vec2(marker_width, row_height),
                                );
                                ui.painter().rect_filled(
                                    marker_rect,
                                    0.0,
                                    style::selection_marker_fill(),
                                );
                            }
                            if response.clicked() {
                                let modifiers = ui.input(|i| i.modifiers);
                                if modifiers.shift {
                                    self.controller.extend_browser_selection_to_row(row);
                                } else if modifiers.command || modifiers.ctrl {
                                    self.controller.toggle_browser_row_selection(row);
                                } else {
                                    self.controller.focus_browser_row(row);
                                }
                            }
                            if is_focused {
                                ui.painter().rect_stroke(
                                    response.rect,
                                    0.0,
                                    style::focused_row_stroke(),
                                    StrokeKind::Inside,
                                );
                            }
                            self.browser_sample_menu(&response, row, &path, &label);
                            if response.drag_started() {
                                if let Some(pos) = response.interact_pointer_pos() {
                                    let name = path.to_string_lossy().to_string();
                                    self.controller.start_sample_drag(path.clone(), name, pos);
                                }
                            } else if drag_active && response.dragged() {
                                if let Some(pos) = response.interact_pointer_pos() {
                                    self.controller.update_active_drag(
                                        pos,
                                        None,
                                        false,
                                        Some(drop_target),
                                    );
                                }
                            } else if response.drag_stopped() {
                                self.controller.finish_active_drag();
                            }
                        });
                    }
                });
            scroll_response
        });
        let viewport_height = frame_response.inner.inner_rect.height();
        let content_height = frame_response.inner.content_size.y;
        let max_offset = (content_height - viewport_height).max(0.0);
        let mut desired_offset = frame_response.inner.state.offset.y;
        if let (Some(row), true) = (selected_row, autoscroll_enabled) {
            desired_offset =
                scroll_offset_to_reveal_row(desired_offset, row, row_metrics, viewport_height, 1.0);
            self.controller.ui.browser.autoscroll = false;
        }
        let mut state = frame_response.inner.state;
        state.offset.y = desired_offset.clamp(0.0, max_offset);
        state.store(ui.ctx(), frame_response.inner.id);
        if drag_active {
            if let Some(pointer) = pointer_pos {
                if frame_response.response.rect.contains(pointer) {
                    self.controller
                        .update_active_drag(pointer, None, false, Some(drop_target));
                }
            }
        }
        if drag_active {
            if let Some(pointer) = pointer_pos {
                if frame_response.response.rect.contains(pointer) {
                    ui.painter().rect_stroke(
                        frame_response.response.rect,
                        0.0,
                        style::drag_target_stroke(),
                        StrokeKind::Inside,
                    );
                }
            }
        }
    }

    fn browser_sample_menu(
        &mut self,
        response: &egui::Response,
        row: usize,
        path: &Path,
        label: &str,
    ) {
        response.context_menu(|ui| {
            let palette = style::palette();
            let mut close_menu = false;
            let action_rows = self.controller.action_rows_from_primary(row);
            ui.label(RichText::new(label.to_string()).color(palette.text_primary));
            self.sample_tag_menu(ui, &mut close_menu, |app, tag| {
                app.controller
                    .tag_browser_samples(&action_rows, tag)
                    .is_ok()
            });
            if ui
                .button("Normalize (overwrite)")
                .on_hover_text("Scale to full range and overwrite the wav")
                .clicked()
            {
                if self
                    .controller
                    .normalize_browser_samples(&action_rows)
                    .is_ok()
                {
                    close_menu = true;
                }
            }
            ui.separator();
            let default_name = path.file_name().and_then(|n| n.to_str()).unwrap_or(label);
            let rename_id = ui.make_persistent_id(format!("rename:triage:{}", path.display()));
            if self.sample_rename_controls(ui, rename_id, default_name, |app, value| {
                app.controller.rename_browser_sample(row, value).is_ok()
            }) {
                close_menu = true;
            }
            let delete_btn =
                egui::Button::new(RichText::new("Delete file").color(style::destructive_text()));
            if ui.add(delete_btn).clicked() {
                if self.controller.delete_browser_samples(&action_rows).is_ok() {
                    close_menu = true;
                }
            }
            if close_menu {
                ui.close();
            }
        });
    }

    fn render_sample_browser_filter(&mut self, ui: &mut Ui) {
        let palette = style::palette();
        ui.horizontal(|ui| {
            ui.label(RichText::new("Filter").color(palette.text_primary));
            for filter in [
                TriageFlagFilter::All,
                TriageFlagFilter::Keep,
                TriageFlagFilter::Trash,
                TriageFlagFilter::Untagged,
            ] {
                let selected = self.controller.ui.browser.filter == filter;
                let label = match filter {
                    TriageFlagFilter::All => "All",
                    TriageFlagFilter::Keep => "Keep",
                    TriageFlagFilter::Trash => "Trash",
                    TriageFlagFilter::Untagged => "Untagged",
                };
                if ui.selectable_label(selected, label).clicked() {
                    self.controller.set_browser_filter(filter);
                }
            }
        });
    }
}
