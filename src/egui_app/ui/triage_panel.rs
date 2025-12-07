use super::helpers::{
    RowMetrics, clamp_label_for_width, list_row_height, render_list_row,
    scroll_offset_to_reveal_row,
};
use super::*;
use crate::egui_app::state::TriageFilter;
use crate::sample_sources::SampleTag;
use eframe::egui::{self, Color32, RichText, Stroke, Ui};
use std::path::Path;

impl EguiApp {
    pub(super) fn render_triage(&mut self, ui: &mut Ui) {
        let selected_row = self.controller.ui.triage.selected_visible;
        let loaded_row = self.controller.ui.triage.loaded_visible;
        let drop_target = self.controller.triage_drop_target();
        self.render_triage_filter(ui);
        ui.add_space(6.0);
        let list_height = ui.available_height().max(0.0);
        let drag_active = self.controller.ui.drag.payload.is_some();
        let pointer_pos = ui
            .input(|i| i.pointer.hover_pos().or_else(|| i.pointer.interact_pos()))
            .or(self.controller.ui.drag.position);
        let triage_autoscroll = self.controller.ui.triage.autoscroll
            && self.controller.ui.collections.selected_sample.is_none();
        let row_height = list_row_height(ui);
        let row_metrics = RowMetrics {
            height: row_height,
            spacing: ui.spacing().item_spacing.y,
        };
        let total_rows = self.controller.visible_triage_indices().len();
        let bg_frame = egui::Frame::none().fill(Color32::from_rgb(16, 16, 16));
        let frame_response = bg_frame.show(ui, |ui| {
            let scroll_response = egui::ScrollArea::vertical()
                .id_source("triage_scroll_single")
                .max_height(list_height)
                .show_rows(ui, row_height, total_rows, |ui, row_range| {
                    for row in row_range {
                        let entry_index = {
                            let indices = self.controller.visible_triage_indices();
                            match indices.get(row) {
                                Some(index) => *index,
                                None => continue,
                            }
                        };
                        let (tag, path) = match self.controller.wav_entry(entry_index) {
                            Some(entry) => (entry.tag, entry.relative_path.clone()),
                            None => continue,
                        };
                        let is_selected = selected_row == Some(row);
                        let is_loaded = loaded_row == Some(row);
                        let row_width = ui.available_width();
                        let padding = ui.spacing().button_padding.x * 2.0;
                        let mut label = self
                            .controller
                            .wav_label(entry_index)
                            .unwrap_or_else(|| path.to_string_lossy().to_string());
                        if is_loaded {
                            label.push_str(" • loaded");
                        }
                        let label = clamp_label_for_width(&label, row_width - padding);
                        let bg = triage_row_bg(tag, is_selected);
                        ui.push_id(&path, |ui| {
                            let response = render_list_row(
                                ui,
                                &label,
                                row_width,
                                row_height,
                                bg,
                                Color32::WHITE,
                                egui::Sense::click_and_drag(),
                            );
                            if response.clicked() {
                                self.controller.select_from_triage(&path);
                            }
                            self.triage_sample_menu(&response, row, &path, &label);
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
        if let (Some(row), true) = (selected_row, triage_autoscroll) {
            desired_offset =
                scroll_offset_to_reveal_row(desired_offset, row, row_metrics, viewport_height, 1.0);
            self.controller.ui.triage.autoscroll = false;
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
                        6.0,
                        Stroke::new(2.0, Color32::from_rgba_unmultiplied(80, 140, 200, 180)),
                    );
                }
            }
        }
    }

    fn triage_sample_menu(
        &mut self,
        response: &egui::Response,
        row: usize,
        path: &Path,
        label: &str,
    ) {
        response.context_menu(|ui| {
            let mut close_menu = false;
            ui.label(RichText::new(label.to_string()).color(Color32::LIGHT_GRAY));
            self.sample_tag_menu(ui, &mut close_menu, |app, tag| {
                app.controller.tag_triage_sample(row, tag).is_ok()
            });
            if ui
                .button("Normalize (overwrite)")
                .on_hover_text("Scale to full range and overwrite the wav")
                .clicked()
            {
                if self.controller.normalize_triage_sample(row).is_ok() {
                    close_menu = true;
                }
            }
            ui.separator();
            let default_name = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or(label);
            let rename_id =
                ui.make_persistent_id(format!("rename:triage:{}", path.display()));
            if self.sample_rename_controls(
                ui,
                rename_id,
                default_name,
                |app, value| app.controller.rename_triage_sample(row, value).is_ok(),
            ) {
                close_menu = true;
            }
            let delete_btn = egui::Button::new(
                RichText::new("Delete file").color(Color32::from_rgb(255, 160, 160)),
            );
            if ui.add(delete_btn).clicked() {
                if self.controller.delete_triage_sample(row).is_ok() {
                    close_menu = true;
                }
            }
            if close_menu {
                ui.close_menu();
            }
        });
    }

    fn render_triage_filter(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.label(RichText::new("Filter").color(Color32::from_rgb(210, 210, 210)));
            for filter in [
                TriageFilter::All,
                TriageFilter::Keep,
                TriageFilter::Trash,
                TriageFilter::Untagged,
            ] {
                let selected = self.controller.ui.triage.filter == filter;
                let label = match filter {
                    TriageFilter::All => "All",
                    TriageFilter::Keep => "Keep",
                    TriageFilter::Trash => "Trash",
                    TriageFilter::Untagged => "Untagged",
                };
                if ui.selectable_label(selected, label).clicked() {
                    self.controller.set_triage_filter(filter);
                }
            }
        });
    }
}

fn triage_row_bg(tag: SampleTag, is_selected: bool) -> Option<Color32> {
    match tag {
        SampleTag::Trash => Some(if is_selected {
            Color32::from_rgba_unmultiplied(160, 72, 72, 180)
        } else {
            Color32::from_rgba_unmultiplied(128, 48, 48, 64)
        }),
        SampleTag::Keep => Some(if is_selected {
            Color32::from_rgba_unmultiplied(72, 144, 100, 180)
        } else {
            Color32::from_rgba_unmultiplied(56, 112, 76, 64)
        }),
        SampleTag::Neutral => is_selected.then_some(Color32::from_rgb(36, 36, 36)),
    }
}
