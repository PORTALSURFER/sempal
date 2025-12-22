use eframe::egui;

pub(crate) fn map_to_screen(
    x: f32,
    y: f32,
    rect: egui::Rect,
    center: egui::Pos2,
    scale: f32,
    pan: egui::Vec2,
) -> egui::Pos2 {
    let dx = (x - center.x) * scale;
    let dy = (y - center.y) * scale;
    egui::pos2(rect.center().x + dx + pan.x, rect.center().y + dy + pan.y)
}

pub(crate) fn render_heatmap(
    painter: &egui::Painter,
    rect: egui::Rect,
    points: &[crate::egui_app::state::MapPoint],
    center: egui::Pos2,
    scale: f32,
    pan: egui::Vec2,
    bins: usize,
) {
    let mut counts = vec![0u32; bins * bins];
    let width = rect.width().max(1.0);
    let height = rect.height().max(1.0);
    for point in points {
        let pos = map_to_screen(point.x, point.y, rect, center, scale, pan);
        if !rect.contains(pos) {
            continue;
        }
        let nx = ((pos.x - rect.min.x) / width).clamp(0.0, 0.999);
        let ny = ((pos.y - rect.min.y) / height).clamp(0.0, 0.999);
        let ix = (nx * bins as f32) as usize;
        let iy = (ny * bins as f32) as usize;
        let idx = iy * bins + ix;
        if let Some(cell) = counts.get_mut(idx) {
            *cell = cell.saturating_add(1);
        }
    }
    let max_count = counts.iter().copied().max().unwrap_or(1).max(1) as f32;
    for iy in 0..bins {
        for ix in 0..bins {
            let idx = iy * bins + ix;
            let count = counts[idx] as f32;
            if count <= 0.0 {
                continue;
            }
            let intensity = (count / max_count).clamp(0.0, 1.0);
            let alpha = (intensity * 200.0) as u8;
            let color = egui::Color32::from_rgba_premultiplied(80, 180, 255, alpha);
            let cell_w = rect.width() / bins as f32;
            let cell_h = rect.height() / bins as f32;
            let min = egui::pos2(
                rect.min.x + ix as f32 * cell_w,
                rect.min.y + iy as f32 * cell_h,
            );
            let max = egui::pos2(min.x + cell_w, min.y + cell_h);
            painter.rect_filled(egui::Rect::from_min_max(min, max), 0.0, color);
        }
    }
}
