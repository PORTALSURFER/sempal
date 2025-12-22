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
) -> usize {
    render_heatmap_with_color(
        painter,
        rect,
        points,
        center,
        scale,
        pan,
        bins,
        |_point| egui::Color32::from_rgba_premultiplied(80, 180, 255, 255),
    )
}

pub(crate) fn render_heatmap_with_color<F>(
    painter: &egui::Painter,
    rect: egui::Rect,
    points: &[crate::egui_app::state::MapPoint],
    center: egui::Pos2,
    scale: f32,
    pan: egui::Vec2,
    bins: usize,
    color_for_point: F,
) -> usize
where
    F: Fn(&crate::egui_app::state::MapPoint) -> egui::Color32,
{
    let mut counts = vec![0u32; bins * bins];
    let mut r_sums = vec![0f32; bins * bins];
    let mut g_sums = vec![0f32; bins * bins];
    let mut b_sums = vec![0f32; bins * bins];
    let width = rect.width().max(1.0);
    let height = rect.height().max(1.0);
    for point in points {
        if let Some(idx) =
            heatmap_bin_index(point, rect, center, scale, pan, width, height, bins)
        {
            accumulate_bin(idx, point, &color_for_point, &mut counts, &mut r_sums, &mut g_sums, &mut b_sums);
        }
    }
    render_heatmap_bins(painter, rect, bins, &counts, &r_sums, &g_sums, &b_sums)
}

fn heatmap_bin_index(
    point: &crate::egui_app::state::MapPoint,
    rect: egui::Rect,
    center: egui::Pos2,
    scale: f32,
    pan: egui::Vec2,
    width: f32,
    height: f32,
    bins: usize,
) -> Option<usize> {
    let pos = map_to_screen(point.x, point.y, rect, center, scale, pan);
    if !rect.contains(pos) {
        return None;
    }
    let nx = ((pos.x - rect.min.x) / width).clamp(0.0, 0.999);
    let ny = ((pos.y - rect.min.y) / height).clamp(0.0, 0.999);
    let ix = (nx * bins as f32) as usize;
    let iy = (ny * bins as f32) as usize;
    Some(iy * bins + ix)
}

fn accumulate_bin<F>(
    idx: usize,
    point: &crate::egui_app::state::MapPoint,
    color_for_point: &F,
    counts: &mut [u32],
    r_sums: &mut [f32],
    g_sums: &mut [f32],
    b_sums: &mut [f32],
) where
    F: Fn(&crate::egui_app::state::MapPoint) -> egui::Color32,
{
    let color = color_for_point(point);
    counts[idx] = counts[idx].saturating_add(1);
    r_sums[idx] += color.r() as f32;
    g_sums[idx] += color.g() as f32;
    b_sums[idx] += color.b() as f32;
}

fn render_heatmap_bins(
    painter: &egui::Painter,
    rect: egui::Rect,
    bins: usize,
    counts: &[u32],
    r_sums: &[f32],
    g_sums: &[f32],
    b_sums: &[f32],
) -> usize {
    let mut drawn = 0usize;
    let max_count = counts.iter().copied().max().unwrap_or(1).max(1) as f32;
    let cell_w = rect.width() / bins as f32;
    let cell_h = rect.height() / bins as f32;
    for iy in 0..bins {
        for ix in 0..bins {
            let idx = iy * bins + ix;
            let count = counts[idx] as f32;
            if count <= 0.0 {
                continue;
            }
            drawn += 1;
            let intensity = (count / max_count).clamp(0.0, 1.0);
            let alpha = (intensity * 200.0) as u8;
            let avg = 1.0 / count;
            let r = (r_sums[idx] * avg).round().clamp(0.0, 255.0) as u8;
            let g = (g_sums[idx] * avg).round().clamp(0.0, 255.0) as u8;
            let b = (b_sums[idx] * avg).round().clamp(0.0, 255.0) as u8;
            let color = egui::Color32::from_rgba_unmultiplied(r, g, b, alpha);
            let min = egui::pos2(rect.min.x + ix as f32 * cell_w, rect.min.y + iy as f32 * cell_h);
            let max = egui::pos2(min.x + cell_w, min.y + cell_h);
            painter.rect_filled(egui::Rect::from_min_max(min, max), 0.0, color);
        }
    }
    drawn
}
