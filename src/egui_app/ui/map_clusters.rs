use super::style;
use crate::egui_app::state::{MapBounds, MapClusterCentroid};
use eframe::egui;
use std::collections::HashMap;
use std::f32::consts::PI;

pub(crate) struct ClusterStats {
    pub cluster_count: usize,
    pub min_cluster_size: usize,
    pub max_cluster_size: usize,
    pub assigned_count: usize,
    pub missing_count: usize,
    pub total_count: usize,
}

pub(crate) fn compute_cluster_stats(
    points: &[crate::egui_app::state::MapPoint],
) -> Option<ClusterStats> {
    let mut cluster_sizes: HashMap<i32, usize> = HashMap::new();
    let mut assigned_count = 0usize;
    let mut missing_count = 0usize;
    for point in points {
        let Some(cluster_id) = point.cluster_id else {
            missing_count += 1;
            continue;
        };
        assigned_count += 1;
        *cluster_sizes.entry(cluster_id).or_insert(0) += 1;
    }
    if assigned_count == 0 {
        return None;
    }
    let (min_cluster_size, max_cluster_size) = if cluster_sizes.is_empty() {
        (0, 0)
    } else {
        let mut min_size = usize::MAX;
        let mut max_size = 0usize;
        for size in cluster_sizes.values() {
            min_size = min_size.min(*size);
            max_size = max_size.max(*size);
        }
        (min_size, max_size)
    };
    Some(ClusterStats {
        cluster_count: cluster_sizes.len(),
        min_cluster_size,
        max_cluster_size,
        assigned_count,
        missing_count,
        total_count: points.len(),
    })
}

pub(crate) fn cluster_color(
    cluster_id: i32,
    centroids: &HashMap<i32, MapClusterCentroid>,
    bounds: &MapBounds,
    palette: &style::Palette,
    alpha: u8,
) -> egui::Color32 {
    let Some(centroid) = centroids.get(&cluster_id) else {
        return palette.accent_mint;
    };
    let id_color = cluster_id_color(cluster_id, palette, alpha);
    let pos_color = position_based_color(centroid, bounds, palette, alpha);
    blend_colors(id_color, pos_color, 0.8, 0.2)
}

pub(crate) fn distance_shaded_cluster_color(
    point: &crate::egui_app::state::MapPoint,
    centroids: &HashMap<i32, MapClusterCentroid>,
    bounds: &MapBounds,
    palette: &style::Palette,
    alpha: u8,
    map_diagonal: f32,
) -> egui::Color32 {
    let Some(cluster_id) = point.cluster_id else {
        return palette.accent_mint;
    };
    if map_diagonal <= 0.0 {
        return blend_colors(
            cluster_color(cluster_id, centroids, bounds, palette, alpha),
            point_position_color(point.x, point.y, bounds, palette, alpha),
            0.65,
            0.35,
        );
    }
    let Some(primary) = centroids.get(&cluster_id) else {
        return blend_colors(
            cluster_color(cluster_id, centroids, bounds, palette, alpha),
            point_position_color(point.x, point.y, bounds, palette, alpha),
            0.65,
            0.35,
        );
    };
    let dist = distance(point.x, point.y, primary.x, primary.y);
    let base = blend_colors(
        cluster_color(cluster_id, centroids, bounds, palette, alpha),
        point_position_color(point.x, point.y, bounds, palette, alpha),
        0.65,
        0.35,
    );
    shade_by_distance(
        base,
        dist,
        map_diagonal,
    )
}

pub(crate) fn cluster_centroids(
    points: &[crate::egui_app::state::MapPoint],
) -> HashMap<i32, MapClusterCentroid> {
    let mut sums: HashMap<i32, (f32, f32, usize)> = HashMap::new();
    for point in points {
        let Some(cluster_id) = point.cluster_id else {
            continue;
        };
        let entry = sums.entry(cluster_id).or_insert((0.0, 0.0, 0));
        entry.0 += point.x;
        entry.1 += point.y;
        entry.2 += 1;
    }
    let mut centroids = HashMap::new();
    for (cluster_id, (sum_x, sum_y, count)) in sums {
        if count == 0 {
            continue;
        }
        centroids.insert(
            cluster_id,
            MapClusterCentroid {
                x: sum_x / count as f32,
                y: sum_y / count as f32,
                count,
            },
        );
    }
    centroids
}

