use super::helpers::{clamp_label_for_width, list_row_height, render_list_row};
use super::*;
use eframe::egui::{self, Color32, RichText, Stroke, Ui};

impl EguiApp {
    pub(super) fn render_triage(&mut self, ui: &mut Ui) {
        let spacing = 8.0;
        let selected = self.controller.ui.triage.selected;
        let loaded = self.controller.ui.triage.loaded;

        ui.columns(3, |columns| {
            self.render_triage_column(
                &mut columns[0],
                "Trash",
                TriageColumn::Trash,
                Color32::from_rgb(198, 143, 143),
                selected,
                loaded,
            );
            columns[0].add_space(spacing);
            self.render_triage_column(
                &mut columns[1],
                "Samples",
                TriageColumn::Neutral,
                Color32::from_rgb(208, 208, 208),
                selected,
                loaded,
            );
            columns[1].add_space(spacing);
            self.render_triage_column(
                &mut columns[2],
                "Keep",
                TriageColumn::Keep,
                Color32::from_rgb(158, 201, 167),
                selected,
                loaded,
            );
        });
    }

    fn render_triage_column(
        &mut self,
        ui: &mut Ui,
        title: &str,
        column: TriageColumn,
        accent: Color32,
        selected: Option<TriageIndex>,
        loaded: Option<TriageIndex>,
    ) {
        ui.label(RichText::new(title).color(accent));
        ui.add_space(6.0);
        let drag_active = self.controller.ui.drag.active_path.is_some();
        let pointer_pos = ui
            .input(|i| i.pointer.hover_pos().or_else(|| i.pointer.interact_pos()))
            .or(self.controller.ui.drag.position);
        let selected_row = match selected {
            Some(TriageIndex { column: c, row }) if c == column => Some(row),
            _ => None,
        };
        let loaded_row = match loaded {
            Some(TriageIndex { column: c, row }) if c == column => Some(row),
            _ => None,
        };
        let triage_autoscroll = self.controller.ui.triage.autoscroll
            && self.controller.ui.collections.selected_sample.is_none();
        let row_height = list_row_height(ui);
        let total_rows = self.controller.triage_indices(column).len();
        let bg_frame = egui::Frame::none().fill(Color32::from_rgb(16, 16, 16));
        let frame_response = bg_frame.show(ui, |ui| {
            let scroll_response = egui::ScrollArea::vertical()
                .id_source(format!("triage_scroll_{title}"))
                .show_rows(ui, row_height, total_rows, |ui, row_range| {
                    for row in row_range {
                        let entry_index = {
                            let indices = self.controller.triage_indices(column);
                            match indices.get(row) {
                                Some(index) => *index,
                                None => continue,
                            }
                        };
                        let Some(entry) = self.controller.wav_entry(entry_index) else {
                            continue;
                        };

                        let is_selected = selected_row == Some(row);
                        let is_loaded = loaded_row == Some(row);
                        let path = entry.relative_path.clone();
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
                        let bg = is_selected.then_some(Color32::from_rgb(30, 30, 30));
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
                            if response.drag_started() {
                                if let Some(pos) = response.interact_pointer_pos() {
                                    let name = path.to_string_lossy().to_string();
                                    self.controller.start_sample_drag(path.clone(), name, pos);
                                }
                            } else if drag_active && response.dragged() {
                                if let Some(pos) = response.interact_pointer_pos() {
                                    self.controller.update_sample_drag(
                                        pos,
                                        None,
                                        false,
                                        Some(column),
                                    );
                                }
                            } else if response.drag_stopped() {
                                self.controller.finish_sample_drag();
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
            desired_offset = (row as f32 + 0.5) * row_height - viewport_height * 0.5;
            self.controller.ui.triage.autoscroll = false;
        }
        let snapped_offset = (desired_offset / row_height)
            .round()
            .clamp(0.0, max_offset / row_height)
            * row_height;
        let mut state = frame_response.inner.state;
        state.offset.y = snapped_offset.clamp(0.0, max_offset);
        state.store(ui.ctx(), frame_response.inner.id);
        if drag_active {
            if let Some(pointer) = pointer_pos {
                if frame_response.response.rect.contains(pointer) {
                    self.controller
                        .update_sample_drag(pointer, None, false, Some(column));
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
}
