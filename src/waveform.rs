use std::path::Path;

use egui::{Color32, ColorImage};
use hound::SampleFormat;
use serde::{Deserialize, Serialize};

/// Waveform pixels and audio payload loaded from disk.
pub struct LoadedWaveform {
    pub image: ColorImage,
    pub audio_bytes: Vec<u8>,
    pub duration_seconds: f32,
}

/// Raw audio data decoded from a wav file, ready to render or play.
#[derive(Clone)]
pub struct DecodedWaveform {
    pub samples: Vec<f32>,
    pub duration_seconds: f32,
    pub sample_rate: u32,
    pub channels: u16,
}

impl DecodedWaveform {
    pub fn channel_count(&self) -> usize {
        self.channels.max(1) as usize
    }

    pub fn frame_count(&self) -> usize {
        let channels = self.channel_count();
        if channels == 0 {
            0
        } else {
            self.samples.len() / channels
        }
    }
}

/// Visual presentation mode for multi-channel audio.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum WaveformChannelView {
    /// Collapse all channels into one envelope using per-frame min/max to avoid phase cancellation.
    #[default]
    Mono,
    /// Render the first two channels separately in a stacked stereo view.
    SplitStereo,
}

#[derive(Clone, Debug, PartialEq)]
pub enum WaveformColumnView {
    Mono(Vec<(f32, f32)>),
    SplitStereo {
        left: Vec<(f32, f32)>,
        right: Vec<(f32, f32)>,
    },
}

/// Renders averaged waveforms from wav samples.
#[derive(Clone)]
pub struct WaveformRenderer {
    width: u32,
    height: u32,
    background: Color32,
    foreground: Color32,
}

