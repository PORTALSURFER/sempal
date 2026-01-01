mod density;
mod lines;

use super::WaveformRenderer;
use egui::ColorImage;

impl WaveformRenderer {
    pub(super) fn smoothing_radius(frames_per_column: f32, width: u32) -> usize {
        if width < 3 {
            return 0;
        }
        if frames_per_column > 8.0 {
            2
        } else if frames_per_column > 2.0 {
            1
        } else {
            0
        }
    }

    pub(super) fn smooth_columns(columns: &[(f32, f32)], radius: usize) -> Vec<(f32, f32)> {
        if radius == 0 || columns.len() < 2 {
            return columns.to_vec();
        }
        let mut smoothed = Vec::with_capacity(columns.len());
        let len = columns.len();
        for idx in 0..len {
            let start = idx.saturating_sub(radius);
            let end = (idx + radius + 1).min(len);
            let mut min_sum = 0.0_f32;
            let mut max_sum = 0.0_f32;
            let mut weight_sum = 0.0_f32;
            for i in start..end {
                let dist = idx.abs_diff(i) as f32;
                let weight = (radius as f32 + 1.0 - dist).max(0.0);
                let (min, max) = columns[i];
                min_sum += min * weight;
                max_sum += max * weight;
                weight_sum += weight;
            }
            let denom = weight_sum.max(1.0);
            smoothed.push((min_sum / denom, max_sum / denom));
        }
        smoothed
    }

    pub(super) fn blit_image(target: &mut ColorImage, source: &ColorImage, y_offset: usize) {
        let width = target.size[0].min(source.size[0]);
        for y in 0..source.size[1] {
            let dest_y = y + y_offset;
            if dest_y >= target.size[1] {
                break;
            }
            let dest_offset = dest_y * target.size[0];
            let src_offset = y * source.size[0];
            let len = width.min(target.size[0]).min(source.size[0]);
            if let (Some(dest), Some(src)) = (
                target.pixels.get_mut(dest_offset..dest_offset + len),
                source.pixels.get(src_offset..src_offset + len),
            ) {
                dest.copy_from_slice(src);
            }
        }
    }
}
