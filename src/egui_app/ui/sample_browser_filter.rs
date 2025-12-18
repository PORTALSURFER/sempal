use super::style;
use super::*;
use crate::egui_app::state::TriageFlagFilter;
use eframe::egui::{self, RichText, Ui};

impl EguiApp {
    pub(super) fn render_sample_browser_filter(&mut self, ui: &mut Ui) {
        let palette = style::palette();
        let visible_count = self.controller.visible_browser_indices().len();
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
            let categories = self.controller.prediction_categories();
            if !categories.is_empty() {
                let mut selected = self.controller.ui.browser.category_filter.clone();
                egui::ComboBox::from_id_salt("category_filter")
                    .selected_text(
                        selected
                            .as_deref()
                            .map(|s| format!("Category: {s}"))
                            .unwrap_or_else(|| "Category: Any".to_string()),
                    )
                    .show_ui(ui, |ui| {
                        if ui.selectable_label(selected.is_none(), "Any").clicked() {
                            selected = None;
                        }
                        for category in &categories {
                            let is_selected = selected.as_deref() == Some(category.as_str());
                            if ui.selectable_label(is_selected, category).clicked() {
                                selected = Some(category.clone());
                            }
                        }
                    });
                if selected != self.controller.ui.browser.category_filter {
                    self.controller.set_category_filter(selected);
                }

                ui.add_space(ui.spacing().item_spacing.x);
                let mut threshold = self.controller.ui.browser.confidence_threshold;
                let slider = egui::Slider::new(&mut threshold, 0.0..=1.0)
                    .show_value(false)
                    .text("Conf");
                let response = ui.add(slider).on_hover_text("Minimum prediction confidence");
                if response.changed() {
                    self.controller.set_confidence_threshold(threshold);
                }
            }

            ui.add_space(ui.spacing().item_spacing.x);
            let random_mode_enabled = self.controller.random_navigation_mode_enabled();
            let dice_label = RichText::new("🎲").color(if random_mode_enabled {
                palette.text_primary
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
