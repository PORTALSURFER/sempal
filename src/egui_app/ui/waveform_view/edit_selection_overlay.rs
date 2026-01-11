use super::selection_geometry::selection_rect_for_view;
use super::style;
use super::*;
use crate::egui_app::state::WaveformView;
use eframe::egui::{self, StrokeKind};

pub(super) fn render_edit_selection_overlay(
    app: &mut EguiApp,
    ui: &mut egui::Ui,
    rect: egui::Rect,
    view: WaveformView,
    view_width: f64,
) {
    let Some(selection) = app.controller.ui.waveform.edit_selection else {
        return;
    };

    let selection_rect = selection_rect_for_view(selection, rect, view, view_width);
    let highlight = egui::Color32::from_rgb(76, 122, 218);
    let fill = style::with_alpha(highlight, 50);
    let stroke = egui::Stroke::new(1.5, style::with_alpha(highlight, 180));
    let painter = ui.painter();
    painter.rect_filled(selection_rect, 0.0, fill);
    painter.rect_stroke(selection_rect, 0.0, stroke, StrokeKind::Inside);

}
