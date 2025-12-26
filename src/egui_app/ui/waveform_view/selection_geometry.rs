use crate::selection::SelectionEdge;
use eframe::egui::{self, Color32, Stroke};

fn selection_handle_height(selection_rect: egui::Rect) -> f32 {
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
}