impl WaveformRenderer {
    /// Create a renderer with the target image size and colors.
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            background: Color32::from_rgb(18, 16, 14),
            foreground: Color32::from_rgb(250, 246, 240),
        }
    }

    /// Current render target dimensions.
    pub fn dimensions(&self) -> (u32, u32) {
        (self.width, self.height)
    }

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

    /// Load a wav file from disk and return its pixels, raw bytes, and duration.
    pub fn load_waveform(&self, path: &Path) -> Result<LoadedWaveform, String> {
        let bytes = std::fs::read(path)
            .map_err(|error| format!("Failed to read {}: {error}", path.display()))?;
        let decoded = self.decode_from_bytes(&bytes)?;
        let image = self.render_color_image_for_mode(&decoded, WaveformChannelView::Mono);
        Ok(LoadedWaveform {
            image,
            audio_bytes: bytes,
            duration_seconds: decoded.duration_seconds,
        })
    }

    /// Decode wav bytes into samples and duration without rendering.
    pub fn decode_from_bytes(&self, bytes: &[u8]) -> Result<DecodedWaveform, String> {
        let (samples, duration_seconds, sample_rate, channels) = self.load_samples(bytes)?;
        Ok(DecodedWaveform {
            samples,
            duration_seconds,
            sample_rate,
            channels,
        })
    }

    /// Render an egui color image for a decoded waveform in the given channel view.
    pub fn render_color_image_for_mode(
        &self,
        decoded: &DecodedWaveform,
        view: WaveformChannelView,
    ) -> ColorImage {
        self.render_color_image_with_size(
            &decoded.samples,
            decoded.channel_count(),
            view,
            self.width,
            self.height,
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
        // Oversample horizontally to reduce aliasing, then combine down to the requested size.
        let oversample = Self::oversample_factor(width, samples.len() / channels.max(1));
        let oversampled_width = width.saturating_mul(oversample);
        let oversampled =
            Self::sample_columns_for_width(samples, channels, oversampled_width, view);
        let columns = if oversample == 1 {
            oversampled
        } else {
            Self::downsample_columns_view(&oversampled, oversample as usize, width as usize)
        };
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

    /// Decode bytes into interleaved samples and duration seconds.
    fn load_samples(&self, bytes: &[u8]) -> Result<(Vec<f32>, f32, u32, u16), String> {
        let mut reader = hound::WavReader::new(std::io::Cursor::new(bytes))
            .map_err(|error| format!("Invalid wav: {error}"))?;
        let spec = reader.spec();
        let channels = spec.channels.max(1) as usize;

        let samples = match spec.sample_format {
            SampleFormat::Float => Self::read_float_samples(&mut reader)?,
            SampleFormat::Int => Self::read_int_samples(&mut reader, spec.bits_per_sample)?,
        };
        let frames = if channels == 0 {
            0
        } else {
            samples.len() / channels
        };
        let duration = frames as f32 / spec.sample_rate.max(1) as f32;

        Ok((samples, duration, spec.sample_rate, spec.channels))
    }

    fn read_float_samples(
        reader: &mut hound::WavReader<std::io::Cursor<&[u8]>>,
    ) -> Result<Vec<f32>, String> {
        let raw: Vec<f32> = reader
            .samples::<f32>()
            .map(|s| s.map_err(|error| format!("Sample error: {error}")))
            .collect::<Result<_, _>>()?;
        Ok(raw)
    }

    fn read_int_samples(
        reader: &mut hound::WavReader<std::io::Cursor<&[u8]>>,
        bits_per_sample: u16,
    ) -> Result<Vec<f32>, String> {
        let scale = (1i64 << bits_per_sample.saturating_sub(1)).max(1) as f32;
        let raw: Vec<f32> = reader
            .samples::<i32>()
            .map(|s| {
                s.map(|v| v as f32 / scale)
                    .map_err(|error| format!("Sample error: {error}"))
            })
            .collect::<Result<_, _>>()?;
        Ok(raw)
    }

    fn sample_columns_for_width(
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

    fn downsample_columns(
        columns: &[(f32, f32)],
        factor: usize,
        target_width: usize,
    ) -> Vec<(f32, f32)> {
        if factor <= 1 {
            return columns.to_vec();
        }
        let mut result = vec![(0.0, 0.0); target_width.max(1)];
        for (i, slot) in result.iter_mut().enumerate() {
            let start = i.saturating_mul(factor);
            let end = ((i + 1).saturating_mul(factor)).min(columns.len());
            let slice = &columns[start..end.max(start + 1).min(columns.len())];
            let mut min_v: f32 = 1.0;
            let mut max_v: f32 = -1.0;
            for (lo, hi) in slice {
                min_v = min_v.min(*lo);
                max_v = max_v.max(*hi);
            }
            *slot = (min_v, max_v);
        }
        result
    }

    fn downsample_columns_view(
        columns: &WaveformColumnView,
        factor: usize,
        target_width: usize,
    ) -> WaveformColumnView {
        if factor <= 1 {
            return columns.clone();
        }
        match columns {
            WaveformColumnView::Mono(cols) => {
                WaveformColumnView::Mono(Self::downsample_columns(cols, factor, target_width))
            }
            WaveformColumnView::SplitStereo { left, right } => WaveformColumnView::SplitStereo {
                left: Self::downsample_columns(left, factor, target_width),
                right: Self::downsample_columns(right, factor, target_width),
            },
        }
    }

    fn paint_color_image_for_size(
        columns: &[(f32, f32)],
        width: u32,
        height: u32,
        foreground: Color32,
        background: Color32,
    ) -> ColorImage {
        let mut image = ColorImage::new(
            [width as usize, height as usize],
            vec![
                Color32::from_rgba_unmultiplied(background.r(), background.g(), background.b(), 0,);
                (width as usize) * (height as usize)
            ],
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

        let mut image = ColorImage::new(
            [width as usize, height as usize],
            vec![
                Color32::from_rgba_unmultiplied(background.r(), background.g(), background.b(), 0,);
                (width as usize) * (height as usize)
            ],
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

    fn oversample_factor(width: u32, frame_count: usize) -> u32 {
        if frame_count == 0 {
            return 1;
        }
        if width <= 1_024 {
            8
        } else if width <= 4_096 {
            4
        } else {
            2
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
    fn render_color_image_respects_requested_size() {
        let renderer = WaveformRenderer::new(2, 2);
        let image =
            renderer.render_color_image_with_size(&[0.0, 0.5], 1, WaveformChannelView::Mono, 4, 6);
        assert_eq!(image.size, [4, 6]);
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
