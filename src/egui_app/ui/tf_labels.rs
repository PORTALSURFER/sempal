use super::style;
use super::*;
use crate::egui_app::state::TfLabelCreatePrompt;
use eframe::egui::{self, RichText};

const DEFAULT_TF_THRESHOLD: f32 = 0.75;
const DEFAULT_TF_GAP: f32 = 0.1;
const DEFAULT_TF_TOPK: i64 = 3;
const DEFAULT_TF_WEIGHT: f32 = 1.0;

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
        self.controller.ui.tf_labels.create_prompt = Some(TfLabelCreatePrompt {
            name,
            threshold: DEFAULT_TF_THRESHOLD,
            gap: DEFAULT_TF_GAP,
            topk: DEFAULT_TF_TOPK,
            anchor_sample_id,
        });
    }

    fn render_tf_label_create_prompt(&mut self, ctx: &egui::Context) {
        let Some(prompt) = self.controller.ui.tf_labels.create_prompt.as_mut() else {
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
                                        .add_tf_anchor(&label.label_id, sample_id, DEFAULT_TF_WEIGHT)
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
            self.controller.ui.tf_labels.create_prompt = None;
        }
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
}
