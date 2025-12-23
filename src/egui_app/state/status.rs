use crate::egui_app::ui::style;
use egui::Color32;

/// Status badge + text shown in the footer.
#[derive(Clone, Debug, PartialEq)]
pub struct StatusBarState {
    pub text: String,
    pub badge_label: String,
    pub badge_color: Color32,
    pub log: Vec<String>,
}

impl StatusBarState {
    /// Default status shown when no source is selected.
    pub fn idle() -> Self {
        Self {
            text: "Add a sample source to get started".into(),
            badge_label: "Idle".into(),
            badge_color: style::status_badge_color(style::StatusTone::Idle),
            log: Vec::new(),
        }
    }

    pub fn log_text(&self) -> String {
        if self.log.is_empty() {
            return String::new();
        }
        self.log.join("\n")
    }
}
