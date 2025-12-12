use super::{DecodedWaveform, WaveformRenderer};
use hound::SampleFormat;

impl WaveformRenderer {
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
}
