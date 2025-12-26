use super::selection_drag;
use super::selection_geometry::{
    paint_selection_edge_bracket, selection_edge_handle_rect, selection_handle_rect,
};
use super::selection_menu;
use super::style;
use super::*;
use crate::egui_app::state::WaveformView;
use crate::selection::SelectionEdge;
use eframe::egui::{self, Color32, CursorIcon, TextStyle, text::LayoutJob};

pub(super) fn render_selection_overlay(
    app: &mut EguiApp,
    ui: &mut egui::Ui,
    rect: egui::Rect,
    palette: &style::Palette,
    view: WaveformView,
    view_width: f32,
    highlight: Color32,
    pointer_pos: Option<egui::Pos2>,
) -> bool {
    let Some(selection) = app.controller.ui.waveform.selection else {
        return false;
    };

    let start_norm = ((selection.start() - view.start) / view_width).clamp(0.0, 1.0);
    let end_norm = ((selection.end() - view.start) / view_width).clamp(0.0, 1.0);
    let width = rect.width() * (end_norm - start_norm).max(0.0);
    let x = rect.left() + rect.width() * start_norm;
    let selection_rect =
        egui::Rect::from_min_size(egui::pos2(x, rect.top()), egui::vec2(width, rect.height()));

    let handle_rect = selection_handle_rect(selection_rect);
    let handle_response = ui.interact(
        handle_rect,
        ui.id().with("selection_handle"),
        egui::Sense::drag(),
    );
    let handle_hovered = handle_response.hovered() || handle_response.dragged();
    let handle_color = if handle_hovered {
        style::with_alpha(highlight, 235)
    } else {
        style::with_alpha(highlight, 205)
    };
    {
        let painter = ui.painter();
        painter.rect_filled(selection_rect, 0.0, style::with_alpha(highlight, 60));
        painter.rect_filled(handle_rect, 0.0, handle_color);
    }
    selection_drag::handle_selection_handle_drag(
        app,
        ui,
        rect,
        view,
        view_width,
        selection,
        &handle_response,
    );

    if let Some(duration_label) = app.controller.ui.waveform.selection_duration.as_deref() {
        let painter = ui.painter();
        let text_color = style::with_alpha(palette.bg_secondary, 240);
        let bar_color = style::with_alpha(highlight, 80);
        let galley = ui.ctx().fonts_mut(|f| {
            f.layout_job(LayoutJob::simple_singleline(
                duration_label.to_string(),
                TextStyle::Small.resolve(ui.style()),
                text_color,
            ))
        });
        let padding = egui::vec2(8.0, 2.0);
        let bar_height = galley.size().y + padding.y * 2.0;
        let bar_rect = egui::Rect::from_min_size(
            egui::pos2(selection_rect.left(), selection_rect.bottom() - bar_height),
            egui::vec2(selection_rect.width(), bar_height),
        );
        painter.rect_filled(bar_rect, 0.0, bar_color);
        let text_pos = egui::pos2(
            (bar_rect.right() - padding.x - galley.size().x).max(bar_rect.left() + padding.x),
            bar_rect.top() + (bar_height - galley.size().y) * 0.5,
        );
        painter.galley(text_pos, galley, text_color);
    }

    draw_bpm_guides(app, ui, rect, selection, view, view_width, highlight);

    let start_edge_rect = selection_edge_handle_rect(selection_rect, SelectionEdge::Start);
    let end_edge_rect = selection_edge_handle_rect(selection_rect, SelectionEdge::End);
    let start_edge_response = ui.interact(
        start_edge_rect,
        ui.id().with("selection_edge_start"),
        egui::Sense::click_and_drag(),
    );
    let end_edge_response = ui.interact(
        end_edge_rect,
        ui.id().with("selection_edge_end"),
        egui::Sense::click_and_drag(),
    );
    let start_edge_pointer_down = start_edge_response.is_pointer_button_down_on();
    let end_edge_pointer_down = end_edge_response.is_pointer_button_down_on();
    let edge_dragging = start_edge_pointer_down
        || end_edge_pointer_down
        || start_edge_response.dragged()
        || start_edge_response.drag_started()
        || end_edge_response.dragged()
        || end_edge_response.drag_started();
    for (edge, edge_rect, edge_response) in [
        (SelectionEdge::Start, start_edge_rect, start_edge_response),
        (SelectionEdge::End, end_edge_rect, end_edge_response),
    ] {
        let edge_pos = match edge {
            SelectionEdge::Start => selection_rect.left(),
            SelectionEdge::End => selection_rect.right(),
        };
        selection_drag::handle_selection_edge_drag(
            app,
            rect,
            view,
            view_width,
            edge,
            &edge_response,
            edge_pos,
        );
        let edge_hovered = pointer_pos.is_some_and(|p| edge_rect.contains(p))
            || edge_response.hovered()
            || edge_response.is_pointer_button_down_on()
            || edge_response.dragged();
        if edge_hovered {
            let color = highlight;
            paint_selection_edge_bracket(ui.painter(), edge_rect, edge, color);
            ui.output_mut(|o| o.cursor_icon = CursorIcon::ResizeHorizontal);
        }
    }
    selection_drag::sync_selection_edge_drag_release(app, ui.ctx());
    selection_menu::attach_selection_context_menu(app, ui, selection_rect);

    edge_dragging
}

fn draw_bpm_guides(
    app: &mut EguiApp,
    ui: &mut egui::Ui,
    rect: egui::Rect,
    selection: crate::selection::SelectionRange,
    view: WaveformView,
    view_width: f32,
    highlight: Color32,
) {
    if !app.controller.ui.waveform.bpm_snap_enabled {
        return;
    }
    let bpm = app.controller.ui.waveform.bpm_value.unwrap_or(0.0);
    if !bpm.is_finite() || bpm <= 0.0 {
        return;
    }
    let duration = app.controller.loaded_audio_duration_seconds().unwrap_or(0.0);
    if !duration.is_finite() || duration <= 0.0 {
        return;
    }
    let step = 60.0 / bpm / duration;
    if !step.is_finite() || step <= 0.0 {
        return;
    }
    let painter = ui.painter();
    let stroke = egui::Stroke::new(1.0, style::with_alpha(highlight, 140));
    let triage_red = style::with_alpha(style::semantic_palette().triage_trash, 200);
    let triage_stroke = egui::Stroke::new(1.0, triage_red);
    let mut beat = selection.start() + step;
    let end = selection.end();
    let mut beat_index = 1usize;
    while beat < end {
        let normalized = ((beat - view.start) / view_width).clamp(0.0, 1.0);
        let x = rect.left() + rect.width() * normalized;
        let line_stroke = if beat_index % 4 == 0 {
            triage_stroke
        } else {
            stroke
        };
        painter.line_segment(
            [egui::pos2(x, rect.top()), egui::pos2(x, rect.bottom())],
            line_stroke,
        );
        beat += step;
        beat_index += 1;
    }
}
