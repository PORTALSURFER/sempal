use super::style;
use eframe::egui;

const MISSING_PREFIX: &str = "! ";
const FAILED_SUFFIX: &str = " • FAILED";

pub(super) struct StatusBadgeLabel {
    pub label: String,
    pub text_color: egui::Color32,
    pub hover_text: Option<String>,
}

pub(super) fn missing_prefix() -> &'static str {
    MISSING_PREFIX
}

pub(super) fn missing_text_color() -> egui::Color32 {
    style::missing_text()
}

pub(super) fn apply_sample_status(
    base_label: impl Into<String>,
    base_color: egui::Color32,
    missing: bool,
    analysis_failure: Option<&str>,
) -> StatusBadgeLabel {
    let mut label = base_label.into();
    let mut text_color = base_color;
    let mut hover_text = None;
    if let Some(reason) = analysis_failure {
        label.push_str(FAILED_SUFFIX);
        text_color = style::destructive_text();
        let reason = reason.lines().next().unwrap_or(reason);
        hover_text = Some(format!("Analysis failed: {reason}"));
    }
    if missing {
        label.insert_str(0, MISSING_PREFIX);
        text_color = style::missing_text();
    }
    StatusBadgeLabel {
        label,
        text_color,
        hover_text,
    }
}
