use super::style;
use super::*;
use crate::egui_app::state::{TfLabelCalibrationSample, TfLabelCalibrationState};
use crate::egui_app::view_model;
use eframe::egui::{self, RichText};
use std::collections::HashMap;

const CALIBRATION_CANDIDATE_K: usize = 1000;
const CALIBRATION_TOP_K: usize = 200;
const THRESHOLD_MARGIN: f32 = 0.02;

impl EguiApp {
    pub(super) fn open_tf_label_calibration(&mut self, label: &crate::egui_app::controller::TfLabel) {
        self.refresh_tf_label_calibration(label.label_id.clone(), label.name.clone());
    }

    pub(super) fn render_tf_label_calibration_window(&mut self, ctx: &egui::Context) {
        let Some(mut state) = self.controller.ui.tf_labels.calibration.take() else {
            return;
        };
        let palette = style::palette();
        let current_label = match self.find_tf_label_by_id(&state.label_id) {
            Ok(label) => label,
            Err(err) => {
                self.controller.set_status(
                    format!("Load label failed: {err}"),
                    style::StatusTone::Error,
                );
                return;
            }
        };
        let Some(label) = current_label else {
            self.controller.set_status(
                "Label no longer exists".to_string(),
                style::StatusTone::Warning,
            );
            return;
        };

        let (suggested_threshold, suggested_gap) =
            suggest_thresholds(&state.samples, &state.decisions, label.threshold, label.gap);
        state.suggested_threshold = suggested_threshold;
        state.suggested_gap = suggested_gap;

        let mut close = false;
        egui::Window::new("Label calibration")
            .collapsible(false)
            .resizable(true)
            .default_size([640.0, 520.0])
            .show(ctx, |ui| {
                ui.label(
                    RichText::new(format!("Label: {}", state.label_name))
                        .color(palette.text_primary)
                        .strong(),
                );
                ui.label(
                    RichText::new(format!(
                        "Current threshold {:.3}, gap {:.3}",
                        label.threshold, label.gap
                    ))
                    .color(palette.text_muted),
                );

                let (up_count, down_count) = count_votes(&state.decisions);
                ui.horizontal(|ui| {
                    ui.label(format!("Thumbs up: {up_count}"));
                    ui.label(format!("Thumbs down: {down_count}"));
                    ui.label(format!("Samples: {}", state.samples.len()));
                });

                ui.add_space(ui.spacing().item_spacing.y);
                if let Some(value) = state.suggested_threshold {
                    ui.label(
                        RichText::new(format!("Suggested threshold: {value:.3}"))
                            .color(palette.text_primary),
                    );
                } else {
                    ui.label(
                        RichText::new("Suggested threshold: need thumbs up samples")
                            .color(palette.text_muted),
                    );
                }
                if let Some(value) = state.suggested_gap {
                    ui.label(
                        RichText::new(format!("Suggested gap: {value:.3}"))
                            .color(palette.text_primary),
                    );
                } else {
                    ui.label(
                        RichText::new("Suggested gap: need both up/down samples")
                            .color(palette.text_muted),
                    );
                }

                ui.add_space(ui.spacing().item_spacing.y);
                egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
                    egui::Grid::new("tf_label_calibration_grid")
                        .striped(true)
                        .show(ui, |ui| {
                            ui.label(RichText::new("Sample").color(palette.text_muted));
                            ui.label(RichText::new("Score").color(palette.text_muted));
                            ui.label(RichText::new("Bucket").color(palette.text_muted));
                            ui.label(RichText::new("Vote").color(palette.text_muted));
                            ui.end_row();
                            for sample in &state.samples {
                                let sample_label = sample_label_from_id(&sample.sample_id);
                                let decision = state.decisions.get(&sample.sample_id).copied();
                                let bucket_label = match sample.bucket {
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
                                ui.label(sample_label);
                                ui.label(format!("{:.3}", sample.score));
                                ui.label(bucket_label);
                                ui.horizontal(|ui| {
                                    if ui
                                        .selectable_label(decision == Some(true), "Up")
                                        .clicked()
                                    {
                                        state.decisions.insert(sample.sample_id.clone(), true);
                                    }
                                    if ui
                                        .selectable_label(decision == Some(false), "Down")
                                        .clicked()
                                    {
                                        state.decisions.insert(sample.sample_id.clone(), false);
                                    }
                                    if decision.is_some() && ui.button("Clear").clicked() {
                                        state.decisions.remove(&sample.sample_id);
                                    }
                                    if ui.button("Preview").clicked() {
                                        if let Err(err) =
                                            self.controller.preview_sample_by_id(&sample.sample_id)
                                        {
                                            self.controller.set_status(
                                                format!("Preview failed: {err}"),
                                                style::StatusTone::Error,
                                            );
                                        }
                                    }
                                });
                                ui.end_row();
                            }
                        });
                });

                ui.add_space(ui.spacing().item_spacing.y);
                ui.horizontal(|ui| {
                    if ui.button("Refresh sample set").clicked() {
                        self.refresh_tf_label_calibration(label.label_id.clone(), label.name.clone());
                        close = true;
                    }
                    let can_apply = state.suggested_threshold.is_some();
                    if ui
                        .add_enabled(can_apply, egui::Button::new("Apply suggested values"))
                        .clicked()
                    {
                        let next_threshold = state.suggested_threshold.unwrap_or(label.threshold);
                        let next_gap = state.suggested_gap.unwrap_or(label.gap);
                        match self.controller.update_tf_label(
                            &label.label_id,
                            &label.name,
                            next_threshold,
                            next_gap,
                            label.topk,
                        ) {
                            Ok(()) => {
                                self.controller.clear_tf_label_score_cache();
                                self.controller.set_status(
                                    format!(
                                        "Saved threshold {:.3}, gap {:.3}",
                                        next_threshold, next_gap
                                    ),
                                    style::StatusTone::Info,
                                );
                            }
                            Err(err) => {
                                self.controller.set_status(
                                    format!("Save failed: {err}"),
                                    style::StatusTone::Error,
                                );
                            }
                        }
                    }
                    if ui.button("Close").clicked() {
                        close = true;
                    }
                });
            });

