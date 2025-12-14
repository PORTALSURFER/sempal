use super::{DecodedWaveform, WaveformDecodeError, WaveformPeaks, WaveformRenderer};
use hound::SampleFormat;
use rodio::{Decoder, Source};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

static NEXT_CACHE_TOKEN: AtomicU64 = AtomicU64::new(1);

impl WaveformRenderer {
    /// Decode wav bytes into samples and duration without rendering.
    pub fn decode_from_bytes(&self, bytes: &[u8]) -> Result<DecodedWaveform, WaveformDecodeError> {
        self.load_decoded(bytes)
    }

    const MAX_FULL_SAMPLE_FRAMES: usize = 2_500_000;

    fn load_decoded(&self, bytes: &[u8]) -> Result<DecodedWaveform, WaveformDecodeError> {
        let cache_token = NEXT_CACHE_TOKEN.fetch_add(1, Ordering::Relaxed);
        let mut reader = match hound::WavReader::new(std::io::Cursor::new(bytes)) {
            Ok(reader) => reader,
            Err(_) => return self.load_decoded_via_rodio(bytes, cache_token),
        };
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
                cache_token,
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
            cache_token,
            samples,
            peaks: None,
            duration_seconds,
            sample_rate: spec.sample_rate,
            channels: spec.channels,
        })
    }

    fn load_decoded_via_rodio(
        &self,
        bytes: &[u8],
        cache_token: u64,
    ) -> Result<DecodedWaveform, WaveformDecodeError> {
        let owned: Arc<[u8]> = Arc::from(bytes.to_vec());
        let byte_len = owned.len() as u64;
        let decoder = Decoder::builder()
            .with_data(std::io::Cursor::new(owned))
            .with_byte_len(byte_len)
            .with_seekable(false)
            .with_hint("wav")
            .build()
            .map_err(|error| WaveformDecodeError::Invalid {
                message: error.to_string(),
            })?;

        let sample_rate = decoder.sample_rate().max(1);
        let channels = decoder.channels().max(1);
        let duration_seconds = decoder
            .total_duration()
            .map(|duration| duration.as_secs_f32());
        let frames_estimate = duration_seconds
            .map(|secs| (secs * sample_rate as f32).round().max(0.0) as usize)
            .unwrap_or(0);

        if frames_estimate > Self::MAX_FULL_SAMPLE_FRAMES {
            return self.build_rodio_peaks(
                decoder,
                cache_token,
                sample_rate,
                channels,
                frames_estimate,
            );
        }

        let samples: Vec<f32> = decoder.collect();
        let frames = samples.len() / channels as usize;
        let duration_seconds = frames as f32 / sample_rate as f32;
        Ok(DecodedWaveform {
            cache_token,
            samples,
            peaks: None,
            duration_seconds,
            sample_rate,
            channels,
        })
    }

    fn build_rodio_peaks<I>(
        &self,
        mut samples: I,
        cache_token: u64,
        sample_rate: u32,
        channels: u16,
        frames_estimate: usize,
    ) -> Result<DecodedWaveform, WaveformDecodeError>
    where
        I: Iterator<Item = f32>,
    {
        let channels_usize = channels as usize;
        let bucket_size_frames = Self::peak_bucket_size(frames_estimate).max(1);
        let bucket_count_est = frames_estimate.div_ceil(bucket_size_frames).max(1);

        let mut mono = vec![(1.0_f32, -1.0_f32); bucket_count_est];
        let mut left = if channels_usize >= 2 {
            Some(vec![(1.0_f32, -1.0_f32); bucket_count_est])
        } else {
            None
        };
        let mut right = if channels_usize >= 2 {
            Some(vec![(1.0_f32, -1.0_f32); bucket_count_est])
        } else {
            None
        };

        let mut total_frames = 0usize;
        loop {
            let bucket = total_frames / bucket_size_frames;
            if bucket >= mono.len() {
                mono.push((1.0, -1.0));
                if let Some(left_peaks) = left.as_mut() {
                    left_peaks.push((1.0, -1.0));
                }
                if let Some(right_peaks) = right.as_mut() {
                    right_peaks.push((1.0, -1.0));
                }
            }
            let mut frame_min = 1.0_f32;
            let mut frame_max = -1.0_f32;
            for ch in 0..channels_usize {
                let Some(sample) = samples.next() else {
                    let duration_seconds = total_frames as f32 / sample_rate as f32;
                    let bucket_count = mono.len();
                    mono.truncate(bucket_count);
                    if let Some(left_peaks) = left.as_mut() {
                        left_peaks.truncate(bucket_count);
                    }
                    if let Some(right_peaks) = right.as_mut() {
                        right_peaks.truncate(bucket_count);
                    }
                    return Ok(DecodedWaveform {
                        cache_token,
                        samples: Vec::new(),
                        peaks: Some(WaveformPeaks {
                            total_frames,
                            channels,
                            bucket_size_frames,
                            mono,
                            left,
                            right,
                        }),
                        duration_seconds,
                        sample_rate,
                        channels,
                    });
                };
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
            total_frames = total_frames.saturating_add(1);
        }
    }

    fn read_float_samples(
        reader: &mut hound::WavReader<std::io::Cursor<&[u8]>>,
    ) -> Result<Vec<f32>, WaveformDecodeError> {
        let raw: Vec<f32> = reader
            .samples::<f32>()
            .map(|s| s.map_err(|source| WaveformDecodeError::Sample { source }))
            .collect::<Result<_, _>>()?;
        Ok(raw)
    }

    fn read_int_samples(
        reader: &mut hound::WavReader<std::io::Cursor<&[u8]>>,
        bits_per_sample: u16,
    ) -> Result<Vec<f32>, WaveformDecodeError> {
        let scale = (1i64 << bits_per_sample.saturating_sub(1)).max(1) as f32;
        let raw: Vec<f32> = reader
            .samples::<i32>()
            .map(|s| {
                s.map(|v| v as f32 / scale)
                    .map_err(|source| WaveformDecodeError::Sample { source })
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
    ) -> Result<WaveformPeaks, WaveformDecodeError> {
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
            .map(|s| s.map_err(|source| WaveformDecodeError::Sample { source }));
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
    ) -> Result<WaveformPeaks, WaveformDecodeError> {
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
            .map(|s| s.map_err(|source| WaveformDecodeError::Sample { source }));
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

    #[test]
    fn rodio_fallback_decodes_ill_formed_riff_size() {
        let renderer = WaveformRenderer::new(12, 12);
        let mut bytes = wav_bytes_int(16, 1, &[0, 1000, -1000, 0]);

        // Corrupt the redundant `nAvgBytesPerSec` field (byte rate) in the fmt chunk so that
        // `hound` rejects the file as ill-formed, while tolerant decoders still accept it.
        //
        // Layout for a basic PCM wav:
        // - RIFF header: 12 bytes
        // - fmt chunk header: 8 bytes ("fmt " + len)
        // - fmt chunk body starts with: u16 tag, u16 channels, u32 sample_rate, u32 byte_rate, ...
        let byte_rate_offset = 12 + 8 + 2 + 2 + 4;
        if bytes.len() >= byte_rate_offset + 4 {
            bytes[byte_rate_offset..byte_rate_offset + 4].copy_from_slice(&0u32.to_le_bytes());
        }

        assert!(
            hound::WavReader::new(std::io::Cursor::new(bytes.as_slice())).is_err(),
            "expected hound to reject the file"
        );

        let decoded = renderer
            .decode_from_bytes(&bytes)
            .expect("rodio fallback should decode");
        assert_eq!(decoded.channels, 1);
        assert_eq!(decoded.sample_rate, 48_000);
        assert!(!decoded.samples.is_empty());
        assert!(decoded.duration_seconds > 0.0);
    }
}
