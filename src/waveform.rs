use std::path::Path;

use hound::SampleFormat;
use slint::{Image, Rgb8Pixel, SharedPixelBuffer};

/// Waveform pixels and audio payload loaded from disk.
pub struct LoadedWaveform {
    pub image: Image,
    pub audio_bytes: Vec<u8>,
    pub duration_seconds: f32,
}

/// Renders averaged waveforms from wav samples.
#[derive(Clone)]
pub struct WaveformRenderer {
    width: u32,
    height: u32,
    background: Rgb8Pixel,
    foreground: Rgb8Pixel,
}

impl WaveformRenderer {
    /// Create a renderer with the target image size and colors.
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            background: Rgb8Pixel {
                r: 16,
                g: 16,
                b: 24,
            },
            foreground: Rgb8Pixel {
                r: 0,
                g: 200,
                b: 255,
            },
        }
    }

    /// Produce an empty waveform image with the configured styling.
    pub fn empty_image(&self) -> Image {
        self.render_waveform(&[])
    }

    pub fn load_waveform(&self, path: &Path) -> Result<LoadedWaveform, String> {
        let bytes = std::fs::read(path)
            .map_err(|error| format!("Failed to read {}: {error}", path.display()))?;
        let (samples, duration_seconds) = self.load_samples(&bytes)?;
        let image = self.render_waveform(&samples);
        Ok(LoadedWaveform {
            image,
            audio_bytes: bytes,
            duration_seconds,
        })
    }

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

    fn average_channels(raw: Vec<f32>, channels: usize) -> Vec<f32> {
        raw.chunks(channels)
            .map(|frame| frame.iter().copied().sum::<f32>() / channels as f32)
            .collect()
    }

    fn render_waveform(&self, samples: &[f32]) -> Image {
        let columns = self.sample_columns(samples);
        self.paint_image(&columns)
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

    fn paint_image(&self, columns: &[(f32, f32)]) -> Image {
        let mut buffer = SharedPixelBuffer::<Rgb8Pixel>::new(self.width, self.height);
        self.fill_background(buffer.make_mut_slice());
        self.draw_columns(columns, buffer.make_mut_slice());
        Image::from_rgb8(buffer)
    }

    fn fill_background(&self, pixels: &mut [Rgb8Pixel]) {
        for pixel in pixels {
            *pixel = self.background;
        }
    }

    fn draw_columns(&self, columns: &[(f32, f32)], pixels: &mut [Rgb8Pixel]) {
        let stride = self.width as usize;
        let mid = (self.height / 2) as f32;
        let limit = self.height.saturating_sub(1) as f32;

        for (x, (min, max)) in columns.iter().enumerate() {
            let top = (mid - max * (mid - 1.0)).clamp(0.0, limit) as u32;
            let bottom = (mid - min * (mid - 1.0)).clamp(0.0, limit) as u32;
            for y in top..=bottom {
                pixels[y as usize * stride + x] = self.foreground;
            }
        }
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
}
