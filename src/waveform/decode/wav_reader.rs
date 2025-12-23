use super::peaks;
use crate::waveform::{DecodedWaveform, WaveformDecodeError, WaveformRenderer};
use hound::SampleFormat;
use std::sync::Arc;
#[cfg(test)]
use std::sync::atomic::{AtomicUsize, Ordering};

#[cfg(test)]
static WAV_DECODE_COUNT: AtomicUsize = AtomicUsize::new(0);

impl WaveformRenderer {
    pub(super) fn load_decoded_wav(
        &self,
        bytes: &[u8],
        cache_token: u64,
        max_frames: usize,
    ) -> Result<Option<DecodedWaveform>, WaveformDecodeError> {
        let mut reader = match hound::WavReader::new(std::io::Cursor::new(bytes)) {
            Ok(reader) => reader,
            Err(_) => return Ok(None),
        };
        #[cfg(test)]
        WAV_DECODE_COUNT.fetch_add(1, Ordering::Relaxed);

        let spec = reader.spec();
        let channels = spec.channels.max(1) as usize;
        let frames = reader.duration() as usize;
        let duration_seconds = frames as f32 / spec.sample_rate.max(1) as f32;

        if frames > max_frames {
            let peaks = match spec.sample_format {
                SampleFormat::Float => peaks::build_peaks_from_float(&mut reader, channels)?,
                SampleFormat::Int => {
                    peaks::build_peaks_from_int(&mut reader, channels, spec.bits_per_sample)?
                }
            };
            return Ok(Some(DecodedWaveform {
                cache_token,
                samples: Arc::from(Vec::new()),
                peaks: Some(Arc::new(peaks)),
                duration_seconds,
                sample_rate: spec.sample_rate,
                channels: spec.channels,
            }));
        }

        let samples = match spec.sample_format {
            SampleFormat::Float => read_float_samples(&mut reader)?,
            SampleFormat::Int => read_int_samples(&mut reader, spec.bits_per_sample)?,
        };

        Ok(Some(DecodedWaveform {
            cache_token,
            samples: Arc::from(samples),
            peaks: None,
            duration_seconds,
            sample_rate: spec.sample_rate,
            channels: spec.channels,
        }))
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

#[cfg(test)]
pub(super) fn reset_wav_decode_count() {
    WAV_DECODE_COUNT.store(0, Ordering::Relaxed);
}

#[cfg(test)]
pub(super) fn wav_decode_count() -> usize {
    WAV_DECODE_COUNT.load(Ordering::Relaxed)
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
            let mut writer = hound::WavWriter::new(&mut cursor, spec).expect("create wav writer");
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
            let mut writer = hound::WavWriter::new(&mut cursor, spec).expect("create wav writer");
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
