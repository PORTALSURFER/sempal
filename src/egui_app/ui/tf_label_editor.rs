use super::style;
use super::*;
use crate::egui_app::state::TfLabelCandidateCache;
use crate::sample_sources::config::TfLabelAggregationMode;
use eframe::egui::{self, RichText};
impl EguiApp {
    pub(super) fn render_tf_label_editor(&mut self, ctx: &egui::Context) {
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
                ui.horizontal(|ui| {
                    ui.label("Aggregation");
                    let mut mode = self.controller.ui.tf_labels.aggregation_mode;
                    if ui
                        .selectable_label(mode == TfLabelAggregationMode::MeanTopK, "TopK mean")
                        .clicked()
                    {
                        mode = TfLabelAggregationMode::MeanTopK;
                    }
                    if ui
                        .selectable_label(mode == TfLabelAggregationMode::Max, "Max")
                        .clicked()
                    {
                        mode = TfLabelAggregationMode::Max;
                    }
                    if mode != self.controller.ui.tf_labels.aggregation_mode {
                        self.controller.set_tf_label_aggregation_mode(mode);
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
                    ui.horizontal(|ui| {
                        if ui.button("Find matches").clicked() {
                            match self
                                .controller
                                .tf_label_candidate_matches_for_label(&label.label_id, 500, 40)
                            {
                                Ok(matches) => {
                                    self.controller.ui.tf_labels.last_candidate_label_id =
                                        Some(label.label_id.clone());
                                    self.controller.ui.tf_labels.last_candidate_results = matches
                                        .into_iter()
                                        .map(|entry| TfLabelCandidateCache {
                                            sample_id: entry.sample_id,
                                            score: entry.score,
                                            bucket: entry.bucket,
                                        })
                                        .collect();
                                    self.controller.set_status(
                                        format!("Found matches for {}", label.name),
                                        style::StatusTone::Info,
                                    );
                                }
                                Err(err) => {
                                    self.controller.set_status(
                                        format!("Match search failed: {err}"),
                                        style::StatusTone::Error,
                                    );
                                }
                            }
                        }
                        if self
                            .controller
                            .ui
                            .tf_labels
                            .last_candidate_label_id
                            .as_deref()
                            == Some(&label.label_id)
                            && ui.button("Clear matches").clicked()
                        {
                            self.controller.ui.tf_labels.last_candidate_label_id = None;
                            self.controller.ui.tf_labels.last_candidate_results.clear();
                        }
                        if ui.button("Calibrate").clicked() {
                            self.open_tf_label_calibration(&label);
                        }
                    });

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
                                    self.controller.clear_tf_label_score_cache();
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
                                    self.controller.clear_tf_label_score_cache();
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
                                        self.controller.clear_tf_label_score_cache();
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
                                        self.controller.clear_tf_label_score_cache();
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

                    if self
                        .controller
                        .ui
                        .tf_labels
                        .last_candidate_label_id
                        .as_deref()
                        == Some(&label.label_id)
                    {
                        let candidates = self.controller.ui.tf_labels.last_candidate_results.clone();
                        if candidates.is_empty() {
                            ui.label(
                                RichText::new("No matches returned.").color(palette.text_muted),
                            );
                        } else {
                            ui.add_space(ui.spacing().item_spacing.y);
                            ui.label(
                                RichText::new("Top matches").color(palette.text_primary),
                            );
                            egui::Grid::new(format!("tf_label_matches_{}", label.label_id))
                                .striped(true)
                                .show(ui, |ui| {
                                    ui.label(RichText::new("Sample").color(palette.text_muted));
                                    ui.label(RichText::new("Score").color(palette.text_muted));
                                    ui.label(RichText::new("Bucket").color(palette.text_muted));
                                    ui.label(RichText::new("Actions").color(palette.text_muted));
                                    ui.end_row();
                                    let mut remove_indices = Vec::new();
                                    for (idx, candidate) in candidates.iter().take(20).enumerate() {
                                        let bucket_label = match candidate.bucket {
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
                                        ui.label(&candidate.sample_id);
                                        ui.label(format!("{:.3}", candidate.score));
                                        ui.label(bucket_label);
                                        ui.horizontal(|ui| {
                                            if ui.button("Preview").clicked() {
                                                if let Err(err) = self
                                                    .controller
                                                    .preview_sample_by_id(&candidate.sample_id)
                                                {
                                                    self.controller.set_status(
                                                        format!("Preview failed: {err}"),
                                                        style::StatusTone::Error,
                                                    );
                                                }
                                            }
                                            if ui.button("Accept").clicked() {
                                                if let Err(err) = self.controller.add_tf_anchor(
                                                    &label.label_id,
                                                    &candidate.sample_id,
                                                    1.0,
                                                ) {
                                                    self.controller.set_status(
                                                        format!("Add anchor failed: {err}"),
                                                        style::StatusTone::Error,
                                                    );
                                                } else {
                                                    self.controller.set_status(
                                                        "Anchor added".to_string(),
                                                        style::StatusTone::Info,
                                                    );
                                                    remove_indices.push(idx);
                                                }
                                            }
                                            if ui.button("Reject").clicked() {
                                                remove_indices.push(idx);
                                            }
                                        });
                                        ui.end_row();
                                    }
                                    if !remove_indices.is_empty() {
                                        let mut updated = candidates.clone();
                                        for idx in remove_indices.into_iter().rev() {
                                            if idx < updated.len() {
                                                updated.remove(idx);
                                            }
                                        }
                                        self.controller.ui.tf_labels.last_candidate_results = updated;
                                    }
                                });
                            ui.horizontal(|ui| {
                                let high_count = candidates
                                    .iter()
                                    .filter(|candidate| {
                                        candidate.bucket
                                            == crate::analysis::anchor_scoring::ConfidenceBucket::High
                                    })
                                    .count();
                                if ui
                                    .add_enabled(
                                        high_count > 0,
                                        egui::Button::new("Auto-tag high confidence"),
                                    )
                                    .on_hover_text("Adds high-confidence matches as anchors")
                                    .clicked()
                                {
                                    self.controller.ui.tf_labels.auto_tag_prompt = Some(
                                        crate::egui_app::state::TfAutoTagPrompt {
                                            label_id: label.label_id.clone(),
                                            label_name: label.name.clone(),
                                        },
                                    );
                                }
                            });
                        }
                    }
                }
            });
    }
}
