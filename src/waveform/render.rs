mod cache;
mod paint;

use super::{DecodedWaveform, WaveformChannelView, WaveformColumnView, WaveformRenderer};
use egui::ColorImage;

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
            return self.render_color_image_with_size(
                &[],
                1,
                WaveformChannelView::Mono,
                width,
                height,
            );
        }

        let start = view_start.clamp(0.0, 1.0);
        let end = view_end.clamp(start, 1.0);
        let fraction = (end - start).max(0.000_001);

        if decoded.samples.is_empty() {
            if let Some(peaks) = decoded.peaks.as_deref() {
                let columns = peaks.sample_columns_for_view(start, end, width, view);
                let frames_per_column = (frame_count as f32 * fraction / width as f32).max(1.0);
                let smooth_radius = Self::smoothing_radius(frames_per_column, width);
                return match columns {
                    WaveformColumnView::Mono(cols) => {
                        let cols = Self::smooth_columns(&cols, smooth_radius);
                        Self::paint_color_image_for_size_with_density(
                            &cols,
                            width,
                            height,
                            self.foreground,
                            self.background,
                            frames_per_column,
                        )
                    }
                    WaveformColumnView::SplitStereo { left, right } => {
                        let left = Self::smooth_columns(&left, smooth_radius);
                        let right = Self::smooth_columns(&right, smooth_radius);
                        Self::paint_split_color_image_with_density(
                            &left,
                            &right,
                            width,
                            height,
                            self.foreground,
                            self.background,
                            frames_per_column,
                        )
                    }
                };
            }
            return self.render_color_image_with_size(
                &[],
                1,
                WaveformChannelView::Mono,
                width,
                height,
            );
        }

        if let Some(image) = self.render_cached_view(decoded, start, end, view, width, height) {
            return image;
        }

        // Fallback: sample only the visible frames directly.
        let start_frame =
            ((start * frame_count as f32).floor() as usize).min(frame_count.saturating_sub(1));
        let mut end_frame =
            ((end * frame_count as f32).ceil() as usize).clamp(start_frame + 1, frame_count);
        if end_frame <= start_frame {
            end_frame = (start_frame + 1).min(frame_count);
        }
        let start_idx = start_frame.saturating_mul(channels);
        let end_idx = end_frame
            .saturating_mul(channels)
            .min(decoded.samples.len());
        self.render_color_image_with_size(
            &decoded.samples[start_idx..end_idx],
            channels,
            view,
            width,
            height,
        )
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
        let frame_count = samples.len() / channels.max(1);
        let frames_per_column = (frame_count as f32 / width as f32).max(1.0);
        if frames_per_column <= 1.2 {
            return match view {
                WaveformChannelView::Mono => Self::paint_line_image(
                    samples,
                    channels,
                    width,
                    height,
                    self.foreground,
                    self.background,
                    None,
                ),
                WaveformChannelView::SplitStereo => Self::paint_split_line_image(
                    samples,
                    channels,
                    width,
                    height,
                    self.foreground,
                    self.background,
                ),
            };
        }
        let columns = Self::sample_columns_for_width(samples, channels, width, view);
        let smooth_radius = Self::smoothing_radius(frames_per_column, width);
        match columns {
            WaveformColumnView::Mono(cols) => {
                let cols = Self::smooth_columns(&cols, smooth_radius);
                Self::paint_color_image_for_size_with_density(
                    &cols,
                    width,
                    height,
                    self.foreground,
                    self.background,
                    frames_per_column,
                )
            }
            WaveformColumnView::SplitStereo { left, right } => {
                let left = Self::smooth_columns(&left, smooth_radius);
                let right = Self::smooth_columns(&right, smooth_radius);
                Self::paint_split_color_image_with_density(
                    &left,
                    &right,
                    width,
                    height,
                    self.foreground,
                    self.background,
                    frames_per_column,
                )
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
            cache_token: 1,
            samples: std::sync::Arc::from(vec![0.0, 0.5, -0.25, 0.25]),
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

    #[test]
    fn columns_window_clamps_to_last_window() {
        let renderer = WaveformRenderer::new(2, 2);
        assert_eq!(renderer.columns_window(1.0, 10, 4), Some((6, 10)));
    }

    #[test]
    fn columns_window_rejects_invalid_sizes() {
        let renderer = WaveformRenderer::new(2, 2);
        assert_eq!(renderer.columns_window(0.0, 2, 4), None);
        assert_eq!(renderer.columns_window(0.0, 10, 0), None);
    }

    #[test]
    fn smoothing_radius_handles_boundaries() {
        assert_eq!(WaveformRenderer::smoothing_radius(2.0, 5), 0);
        assert_eq!(WaveformRenderer::smoothing_radius(2.01, 5), 1);
        assert_eq!(WaveformRenderer::smoothing_radius(8.0, 5), 1);
        assert_eq!(WaveformRenderer::smoothing_radius(8.01, 5), 2);
        assert_eq!(WaveformRenderer::smoothing_radius(9.0, 2), 0);
    }
}