pub(crate) fn blended_cluster_color(
    point: &crate::egui_app::state::MapPoint,
    centroids: &HashMap<i32, MapClusterCentroid>,
    bounds: &MapBounds,
    palette: &style::Palette,
    alpha: u8,
    map_diagonal: f32,
    blend_threshold: f32,
) -> egui::Color32 {
    let Some(cluster_id) = point.cluster_id else {
        return palette.accent_mint;
    };
    if blend_threshold <= 0.0 || map_diagonal <= 0.0 {
        return cluster_color(cluster_id, centroids, bounds, palette, alpha);
    }
    let Some(primary) = centroids.get(&cluster_id) else {
        return cluster_color(cluster_id, centroids, bounds, palette, alpha);
    };
    let threshold = (map_diagonal * blend_threshold).max(1e-6);
    let primary_dist = distance(point.x, point.y, primary.x, primary.y);

    let mut nearby: Vec<(i32, f32)> = centroids
        .iter()
        .map(|(id, centroid)| (*id, distance(point.x, point.y, centroid.x, centroid.y)))
        .collect();
    nearby.sort_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    nearby.truncate(3);

    let mut blended: Option<(egui::Color32, f32)> = None;
    for (id, dist) in nearby {
        let weight = (-dist / threshold).exp();
        let color = cluster_id_color(id, palette, alpha);
        blended = Some(match blended {
            None => (color, weight),
            Some((acc, acc_w)) => (blend_colors(acc, color, acc_w, weight), acc_w + weight),
        });
    }
    let color = blended.map(|(c, _)| c).unwrap_or_else(|| {
        cluster_id_color(cluster_id, palette, alpha)
    });
    let base = blend_colors(
        color,
        point_position_color(point.x, point.y, bounds, palette, alpha),
        0.65,
        0.35,
    );
    shade_by_distance(base, primary_dist, map_diagonal)
}

pub(crate) fn filter_points(
    points: &[crate::egui_app::state::MapPoint],
    overlay: bool,
    filter: Option<i32>,
) -> Vec<crate::egui_app::state::MapPoint> {
    if !overlay && filter.is_none() {
        return points.to_vec();
    }
    points
        .iter()
        .filter(|point| {
            if let Some(target) = filter {
                return point.cluster_id == Some(target);
            }
            true
        })
        .cloned()
        .collect()
}

fn hsv_to_rgb(h: f32, s: f32, v: f32) -> (u8, u8, u8) {
    let c = v * s;
    let hh = (h / 60.0) % 6.0;
    let x = c * (1.0 - ((hh % 2.0) - 1.0).abs());
    let (r1, g1, b1) = match hh as i32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = v - c;
    let r = ((r1 + m) * 255.0).round().clamp(0.0, 255.0) as u8;
    let g = ((g1 + m) * 255.0).round().clamp(0.0, 255.0) as u8;
    let b = ((b1 + m) * 255.0).round().clamp(0.0, 255.0) as u8;
    (r, g, b)
}

fn position_based_color(
    centroid: &MapClusterCentroid,
    bounds: &MapBounds,
    palette: &style::Palette,
    alpha: u8,
) -> egui::Color32 {
    let width = (bounds.max_x - bounds.min_x).abs();
    let height = (bounds.max_y - bounds.min_y).abs();
    if width <= f32::EPSILON || height <= f32::EPSILON {
        return palette.accent_mint;
    }
    let fx = ((centroid.x - bounds.min_x) / width).clamp(0.0, 1.0);
    let fy = ((centroid.y - bounds.min_y) / height).clamp(0.0, 1.0);
    let dx = fx - 0.5;
    let dy = fy - 0.5;
    let angle = (dy.atan2(dx) + PI) / (2.0 * PI);
    let radius = (dx * dx + dy * dy).sqrt().clamp(0.0, 1.0);
    let hue = angle * 360.0;
    let saturation = 0.35 + 0.25 * radius;
    let value = 0.70 + 0.20 * (1.0 - radius);
    let (r, g, b) = hsv_to_rgb(hue, saturation, value);
    egui::Color32::from_rgba_unmultiplied(r, g, b, alpha)
}

