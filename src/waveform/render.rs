use super::{DecodedWaveform, WaveformChannelView, WaveformColumnView, WaveformRenderer};
use egui::{Color32, ColorImage};

impl WaveformRenderer {
    /// Produce an empty waveform as an egui color image.
    pub fn empty_color_image(&self) -> ColorImage {
        self.render_color_image_with_size(
            &[],
            1,
            WaveformChannelView::Mono,
            self.width,
            self.height,
        )
    }

    /// Render an egui color image for a decoded waveform in the given channel view.
    pub fn render_color_image_for_mode(
        &self,
        decoded: &DecodedWaveform,
        view: WaveformChannelView,
    ) -> ColorImage {
        if decoded.samples.is_empty() {
            return self.render_color_image_for_view_with_size(
                decoded,
                0.0,
                1.0,
                view,
                self.width,
                self.height,
            );
        }
        self.render_color_image_with_size(
            &decoded.samples,
            decoded.channel_count(),
            view,
            self.width,
            self.height,
        )
    }

    /// Render an egui color image for a decoded waveform over a normalized view window.
    ///
    /// Uses a cached full-width column envelope keyed by zoom (view fraction) to reduce work
    /// during panning at a constant zoom level.
    pub fn render_color_image_for_view_with_size(
        &self,
        decoded: &DecodedWaveform,
        view_start: f32,
        view_end: f32,
        view: WaveformChannelView,
        width: u32,
        height: u32,
    ) -> ColorImage {
        let width = width.max(1);
        let height = height.max(1);
        let channels = decoded.channel_count();
        let frame_count = decoded.frame_count();
        if frame_count == 0 {
            return self.render_color_image_with_size(&[], 1, WaveformChannelView::Mono, width, height);
        }

        let start = view_start.clamp(0.0, 1.0);
        let end = view_end.clamp(start, 1.0);
        let fraction = (end - start).max(0.000_001);

        if decoded.samples.is_empty() {
            if let Some(peaks) = decoded.peaks.as_ref() {
                let columns = peaks.sample_columns_for_view(start, end, width, view);
                return match columns {
                    WaveformColumnView::Mono(cols) => Self::paint_color_image_for_size(
                        &cols,
                        width,
                        height,
                        self.foreground,
                        self.background,
                    ),
                    WaveformColumnView::SplitStereo { left, right } => Self::paint_split_color_image(
                        &left,
                        &right,
                        width,
                        height,
                        self.foreground,
                        self.background,
                    ),
                };
            }
            return self.render_color_image_with_size(&[], 1, WaveformChannelView::Mono, width, height);
        }

        let full_width = self.cached_full_width(width, fraction, frame_count);
        if let Some((start_col, end_col)) = self.columns_window(start, full_width, width) {
            let cached = self
                .zoom_cache
                .get_or_compute(&decoded.samples, channels, view, full_width);
            return match cached {
                super::zoom_cache::CachedColumns::Mono(cols) => Self::paint_color_image_for_size(
                    &cols[start_col..end_col],
                    width,
                    height,
                    self.foreground,
                    self.background,
                ),
                super::zoom_cache::CachedColumns::SplitStereo { left, right } => {
                    Self::paint_split_color_image(
                        &left[start_col..end_col],
                        &right[start_col..end_col],
                        width,
                        height,
                        self.foreground,
                        self.background,
                    )
                }
            };
        }

        // Fallback: sample only the visible frames directly.
        let start_frame = ((start * frame_count as f32).floor() as usize)
            .min(frame_count.saturating_sub(1));
        let mut end_frame =
            ((end * frame_count as f32).ceil() as usize).clamp(start_frame + 1, frame_count);
        if end_frame <= start_frame {
            end_frame = (start_frame + 1).min(frame_count);
        }
        let start_idx = start_frame.saturating_mul(channels);
        let end_idx = end_frame.saturating_mul(channels).min(decoded.samples.len());
        self.render_color_image_with_size(&decoded.samples[start_idx..end_idx], channels, view, width, height)
    }

    /// Render an egui color image at an explicit size.
    pub fn render_color_image_with_size(
        &self,
        samples: &[f32],
        channels: usize,
        view: WaveformChannelView,
        width: u32,
        height: u32,
    ) -> ColorImage {
        let width = width.max(1);
        let height = height.max(1);
        let columns = Self::sample_columns_for_width(samples, channels, width, view);
        match columns {
            WaveformColumnView::Mono(cols) => Self::paint_color_image_for_size(
                &cols,
                width,
                height,
                self.foreground,
                self.background,
            ),
            WaveformColumnView::SplitStereo { left, right } => Self::paint_split_color_image(
                &left,
                &right,
                width,
                height,
                self.foreground,
                self.background,
            ),
        }
    }

