use super::drag_targets;
use super::flat_items_list::{FlatItemsListConfig, render_flat_items_list};
use super::helpers::{
    NumberColumn, RowBackground, RowMarker, clamp_label_for_width, loop_badge_space,
    render_list_row,
};
use super::status_badges;
use super::style;
use super::*;
use crate::egui_app::state::{
    DragSource, FocusContext, SampleBrowserActionPrompt, SampleBrowserTab,
};
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
        let now_epoch = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;
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
        let total_rows = self.controller.visible_browser_len();
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
                let entry_index = match self.controller.visible_browser_index(row) {
                    Some(index) => index,
                    None => return,
                };
                let (tag, path, looped, missing, last_played_at) = match self.controller.wav_entry(entry_index) {
                    Some(entry) => (
                        entry.tag,
                        entry.relative_path.clone(),
                        entry.looped,
                        entry.missing,
                        entry.last_played_at,
                    ),
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
                let similar_query = self.controller.ui.browser.similar_query.as_ref();
                let is_anchor = similar_query.and_then(|sim| sim.anchor_index) == Some(entry_index);
                let similar_strength =
                    similar_query.and_then(|sim| sim.display_strength_for_index(entry_index));
                let marker_color = style::triage_marker_color(tag);
                let triage_marker_width =
                    marker_color.as_ref().map(|_| style::triage_marker_width());
                let triage_marker = marker_color.map(|color| RowMarker {
                    width: style::triage_marker_width(),
                    color,
                });
                let needs_similarity_data = self
                    .controller
                    .cached_feature_status_for_entry(entry_index)
                    .is_some_and(|status| !status.has_embedding);
                let indicator_radius = if needs_similarity_data {
                    style::similarity_missing_dot_radius()
                } else {
                    0.0
                };
                let indicator_space = if needs_similarity_data {
                    indicator_radius * 2.0 + metrics.padding * 0.5
                } else {
                    0.0
                };
                let loop_space = if looped { loop_badge_space(ui) } else { 0.0 };
                let trailing_space = indicator_space
                    + triage_marker_width
                        .map(|width| width + metrics.padding * 0.5)
                        .unwrap_or(0.0)
                    + loop_space;

                let mut base_label = self
                    .controller
                    .wav_label(entry_index)
                    .unwrap_or_else(|| view_model::sample_display_label(&path));
                if is_loaded {
                    base_label.push_str(" • loaded");
                }
                let analysis_failure = self
                    .controller
                    .analysis_failure_for_entry(entry_index)
                    .map(str::to_string);
                let base_color = style::playback_age_label_color(last_played_at, now_epoch);
                let status_label = status_badges::apply_sample_status(
                    base_label,
                    base_color,
                    missing,
                    analysis_failure.as_deref(),
                );
                let display_label = status_label.label.clone();

                let row_label_width = row_width
                    - metrics.padding
                    - metrics.number_width
                    - metrics.number_gap
                    - trailing_space;
                let row_label = if rename_match {
                    String::new()
                } else {
                    clamp_label_for_width(&status_label.label, row_label_width)
                };
                let mut row_bg = None;
                if drag_active
                    && pointer_pos
                        .as_ref()
                        .is_some_and(|pos| ui.cursor().contains(*pos))
                    && is_selected
                {
                    row_bg = Some(style::duplicate_hover_fill());
                } else if is_focused {
                    row_bg = Some(style::row_selected_fill());
                } else if is_selected {
                    row_bg = Some(style::row_multi_selected_fill());
                }
                let skip_hover = is_anchor;
                if is_anchor {
                    row_bg = Some(style::similar_anchor_fill());
                }
                let row_background = if let Some(strength) = similar_strength.filter(|_| !is_anchor)
                {
                    RowBackground::Gradient {
                        left: style::similar_score_fill(strength),
                        right: row_bg.unwrap_or_else(style::compartment_fill),
                    }
                } else {
                    RowBackground::from_option(row_bg)
                };
                let number_text = format!("{}", row + 1);
                let text_color = status_label.text_color;

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
                            background: row_background,
                            skip_hover,
                            text_color,
                            sense,
                            number: Some(NumberColumn {
                                text: &number_text,
                                width: metrics.number_width,
                                color: palette.text_muted,
                            }),
                            marker: triage_marker,
                            rating: Some(tag),
                            looped,
                        },
                    );
                    let response = if let Some(hover) = status_label.hover_text.as_deref() {
                        response.on_hover_text(hover)
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
                    if needs_similarity_data {
                        let dot_center = egui::pos2(
                            response.rect.right()
                                - triage_marker_width.unwrap_or(0.0)
                                - metrics.padding * 0.5
                                - indicator_radius,
                            response.rect.center().y,
                        );
                        ui.painter().circle_filled(
                            dot_center,
                            indicator_radius,
                            style::similarity_missing_dot_fill(),
                        );
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

                    let row_drag_source = self
                        .controller
                        .ui
                        .drag
                        .origin_source
                        .unwrap_or(DragSource::Browser);
                    self.handle_sample_row_drag(
                        ui,
                        &response,
                        drag_active,
                        crate::egui_app::state::DragTarget::BrowserTriage(drop_target),
                        row_drag_source,
                        &path,
                    );
                });
            },
        );

        if autoscroll_to.is_some() {
            self.controller.ui.browser.autoscroll = false;
        }

        let drag_source = self
            .controller
            .ui
            .drag
            .origin_source
            .unwrap_or(DragSource::Browser);
        drag_targets::handle_drop_zone(
            ui,
            &mut self.controller,
            drag_active,
            pointer_pos,
            list_response.frame_rect,
            drag_source,
            crate::egui_app::state::DragTarget::BrowserTriage(drop_target),
            style::drag_target_stroke(),
            StrokeKind::Inside,
        );
    }
}
