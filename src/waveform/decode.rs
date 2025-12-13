use super::{DecodedWaveform, WaveformPeaks, WaveformRenderer};
use hound::SampleFormat;

impl WaveformRenderer {
    /// Decode wav bytes into samples and duration without rendering.
    pub fn decode_from_bytes(&self, bytes: &[u8]) -> Result<DecodedWaveform, String> {
        self.load_decoded(bytes)
    }

    const MAX_FULL_SAMPLE_FRAMES: usize = 2_500_000;

    fn load_decoded(&self, bytes: &[u8]) -> Result<DecodedWaveform, String> {
        let mut reader = hound::WavReader::new(std::io::Cursor::new(bytes))
            .map_err(|error| format!("Invalid wav: {error}"))?;
        let spec = reader.spec();
        let channels = spec.channels.max(1) as usize;
        let frames = reader.duration() as usize;
        let duration_seconds = frames as f32 / spec.sample_rate.max(1) as f32;

        if frames > Self::MAX_FULL_SAMPLE_FRAMES {
            let peaks = match spec.sample_format {
                SampleFormat::Float => Self::build_peaks_from_float(&mut reader, channels)?,
                SampleFormat::Int => {
                    Self::build_peaks_from_int(&mut reader, channels, spec.bits_per_sample)?
                }
            };
            return Ok(DecodedWaveform {
                samples: Vec::new(),
                peaks: Some(peaks),
                duration_seconds,
                sample_rate: spec.sample_rate,
                channels: spec.channels,
            });
        }

        let samples = match spec.sample_format {
            SampleFormat::Float => Self::read_float_samples(&mut reader)?,
            SampleFormat::Int => Self::read_int_samples(&mut reader, spec.bits_per_sample)?,
        };

        Ok(DecodedWaveform {
            samples,
            peaks: None,
            duration_seconds,
            sample_rate: spec.sample_rate,
            channels: spec.channels,
        })
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

    fn peak_bucket_size(frames: usize) -> usize {
        if frames >= 60_000_000 {
            8_192
        } else if frames >= 10_000_000 {
            4_096
        } else {
            2_048
        }
    }

    fn build_peaks_from_float(
        reader: &mut hound::WavReader<std::io::Cursor<&[u8]>>,
        channels: usize,
    ) -> Result<WaveformPeaks, String> {
        let total_frames = reader.duration() as usize;
        let bucket_size_frames = Self::peak_bucket_size(total_frames).max(1);
        let bucket_count = total_frames.div_ceil(bucket_size_frames).max(1);

        let mut mono = vec![(1.0_f32, -1.0_f32); bucket_count];
        let mut left = if channels >= 2 {
            Some(vec![(1.0_f32, -1.0_f32); bucket_count])
        } else {
            None
        };
        let mut right = if channels >= 2 {
            Some(vec![(1.0_f32, -1.0_f32); bucket_count])
        } else {
            None
        };

        let mut iter = reader
            .samples::<f32>()
            .map(|s| s.map_err(|error| format!("Sample error: {error}")));
        for frame in 0..total_frames {
            let bucket = frame / bucket_size_frames;
            let mut frame_min = 1.0_f32;
            let mut frame_max = -1.0_f32;
            for ch in 0..channels {
                let sample = iter
                    .next()
                    .transpose()?
                    .unwrap_or(0.0)
                    .clamp(-1.0, 1.0);
                frame_min = frame_min.min(sample);
                frame_max = frame_max.max(sample);
                if ch == 0 {
                    if let Some(left_peaks) = left.as_mut() {
                        let (min, max) = &mut left_peaks[bucket];
                        *min = (*min).min(sample);
                        *max = (*max).max(sample);
                    }
                } else if ch == 1 {
                    if let Some(right_peaks) = right.as_mut() {
                        let (min, max) = &mut right_peaks[bucket];
                        *min = (*min).min(sample);
                        *max = (*max).max(sample);
                    }
                }
            }
            let (min, max) = &mut mono[bucket];
            *min = (*min).min(frame_min);
            *max = (*max).max(frame_max);
        }

        Ok(WaveformPeaks {
            total_frames,
            channels: channels.min(u16::MAX as usize) as u16,
            bucket_size_frames,
            mono,
            left,
            right,
        })
    }

    fn build_peaks_from_int(
        reader: &mut hound::WavReader<std::io::Cursor<&[u8]>>,
        channels: usize,
        bits_per_sample: u16,
    ) -> Result<WaveformPeaks, String> {
        let scale = (1i64 << bits_per_sample.saturating_sub(1)).max(1) as f32;
        let total_frames = reader.duration() as usize;
        let bucket_size_frames = Self::peak_bucket_size(total_frames).max(1);
        let bucket_count = total_frames.div_ceil(bucket_size_frames).max(1);

        let mut mono = vec![(1.0_f32, -1.0_f32); bucket_count];
        let mut left = if channels >= 2 {
            Some(vec![(1.0_f32, -1.0_f32); bucket_count])
        } else {
            None
        };
        let mut right = if channels >= 2 {
            Some(vec![(1.0_f32, -1.0_f32); bucket_count])
        } else {
            None
        };

        let mut iter = reader
            .samples::<i32>()
            .map(|s| s.map_err(|error| format!("Sample error: {error}")));
        for frame in 0..total_frames {
            let bucket = frame / bucket_size_frames;
            let mut frame_min = 1.0_f32;
            let mut frame_max = -1.0_f32;
            for ch in 0..channels {
                let sample = iter
                    .next()
                    .transpose()?
                    .unwrap_or(0)
                    as f32
                    / scale;
                let sample = sample.clamp(-1.0, 1.0);
                frame_min = frame_min.min(sample);
                frame_max = frame_max.max(sample);
                if ch == 0 {
                    if let Some(left_peaks) = left.as_mut() {
                        let (min, max) = &mut left_peaks[bucket];
                        *min = (*min).min(sample);
                        *max = (*max).max(sample);
                    }
                } else if ch == 1 {
                    if let Some(right_peaks) = right.as_mut() {
                        let (min, max) = &mut right_peaks[bucket];
                        *min = (*min).min(sample);
                        *max = (*max).max(sample);
                    }
                }
            }
            let (min, max) = &mut mono[bucket];
            *min = (*min).min(frame_min);
            *max = (*max).max(frame_max);
        }

        Ok(WaveformPeaks {
            total_frames,
            channels: channels.min(u16::MAX as usize) as u16,
            bucket_size_frames,
            mono,
            left,
            right,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wav_bytes_int(bits_per_sample: u16, channels: u16, samples: &[i32]) -> Vec<u8> {
        let spec = hound::WavSpec {
            channels,
            sample_rate: 48_000,
            bits_per_sample,
            sample_format: SampleFormat::Int,
        };
        let mut cursor = std::io::Cursor::new(Vec::new());
        {
            let mut writer =
                hound::WavWriter::new(&mut cursor, spec).expect("create wav writer");
            for &sample in samples {
                writer.write_sample(sample).expect("write sample");
            }
            writer.finalize().expect("finalize wav");
        }
        cursor.into_inner()
    }

    fn wav_bytes_i16(channels: u16, samples: &[i16]) -> Vec<u8> {
        let spec = hound::WavSpec {
            channels,
            sample_rate: 48_000,
            bits_per_sample: 16,
            sample_format: SampleFormat::Int,
        };
        let mut cursor = std::io::Cursor::new(Vec::new());
        {
            let mut writer =
                hound::WavWriter::new(&mut cursor, spec).expect("create wav writer");
            for &sample in samples {
                writer.write_sample(sample).expect("write sample");
            }
            writer.finalize().expect("finalize wav");
        }
        cursor.into_inner()
    }

    #[test]
    fn decodes_24bit_int_scaling() {
        let bits = 24;
        let scale = (1i64 << (bits - 1)) as f32;
        let max_pos = (scale as i32) - 1;
        let min_neg = -(scale as i32);
        let bytes = wav_bytes_int(bits, 1, &[0, max_pos, min_neg, 1, -1]);

        let renderer = WaveformRenderer::new(1, 1);
        let decoded = renderer
            .decode_from_bytes(&bytes)
            .expect("decode 24-bit wav");

        let expected = vec![
            0.0,
            max_pos as f32 / scale,
            min_neg as f32 / scale,
            1.0 / scale,
            -1.0 / scale,
        ];
        assert_eq!(decoded.samples.len(), expected.len());
        for (got, exp) in decoded.samples.iter().zip(expected) {
            assert!((got - exp).abs() < 1e-6, "got {got}, expected {exp}");
        }
    }

    #[test]
    fn decodes_16bit_int_scaling_and_interleaving() {
        let scale = (1i64 << 15) as f32;
        let max_pos = i16::MAX;
        let min_neg = i16::MIN;
        let bytes = wav_bytes_i16(2, &[0, max_pos, min_neg, 1, -1, 0]);

        let renderer = WaveformRenderer::new(1, 1);
        let decoded = renderer
            .decode_from_bytes(&bytes)
            .expect("decode 16-bit wav");

        let expected = vec![
            0.0,
            max_pos as f32 / scale,
            min_neg as f32 / scale,
            1.0 / scale,
            -1.0 / scale,
            0.0,
        ];
        assert_eq!(decoded.samples.len(), expected.len());
        for (got, exp) in decoded.samples.iter().zip(expected) {
            assert!((got - exp).abs() < 1e-6, "got {got}, expected {exp}");
        }
    }

    #[test]
    fn decodes_32bit_int_scaling() {
        let bits = 32;
        let scale = (1i64 << 31) as f32;
        let max_pos = i32::MAX;
        let min_neg = i32::MIN;
        let bytes = wav_bytes_int(bits, 1, &[0, max_pos, min_neg, 1, -1]);

        let renderer = WaveformRenderer::new(1, 1);
        let decoded = renderer
            .decode_from_bytes(&bytes)
            .expect("decode 32-bit wav");

        let expected = vec![
            0.0,
            max_pos as f32 / scale,
            min_neg as f32 / scale,
            1.0 / scale,
            -1.0 / scale,
        ];
        assert_eq!(decoded.samples.len(), expected.len());
        for (got, exp) in decoded.samples.iter().zip(expected) {
            assert!((got - exp).abs() < 1e-6, "got {got}, expected {exp}");
        }
    }
}
