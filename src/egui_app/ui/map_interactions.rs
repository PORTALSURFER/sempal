use super::map_render;
use eframe::egui;

pub(crate) fn find_hover_point(
    points: &[crate::egui_app::state::MapPoint],
    rect: egui::Rect,
    center: egui::Pos2,
    scale: f32,
    pan: egui::Vec2,
    pointer: Option<egui::Pos2>,
) -> Option<(crate::egui_app::state::MapPoint, egui::Pos2)> {
    let pointer = pointer?;
    if !rect.contains(pointer) {
        return None;
    }
    let mut best: Option<(crate::egui_app::state::MapPoint, egui::Pos2, f32)> = None;
    for point in points {
        let pos = map_render::map_to_screen(point.x, point.y, rect, center, scale, pan);
        let dist_sq = pos.distance_sq(pointer);
        if dist_sq > 36.0 {
            continue;
        }
        match best {
            Some((_, _, best_sq)) if dist_sq >= best_sq => {}
            _ => {
                best = Some((point.clone(), pos, dist_sq));
            }
        }
    }
    best.map(|(point, pos, _)| (point, pos))
}
