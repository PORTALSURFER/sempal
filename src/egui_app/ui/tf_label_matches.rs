use super::style;
use super::*;
use crate::egui_app::controller::TfLabel;
use eframe::egui::{self, RichText};

impl EguiApp {
    pub(super) fn render_tf_label_matches(&mut self, ui: &mut egui::Ui, label: &TfLabel) {
        let palette = style::palette();
        if self
            .controller
            .ui
            .tf_labels
            .last_candidate_label_id
            .as_deref()
            != Some(&label.label_id)
        {
            return;
        }
        let candidates = self.controller.ui.tf_labels.last_candidate_results.clone();
        if candidates.is_empty() {
            ui.label(RichText::new("No matches returned.").color(palette.text_muted));
            return;
        }
        ui.add_space(ui.spacing().item_spacing.y);
        ui.label(RichText::new("Top matches").color(palette.text_primary));
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
                                self.controller
                                    .set_status("Anchor added".to_string(), style::StatusTone::Info);
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
                    candidate.bucket == crate::analysis::anchor_scoring::ConfidenceBucket::High
                })
                .count();
            if ui
                .add_enabled(high_count > 0, egui::Button::new("Auto-tag high confidence"))
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
