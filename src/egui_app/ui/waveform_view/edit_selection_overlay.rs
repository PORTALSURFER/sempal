use super::selection_geometry::selection_rect_for_view;
use super::selection_menu;
use super::style;
use super::*;
use crate::egui_app::state::WaveformView;
use eframe::egui::{self, StrokeKind};

pub(super) fn render_edit_selection_overlay(
    app: &mut EguiApp,
    ui: &mut egui::Ui,
    rect: egui::Rect,
    palette: &style::Palette,
    view: WaveformView,
    view_width: f32,
) {
    let Some(selection) = app.controller.ui.waveform.edit_selection else {
        return;
    };

    let selection_rect = selection_rect_for_view(selection, rect, view, view_width);
    let fill = style::with_alpha(palette.warning, 50);
    let stroke = egui::Stroke::new(1.5, style::with_alpha(palette.warning, 180));
    let painter = ui.painter();
    painter.rect_filled(selection_rect, 0.0, fill);
    painter.rect_stroke(selection_rect, 0.0, stroke, StrokeKind::Inside);

    let selection_menu = ui.interact(
        selection_rect,
        ui.id().with("edit_selection_context_menu"),
        egui::Sense::click(),
    );
    selection_menu.context_menu(|ui| {
        selection_menu::render_selection_context_menu(app, ui);
    });
}
