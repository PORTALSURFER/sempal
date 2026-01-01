use super::normalize::clamp_sample;
use super::peaks;
use crate::waveform::{DecodedWaveform, WaveformDecodeError, WaveformPeaks, WaveformRenderer};
use rodio::{Decoder, Source};
use std::sync::Arc;
#[cfg(test)]
use std::sync::atomic::{AtomicUsize, Ordering};

#[cfg(test)]
static RODIO_DECODE_COUNT: AtomicUsize = AtomicUsize::new(0);

impl WaveformRenderer {
    pub(super) fn load_decoded_via_rodio(
        &self,
        bytes: &[u8],
        cache_token: u64,
        max_frames: usize,
    ) -> Result<DecodedWaveform, WaveformDecodeError> {
        #[cfg(test)]
        RODIO_DECODE_COUNT.fetch_add(1, Ordering::Relaxed);

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

        if frames_estimate > max_frames {
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
            samples: Arc::from(samples),
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
        let bucket_size_frames = peaks::peak_bucket_size(frames_estimate).max(1);
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
            let mut frame_count = 0usize;
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
                        samples: Arc::from(Vec::new()),
                        peaks: Some(Arc::new(WaveformPeaks {
                            total_frames,
                            channels,
                            bucket_size_frames,
                            mono,
                            left,
                            right,
                        })),
                        duration_seconds,
                        sample_rate,
                        channels,
                    });
                };
                let sample = clamp_sample(sample);
                frame_min = frame_min.min(sample);
                frame_max = frame_max.max(sample);
                frame_count = frame_count.saturating_add(1);
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
            if frame_count == 0 {
                *min = (*min).min(0.0);
                *max = (*max).max(0.0);
            } else {
                *min = (*min).min(frame_min);
                *max = (*max).max(frame_max);
            }
            total_frames = total_frames.saturating_add(1);
        }
    }
}

#[cfg(test)]
pub(super) fn reset_rodio_decode_count() {
    RODIO_DECODE_COUNT.store(0, Ordering::Relaxed);
}

#[cfg(test)]
pub(super) fn rodio_decode_count() -> usize {
    RODIO_DECODE_COUNT.load(Ordering::Relaxed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use hound::SampleFormat;

    fn wav_bytes_int(bits_per_sample: u16, channels: u16, samples: &[i32]) -> Vec<u8> {
        let spec = hound::WavSpec {
            channels,
            sample_rate: 48_000,
            bits_per_sample,
            sample_format: SampleFormat::Int,
        };
        let mut cursor = std::io::Cursor::new(Vec::new());
        {
            let mut writer = hound::WavWriter::new(&mut cursor, spec).expect("create wav writer");
            for &sample in samples {
                writer.write_sample(sample).expect("write sample");
            }
            writer.finalize().expect("finalize wav");
        }
        cursor.into_inner()
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
