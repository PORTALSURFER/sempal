use crate::egui_app::ui::style;
use egui::Color32;

/// Status badge + text shown in the footer.
#[derive(Clone, Debug, PartialEq)]
pub struct StatusBarState {
    /// Main status message text.
    pub text: String,
    /// Badge label shown next to the status.
    pub badge_label: String,
    /// Badge color.
    pub badge_color: Color32,
    /// Rolling status log entries.
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

    /// Concatenate log entries into a single displayable string.
    pub fn log_text(&self) -> String {
        if self.log.is_empty() {
            return String::new();
        }
        self.log.join("\n")
    }
}
