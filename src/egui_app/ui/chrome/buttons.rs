use eframe::egui::{self, RichText};

use crate::egui_app::ui::style;

pub(crate) fn action_button(label: &str) -> egui::Button<'_> {
    egui::Button::new(RichText::new(label).color(style::palette().text_primary))
}

pub(crate) fn destructive_button(label: &str) -> egui::Button<'_> {
    egui::Button::new(RichText::new(label).color(style::destructive_text()))
}
