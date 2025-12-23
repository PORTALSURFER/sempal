use crate::waveform::{DecodedWaveform, WaveformDecodeError, WaveformRenderer};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_CACHE_TOKEN: AtomicU64 = AtomicU64::new(1);

impl WaveformRenderer {
    pub(super) const MAX_FULL_SAMPLE_FRAMES: usize = 2_500_000;

    pub(super) fn load_decoded(&self, bytes: &[u8]) -> Result<DecodedWaveform, WaveformDecodeError> {
        self.load_decoded_with_limit(bytes, Self::MAX_FULL_SAMPLE_FRAMES)
    }

    fn load_decoded_with_limit(
        &self,
        bytes: &[u8],
        max_frames: usize,
    ) -> Result<DecodedWaveform, WaveformDecodeError> {
        let cache_token = NEXT_CACHE_TOKEN.fetch_add(1, Ordering::Relaxed);
        if let Some(decoded) = self.load_decoded_wav(bytes, cache_token, max_frames)? {
            return Ok(decoded);
        }
        self.load_decoded_via_rodio(bytes, cache_token, max_frames)
    }

    #[cfg(test)]
    pub(crate) fn load_decoded_with_max_frames(
        &self,
        bytes: &[u8],
        max_frames: usize,
    ) -> Result<DecodedWaveform, WaveformDecodeError> {
        self.load_decoded_with_limit(bytes, max_frames)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::{rodio_reader, wav_reader};
    use hound::SampleFormat;

    fn wav_bytes_i16(channels: u16, samples: &[i16]) -> Vec<u8> {
        let spec = hound::WavSpec {
            channels,
            sample_rate: 8_000,
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
    fn decode_reports_invalid_data_errors() {
        let renderer = WaveformRenderer::new(12, 12);
        let bytes = vec![0, 1, 2, 3, 4, 5];
        let err = renderer.decode_from_bytes(&bytes).unwrap_err();
        assert!(matches!(err, WaveformDecodeError::Invalid { .. }));
    }

    #[test]
    fn decode_prefers_wav_reader_when_valid() {
        wav_reader::reset_wav_decode_count();
        rodio_reader::reset_rodio_decode_count();

        let renderer = WaveformRenderer::new(12, 12);
        let bytes = wav_bytes_i16(1, &[0, 1000, -1000, 0]);
        let decoded = renderer
            .load_decoded(&bytes)
            .expect("decode wav via hound");

        assert!(decoded.peaks.is_none());
        assert!(decoded.samples.len() > 0);
        assert_eq!(wav_reader::wav_decode_count(), 1);
        assert_eq!(rodio_reader::rodio_decode_count(), 0);
    }

    #[test]
    fn decode_falls_back_to_rodio_when_wav_invalid() {
        wav_reader::reset_wav_decode_count();
        rodio_reader::reset_rodio_decode_count();

        let renderer = WaveformRenderer::new(12, 12);
        let mut bytes = wav_bytes_i16(1, &[0, 1000, -1000, 0]);

        let byte_rate_offset = 12 + 8 + 2 + 2 + 4;
        if bytes.len() >= byte_rate_offset + 4 {
            bytes[byte_rate_offset..byte_rate_offset + 4].copy_from_slice(&0u32.to_le_bytes());
        }

        let decoded = renderer
            .load_decoded(&bytes)
            .expect("decode via rodio fallback");

        assert!(decoded.samples.len() > 0);
        assert_eq!(wav_reader::wav_decode_count(), 0);
        assert_eq!(rodio_reader::rodio_decode_count(), 1);
    }

    #[test]
    fn peak_only_branch_preserves_duration_and_frames() {
        let renderer = WaveformRenderer::new(12, 12);
        let samples = vec![0_i16; 64];
        let bytes = wav_bytes_i16(1, &samples);

        let full = renderer
            .load_decoded(&bytes)
            .expect("decode full samples");
        let peaks_only = renderer
            .load_decoded_with_max_frames(&bytes, 1)
            .expect("decode peaks only");

        assert!(peaks_only.samples.is_empty());
        assert!(peaks_only.peaks.is_some());
        assert_eq!(full.frame_count(), peaks_only.frame_count());
        assert!((full.duration_seconds - peaks_only.duration_seconds).abs() < 1e-6);
    }
}
