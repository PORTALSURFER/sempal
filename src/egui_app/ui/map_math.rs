use eframe::egui;

pub(crate) fn world_bounds_from_view(
    rect: egui::Rect,
    center: egui::Pos2,
    scale: f32,
    pan: egui::Vec2,
) -> crate::egui_app::state::MapQueryBounds {
    let to_world = |pos: egui::Pos2| {
        let dx = (pos.x - rect.center().x - pan.x) / scale;
        let dy = (pos.y - rect.center().y - pan.y) / scale;
        (center.x + dx, center.y + dy)
    };
    let (min_x, min_y) = to_world(rect.min);
    let (max_x, max_y) = to_world(rect.max);
    crate::egui_app::state::MapQueryBounds {
        min_x: min_x.min(max_x),
        max_x: min_x.max(max_x),
        min_y: min_y.min(max_y),
        max_y: min_y.max(max_y),
    }
}

pub(crate) fn should_requery(
    last: &Option<crate::egui_app::state::MapQueryBounds>,
    next: &crate::egui_app::state::MapQueryBounds,
) -> bool {
    match last {
        None => true,
        Some(prev) => {
            let dx = (prev.min_x - next.min_x).abs()
                + (prev.max_x - next.max_x).abs();
            let dy = (prev.min_y - next.min_y).abs()
                + (prev.max_y - next.max_y).abs();
            dx + dy > 0.05
        }
    }
}
