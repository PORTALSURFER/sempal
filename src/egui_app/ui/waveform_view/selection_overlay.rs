use super::selection_drag;
use super::selection_geometry::{
    paint_selection_edge_bracket, selection_edge_handle_rect, selection_handle_rect,
};
use super::selection_menu;
use super::style;
use super::*;
use crate::egui_app::state::WaveformView;
use crate::selection::{SelectionEdge, SelectionRange};
use eframe::egui::{self, Color32, CursorIcon, TextStyle, text::LayoutJob};

const LOOP_BAR_HEIGHT: f32 = 6.0;
const LOOP_BAR_HANDLE_WIDTH: f32 = 8.0;
const LOOP_BAR_MIN_DURATION_SECS: f32 = 0.1;

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
    let selection = app.controller.ui.waveform.selection;
    let loop_range = selection.unwrap_or_else(|| SelectionRange::new(0.0, 1.0));
    let loop_bar_rect = loop_bar_rect(rect, view, view_width, loop_range);
    let loop_dragging = handle_loop_bar_drag(
        app,
        ui,
        rect,
        view,
        view_width,
        loop_range,
        loop_bar_rect,
    );

    let Some(selection) = selection else {
        return loop_dragging;
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

    edge_dragging || loop_dragging
}

fn loop_bar_rect(
    rect: egui::Rect,
    view: WaveformView,
    view_width: f32,
    range: SelectionRange,
) -> egui::Rect {
    let clamped_start = range.start().clamp(0.0, 1.0);
    let clamped_end = range.end().clamp(clamped_start, 1.0);
    let start_norm = ((clamped_start - view.start) / view_width).clamp(0.0, 1.0);
    let end_norm = ((clamped_end - view.start) / view_width).clamp(0.0, 1.0);
    let width = (end_norm - start_norm).max(0.0) * rect.width();
    egui::Rect::from_min_size(
        egui::pos2(rect.left() + rect.width() * start_norm, rect.top()),
        egui::vec2(width.max(2.0), LOOP_BAR_HEIGHT),
    )
}

fn handle_loop_bar_drag(
    app: &mut EguiApp,
    ui: &mut egui::Ui,
    rect: egui::Rect,
    view: WaveformView,
    view_width: f32,
    range: SelectionRange,
    bar_rect: egui::Rect,
) -> bool {
    let handle_height = bar_rect.height().max(LOOP_BAR_HEIGHT);
    let left_handle = egui::Rect::from_min_size(
        egui::pos2(bar_rect.left() - LOOP_BAR_HANDLE_WIDTH * 0.5, bar_rect.top()),
        egui::vec2(LOOP_BAR_HANDLE_WIDTH, handle_height),
    );
    let right_handle = egui::Rect::from_min_size(
        egui::pos2(bar_rect.right() - LOOP_BAR_HANDLE_WIDTH * 0.5, bar_rect.top()),
        egui::vec2(LOOP_BAR_HANDLE_WIDTH, handle_height),
    );
    let left_response = ui.interact(
        left_handle,
        ui.id().with("loop_bar_edge_start"),
        egui::Sense::click_and_drag(),
    );
    let right_response = ui.interact(
        right_handle,
        ui.id().with("loop_bar_edge_end"),
        egui::Sense::click_and_drag(),
    );
    let dragging = left_response.dragged()
        || left_response.drag_started()
        || right_response.dragged()
        || right_response.drag_started()
        || left_response.is_pointer_button_down_on()
        || right_response.is_pointer_button_down_on();
    if dragging {
        ui.output_mut(|o| o.cursor_icon = CursorIcon::ResizeHorizontal);
    }

    let Some(duration_seconds) = app.controller.loaded_audio_duration_seconds() else {
        return dragging;
    };
    if duration_seconds <= 0.0 {
        return dragging;
    }
    let min_width = (LOOP_BAR_MIN_DURATION_SECS / duration_seconds).clamp(0.0, 1.0);
    let normalize = |pos: egui::Pos2| {
        ((pos.x - rect.left()) / rect.width())
            .mul_add(view_width, view.start)
            .clamp(0.0, 1.0)
    };

    if let Some(pos) = left_response.interact_pointer_pos()
        && (left_response.dragged() || left_response.drag_started())
    {
        let new_start = normalize(pos).min(range.end() - min_width).clamp(0.0, 1.0);
        app.controller
            .set_selection_range(SelectionRange::new(new_start, range.end()));
        return true;
    }
    if let Some(pos) = right_response.interact_pointer_pos()
        && (right_response.dragged() || right_response.drag_started())
    {
        let new_end = normalize(pos).max(range.start() + min_width).clamp(0.0, 1.0);
        app.controller
            .set_selection_range(SelectionRange::new(range.start(), new_end));
        return true;
    }

    dragging
}
