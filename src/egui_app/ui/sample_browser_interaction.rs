use super::*;
use crate::egui_app::state::{DragPayload, DragSample, DragSource, DragTarget};
use crate::egui_app::ui::style::StatusTone;
use crate::egui_app::view_model;
use eframe::egui::{self, RichText};
use std::path::Path;

impl EguiApp {
    pub(super) fn handle_browser_row_click(
        &mut self,
        ui: &egui::Ui,
        response: &egui::Response,
        row: usize,
    ) {
        if response.clicked() {
            let modifiers = ui.input(|i| i.modifiers);
            let ctrl = modifiers.command || modifiers.ctrl;
            if modifiers.shift && ctrl {
                self.controller.add_range_browser_selection(row);
            } else if modifiers.shift {
                self.controller.extend_browser_selection_to_row(row);
            } else if ctrl {
                self.controller.toggle_browser_row_selection(row);
            } else {
                self.controller.clear_browser_selection();
                self.controller.focus_browser_row_only(row);
            }
        }
    }

    pub(super) fn handle_sample_row_drag(
        &mut self,
        ui: &mut egui::Ui,
        response: &egui::Response,
        drag_active: bool,
        drop_target: DragTarget,
        drag_source: DragSource,
        path: &Path,
    ) {
        let drag_path = path.to_path_buf();
        let drag_label = view_model::sample_display_label(path);
        let selected_paths = self.controller.ui.browser.selected_paths.clone();
        let is_multi_drag = selected_paths.len() > 1;
        let pending_path = drag_path.clone();
        let pending_label = drag_label.clone();
        let match_path = drag_path.clone();
        let pending_selected = selected_paths.clone();
        drag_targets::handle_sample_row_drag(
            ui,
            response,
            drag_active,
            &mut self.controller,
            drag_source,
            drop_target,
            move |pos, controller| {
                if let Some(source) = controller.current_source() {
                    if is_multi_drag {
                        let samples = selected_paths
                            .iter()
                            .map(|path| DragSample {
                                source_id: source.id.clone(),
                                relative_path: path.clone(),
                            })
                            .collect();
                        controller.start_samples_drag(
                            samples,
                            format!("{} samples", selected_paths.len()),
                            pos,
                        );
                    } else {
                        controller.start_sample_drag(source.id.clone(), drag_path, drag_label, pos);
                    }
                } else {
                    controller.set_status("Select a source before dragging", StatusTone::Warning);
                }
            },
            move |pos, controller| {
                let source = controller.current_source()?;
                let payload = if pending_selected.len() > 1 {
                    DragPayload::Samples {
                        samples: pending_selected
                            .iter()
                            .map(|path| DragSample {
                                source_id: source.id.clone(),
                                relative_path: path.clone(),
                            })
                            .collect(),
                    }
                } else {
                    DragPayload::Sample {
                        source_id: source.id.clone(),
                        relative_path: pending_path,
                    }
                };
                let label = if matches!(payload, DragPayload::Samples { .. }) {
                    format!("{} samples", pending_selected.len())
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
                DragPayload::Sample { relative_path, .. } => *relative_path == match_path,
                DragPayload::Samples { samples } => samples
                    .iter()
                    .any(|sample| sample.relative_path == match_path),
                DragPayload::Folder { .. } => false,
                DragPayload::Selection { .. } => false,
            },
        );
    }

    pub(super) fn browser_sample_menu(
        &mut self,
        response: &egui::Response,
        row: usize,
        path: &Path,
        label: &str,
        missing: bool,
    ) {
        response.context_menu(|ui| {
            let palette = style::palette();
            let mut close_menu = false;
            let action_rows = self.controller.action_rows_from_primary(row);
            ui.label(RichText::new(label.to_string()).color(palette.text_primary));
            if ui.button("Open in file explorer").clicked() {
                self.controller.reveal_browser_sample_in_file_explorer(path);
                close_menu = true;
            }
            if ui.button("Find similar").clicked() {
                if let Err(err) = self.controller.find_similar_for_visible_row(row) {
                    self.controller
                        .set_status(format!("Find similar failed: {err}"), StatusTone::Error);
                } else {
                    close_menu = true;
                    ui.close();
                }
            }
            if ui.button("Find duplicates").clicked() {
                if let Err(err) = self.controller.find_duplicates_for_visible_row(row) {
                    self.controller
                        .set_status(format!("Find duplicates failed: {err}"), StatusTone::Error);
                } else {
                    close_menu = true;
                    ui.close();
                }
            }
            if ui.button("Recalculate similarity").clicked() {
                if let Err(err) = self
                    .controller
                    .recalc_similarity_for_browser_rows(&action_rows)
                {
                    self.controller.set_status(
                        format!("Similarity prep failed: {err}"),
                        StatusTone::Error,
                    );
                } else {
                    close_menu = true;
                    ui.close();
                }
            }
            ui.separator();
            self.sample_tag_menu(ui, &mut close_menu, |app, tag| {
                app.controller
                    .tag_browser_samples(&action_rows, tag, row)
                    .is_ok()
            });
            if ui
                .button("Normalize (overwrite)")
                .on_hover_text("Scale to full range and overwrite the wav")
                .clicked()
                && self
                    .controller
                    .normalize_browser_samples(&action_rows)
                    .is_ok()
            {
                close_menu = true;
            }
            let crossfade_btn = ui
                .button("Apply Seamless Loop Crossfade")
                .on_hover_text("Alt-click to customize the crossfade depth");
            if crossfade_btn.clicked() {
                let alt_click = ui.input(|i| i.modifiers.alt);
                if alt_click {
                    if let Err(err) = self
                        .controller
                        .request_loop_crossfade_prompt_for_browser_row(row)
                    {
                        self.controller.set_status(err, StatusTone::Error);
                    } else {
                        close_menu = true;
                    }
                } else if let Err(err) = self.controller.loop_crossfade_browser_samples(
                    &action_rows,
                    crate::egui_app::state::LoopCrossfadeSettings::default(),
                    row,
                ) {
                    self.controller.set_status(err, StatusTone::Error);
                } else {
                    close_menu = true;
                }
            }
            let default_name = view_model::sample_display_label(path);
            let rename_id = ui.make_persistent_id(format!("rename:triage:{}", path.display()));
            if self.sample_rename_controls(ui, rename_id, default_name.as_str(), |app, value| {
                app.controller.rename_browser_sample(row, value).is_ok()
            }) {
                close_menu = true;
            }
            let delete_btn =
                egui::Button::new(RichText::new("Delete file").color(style::destructive_text()));
            if ui.add(delete_btn).clicked()
                && self.controller.delete_browser_samples(&action_rows).is_ok()
            {
                close_menu = true;
            }

            if missing {
                let dead_rows: Vec<usize> = action_rows
                    .iter()
                    .copied()
                    .filter(|&visible_row| {
                        self.controller
                            .visible_browser_index(visible_row)
                            .and_then(|entry_idx| self.controller.wav_entry(entry_idx))
                            .is_some_and(|entry| entry.missing)
                    })
                    .collect();
                let label = if dead_rows.len() <= 1 {
                    "Remove dead link"
                } else {
                    "Remove dead links"
                };
                let btn = egui::Button::new(RichText::new(label).color(style::destructive_text()));
                let response = ui
                    .add_enabled(!dead_rows.is_empty(), btn)
                    .on_hover_text("Remove missing items from the library (does not delete files)");
                if response.clicked()
                    && self
                        .controller
                        .remove_dead_link_browser_samples(&dead_rows)
                        .is_ok()
                {
                    close_menu = true;
                }
            }
            if close_menu {
                ui.close();
            }
        });
    }
}
