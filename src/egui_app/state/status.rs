use crate::egui_app::ui::style;
use egui::Color32;

/// Status badge + text shown in the footer.
#[derive(Clone, Debug, PartialEq)]
pub struct StatusBarState {
    pub text: String,
    pub badge_label: String,
    pub badge_color: Color32,
}

impl StatusBarState {
    pub fn idle() -> Self {
        Self {
            text: "Add a sample source to get started".into(),
            badge_label: "Idle".into(),
            badge_color: style::status_badge_color(style::StatusTone::Idle),
        }
    }
}
