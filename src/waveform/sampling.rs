use super::{WaveformChannelView, WaveformColumnView, WaveformRenderer};

impl WaveformRenderer {
    /// Build column extrema for the provided samples using the renderer width and mono view.
    pub fn sample_columns(&self, samples: &[f32]) -> Vec<(f32, f32)> {
        match Self::sample_columns_for_width(samples, 1, self.width, WaveformChannelView::Mono) {
            WaveformColumnView::Mono(cols) => cols,
            _ => unreachable!("mono view should not produce split columns"),
        }
    }

    /// Build column extrema for the provided samples using the requested view.
    pub fn sample_columns_for_mode(
        samples: &[f32],
        channels: usize,
        width: u32,
        view: WaveformChannelView,
    ) -> WaveformColumnView {
        Self::sample_columns_for_width(samples, channels, width, view)
    }

    pub(super) fn sample_columns_for_width(
        samples: &[f32],
        channels: usize,
        width: u32,
        view: WaveformChannelView,
    ) -> WaveformColumnView {
        let width = width.max(1) as usize;
        let channels = channels.max(1);
        let frame_count = samples.len() / channels;
        if frame_count == 0 {
            return WaveformColumnView::Mono(vec![(0.0, 0.0); width]);
        }
        match view {
            WaveformChannelView::Mono => {
                let columns = Self::sample_channel_columns(samples, channels, width, None);
                WaveformColumnView::Mono(columns)
            }
            WaveformChannelView::SplitStereo => {
                let left = Self::sample_channel_columns(samples, channels, width, Some(0));
                let right = Self::sample_channel_columns(samples, channels, width, Some(1));
                WaveformColumnView::SplitStereo { left, right }
            }
        }
    }

