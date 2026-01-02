use crate::egui_app::state::WaveformView;
use crate::selection::{SelectionEdge, SelectionRange};
use eframe::egui::{self, Color32, Stroke};

pub(super) fn normalized_range_in_view(
    start: f32,
    end: f32,
    view: WaveformView,
    view_width: f32,
) -> (f32, f32) {
    let width = view_width.max(1.0e-6);
    let start_norm = ((start - view.start) / width).clamp(0.0, 1.0);
    let end_norm = ((end - view.start) / width).clamp(start_norm, 1.0);
    (start_norm, end_norm)
}

pub(super) fn selection_rect_for_view(
    selection: SelectionRange,
    rect: egui::Rect,
    view: WaveformView,
    view_width: f32,
) -> egui::Rect {
    let (start_norm, end_norm) =
        normalized_range_in_view(selection.start(), selection.end(), view, view_width);
    let width = rect.width() * (end_norm - start_norm).max(0.0);
    let x = rect.left() + rect.width() * start_norm;
    egui::Rect::from_min_size(egui::pos2(x, rect.top()), egui::vec2(width, rect.height()))
}

pub(super) fn loop_bar_rect(
    rect: egui::Rect,
    view: WaveformView,
    view_width: f32,
    selection: Option<SelectionRange>,
    bar_height: f32,
) -> egui::Rect {
    let (loop_start, loop_end) = selection
        .map(|range| (range.start(), range.end()))
        .unwrap_or((0.0, 1.0));
    let clamped_start = loop_start.clamp(0.0, 1.0);
    let clamped_end = loop_end.clamp(clamped_start, 1.0);
    let (start_norm, end_norm) =
        normalized_range_in_view(clamped_start, clamped_end, view, view_width);
    let width = (end_norm - start_norm).max(0.0) * rect.width();
    egui::Rect::from_min_size(
        egui::pos2(rect.left() + rect.width() * start_norm, rect.top()),
        egui::vec2(width.max(2.0), bar_height),
    )
}

pub(super) fn selection_handle_height(selection_rect: egui::Rect) -> f32 {
    (selection_rect.height() / 7.0).max(8.0)
}

pub(super) fn selection_handle_rect(selection_rect: egui::Rect) -> egui::Rect {
    let handle_height = selection_handle_height(selection_rect);
    egui::Rect::from_min_size(
        egui::pos2(
            selection_rect.left(),
            selection_rect.bottom() - handle_height,
        ),
        egui::vec2(selection_rect.width(), handle_height),
    )
}

const EDGE_HANDLE_WIDTH: f32 = 18.0;
const EDGE_ICON_HEIGHT_FRACTION: f32 = 0.8;
const EDGE_ICON_MIN_SIZE: f32 = 12.0;
const EDGE_BRACKET_STROKE: f32 = 1.5;

pub(super) fn selection_edge_handle_rect(
    selection_rect: egui::Rect,
    edge: SelectionEdge,
) -> egui::Rect {
    let width = EDGE_HANDLE_WIDTH;
    let handle_height = selection_handle_height(selection_rect);
    let height = (selection_rect.height() - handle_height).max(0.0);
    let x = match edge {
        SelectionEdge::Start => selection_rect.left() - width * 0.5,
        SelectionEdge::End => selection_rect.right() - width * 0.5,
    };
    egui::Rect::from_min_size(
        egui::pos2(x, selection_rect.top()),
        egui::vec2(width, height),
    )
}

pub(super) fn paint_selection_edge_bracket(
    painter: &egui::Painter,
    edge_rect: egui::Rect,
    _edge: SelectionEdge,
    color: Color32,
) {
    let height = (edge_rect.height() * EDGE_ICON_HEIGHT_FRACTION)
        .clamp(EDGE_ICON_MIN_SIZE, edge_rect.height());
    let half_height = height * 0.5;
    let center = edge_rect.center();
    let top = center.y - half_height;
    let bottom = center.y + half_height;
    let stroke = Stroke::new(EDGE_BRACKET_STROKE, color);
    painter.line_segment(
        [egui::pos2(center.x, top), egui::pos2(center.x, bottom)],
        stroke,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edge_handles_do_not_overlap_drag_handle() {
        let selection_rect =
            egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(50.0, 12.0));
        let handle_rect = selection_handle_rect(selection_rect);
        let start_edge_rect = selection_edge_handle_rect(selection_rect, SelectionEdge::Start);
        let end_edge_rect = selection_edge_handle_rect(selection_rect, SelectionEdge::End);

        assert!(start_edge_rect.bottom() <= handle_rect.top());
        assert!(end_edge_rect.bottom() <= handle_rect.top());
    }

    #[test]
    fn selection_rect_clamps_to_visible_view() {
        let rect = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(100.0, 20.0));
        let view = WaveformView {
            start: 0.25,
            end: 0.75,
        };
        let selection = SelectionRange::new(0.0, 1.0);
        let selection_rect = selection_rect_for_view(selection, rect, view, view.width());
        assert!((selection_rect.left() - rect.left()).abs() < 1.0e-6);
        assert!((selection_rect.width() - rect.width()).abs() < 1.0e-6);
    }

    #[test]
    fn selection_rect_clamps_to_zero_width_when_offscreen() {
        let rect = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(120.0, 12.0));
        let view = WaveformView { start: 0.5, end: 1.0 };
        let selection = SelectionRange::new(0.0, 0.1);
        let selection_rect = selection_rect_for_view(selection, rect, view, view.width());
        assert!((selection_rect.width() - 0.0).abs() < 1.0e-6);
    }

    #[test]
    fn loop_bar_rect_clamps_to_view_bounds() {
        let rect = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(80.0, 12.0));
        let view = WaveformView { start: 0.5, end: 1.0 };
        let selection = SelectionRange::new(0.0, 0.25);
        let bar_rect = loop_bar_rect(rect, view, view.width(), Some(selection), 12.0);
        assert!((bar_rect.left() - rect.left()).abs() < 1.0e-6);
    }
}
