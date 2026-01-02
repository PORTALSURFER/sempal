use super::EguiApp;
use super::helpers::{RowBackground, clamp_label_for_width, list_row_height, render_list_row};
use super::style;
use crate::egui_app::state::{DragPayload, DragSource, DragTarget, FocusContext};
use crate::egui_app::ui::drag_targets::{handle_drop_zone, pointer_pos_for_drag};
use eframe::egui::{self, RichText, Ui};

impl EguiApp {
    pub(super) fn render_sources_list(&mut self, ui: &mut Ui, height: f32) -> egui::Rect {
        let drag_payload = self.controller.ui.drag.payload.clone();
        let drag_active =
            matches!(drag_payload, Some(DragPayload::Sample { .. } | DragPayload::Samples { .. }));
        let pointer_pos = pointer_pos_for_drag(ui, self.controller.ui.drag.position);
        let output = egui::ScrollArea::vertical()
            .id_salt("sources_scroll")
            .max_height(height)
            .show(ui, |ui| {
                let rows = self.controller.ui.sources.rows.clone();
                let selected = self.controller.ui.sources.selected;
                let row_height = list_row_height(ui);
                for (index, row) in rows.iter().enumerate() {
                    let is_selected = Some(index) == selected;
                    ui.push_id(&row.id, |ui| {
                        let row_width = ui.available_width();
                        let padding = ui.spacing().button_padding.x * 2.0;
                        let base_label = clamp_label_for_width(&row.name, row_width - padding);
                        let label = if row.missing {
                            format!("! {base_label}")
                        } else {
                            base_label
                        };
                        let text_color = if row.missing {
                            style::missing_text()
                        } else {
                            style::high_contrast_text()
                        };
                        let bg = RowBackground::from_option(
                            is_selected.then_some(style::row_selected_fill()),
                        );
                        let response = render_list_row(
                            ui,
                            super::helpers::ListRow {
                                label: &label,
                                row_width,
                                row_height,
                                background: bg,
                                skip_hover: false,
                                text_color,
                                sense: egui::Sense::click(),
                                number: None,
                                marker: None,
                            },
                        )
                        .on_hover_text(&row.path);
                        if response.clicked() {
                            self.controller.select_source_by_index(index);
                            self.controller
                                .focus_context_from_ui(FocusContext::SourcesList);
                        }
                        handle_drop_zone(
                            ui,
                            &mut self.controller,
                            drag_active,
                            pointer_pos,
                            response.rect,
                            DragSource::Sources,
                            DragTarget::SourcesRow(row.id.clone()),
                            style::drag_target_stroke(),
                            egui::StrokeKind::Inside,
                        );
                        self.source_row_menu(&response, index, row);
                    });
                }
            });
        let min_focus_height = list_row_height(ui);
        let focus_height = output
            .content_size
            .y
            .max(min_focus_height)
            .min(output.inner_rect.height());
        let focus_rect = egui::Rect::from_min_size(
            output.inner_rect.min,
            egui::vec2(output.inner_rect.width(), focus_height),
        );
        focus_rect
    }

    fn source_row_menu(
        &mut self,
        response: &egui::Response,
        index: usize,
        row: &crate::egui_app::state::SourceRowView,
    ) {
        response.context_menu(|ui| {
            let palette = style::palette();
            ui.label(RichText::new(row.name.clone()).color(palette.text_primary));
            let mut close_menu = false;
            if ui.button("Quick sync").clicked() {
                self.controller.select_source_by_index(index);
                self.controller.request_quick_sync();
                close_menu = true;
            }
            if ui
                .button("Hard sync (full rescan)")
                .on_hover_text("Prune missing rows and rebuild from disk")
                .clicked()
            {
                self.controller.select_source_by_index(index);
                self.controller.request_hard_sync();
                close_menu = true;
            }
            if ui
                .button("Remove dead links")
                .on_hover_text("Remove missing rows from the library")
                .clicked()
            {
                self.controller.remove_dead_links_for_source(index);
                close_menu = true;
            }
            if ui
                .button("Prepare similarity search")
                .on_hover_text("Scan, embed, build ANN, t-SNE, and cluster for this source")
                .clicked()
            {
                self.controller.select_source_by_index(index);
                self.controller.prepare_similarity_for_selected_source();
                close_menu = true;
            }
            ui.separator();
            ui.label(
                RichText::new("Similarity prep")
                    .color(style::palette().text_muted),
            );
            let mut cap_enabled = self.controller.similarity_prep_duration_cap_enabled();
            if ui
                .checkbox(&mut cap_enabled, "Limit analysis duration")
                .on_hover_text("Skip long files during similarity prep to speed up analysis")
                .changed()
            {
                self.controller
                    .set_similarity_prep_duration_cap_enabled(cap_enabled);
            }
            ui.add_enabled_ui(cap_enabled, |ui| {
                let mut seconds = self.controller.max_analysis_duration_seconds();
                let drag = egui::DragValue::new(&mut seconds)
                    .speed(1.0)
                    .range(1.0..=3600.0)
                    .suffix(" s");
                let response = ui.add(drag).on_hover_text(
                    "Maximum file length to analyze during similarity preparation",
                );
                if response.changed() {
                    self.controller.set_max_analysis_duration_seconds(seconds);
                }
            });
            let mut fast_prep = self.controller.similarity_prep_fast_mode_enabled();
            if ui
                .checkbox(&mut fast_prep, "Fast similarity prep")
                .on_hover_text(
                    "Downsample audio during prep for faster analysis; refine lazily later",
                )
                .changed()
            {
                self.controller
                    .set_similarity_prep_fast_mode_enabled(fast_prep);
            }
            ui.add_enabled_ui(fast_prep, |ui| {
                let mut sample_rate = self.controller.similarity_prep_fast_sample_rate();
                let drag = egui::DragValue::new(&mut sample_rate)
                    .speed(500.0)
                    .range(8_000..=16_000)
                    .suffix(" Hz");
                let response = ui.add(drag).on_hover_text(
                    "Sample rate used for fast similarity prep analysis",
                );
                if response.changed() {
                    self.controller.set_similarity_prep_fast_sample_rate(sample_rate);
                }
            });
            ui.add_enabled_ui(!self.controller.similarity_prep_in_progress(), |ui| {
                let mut force_full = self
                    .controller
                    .similarity_prep_force_full_analysis_next();
                if ui
                    .checkbox(&mut force_full, "Force full reanalysis (next run)")
                    .on_hover_text(
                        "Ignore cached features and embeddings on the next similarity prep run",
                    )
                    .changed()
                {
                    self.controller
                        .set_similarity_prep_force_full_analysis_next(force_full);
                }
            });
            ui.separator();
            if ui.button("Open in file explorer").clicked() {
                self.controller.select_source_by_index(index);
                self.controller.open_source_folder(index);
                close_menu = true;
            }
            if ui.button("Remap source…").clicked() {
                self.controller.select_source_by_index(index);
                self.controller.remap_source_via_dialog(index);
                close_menu = true;
            }
            let remove_btn = egui::Button::new(
                RichText::new("Remove source")
                    .color(style::destructive_text())
                    .strong(),
            );
            if ui.add(remove_btn).clicked() {
                self.controller.remove_source(index);
                close_menu = true;
            }
            if close_menu {
                ui.close();
            }
        });
    }
}
