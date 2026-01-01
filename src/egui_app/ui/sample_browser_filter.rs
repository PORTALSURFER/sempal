use super::style;
use super::*;
use crate::egui_app::state::{SampleBrowserSort, TriageFlagFilter};
use eframe::egui::{self, RichText, Ui};

impl EguiApp {
    pub(super) fn render_sample_browser_filter(&mut self, ui: &mut Ui) {
        let palette = style::palette();
        let visible_count = self.controller.visible_browser_len();
        ui.horizontal(|ui| {
            ui.label(RichText::new("Filter").color(palette.text_primary));
            for filter in [
                TriageFlagFilter::All,
                TriageFlagFilter::Keep,
                TriageFlagFilter::Trash,
                TriageFlagFilter::Untagged,
            ] {
                let selected = self.controller.ui.browser.filter == filter;
                let label = match filter {
                    TriageFlagFilter::All => "All",
                    TriageFlagFilter::Keep => "Keep",
                    TriageFlagFilter::Trash => "Trash",
                    TriageFlagFilter::Untagged => "Untagged",
                };
                if ui.selectable_label(selected, label).clicked() {
                    self.controller.set_browser_filter(filter);
                }
            }
            ui.add_space(ui.spacing().item_spacing.x);
            let mut query = self.controller.ui.browser.search_query.clone();
            let response = ui.add(
                egui::TextEdit::singleline(&mut query)
                    .hint_text("Search samples (f)...")
                    .desired_width(160.0),
            );
            if self.controller.ui.browser.search_focus_requested {
                response.request_focus();
                self.controller.ui.browser.search_focus_requested = false;
            }
            if response.changed() {
                self.controller.set_browser_search(query);
            }

            ui.add_space(ui.spacing().item_spacing.x);
            let selected_row = self.controller.ui.browser.selected_visible;
            let find_similar_btn = egui::Button::new("Find similar")
                .selected(self.controller.ui.browser.similar_query.is_some());
            let find_similar_resp = ui.add_enabled(selected_row.is_some(), find_similar_btn);
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
            ui.label(RichText::new("Sort").color(palette.text_primary));
            let sort_mode = self.controller.ui.browser.sort;
            if ui
                .selectable_label(sort_mode == SampleBrowserSort::ListOrder, "List")
                .clicked()
            {
                self.controller.set_browser_sort(SampleBrowserSort::ListOrder);
            }
            let similarity_available = self.controller.ui.browser.similar_query.is_some();
            let similarity_button = egui::SelectableLabel::new(
                sort_mode == SampleBrowserSort::Similarity,
                "Similarity",
            );
            let similarity_response = ui.add_enabled(similarity_available, similarity_button);
            if similarity_response.clicked() {
                self.controller.set_browser_sort(SampleBrowserSort::Similarity);
            }
            if !similarity_available {
                similarity_response.on_disabled_hover_text("Find similar to enable");
            }
            ui.add_space(ui.spacing().item_spacing.x);
            let random_mode_enabled = self.controller.random_navigation_mode_enabled();
            let dice_label = RichText::new("🎲").color(if random_mode_enabled {
                style::destructive_text()
            } else {
                palette.text_muted
            });
            let dice_button = egui::Button::new(dice_label).selected(random_mode_enabled);
            let dice_response = ui.add(dice_button).on_hover_text(
                "Play a random visible sample (click)\nToggle sticky random navigation (Shift+click)",
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
