use super::style;
use super::*;
use eframe::egui::{self, Color32};

mod markers;
mod playhead;
mod selection;

pub(super) const LOOP_BAR_HEIGHT: f32 = 12.0;

pub(super) fn render_overlays(
    app: &mut EguiApp,
    ui: &mut egui::Ui,
    rect: egui::Rect,
    view: crate::egui_app::state::WaveformView,
    view_width: f32,
    highlight: Color32,
    start_marker_color: Color32,
    to_screen_x: &impl Fn(f32, egui::Rect) -> f32,
) {
    markers::render_markers(app, ui, rect, view, start_marker_color, to_screen_x);
    selection::render_loop_bar(app, ui, rect, view, view_width, highlight);
    playhead::render_playhead(app, ui, rect, view, view_width, highlight, to_screen_x);
}
