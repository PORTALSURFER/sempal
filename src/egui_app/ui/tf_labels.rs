use super::style;
use super::*;
use crate::egui_app::state::TfLabelCreatePrompt;
use eframe::egui::{self, RichText};

impl EguiApp {
    pub(super) fn render_tf_label_windows(&mut self, ctx: &egui::Context) {
        self.render_tf_label_create_prompt(ctx);
        self.render_tf_label_editor(ctx);
    }

    pub(super) fn open_tf_label_editor(&mut self) {
        self.controller.ui.tf_labels.editor_open = true;
    }

    pub(super) fn open_tf_label_create_prompt(
        &mut self,
        name: String,
        anchor_sample_id: Option<String>,
    ) {
        let defaults = crate::analysis::embedding::tf_label_defaults();
        self.controller.ui.tf_labels.create_prompt = Some(TfLabelCreatePrompt {
            name,
            threshold: defaults.threshold,
            gap: defaults.gap,
            topk: defaults.topk,
            anchor_sample_id,
        });
    }

    fn render_tf_label_create_prompt(&mut self, ctx: &egui::Context) {
        let Some(mut prompt) = self.controller.ui.tf_labels.create_prompt.take() else {
            return;
        };
        let palette = style::palette();
        let mut close = false;
        egui::Window::new("Create training-free label")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.label(RichText::new("Label name").color(palette.text_primary));
                ui.text_edit_singleline(&mut prompt.name);
                ui.add_space(ui.spacing().item_spacing.y);

                ui.horizontal(|ui| {
                    ui.label(RichText::new("Threshold").color(palette.text_primary));
                    ui.add(egui::DragValue::new(&mut prompt.threshold).speed(0.01));
                });
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Gap").color(palette.text_primary));
                    ui.add(egui::DragValue::new(&mut prompt.gap).speed(0.01));
                });
                ui.horizontal(|ui| {
                    ui.label(RichText::new("TopK").color(palette.text_primary));
                    ui.add(egui::DragValue::new(&mut prompt.topk).speed(1));
                });

                ui.add_space(ui.spacing().item_spacing.y);
                ui.horizontal(|ui| {
                    if ui.button("Create label").clicked() {
                        match self.controller.create_tf_label(
                            prompt.name.as_str(),
                            prompt.threshold,
                            prompt.gap,
                            prompt.topk,
                        ) {
                            Ok(label) => {
                                if let Some(sample_id) = prompt.anchor_sample_id.as_ref() {
                                    if let Err(err) = self
                                        .controller
                                        .add_tf_anchor(&label.label_id, sample_id, 1.0)
                                    {
                                        self.controller.set_status(
                                            format!("Add anchor failed: {err}"),
                                            style::StatusTone::Error,
                                        );
                                    }
                                }
                                self.controller.set_status(
                                    format!("Created label {}", label.name),
                                    style::StatusTone::Info,
                                );
                                close = true;
                            }
                            Err(err) => {
                                self.controller.set_status(
                                    format!("Create label failed: {err}"),
                                    style::StatusTone::Error,
                                );
                            }
                        }
                    }
                    if ui.button("Cancel").clicked() {
                        close = true;
                    }
                });
            });
        if close {
            return;
        }
        self.controller.ui.tf_labels.create_prompt = Some(prompt);
    }

    fn render_tf_label_editor(&mut self, ctx: &egui::Context) {
        if !self.controller.ui.tf_labels.editor_open {
            return;
        }
        let palette = style::palette();
        let labels = match self.controller.list_tf_labels() {
            Ok(labels) => labels,
            Err(err) => {
                self.controller.set_status(
                    format!("Load labels failed: {err}"),
                    style::StatusTone::Error,
                );
                Vec::new()
            }
        };
        egui::Window::new("Training-free labels")
            .collapsible(false)
            .resizable(true)
            .default_width(460.0)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    if ui.button("New label").clicked() {
                        self.open_tf_label_create_prompt(String::new(), None);
                    }
                });
                ui.add_space(ui.spacing().item_spacing.y);
                self.render_tf_label_match_panel(ui);
                if labels.is_empty() {
                    ui.add_space(ui.spacing().item_spacing.y);
                    ui.label(
                        RichText::new("No training-free labels yet.")
                            .color(palette.text_muted)
                            .italics(),
                    );
                }
                for label in labels {
                    ui.separator();
                    ui.label(RichText::new(&label.name).color(palette.text_primary).strong());

                    let name_id = ui.make_persistent_id(format!("tf_label_name:{}", label.label_id));
                    let mut name = ui.ctx().data_mut(|data| {
                        let value = data.get_temp_mut_or_default::<String>(name_id);
                        if value.is_empty() {
                            *value = label.name.clone();
                        }
                        value.clone()
                    });
                    let threshold_id =
                        ui.make_persistent_id(format!("tf_label_threshold:{}", label.label_id));
                    let mut threshold = ui.ctx().data_mut(|data| {
                        let value = data.get_temp_mut_or_default::<Option<f32>>(threshold_id);
                        if value.is_none() {
                            *value = Some(label.threshold);
                        }
                        value.unwrap_or(label.threshold)
                    });
                    let gap_id = ui.make_persistent_id(format!("tf_label_gap:{}", label.label_id));
                    let mut gap = ui.ctx().data_mut(|data| {
                        let value = data.get_temp_mut_or_default::<Option<f32>>(gap_id);
                        if value.is_none() {
                            *value = Some(label.gap);
                        }
                        value.unwrap_or(label.gap)
                    });
                    let topk_id = ui.make_persistent_id(format!("tf_label_topk:{}", label.label_id));
                    let mut topk = ui.ctx().data_mut(|data| {
                        let value = data.get_temp_mut_or_default::<Option<i64>>(topk_id);
                        if value.is_none() {
                            *value = Some(label.topk);
                        }
                        value.unwrap_or(label.topk)
                    });

                    ui.horizontal(|ui| {
                        ui.label("Name");
                        ui.text_edit_singleline(&mut name);
                    });
                    ui.ctx()
                        .data_mut(|data| data.insert_temp(name_id, name.clone()));
                    ui.horizontal(|ui| {
                        ui.label("Threshold");
                        ui.add(egui::DragValue::new(&mut threshold).speed(0.01));
                        ui.label("Gap");
                        ui.add(egui::DragValue::new(&mut gap).speed(0.01));
                        ui.label("TopK");
                        ui.add(egui::DragValue::new(&mut topk).speed(1));
                    });
                    ui.ctx().data_mut(|data| {
                        data.insert_temp(threshold_id, Some(threshold));
                        data.insert_temp(gap_id, Some(gap));
                        data.insert_temp(topk_id, Some(topk));
                    });
                    ui.horizontal(|ui| {
                        if ui.button("Save").clicked() {
                            match self
                                .controller
                                .update_tf_label(&label.label_id, &name, threshold, gap, topk)
                            {
                                Ok(()) => {
                                    self.controller.set_status(
                                        format!("Updated {}", name),
                                        style::StatusTone::Info,
                                    );
                                }
                                Err(err) => {
                                    self.controller.set_status(
                                        format!("Update failed: {err}"),
                                        style::StatusTone::Error,
                                    );
                                }
                            }
                        }
                        let delete_button = egui::Button::new(
                            RichText::new("Delete").color(style::destructive_text()),
                        );
                        if ui.add(delete_button).clicked() {
                            match self.controller.delete_tf_label(&label.label_id) {
                                Ok(()) => {
                                    self.controller.set_status(
                                        format!("Deleted {}", label.name),
                                        style::StatusTone::Info,
                                    );
                                }
                                Err(err) => {
                                    self.controller.set_status(
                                        format!("Delete failed: {err}"),
                                        style::StatusTone::Error,
                                    );
                                }
                            }
                        }
                    });

                    let anchors = match self.controller.list_tf_anchors(&label.label_id) {
                        Ok(anchors) => anchors,
                        Err(err) => {
                            self.controller.set_status(
                                format!("Load anchors failed: {err}"),
                                style::StatusTone::Error,
                            );
                            Vec::new()
                        }
                    };
                    ui.add_space(ui.spacing().item_spacing.y);
                    ui.label(RichText::new("Anchors").color(palette.text_primary));
                    if anchors.is_empty() {
                        ui.label(
                            RichText::new("No anchors yet. Add via sample context menu.")
                                .color(palette.text_muted),
                        );
                    }
                    for anchor in anchors {
                        let weight_id =
                            ui.make_persistent_id(format!("tf_anchor_weight:{}", anchor.anchor_id));
                        let mut weight = ui.ctx().data_mut(|data| {
                            let value = data.get_temp_mut_or_default::<Option<f32>>(weight_id);
                            if value.is_none() {
                                *value = Some(anchor.weight);
                            }
                            value.unwrap_or(anchor.weight)
                        });
                        ui.horizontal_wrapped(|ui| {
                            ui.label(&anchor.sample_id);
                            ui.label("Weight");
                            ui.add(egui::DragValue::new(&mut weight).speed(0.05));
                            ui.ctx().data_mut(|data| {
                                data.insert_temp(weight_id, Some(weight));
                            });
                            if ui.button("Update").clicked() {
                                match self.controller.update_tf_anchor(&anchor.anchor_id, weight) {
                                    Ok(()) => {
                                        self.controller.set_status(
                                            "Anchor updated".to_string(),
                                            style::StatusTone::Info,
                                        );
                                    }
                                    Err(err) => {
                                        self.controller.set_status(
                                            format!("Anchor update failed: {err}"),
                                            style::StatusTone::Error,
                                        );
                                    }
                                }
                            }
                            let delete_button = egui::Button::new(
                                RichText::new("Remove").color(style::destructive_text()),
                            );
                            if ui.add(delete_button).clicked() {
                                match self.controller.delete_tf_anchor(&anchor.anchor_id) {
                                    Ok(()) => {
                                        self.controller.set_status(
                                            "Anchor removed".to_string(),
                                            style::StatusTone::Info,
                                        );
                                    }
                                    Err(err) => {
                                        self.controller.set_status(
                                            format!("Anchor remove failed: {err}"),
                                            style::StatusTone::Error,
                                        );
                                    }
                                }
                            }
                        });
                    }
                }
            });
    }

    fn render_tf_label_match_panel(&mut self, ui: &mut egui::Ui) {
        let palette = style::palette();
        let Some(row) = self.controller.ui.browser.selected_visible else {
            ui.label(
                RichText::new("Select a sample to preview label scores.")
                    .color(palette.text_muted),
            );
            return;
        };
        let sample_id = match self.controller.sample_id_for_visible_row(row) {
            Ok(sample_id) => sample_id,
            Err(err) => {
                self.controller.set_status(err, style::StatusTone::Error);
                return;
            }
        };
        let sample_label = self
            .controller
            .visible_browser_indices()
            .get(row)
            .copied()
            .and_then(|index| self.controller.wav_entry(index))
            .map(|entry| view_model::sample_display_label(&entry.relative_path))
            .unwrap_or_else(|| "Selected sample".to_string());
        ui.label(RichText::new(format!("Scores for {}", sample_label)).color(palette.text_primary));
        let matches = match self.controller.tf_label_matches_for_sample(&sample_id) {
            Ok(matches) => matches,
            Err(err) => {
                self.controller.set_status(
                    format!("Score labels failed: {err}"),
                    style::StatusTone::Error,
                );
                Vec::new()
            }
        };
        if matches.is_empty() {
            ui.label(RichText::new("No label scores yet.").color(palette.text_muted));
            return;
        }
        egui::Grid::new("tf_label_scores")
            .striped(true)
            .show(ui, |ui| {
                ui.label(RichText::new("Label").color(palette.text_muted));
                ui.label(RichText::new("Score").color(palette.text_muted));
                ui.label(RichText::new("Bucket").color(palette.text_muted));
                ui.label(RichText::new("Gap").color(palette.text_muted));
                ui.label(RichText::new("Anchors").color(palette.text_muted));
                ui.end_row();

                for score in matches {
                    let bucket_label = match score.bucket {
                        crate::analysis::anchor_scoring::ConfidenceBucket::High => {
                            RichText::new("High").color(palette.success)
                        }
                        crate::analysis::anchor_scoring::ConfidenceBucket::Medium => {
                            RichText::new("Medium").color(palette.warning)
                        }
                        crate::analysis::anchor_scoring::ConfidenceBucket::Low => {
                            RichText::new("Low").color(palette.text_muted)
                        }
                    };
                    ui.label(&score.name);
                    ui.label(format!("{:.3}", score.score));
                    ui.label(bucket_label);
                    if score.gap.is_finite() {
                        ui.label(format!("{:.3}", score.gap));
                    } else {
                        ui.label("-");
                    }
                    ui.label(score.anchor_count.to_string());
                    ui.end_row();
                }
            });
    }
}
