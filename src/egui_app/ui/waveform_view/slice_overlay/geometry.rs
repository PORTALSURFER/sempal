use crate::selection::{SelectionEdge, SelectionRange};
use eframe::egui;

use super::SliceOverlayEnv;

pub(super) fn slice_rect(env: &SliceOverlayEnv<'_>, slice: SelectionRange) -> Option<egui::Rect> {
    let start_norm = ((slice.start() as f64 - env.view.start) / env.view_width).clamp(0.0, 1.0);
    let end_norm = ((slice.end() as f64 - env.view.start) / env.view_width).clamp(0.0, 1.0);
    let width = env.rect.width() * (end_norm - start_norm).max(0.0) as f32;
    if width <= 0.0 {
        return None;
    }
    let x = env.rect.left() + env.rect.width() * start_norm as f32;
    Some(egui::Rect::from_min_size(
        egui::pos2(x, env.rect.top()),
        egui::vec2(width, env.rect.height()),
    ))
}

pub(super) fn update_slice_edge(
    range: SelectionRange,
    edge: SelectionEdge,
    position: f32,
) -> SelectionRange {
    let min_width = crate::egui_app::controller::MIN_SELECTION_WIDTH;
    match edge {
        SelectionEdge::Start => {
            let max_start = (range.end() - min_width).max(0.0);
            SelectionRange::new(position.min(max_start), range.end())
        }
        SelectionEdge::End => {
            let min_end = (range.start() + min_width).min(1.0);
            SelectionRange::new(range.start(), position.max(min_end))
        }
    }
}

pub(super) fn edge_position_px(edge: SelectionEdge, slice_rect: egui::Rect) -> f32 {
    match edge {
        SelectionEdge::Start => slice_rect.left(),
        SelectionEdge::End => slice_rect.right(),
    }
}

pub(super) fn to_wave_pos(env: &SliceOverlayEnv<'_>, pos: egui::Pos2) -> f32 {
    let normalized = ((pos.x - env.rect.left()) / env.rect.width()).clamp(0.0, 1.0) as f64;
    normalized
        .mul_add(env.view_width, env.view.start)
        .clamp(0.0, 1.0) as f32
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::egui_app::state::WaveformView;

    fn test_env<'a>(palette: &'a crate::egui_app::ui::style::Palette) -> SliceOverlayEnv<'a> {
        SliceOverlayEnv {
            rect: egui::Rect::from_min_size(egui::pos2(10.0, 20.0), egui::vec2(200.0, 50.0)),
            view: WaveformView {
                start: 0.25,
                end: 0.75,
            },
            view_width: 0.5,
            pointer_pos: None,
            palette,
            slice_color: egui::Color32::WHITE,
        }
    }

    #[test]
    fn slice_rect_clamps_to_visible_range() {
        let palette = crate::egui_app::ui::style::palette();
        let env = test_env(&palette);
        let range = SelectionRange::new(0.1, 0.9);
        let rect = slice_rect(&env, range).expect("slice should be visible");
        assert!((rect.left() - env.rect.left()).abs() < f32::EPSILON);
        assert!((rect.width() - env.rect.width()).abs() < f32::EPSILON);
    }

    #[test]
    fn slice_rect_returns_none_when_outside_view() {
        let palette = crate::egui_app::ui::style::palette();
        let env = test_env(&palette);
        let range = SelectionRange::new(0.0, 0.1);
        assert!(slice_rect(&env, range).is_none());
    }

    #[test]
    fn update_slice_edge_clamps_start_and_end() {
        let range = SelectionRange::new(0.2, 0.4);
        let start = update_slice_edge(range, SelectionEdge::Start, 0.39);
        assert!((start.end() - 0.4).abs() < f32::EPSILON);
        assert!(
            start.start() <= 0.4 - crate::egui_app::controller::MIN_SELECTION_WIDTH + f32::EPSILON
        );
        let end = update_slice_edge(range, SelectionEdge::End, 0.21);
        assert!((end.start() - 0.2).abs() < f32::EPSILON);
        assert!(end.end() >= 0.2 + crate::egui_app::controller::MIN_SELECTION_WIDTH - f32::EPSILON);
    }
}
