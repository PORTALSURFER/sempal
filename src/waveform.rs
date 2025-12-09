use std::path::Path;

use egui::{Color32, ColorImage};
use hound::SampleFormat;

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
        self.render_color_image_with_size(&[], self.width, self.height)
    }

    /// Load a wav file from disk and return its pixels, raw bytes, and duration.
    pub fn load_waveform(&self, path: &Path) -> Result<LoadedWaveform, String> {
        let bytes = std::fs::read(path)
            .map_err(|error| format!("Failed to read {}: {error}", path.display()))?;
        let decoded = self.decode_from_bytes(&bytes)?;
        let image = self.render_color_image(&decoded.samples);
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

    /// Render an egui color image from already-decoded samples.
    pub fn render_color_image(&self, samples: &[f32]) -> ColorImage {
        let columns = self.sample_columns(samples);
        self.paint_color_image(&columns)
    }

    /// Render an egui color image at an explicit size.
    pub fn render_color_image_with_size(
        &self,
        samples: &[f32],
        width: u32,
        height: u32,
    ) -> ColorImage {
        let width = width.max(1);
        let height = height.max(1);
        // Oversample horizontally to reduce aliasing, then combine down to the requested size.
        let oversample = if width <= 4_096 { 4 } else { 2 };
        let oversampled_width = width.saturating_mul(oversample);
        let oversampled = Self::sample_columns_for_width(samples, oversampled_width);
        let columns = if oversample == 1 {
            oversampled
        } else {
            Self::downsample_columns(&oversampled, oversample as usize, width as usize)
        };
        Self::paint_color_image_for_size(&columns, width, height, self.foreground, self.background)
    }

    /// Decode bytes into mono samples and duration seconds.
    fn load_samples(&self, bytes: &[u8]) -> Result<(Vec<f32>, f32, u32, u16), String> {
        let mut reader = hound::WavReader::new(std::io::Cursor::new(bytes))
            .map_err(|error| format!("Invalid wav: {error}"))?;
        let spec = reader.spec();
        let channels = spec.channels.max(1) as usize;

        let samples = match spec.sample_format {
            SampleFormat::Float => Self::read_float_samples(&mut reader, channels)?,
            SampleFormat::Int => {
                Self::read_int_samples(&mut reader, spec.bits_per_sample, channels)?
            }
        };
        let duration = samples.len() as f32 / spec.sample_rate.max(1) as f32;

        Ok((samples, duration, spec.sample_rate, spec.channels))
    }

    fn read_float_samples(
        reader: &mut hound::WavReader<std::io::Cursor<&[u8]>>,
        channels: usize,
    ) -> Result<Vec<f32>, String> {
        let raw: Vec<f32> = reader
            .samples::<f32>()
            .map(|s| s.map_err(|error| format!("Sample error: {error}")))
            .collect::<Result<_, _>>()?;
        Ok(Self::average_channels(raw, channels))
    }

    fn read_int_samples(
        reader: &mut hound::WavReader<std::io::Cursor<&[u8]>>,
        bits_per_sample: u16,
        channels: usize,
    ) -> Result<Vec<f32>, String> {
        let scale = (1i64 << bits_per_sample.saturating_sub(1)).max(1) as f32;
        let raw: Vec<f32> = reader
            .samples::<i32>()
            .map(|s| {
                s.map(|v| v as f32 / scale)
                    .map_err(|error| format!("Sample error: {error}"))
            })
            .collect::<Result<_, _>>()?;
        Ok(Self::average_channels(raw, channels))
    }

    /// Average multi-channel frames down to mono samples.
    fn average_channels(raw: Vec<f32>, channels: usize) -> Vec<f32> {
        raw.chunks(channels)
            .map(|frame| frame.iter().copied().sum::<f32>() / channels as f32)
            .collect()
    }

    fn sample_columns(&self, samples: &[f32]) -> Vec<(f32, f32)> {
        Self::sample_columns_for_width(samples, self.width)
    }

    fn sample_columns_for_width(samples: &[f32], width: u32) -> Vec<(f32, f32)> {
        let width = width.max(1) as usize;
        if samples.is_empty() {
            return vec![(0.0, 0.0); width];
        }

        let sample_count = samples.len();
        let total = sample_count as f32;
        let mut columns = vec![(0.0, 0.0); width];

        for (x, col) in columns.iter_mut().enumerate() {
            let start = ((x as f32 * total) / width as f32)
                .floor()
                .min(sample_count.saturating_sub(1) as f32) as usize;
            let mut end = (((x as f32 + 1.0) * total) / width as f32)
                .ceil()
                .max((start + 1) as f32)
                .min(sample_count as f32) as usize;
            if end <= start {
                end = (start + 1).min(sample_count);
            }
            let mut min: f32 = 1.0;
            let mut max: f32 = -1.0;
            for &sample in &samples[start..end] {
                let clamped = sample.clamp(-1.0, 1.0);
                min = min.min(clamped);
                max = max.max(clamped);
            }
            *col = (min, max);
        }

        columns
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

    fn paint_color_image(&self, columns: &[(f32, f32)]) -> ColorImage {
        Self::paint_color_image_for_size(
            columns,
            self.width,
            self.height,
            self.foreground,
            self.background,
        )
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn samples_are_not_normalized() {
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
        let image = renderer.render_color_image_with_size(&[0.0, 0.5], 4, 6);
        assert_eq!(image.size, [4, 6]);
    }

    #[test]
    fn sample_columns_cover_tail_sample() {
        let samples = [0.1_f32, 0.1, 0.1, 0.1, 0.9];
        let columns = WaveformRenderer::sample_columns_for_width(&samples, 2);
        assert!((columns[1].1 - 0.9).abs() < 1e-6);
    }

    #[test]
    fn sample_columns_replicate_sparse_audio() {
        let samples = [0.75_f32];
        let columns = WaveformRenderer::sample_columns_for_width(&samples, 4);
        assert_eq!(columns, vec![(0.75, 0.75); 4]);
    }
}
