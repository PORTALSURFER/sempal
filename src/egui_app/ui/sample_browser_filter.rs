use super::style;
use super::*;
use crate::egui_app::controller::hotkeys;
use crate::egui_app::state::{SampleBrowserSort, TriageFlagFilter};
use eframe::egui::{self, RichText, Ui};

impl EguiApp {
    pub(super) fn render_sample_browser_filter(&mut self, ui: &mut Ui) {
        let palette = style::palette();
        let semantic = style::semantic_palette();
        let tooltip_mode = self.controller.ui.controls.tooltip_mode;
        let visible_count = self.controller.visible_browser_len();
        ui.horizontal(|ui| {
            let rating_filter_active = !self.controller.ui.browser.rating_filter.is_empty();
            let clear_color = if rating_filter_active {
                palette.text_primary
            } else {
                palette.text_muted
            };
            let clear_icon = RichText::new("⦸").color(clear_color);
            let clear_response = ui.add(egui::Button::new(clear_icon));
            let clear_response = helpers::tooltip(
                clear_response,
                "Clear filters",
                "Clear rating filters.",
                tooltip_mode,
            );
            if clear_response.clicked() {
                let needs_clear = rating_filter_active
                    || self.controller.ui.browser.filter != TriageFlagFilter::All;
                if needs_clear {
                    self.controller.ui.browser.rating_filter.clear();
                    self.controller.ui.browser.filter = TriageFlagFilter::All;
                    self.controller.rebuild_browser_lists();
                }
            }
            ui.add_space(ui.spacing().item_spacing.x * 0.6);
            let square_size = 12.0;
            let square_rounding = 1.5;
            let square_gap = ui.spacing().item_spacing.x * 0.4;
            let levels = [-3, -2, -1, 0, 1, 2, 3];
            for (idx, level) in levels.iter().enumerate() {
                let selected = self.controller.ui.browser.rating_filter.contains(level);
                let (base_color, level_strength) = match level {
                    -3..=-1 => (
                        semantic.triage_trash,
                        (*level).abs() as f32 / 3.0,
                    ),
                    0 => (palette.text_primary, 0.0),
                    1..=3 => (
                        semantic.triage_keep,
                        (*level).abs() as f32 / 3.0,
                    ),
                    _ => (palette.text_primary, 0.0),
                };
                let neutral_alpha = if selected { 220 } else { 90 };
                let level_alpha = if selected {
                    220
                } else {
                    (70.0 + (120.0 * level_strength)).round() as u8
                };
                let fill = if *level == 0 {
                    style::with_alpha(base_color, neutral_alpha)
                } else {
                    style::with_alpha(base_color, level_alpha)
                };
                let stroke = if selected {
                    egui::Stroke::new(1.0, palette.text_primary)
                } else {
                    egui::Stroke::new(1.0, palette.panel_outline)
                };
                let (rect, response) = ui.allocate_exact_size(
                    egui::vec2(square_size, square_size),
                    egui::Sense::click(),
                );
                let response = response.on_hover_text("Rating filter");
                ui.painter().rect_filled(rect, square_rounding, fill);
                ui.painter()
                    .rect_stroke(rect, square_rounding, stroke, egui::StrokeKind::Inside);
                if response.clicked() {
                    let modifiers = ui.input(|i| i.modifiers);
                    let additive = modifiers.command || modifiers.ctrl;
                    self.controller.set_browser_rating_filter(*level, additive);
                }
                if idx + 1 < levels.len() {
                    ui.add_space(square_gap);
                }
            }
            ui.add_space(ui.spacing().item_spacing.x);
            let mut query = self.controller.ui.browser.search_query.clone();
            let search_hint = format!(
                "Search samples ({})...",
                hotkeys::format_keypress(&hotkeys::KeyPress::with_command(egui::Key::F))
            );
            let response = ui.add(
                egui::TextEdit::singleline(&mut query)
                    .hint_text(search_hint)
                    .desired_width(160.0),
            );
            if self.controller.ui.browser.search_focus_requested {
                response.request_focus();
                self.controller.ui.browser.search_focus_requested = false;
            }
            if response.changed() {
                self.controller.set_browser_search(query);
            }
            if self.controller.ui.browser.search_busy {
                ui.add(egui::Spinner::new().size(16.0));
            }

            ui.add_space(ui.spacing().item_spacing.x);
            let selected_row = self.controller.ui.browser.selected_visible;
            let find_similar_btn = egui::Button::new("Find similar")
                .selected(self.controller.ui.browser.similar_query.is_some());
            let find_similar_resp = ui.add_enabled(selected_row.is_some(), find_similar_btn);
            let find_similar_resp = helpers::tooltip(
                find_similar_resp,
                "Find similar",
                "Search the entire library for samples that sound similar to the currently selected item. This uses advanced neural embeddings to find matches based on timbre, rhythm, and character.",
                tooltip_mode,
            );
            if find_similar_resp.clicked()
                && let Some(row) = selected_row
            {
                if let Err(err) = self.controller.find_similar_for_visible_row(row) {
                    self.controller
                        .set_status(format!("Find similar failed: {err}"), style::StatusTone::Error);
                }
            }
            ui.add_space(ui.spacing().item_spacing.x);
            if let Some(similar) = self.controller.ui.browser.similar_query.as_ref() {
                ui.label(
                    RichText::new(format!("Similar to {}", similar.label))
                        .color(palette.text_muted),
                );
                if ui.button("Clear similar").clicked() {
                    self.controller.clear_similar_filter();
                }
                ui.add_space(ui.spacing().item_spacing.x);
            }
            ui.add_space(ui.spacing().item_spacing.x);
            let loaded_available = self.controller.ui.loaded_wav.is_some();
            let mut similarity_sort = self.controller.ui.browser.similarity_sort_follow_loaded;
            let similarity_response = ui.add_enabled(
                loaded_available,
                egui::Checkbox::new(&mut similarity_sort, "Similarity sort"),
            );
            let similarity_response = if loaded_available {
                similarity_response
            } else {
                similarity_response.on_disabled_hover_text("Load a sample to enable similarity sort")
            };
            if similarity_response.changed() {
                if similarity_sort {
                    if let Err(err) = self.controller.enable_loaded_similarity_sort() {
                        self.controller
                            .set_status(err, style::StatusTone::Error);
                    }
                } else {
                    self.controller.disable_similarity_sort();
                }
            }
            ui.add_space(ui.spacing().item_spacing.x);
            ui.label(RichText::new("Sort").color(palette.text_primary));
            let current_sort = self.controller.ui.browser.sort;
            let sort_label = match current_sort {
                SampleBrowserSort::ListOrder => "List order",
                SampleBrowserSort::Similarity => "Similarity",
                SampleBrowserSort::PlaybackAgeAsc => "Playback age (oldest)",
                SampleBrowserSort::PlaybackAgeDesc => "Playback age (recent)",
            };
            let mut sort = current_sort;
            egui::ComboBox::from_id_salt("browser_sort")
                .selected_text(sort_label)
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut sort, SampleBrowserSort::ListOrder, "List order");
                    ui.selectable_value(
                        &mut sort,
                        SampleBrowserSort::PlaybackAgeAsc,
                        "Playback age (oldest)",
                    );
                    ui.selectable_value(
                        &mut sort,
                        SampleBrowserSort::PlaybackAgeDesc,
                        "Playback age (recent)",
                    );
                });
            if sort != current_sort {
                self.controller.set_browser_sort(sort);
            }
            ui.add_space(ui.spacing().item_spacing.x);
            let random_mode_enabled = self.controller.random_navigation_mode_enabled();
            let dice_label = RichText::new("🎲").color(if random_mode_enabled {
                style::destructive_text()
            } else {
                palette.text_muted
            });
            let dice_button = egui::Button::new(dice_label).selected(random_mode_enabled);
            let dice_response = helpers::tooltip(
                ui.add(dice_button),
                "Random Navigation",
                "Click to immediately play a random sample from the visible list.\n\nShift+Click to toggle Sticky Random mode, where 'Next' and 'Previous' hotkeys (Arrow Up/Down) will jump to random samples instead of moving sequentially.",
                tooltip_mode,
            );
            if dice_response.clicked() {
                let modifiers = ui.input(|i| i.modifiers);
                if modifiers.shift {
                    self.controller.toggle_random_navigation_mode();
                } else {
                    self.controller.play_random_visible_sample();
                }
            }

            let count_label = format!(
                "{} item{}",
                visible_count,
                if visible_count == 1 { "" } else { "s" }
            );
            ui.allocate_ui_with_layout(
                egui::vec2(ui.available_width(), 0.0),
                egui::Layout::right_to_left(egui::Align::Center),
                |ui| {
                    ui.label(RichText::new(count_label).color(palette.text_muted).small());
                },
            );
        });
    }
}
