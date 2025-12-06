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
pub struct DecodedWaveform {
    pub samples: Vec<f32>,
    pub duration_seconds: f32,
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
            background: Color32::from_rgb(16, 16, 24),
            foreground: Color32::from_rgb(0, 200, 255),
        }
    }

    /// Produce an empty waveform as an egui color image.
    pub fn empty_color_image(&self) -> ColorImage {
        self.render_color_image(&[])
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
        let (samples, duration_seconds) = self.load_samples(bytes)?;
        Ok(DecodedWaveform {
            samples,
            duration_seconds,
        })
    }

    /// Render an egui color image from already-decoded samples.
    pub fn render_color_image(&self, samples: &[f32]) -> ColorImage {
        let columns = self.sample_columns(samples);
        self.paint_color_image(&columns)
    }

    /// Decode bytes into mono samples and duration seconds.
    fn load_samples(&self, bytes: &[u8]) -> Result<(Vec<f32>, f32), String> {
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

        Ok((samples, duration))
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
        let mut cols = vec![(0.0, 0.0); self.width as usize];
        if samples.is_empty() {
            return cols;
        }

        let chunk = (samples.len() / self.width as usize).max(1);

        for (x, col) in cols.iter_mut().enumerate() {
            let start = x * chunk;
            if start >= samples.len() {
                break;
            }
            let end = ((x + 1) * chunk).min(samples.len());
            let mut min: f32 = 1.0;
            let mut max: f32 = -1.0;
            for &sample in &samples[start..end] {
                let clamped = sample.clamp(-1.0, 1.0);
                min = min.min(clamped);
                max = max.max(clamped);
            }
            *col = (min, max);
        }

        cols
    }

    fn paint_color_image(&self, columns: &[(f32, f32)]) -> ColorImage {
        let fg = self.foreground;
        let bg = self.background;
        let mut image = ColorImage::new([self.width as usize, self.height as usize], bg);
        let stride = self.width as usize;
        let mid = (self.height / 2) as f32;
        let limit = self.height.saturating_sub(1) as f32;

        for (x, (min, max)) in columns.iter().enumerate() {
            let top = (mid - max * (mid - 1.0)).clamp(0.0, limit) as u32;
            let bottom = (mid - min * (mid - 1.0)).clamp(0.0, limit) as u32;
            for y in top..=bottom {
                let idx = y as usize * stride + x;
                if let Some(pixel) = image.pixels.get_mut(idx) {
                    *pixel = fg;
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
}
