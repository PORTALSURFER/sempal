use super::style;
use eframe::egui;
use std::collections::HashMap;

pub(crate) struct ClusterStats {
    pub cluster_count: usize,
    pub noise_count: usize,
    pub noise_ratio: f32,
    pub min_cluster_size: usize,
    pub max_cluster_size: usize,
}

pub(crate) fn compute_cluster_stats(
    points: &[crate::egui_app::state::MapPoint],
) -> Option<ClusterStats> {
    let mut cluster_sizes: HashMap<i32, usize> = HashMap::new();
    let mut noise_count = 0usize;
    let mut seen = 0usize;
    for point in points {
        let Some(cluster_id) = point.cluster_id else {
            continue;
        };
        seen += 1;
        if cluster_id < 0 {
            noise_count += 1;
        } else {
            *cluster_sizes.entry(cluster_id).or_insert(0) += 1;
        }
    }
    if seen == 0 {
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
        noise_count,
        noise_ratio: noise_count as f32 / seen as f32,
        min_cluster_size,
        max_cluster_size,
    })
}

pub(crate) fn cluster_color(
    cluster_id: i32,
    palette: &style::Palette,
    alpha: u8,
) -> egui::Color32 {
    if cluster_id < 0 {
        return style::with_alpha(palette.text_muted, alpha);
    }
    let hue = (cluster_id as u32).wrapping_mul(97) % 360;
    let (r, g, b) = hsv_to_rgb(hue as f32, 0.55, 0.85);
    egui::Color32::from_rgba_unmultiplied(r, g, b, alpha)
}

pub(crate) struct ClusterCentroid {
    pub x: f32,
    pub y: f32,
    pub count: usize,
}

pub(crate) fn cluster_centroids(
    points: &[crate::egui_app::state::MapPoint],
) -> HashMap<i32, ClusterCentroid> {
    let mut sums: HashMap<i32, (f32, f32, usize)> = HashMap::new();
    for point in points {
        let Some(cluster_id) = point.cluster_id else {
            continue;
        };
        if cluster_id < 0 {
            continue;
        }
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
            ClusterCentroid {
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
    centroids: &HashMap<i32, ClusterCentroid>,
    palette: &style::Palette,
    alpha: u8,
    map_diagonal: f32,
    blend_threshold: f32,
) -> egui::Color32 {
    let Some(cluster_id) = point.cluster_id else {
        return palette.accent_mint;
    };
    if cluster_id < 0 {
        return style::with_alpha(palette.text_muted, alpha);
    }
    if blend_threshold <= 0.0 || map_diagonal <= 0.0 {
        return cluster_color(cluster_id, palette, alpha);
    }
    let Some(primary) = centroids.get(&cluster_id) else {
        return cluster_color(cluster_id, palette, alpha);
    };
    let threshold = map_diagonal * blend_threshold;
    let primary_dist = distance(point.x, point.y, primary.x, primary.y);
    let mut nearest_other: Option<(i32, f32)> = None;
    for (other_id, centroid) in centroids {
        if *other_id == cluster_id {
            continue;
        }
        let dist = distance(point.x, point.y, centroid.x, centroid.y);
        if dist > threshold {
            continue;
        }
        if nearest_other.map(|(_, best)| dist < best).unwrap_or(true) {
            nearest_other = Some((*other_id, dist));
        }
    }
    let Some((other_id, other_dist)) = nearest_other else {
        return cluster_color(cluster_id, palette, alpha);
    };
    let weight_primary = 1.0 / (primary_dist + 1e-3);
    let weight_other = 1.0 / (other_dist + 1e-3);
    blend_colors(
        cluster_color(cluster_id, palette, alpha),
        cluster_color(other_id, palette, alpha),
        weight_primary,
        weight_other,
    )
}

pub(crate) fn filter_points(
    points: &[crate::egui_app::state::MapPoint],
    overlay: bool,
    hide_noise: bool,
    filter: Option<i32>,
) -> Vec<crate::egui_app::state::MapPoint> {
    if !overlay && filter.is_none() && !hide_noise {
        return points.to_vec();
    }
    points
        .iter()
        .filter(|point| {
            if let Some(target) = filter {
                return point.cluster_id == Some(target);
            }
            if overlay && hide_noise {
                if let Some(cluster_id) = point.cluster_id {
                    return cluster_id >= 0;
                }
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
    let r = (first.r() as f32 * wf + second.r() as f32 * ws).round().clamp(0.0, 255.0) as u8;
    let g = (first.g() as f32 * wf + second.g() as f32 * ws).round().clamp(0.0, 255.0) as u8;
    let b = (first.b() as f32 * wf + second.b() as f32 * ws).round().clamp(0.0, 255.0) as u8;
    let a = (first.a() as f32 * wf + second.a() as f32 * ws).round().clamp(0.0, 255.0) as u8;
    egui::Color32::from_rgba_unmultiplied(r, g, b, a)
}
