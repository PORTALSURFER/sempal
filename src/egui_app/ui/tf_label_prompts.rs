use super::style;
use super::*;
use crate::egui_app::state::{TfAutoTagPrompt, TfLabelCreatePrompt};
use eframe::egui::{self, RichText};

impl EguiApp {
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

    pub(super) fn render_tf_label_create_prompt(&mut self, ctx: &egui::Context) {
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
                                self.controller.clear_tf_label_score_cache();
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

    pub(super) fn render_tf_label_auto_tag_prompt(&mut self, ctx: &egui::Context) {
        let Some(prompt) = self.controller.ui.tf_labels.auto_tag_prompt.clone() else {
            return;
        };
        let palette = style::palette();
        let candidates = self.controller.ui.tf_labels.last_candidate_results.clone();
        let high_matches: Vec<_> = candidates
            .iter()
            .filter(|candidate| {
                candidate.bucket == crate::analysis::anchor_scoring::ConfidenceBucket::High
            })
            .collect();
        let mut close = false;
        egui::Window::new("Auto-tag high confidence")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.label(
                    RichText::new(format!("Label: {}", prompt.label_name))
                        .color(palette.text_primary)
                        .strong(),
                );
                ui.label(
                    RichText::new(format!(
                        "Add {} anchors from high-confidence matches?",
                        high_matches.len()
                    ))
                    .color(palette.text_primary),
                );
                ui.add_space(ui.spacing().item_spacing.y);
                ui.horizontal(|ui| {
                    if ui.button("Confirm").clicked() {
                        let mut added = 0usize;
                        for candidate in &high_matches {
                            if self
                                .controller
                                .add_tf_anchor(&prompt.label_id, &candidate.sample_id, 1.0)
                                .is_ok()
                            {
                                added += 1;
                            }
                        }
                        self.controller.set_status(
                            format!("Added {added} anchors"),
                            style::StatusTone::Info,
                        );
                        self.controller.clear_tf_label_score_cache();
                        close = true;
                    }
                    if ui.button("Cancel").clicked() {
                        close = true;
                    }
                });
            });
        if close {
            self.controller.ui.tf_labels.auto_tag_prompt = None;
        }
    }
}
