use super::style;
use super::*;
use crate::egui_app::state::TfLabelScoreCache;
use crate::egui_app::view_model;
use eframe::egui::{self, RichText};

impl EguiApp {
    pub(super) fn render_tf_label_match_panel(&mut self, ui: &mut egui::Ui) {
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
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(format!("Scores for {}", sample_label)).color(palette.text_primary),
            );
            if ui.button("Refresh scores").clicked() {
                self.controller.clear_tf_label_score_cache();
            }
        });
        let mode = self.controller.ui.tf_labels.aggregation_mode;
        let cache_hit = self.controller.ui.tf_labels.last_score_sample_id.as_deref() == Some(&sample_id)
            && self.controller.ui.tf_labels.last_score_mode == mode;
        let matches: Vec<TfLabelScoreCache> = if cache_hit {
            self.controller.ui.tf_labels.last_scores.clone()
        } else {
            let matches = match self.controller.tf_label_matches_for_sample(&sample_id, mode) {
                Ok(matches) => matches,
                Err(err) => {
                    self.controller.set_status(
                        format!("Score labels failed: {err}"),
                        style::StatusTone::Error,
                    );
                    Vec::new()
                }
            };
            let cache: Vec<TfLabelScoreCache> = matches
                .into_iter()
                .map(|entry| TfLabelScoreCache {
                    label_id: entry.label_id,
                    name: entry.name,
                    score: entry.score,
                    bucket: entry.bucket,
                    gap: entry.gap,
                    anchor_count: entry.anchor_count,
                })
                .collect();
            self.controller.ui.tf_labels.last_score_sample_id = Some(sample_id.clone());
            self.controller.ui.tf_labels.last_score_mode = mode;
            self.controller.ui.tf_labels.last_scores = cache.clone();
            cache
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
