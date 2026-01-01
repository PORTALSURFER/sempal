use super::WaveformRenderer;
use egui::{Color32, ColorImage};

impl WaveformRenderer {
    pub(super) fn paint_color_image_for_size_with_density(
        columns: &[(f32, f32)],
        width: u32,
        height: u32,
        foreground: Color32,
        background: Color32,
        frames_per_column: f32,
    ) -> ColorImage {
        let fill =
            Color32::from_rgba_unmultiplied(background.r(), background.g(), background.b(), 0);
        let mut image = ColorImage::new(
            [width as usize, height as usize],
            vec![fill; (width as usize) * (height as usize)],
        );
        let stride = width as usize;
        let half_height = (height.saturating_sub(1)) as f32 / 2.0;
        let mid = half_height;
        let limit = height.saturating_sub(1) as f32;
        let thickness = Self::band_thickness(frames_per_column, height);
        let density_boost = Self::density_alpha_boost(frames_per_column);
        let fg = (
            foreground.r(),
            foreground.g(),
            foreground.b(),
            foreground.a(),
        );

        for (x, (min, max)) in columns.iter().enumerate() {
            let top = (mid - max * half_height).clamp(0.0, limit);
            let bottom = (mid - min * half_height).clamp(0.0, limit);
            let amp_span = (max - min).abs();
            let amp_scale = (amp_span * 12.0).clamp(0.0, 1.0);
            let column_thickness = 0.8 + (thickness - 0.8) * amp_scale;
            let band_min = top.min(bottom) - column_thickness * 0.5;
            let band_max = top.max(bottom) + column_thickness * 0.5;
            let span = (band_max - band_min).max(column_thickness);
            let start_y = band_min.floor().clamp(0.0, limit) as u32;
            let end_y = band_max.ceil().clamp(0.0, limit) as u32;
            for y in start_y..=end_y {
                let pixel_min = y as f32;
                let pixel_max = pixel_min + 1.0;
                let overlap = (band_max.min(pixel_max) - band_min.max(pixel_min)).max(0.0);
                if overlap <= 0.0 {
                    continue;
                }
                let coverage = (overlap / span).clamp(0.0, 1.0);
                let boosted = (coverage.sqrt() + density_boost).clamp(0.45, 1.0);
                let alpha = ((fg.3 as f32) * boosted).round() as u8;
                let idx = y as usize * stride + x;
                if let Some(pixel) = image.pixels.get_mut(idx) {
                    *pixel = Color32::from_rgba_unmultiplied(fg.0, fg.1, fg.2, alpha);
                }
            }
        }
        image
    }

    pub(super) fn paint_split_color_image_with_density(
        left: &[(f32, f32)],
        right: &[(f32, f32)],
        width: u32,
        height: u32,
        foreground: Color32,
        background: Color32,
        frames_per_column: f32,
    ) -> ColorImage {
        let gap = if height >= 3 { 2 } else { 0 };
        let split_height = height.saturating_sub(gap);
        let top_height = (split_height / 2).max(1);
        let bottom_height = split_height.saturating_sub(top_height).max(1);

        let top = Self::paint_color_image_for_size_with_density(
            left,
            width,
            top_height,
            foreground,
            background,
            frames_per_column,
        );
        let bottom = Self::paint_color_image_for_size_with_density(
            right,
            width,
            bottom_height,
            foreground,
            background,
            frames_per_column,
        );

        let fill =
            Color32::from_rgba_unmultiplied(background.r(), background.g(), background.b(), 0);
        let mut image = ColorImage::new(
            [width as usize, height as usize],
            vec![fill; (width as usize) * (height as usize)],
        );
        Self::blit_image(&mut image, &top, 0);
        let bottom_offset = top_height as usize + gap as usize;
        let clamped_offset = bottom_offset.min(image.size[1]);
        Self::blit_image(&mut image, &bottom, clamped_offset);
        image
    }

    fn band_thickness(frames_per_column: f32, height: u32) -> f32 {
        if !frames_per_column.is_finite() || frames_per_column <= 1.0 {
            return 2.2;
        }
        let boost = (frames_per_column.log2().max(0.0) * 1.8).min(10.0);
        let max_thickness = (height as f32 * 0.78).max(2.2);
        (2.2 + boost).min(max_thickness)
    }

    fn density_alpha_boost(frames_per_column: f32) -> f32 {
        if !frames_per_column.is_finite() || frames_per_column <= 1.0 {
            return 0.0;
        }
        (frames_per_column.log2().max(0.0) * 0.12).min(0.5)
    }
}