    fn cached_full_width(&self, width: u32, view_fraction: f32, frame_count: usize) -> u32 {
        const MAX_CACHED_FULL_WIDTH: u32 = 200_000;
        let desired = ((width as f32) / view_fraction).ceil().max(width as f32) as u32;
        let frame_cap = frame_count.min(u32::MAX as usize) as u32;
        desired.min(frame_cap).min(MAX_CACHED_FULL_WIDTH).max(width)
    }

    fn columns_window(&self, view_start: f32, full_width: u32, width: u32) -> Option<(usize, usize)> {
        let full_width = full_width as usize;
        let width = width as usize;
        if full_width < width || width == 0 {
            return None;
        }
        let max_start = full_width.saturating_sub(width);
        let start = ((view_start * full_width as f32).floor() as usize).min(max_start);
        Some((start, start + width))
    }

    fn paint_color_image_for_size(
        columns: &[(f32, f32)],
        width: u32,
        height: u32,
        foreground: Color32,
        background: Color32,
    ) -> ColorImage {
        let fill = Color32::from_rgba_unmultiplied(background.r(), background.g(), background.b(), 0);
        let mut image = ColorImage::new(
            [width as usize, height as usize],
            vec![fill; (width as usize) * (height as usize)],
        );
        let stride = width as usize;
        let half_height = (height.saturating_sub(1)) as f32 / 2.0;
        let mid = half_height;
        let limit = height.saturating_sub(1) as f32;
        let thickness: f32 = 2.2;
        let fg = (
            foreground.r(),
            foreground.g(),
            foreground.b(),
            foreground.a(),
        );

        for (x, (min, max)) in columns.iter().enumerate() {
            let top = (mid - max * half_height).clamp(0.0, limit);
            let bottom = (mid - min * half_height).clamp(0.0, limit);
            let band_min = top.min(bottom) - thickness * 0.5;
            let band_max = top.max(bottom) + thickness * 0.5;
            let span = (band_max - band_min).max(thickness);
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
                let boosted = coverage.sqrt().max(0.45);
                let alpha = ((fg.3 as f32) * boosted).round() as u8;
                let idx = y as usize * stride + x;
                if let Some(pixel) = image.pixels.get_mut(idx) {
                    *pixel = Color32::from_rgba_unmultiplied(fg.0, fg.1, fg.2, alpha);
                }
            }
        }
        image
    }

    fn paint_split_color_image(
        left: &[(f32, f32)],
        right: &[(f32, f32)],
        width: u32,
        height: u32,
        foreground: Color32,
        background: Color32,
    ) -> ColorImage {
        let gap = if height >= 3 { 2 } else { 0 };
        let split_height = height.saturating_sub(gap);
        let top_height = (split_height / 2).max(1);
        let bottom_height = split_height.saturating_sub(top_height).max(1);

        let top = Self::paint_color_image_for_size(left, width, top_height, foreground, background);
        let bottom =
            Self::paint_color_image_for_size(right, width, bottom_height, foreground, background);

        let fill = Color32::from_rgba_unmultiplied(background.r(), background.g(), background.b(), 0);
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

    fn blit_image(target: &mut ColorImage, source: &ColorImage, y_offset: usize) {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_color_image_respects_requested_size() {
        let renderer = WaveformRenderer::new(2, 2);
        let image =
            renderer.render_color_image_with_size(&[0.0, 0.5], 1, WaveformChannelView::Mono, 4, 6);
        assert_eq!(image.size, [4, 6]);
    }

    #[test]
    fn render_color_image_for_view_respects_requested_size() {
        let renderer = WaveformRenderer::new(2, 2);
        let decoded = DecodedWaveform {
            samples: vec![0.0, 0.5, -0.25, 0.25],
            peaks: None,
            duration_seconds: 1.0,
            sample_rate: 48_000,
            channels: 1,
        };
        let image = renderer.render_color_image_for_view_with_size(
            &decoded,
            0.25,
            0.75,
            WaveformChannelView::Mono,
            5,
            3,
        );
        assert_eq!(image.size, [5, 3]);
    }
}