        if close {
            return;
        }
        self.controller.ui.tf_labels.calibration = Some(state);
    }

    fn refresh_tf_label_calibration(&mut self, label_id: String, label_name: String) {
        match self
            .controller
            .tf_label_candidate_matches_for_label(&label_id, CALIBRATION_CANDIDATE_K, CALIBRATION_TOP_K)
        {
            Ok(matches) => {
                let samples = matches
                    .into_iter()
                    .map(|entry| TfLabelCalibrationSample {
                        sample_id: entry.sample_id,
                        score: entry.score,
                        bucket: entry.bucket,
                    })
                    .collect();
                self.controller.ui.tf_labels.calibration = Some(TfLabelCalibrationState {
                    label_id,
                    label_name,
                    samples,
                    decisions: HashMap::new(),
                    suggested_threshold: None,
                    suggested_gap: None,
                });
            }
            Err(err) => {
                self.controller.set_status(
                    format!("Calibration load failed: {err}"),
                    style::StatusTone::Error,
                );
            }
        }
    }

    fn find_tf_label_by_id(
        &mut self,
        label_id: &str,
    ) -> Result<Option<crate::egui_app::controller::TfLabel>, String> {
        let labels = self.controller.list_tf_labels()?;
        Ok(labels.into_iter().find(|label| label.label_id == label_id))
    }
}

fn suggest_thresholds(
    samples: &[TfLabelCalibrationSample],
    decisions: &HashMap<String, bool>,
    current_threshold: f32,
    _current_gap: f32,
) -> (Option<f32>, Option<f32>) {
    let mut positives = Vec::new();
    let mut negatives = Vec::new();
    for sample in samples {
        match decisions.get(&sample.sample_id) {
            Some(true) => positives.push(sample.score),
            Some(false) => negatives.push(sample.score),
            None => {}
        }
    }
    if positives.is_empty() {
        return (None, None);
    }
    let min_pos = positives
        .iter()
        .copied()
        .fold(f32::INFINITY, f32::min);
    let max_neg = negatives.iter().copied().fold(f32::NEG_INFINITY, f32::max);

    let threshold = if negatives.is_empty() {
        (min_pos - THRESHOLD_MARGIN).max(0.0)
    } else {
        ((min_pos + max_neg) * 0.5).clamp(0.0, 1.0)
    };
    let threshold = threshold.clamp(0.0, 1.0);

    let gap = if negatives.is_empty() {
        None
    } else {
        Some((min_pos - max_neg).max(0.0).min(2.0))
    };

    let threshold = if threshold.is_finite() {
        Some(threshold)
    } else {
        Some(current_threshold)
    };
    (threshold, gap)
}

fn count_votes(decisions: &HashMap<String, bool>) -> (usize, usize) {
    let mut up = 0usize;
    let mut down = 0usize;
    for value in decisions.values() {
        if *value {
            up += 1;
        } else {
            down += 1;
        }
    }
    (up, down)
}

fn sample_label_from_id(sample_id: &str) -> String {
    if let Some((_, rel)) = sample_id.split_once("::") {
        let path = std::path::Path::new(rel);
        return view_model::sample_display_label(path);
    }
    sample_id.to_string()
}
