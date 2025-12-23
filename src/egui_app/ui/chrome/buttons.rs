use eframe::egui::{self, RichText};

use crate::egui_app::ui::style;

pub(super) fn action_button(label: &str) -> egui::Button {
    egui::Button::new(RichText::new(label).color(style::palette().text_primary))
}

pub(super) fn destructive_button(label: &str) -> egui::Button {
    egui::Button::new(RichText::new(label).color(style::destructive_text()))
}
