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