fn point_position_color(
    x: f32,
    y: f32,
    bounds: &MapBounds,
    palette: &style::Palette,
    alpha: u8,
) -> egui::Color32 {
    let width = (bounds.max_x - bounds.min_x).abs();
    let height = (bounds.max_y - bounds.min_y).abs();
    if width <= f32::EPSILON || height <= f32::EPSILON {
        return palette.accent_mint;
    }
    let fx = ((x - bounds.min_x) / width).clamp(0.0, 1.0);
    let fy = ((y - bounds.min_y) / height).clamp(0.0, 1.0);
    let dx = fx - 0.5;
    let dy = fy - 0.5;
    let angle = (dy.atan2(dx) + PI) / (2.0 * PI);
    let radius = (dx * dx + dy * dy).sqrt().clamp(0.0, 1.0);
    let hue = angle * 360.0;
    let saturation = 0.45 + 0.35 * radius;
    let value = 0.78 + 0.18 * (1.0 - radius);
    let (r, g, b) = hsv_to_rgb(hue, saturation, value);
    egui::Color32::from_rgba_unmultiplied(r, g, b, alpha)
}

fn cluster_id_color(cluster_id: i32, palette: &style::Palette, alpha: u8) -> egui::Color32 {
    if cluster_id < 0 {
        return style::with_alpha(palette.text_muted, alpha);
    }
    let id = cluster_id as f32;
    let hue = (id * 137.50776) % 360.0;
    let saturation = 0.62;
    let value = 0.86;
    let (r, g, b) = hsv_to_rgb(hue, saturation, value);
    egui::Color32::from_rgba_unmultiplied(r, g, b, alpha)
}

fn distance(ax: f32, ay: f32, bx: f32, by: f32) -> f32 {
    ((ax - bx).powi(2) + (ay - by).powi(2)).sqrt()
}

fn blend_colors(
    first: egui::Color32,
    second: egui::Color32,
    weight_first: f32,
    weight_second: f32,
) -> egui::Color32 {
    let sum = (weight_first + weight_second).max(1e-6);
    let wf = weight_first / sum;
    let ws = weight_second / sum;
    let r = (first.r() as f32 * wf + second.r() as f32 * ws)
        .round()
        .clamp(0.0, 255.0) as u8;
    let g = (first.g() as f32 * wf + second.g() as f32 * ws)
        .round()
        .clamp(0.0, 255.0) as u8;
    let b = (first.b() as f32 * wf + second.b() as f32 * ws)
        .round()
        .clamp(0.0, 255.0) as u8;
    let a = (first.a() as f32 * wf + second.a() as f32 * ws)
        .round()
        .clamp(0.0, 255.0) as u8;
    egui::Color32::from_rgba_unmultiplied(r, g, b, a)
}

fn shade_by_distance(color: egui::Color32, distance: f32, map_diagonal: f32) -> egui::Color32 {
    if map_diagonal <= 0.0 {
        return color;
    }
    let norm = (distance / (map_diagonal * 0.4)).clamp(0.0, 1.0);
    let shade = 0.55 + 0.45 * (1.0 - norm);
    let r = (color.r() as f32 * shade).round().clamp(0.0, 255.0) as u8;
    let g = (color.g() as f32 * shade).round().clamp(0.0, 255.0) as u8;
    let b = (color.b() as f32 * shade).round().clamp(0.0, 255.0) as u8;
    egui::Color32::from_rgba_unmultiplied(r, g, b, color.a())
}
