use super::drag_targets;
use super::flat_items_list::{FlatItemsListConfig, render_flat_items_list};
use super::helpers::{NumberColumn, RowMarker, clamp_label_for_width, render_list_row};
use super::style;
use super::*;
use crate::egui_app::state::{FocusContext, SampleBrowserActionPrompt, SampleBrowserTab};
use crate::egui_app::view_model;
use eframe::egui::{self, StrokeKind, Ui};

impl EguiApp {
    pub(super) fn render_sample_browser(&mut self, ui: &mut Ui) {
        let palette = style::palette();
        self.controller.prepare_feature_cache_for_browser();
        let selected_row = self.controller.ui.browser.selected_visible;
        let loaded_row = self.controller.ui.browser.loaded_visible;
        let drop_target = self.controller.triage_flag_drop_target();
        let mut tab = self.controller.ui.browser.active_tab;
        ui.horizontal(|ui| {
            if ui
                .selectable_label(tab == SampleBrowserTab::List, "Samples")
                .clicked()
            {
                tab = SampleBrowserTab::List;
            }
            if ui
                .selectable_label(tab == SampleBrowserTab::Map, "Similarity map")
                .clicked()
            {
                tab = SampleBrowserTab::Map;
            }
        });
        if tab != self.controller.ui.browser.active_tab {
            self.controller.ui.browser.active_tab = tab;
        }
        ui.add_space(4.0);
        if self.controller.ui.browser.active_tab == SampleBrowserTab::Map {
            self.render_map_panel(ui);
            return;
        }
        self.render_sample_browser_filter(ui);
        ui.add_space(6.0);

        let list_height = ui.available_height().max(0.0);
        let drag_active = self.controller.ui.drag.payload.is_some();
        let pointer_pos = drag_targets::pointer_pos_for_drag(ui, self.controller.ui.drag.position);
        let autoscroll_enabled = self.controller.ui.browser.autoscroll
            && self.controller.ui.collections.selected_sample.is_none();
        let total_rows = self.controller.visible_browser_indices().len();
        let focused_section = matches!(
            self.controller.ui.focus.context,
            FocusContext::SampleBrowser
        );
        let autoscroll_to = selected_row.filter(|_| autoscroll_enabled);

        let list_response = render_flat_items_list(
            ui,
            FlatItemsListConfig {
                scroll_id_salt: "sample_browser_scroll",
                min_height: list_height,
                total_rows,
                focused_section,
                autoscroll_to,
                autoscroll_padding_rows: 1.0,
            },
            |ui, row, metrics| {
                let entry_index = {
                    let indices = self.controller.visible_browser_indices();
                    match indices.get(row) {
                        Some(index) => *index,
                        None => return,
                    }
                };
                let (tag, path, missing) = match self.controller.wav_entry(entry_index) {
                    Some(entry) => (entry.tag, entry.relative_path.clone(), entry.missing),
                    None => return,
                };
                let rename_match = matches!(
                    self.controller.ui.browser.pending_action,
                    Some(SampleBrowserActionPrompt::Rename { ref target, .. })
                        if target == &path
                );
                let is_focused = selected_row == Some(row);
                let is_selected = self
                    .controller
                    .ui
                    .browser
                    .selected_paths
                    .iter()
                    .any(|p| p == &path);
                let is_loaded = loaded_row == Some(row);
                let row_width = metrics.row_width;
                let triage_marker = style::triage_marker_color(tag).map(|color| RowMarker {
                    width: style::triage_marker_width(),
                    color,
                });
                let trailing_space = triage_marker
                    .as_ref()
                    .map(|marker| marker.width + metrics.padding * 0.5)
                    .unwrap_or(0.0);

                let mut label = self
                    .controller
                    .wav_label(entry_index)
                    .unwrap_or_else(|| view_model::sample_display_label(&path));
                let analysis_failure = self
                    .controller
                    .analysis_failure_for_entry(entry_index)
                    .map(str::to_string);
                if analysis_failure.is_some() {
                    label.push_str(" • FAILED");
                }
                if is_loaded {
                    label.push_str(" • loaded");
                }
                if missing {
                    label = format!("! {label}");
                }
                let display_label = label.clone();

                let row_label_width = row_width
                    - metrics.padding
                    - metrics.number_width
                    - metrics.number_gap
                    - trailing_space;
                let row_label = if rename_match {
                    String::new()
                } else {
                    clamp_label_for_width(&label, row_label_width)
                };
                let bg = if drag_active
                    && pointer_pos
                        .as_ref()
                        .is_some_and(|pos| ui.cursor().contains(*pos))
                    && is_selected
                {
                    Some(style::duplicate_hover_fill())
                } else if is_focused {
                    Some(style::row_selected_fill())
                } else if is_selected {
                    Some(style::row_multi_selected_fill())
                } else {
                    None
                };
                let number_text = format!("{}", row + 1);
                let text_color = if missing {
                    style::missing_text()
                } else if analysis_failure.is_some() {
                    style::destructive_text()
                } else {
                    style::triage_label_color(tag)
                };

                ui.push_id(&path, |ui| {
                    let sense = if rename_match {
                        egui::Sense::hover()
                    } else {
                        egui::Sense::click_and_drag()
                    };
                    let response = render_list_row(
                        ui,
                        super::helpers::ListRow {
                            label: &row_label,
                            row_width,
                            row_height: metrics.row_height,
                            bg,
                            text_color,
                            sense,
                            number: Some(NumberColumn {
                                text: &number_text,
                                width: metrics.number_width,
                                color: palette.text_muted,
                            }),
                            marker: triage_marker,
                        },
                    );
                    let response = if let Some(reason) = analysis_failure.as_deref() {
                        let reason = reason.lines().next().unwrap_or(reason);
                        response.on_hover_text(format!("Analysis failed: {reason}"))
                    } else {
                        response
                    };

                    if is_selected {
                        let marker_width = 4.0;
                        let marker_rect = egui::Rect::from_min_max(
                            response.rect.left_top(),
                            response.rect.left_top() + egui::vec2(marker_width, metrics.row_height),
                        );
                        ui.painter()
                            .rect_filled(marker_rect, 0.0, style::selection_marker_fill());
                    }
                    self.handle_browser_row_click(ui, &response, row);
                    if is_focused {
                        ui.painter().rect_stroke(
                            response.rect,
                            0.0,
                            style::focused_row_stroke(),
                            StrokeKind::Inside,
                        );
                    }

                    if rename_match {
                        self.render_browser_rename_editor(
                            ui,
                            &response,
                            metrics.padding,
                            metrics.number_width,
                            metrics.number_gap,
                            trailing_space,
                        );
                    } else {
                        self.browser_sample_menu(&response, row, &path, &display_label, missing);
                    }

                    self.handle_sample_row_drag(
                        ui,
                        &response,
                        drag_active,
                        crate::egui_app::state::DragTarget::BrowserTriage(drop_target),
                        &path,
                    );
                });
            },
        );

        if autoscroll_to.is_some() {
            self.controller.ui.browser.autoscroll = false;
        }

        if drag_active
            && let Some(pointer) = pointer_pos
            && list_response.frame_rect.contains(pointer)
        {
            let shift_down = ui.input(|i| i.modifiers.shift);
            self.controller.update_active_drag(
                pointer,
                crate::egui_app::state::DragSource::Browser,
                crate::egui_app::state::DragTarget::BrowserTriage(drop_target),
                shift_down,
            );
            ui.painter().rect_stroke(
                list_response.frame_rect,
                0.0,
                style::drag_target_stroke(),
                StrokeKind::Inside,
            );
        }
    }
}

fn lerp_color(a: egui::Color32, b: egui::Color32, t: f32) -> egui::Color32 {
    let t = t.clamp(0.0, 1.0);
    let lerp = |start: u8, end: u8| -> u8 {
        let start = start as f32;
        let end = end as f32;
        (start + (end - start) * t).round().clamp(0.0, 255.0) as u8
    };
    egui::Color32::from_rgb(lerp(a.r(), b.r()), lerp(a.g(), b.g()), lerp(a.b(), b.b()))
}