    fn sample_channel_columns(
        samples: &[f32],
        channels: usize,
        width: usize,
        channel_index: Option<usize>,
    ) -> Vec<(f32, f32)> {
        let frame_count = samples.len() / channels.max(1);
        let total = frame_count as f32;
        let mut columns = vec![(0.0, 0.0); width];
        for (x, col) in columns.iter_mut().enumerate() {
            let start = ((x as f32 * total) / width as f32)
                .floor()
                .min(frame_count.saturating_sub(1) as f32) as usize;
            let mut end = (((x as f32 + 1.0) * total) / width as f32)
                .ceil()
                .max((start + 1) as f32)
                .min(frame_count as f32) as usize;
            if end <= start {
                end = (start + 1).min(frame_count);
            }
            let mut min: f32 = 1.0;
            let mut max: f32 = -1.0;
            match channel_index {
                Some(channel) => {
                    let channel = channel.min(channels.saturating_sub(1));
                    for frame in start..end {
                        let idx = frame.saturating_mul(channels).saturating_add(channel);
                        if let Some(sample) = samples.get(idx) {
                            let clamped = sample.clamp(-1.0, 1.0);
                            min = min.min(clamped);
                            max = max.max(clamped);
                        }
                    }
                }
                None => {
                    for frame in start..end {
                        let frame_start = frame.saturating_mul(channels);
                        let frame_end = frame_start + channels;
                        let mut frame_min = 1.0_f32;
                        let mut frame_max = -1.0_f32;
                        for &sample in &samples[frame_start..frame_end.min(samples.len())] {
                            let clamped = sample.clamp(-1.0, 1.0);
                            frame_min = frame_min.min(clamped);
                            frame_max = frame_max.max(clamped);
                        }
                        min = min.min(frame_min);
                        max = max.max(frame_max);
                    }
                }
            }
            *col = (min, max);
        }
        columns
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mono_sample_columns_use_renderer_width() {
        let renderer = WaveformRenderer::new(2, 4);
        let samples = [0.1, 0.2, 0.3, 0.4];

        let columns = renderer.sample_columns(&samples);

        assert_eq!(columns, vec![(0.1, 0.2), (0.3, 0.4)]);
    }

    #[test]
    fn sample_columns_clamps_to_bounds() {
        let renderer = WaveformRenderer::new(2, 2);
        let samples = [2.0, -3.0, 0.5, -0.5];
        let columns = renderer.sample_columns(&samples);
        assert_eq!(columns, vec![(-1.0, 1.0), (-0.5, 0.5)]);
    }

    #[test]
    fn sample_columns_returns_zeroes_when_empty() {
        let renderer = WaveformRenderer::new(3, 2);
        let columns = renderer.sample_columns(&[]);
        assert_eq!(columns, vec![(0.0, 0.0); 3]);
    }

    #[test]
    fn sample_columns_cover_tail_sample() {
        let samples = [0.1_f32, 0.1, 0.1, 0.1, 0.9];
        let columns =
            WaveformRenderer::sample_columns_for_mode(&samples, 1, 2, WaveformChannelView::Mono);
        let WaveformColumnView::Mono(cols) = columns else {
            panic!("expected mono columns")
        };
        assert!((cols[1].1 - 0.9).abs() < 1e-6);
    }

    #[test]
    fn sample_columns_replicate_sparse_audio() {
        let samples = [0.75_f32];
        let columns =
            WaveformRenderer::sample_columns_for_mode(&samples, 1, 4, WaveformChannelView::Mono);
        let WaveformColumnView::Mono(cols) = columns else {
            panic!("expected mono columns")
        };
        assert_eq!(cols, vec![(0.75, 0.75); 4]);
    }

    #[test]
    fn mono_view_preserves_multi_channel_peaks() {
        let samples = [1.0_f32, -1.0]; // L = 1.0, R = -1.0

        let columns =
            WaveformRenderer::sample_columns_for_mode(&samples, 2, 1, WaveformChannelView::Mono);

        let WaveformColumnView::Mono(cols) = columns else {
            panic!("expected mono columns")
        };
        assert_eq!(cols, vec![(-1.0, 1.0)]);
    }

    #[test]
    fn split_view_shows_individual_channels() {
        let samples = [0.5_f32, -0.25];

        let columns = WaveformRenderer::sample_columns_for_mode(
            &samples,
            2,
            1,
            WaveformChannelView::SplitStereo,
        );

        let WaveformColumnView::SplitStereo { left, right } = columns else {
            panic!("expected split columns")
        };
        assert_eq!(left, vec![(0.5, 0.5)]);
        assert_eq!(right, vec![(-0.25, -0.25)]);
    }

    #[test]
    fn high_zoom_columns_keep_channel_extremes() {
        let samples = [1.0_f32, -1.0];
        let columns =
            WaveformRenderer::sample_columns_for_mode(&samples, 1, 64, WaveformChannelView::Mono);
        let WaveformColumnView::Mono(cols) = columns else {
            panic!("expected mono columns")
        };
        let has_positive = cols
            .iter()
            .any(|(min, max)| (*min - 1.0).abs() < 1e-6 && (*max - 1.0).abs() < 1e-6);
        let has_negative = cols
            .iter()
            .any(|(min, max)| (*min + 1.0).abs() < 1e-6 && (*max + 1.0).abs() < 1e-6);
        assert!(has_positive);
        assert!(has_negative);
    }

    #[test]
    fn split_channels_share_sampling_pipeline() {
        let samples = [0.75_f32, -0.5, -0.25, 0.5];
        let columns = WaveformRenderer::sample_columns_for_mode(
            &samples,
            2,
            8,
            WaveformChannelView::SplitStereo,
        );
        let WaveformColumnView::SplitStereo { left, right } = columns else {
            panic!("expected split columns")
        };
        assert!(
            left.iter()
                .any(|(min, max)| (*min - 0.75).abs() < 1e-6 && (*max - 0.75).abs() < 1e-6)
        );
        assert!(
            right
                .iter()
                .any(|(min, max)| (*min + 0.5).abs() < 1e-6 && (*max + 0.5).abs() < 1e-6)
        );
    }
}
