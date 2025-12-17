use rodio::{Decoder, Source};
use std::fs::File;
use std::io::BufReader;
use std::path::Path;

/// Fixed sample rate used during analysis.
pub(crate) const ANALYSIS_SAMPLE_RATE: u32 = 22_050;

/// Decoded mono audio ready for analysis.
#[derive(Debug)]
#[allow(dead_code)]
pub(crate) struct AnalysisAudio {
    pub(crate) mono: Vec<f32>,
    pub(crate) duration_seconds: f32,
    pub(crate) sample_rate_used: u32,
}

pub(crate) fn decode_for_analysis(path: &Path) -> Result<AnalysisAudio, String> {
    decode_for_analysis_with_rate(path, ANALYSIS_SAMPLE_RATE)
}

fn decode_for_analysis_with_rate(path: &Path, sample_rate: u32) -> Result<AnalysisAudio, String> {
    let decoded = decode_to_interleaved_f32(path)?;
    let mono = downmix_to_mono(&decoded.samples, decoded.channels);
    let mut resampled = resample_linear(&mono, decoded.sample_rate, sample_rate);
    normalize_peak_in_place(&mut resampled);
    let duration_seconds = duration_seconds(resampled.len(), sample_rate);
    Ok(AnalysisAudio {
        mono: resampled,
        duration_seconds,
        sample_rate_used: sample_rate,
    })
}

struct DecodedInterleaved {
    samples: Vec<f32>,
    sample_rate: u32,
    channels: u16,
}

fn decode_to_interleaved_f32(path: &Path) -> Result<DecodedInterleaved, String> {
    let file = File::open(path).map_err(|err| format!("Failed to open {}: {err}", path.display()))?;
    let byte_len = file
        .metadata()
        .map(|meta| meta.len())
        .unwrap_or(0) as u64;
    let hint = path.extension().and_then(|ext| ext.to_str()).map(str::to_ascii_lowercase);
    let mut builder = Decoder::builder()
        .with_data(BufReader::new(file))
        .with_byte_len(byte_len)
        .with_seekable(false);
    if let Some(hint) = hint.as_deref() {
        builder = builder.with_hint(hint);
    }
    let decoder = builder
        .build()
        .map_err(|err| format!("Audio decode failed for {}: {err}", path.display()))?;
    let sample_rate = decoder.sample_rate().max(1);
    let channels = decoder.channels().max(1);
    let samples: Vec<f32> = decoder.collect();
    Ok(DecodedInterleaved {
        samples,
        sample_rate,
        channels,
    })
}

fn downmix_to_mono(samples: &[f32], channels: u16) -> Vec<f32> {
    let channels = channels.max(1) as usize;
    if channels == 1 {
        return samples.iter().copied().map(sanitize_sample).collect();
    }
    let frames = samples.len() / channels;
    let mut mono = Vec::with_capacity(frames);
    for frame in 0..frames {
        let start = frame * channels;
        let end = start + channels;
        let slice = &samples[start..end.min(samples.len())];
        let mut sum = 0.0_f32;
        for &sample in slice {
            sum += sanitize_sample(sample);
        }
        mono.push(sum / channels as f32);
    }
    mono
}

fn resample_linear(samples: &[f32], input_rate: u32, output_rate: u32) -> Vec<f32> {
    let input_rate = input_rate.max(1);
    let output_rate = output_rate.max(1);
    if samples.is_empty() || input_rate == output_rate {
        return samples.to_vec();
    }
    let duration_seconds = samples.len() as f64 / input_rate as f64;
    let out_len = (duration_seconds * output_rate as f64).round().max(1.0) as usize;
    let mut out = Vec::with_capacity(out_len);
    for i in 0..out_len {
        let t = i as f64 / output_rate as f64;
        let pos = t * input_rate as f64;
        out.push(lerp_sample(samples, pos));
    }
    out
}

fn lerp_sample(samples: &[f32], pos: f64) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let idx0 = pos.floor().max(0.0) as usize;
    let frac = (pos - idx0 as f64).clamp(0.0, 1.0) as f32;
    let idx1 = idx0.saturating_add(1).min(samples.len().saturating_sub(1));
    let a = samples.get(idx0).copied().unwrap_or(0.0);
    let b = samples.get(idx1).copied().unwrap_or(a);
    a + (b - a) * frac
}

fn normalize_peak_in_place(samples: &mut [f32]) {
    let mut peak = 0.0_f32;
    for &sample in samples.iter() {
        peak = peak.max(sample.abs());
    }
    if !peak.is_finite() || peak <= 0.0 {
        return;
    }
    let gain = 1.0_f32 / peak;
    for sample in samples.iter_mut() {
        *sample = (*sample * gain).clamp(-1.0, 1.0);
    }
}

fn sanitize_sample(sample: f32) -> f32 {
    if sample.is_finite() {
        sample.clamp(-1.0, 1.0)
    } else {
        0.0
    }
}

fn duration_seconds(sample_count: usize, sample_rate: u32) -> f32 {
    if sample_rate == 0 {
        return 0.0;
    }
    sample_count as f32 / sample_rate as f32
}

#[cfg(test)]
mod tests {
    use super::*;
    use hound::{SampleFormat, WavSpec, WavWriter};
    use tempfile::TempDir;

    #[test]
    fn downmix_averages_channels() {
        let stereo = vec![1.0_f32, -1.0, 0.5, 0.25];
        let mono = downmix_to_mono(&stereo, 2);
        assert_eq!(mono.len(), 2);
        assert!((mono[0] - 0.0).abs() < 1e-6);
        assert!((mono[1] - 0.375).abs() < 1e-6);
    }

    #[test]
    fn resample_linear_preserves_endpoints_for_ramp() {
        let input = vec![0.0_f32, 1.0];
        let out = resample_linear(&input, 1, 2);
        assert_eq!(out.len(), 4);
        assert!((out[0] - 0.0).abs() < 1e-6);
        assert!((out[out.len() - 1] - 1.0).abs() < 1e-6);
    }

    #[test]
    fn normalize_peak_scales_to_unit_peak() {
        let mut samples = vec![0.25_f32, -0.5, 0.125];
        normalize_peak_in_place(&mut samples);
        let peak = samples.iter().copied().map(|v| v.abs()).fold(0.0, f32::max);
        assert!((peak - 1.0).abs() < 1e-6);
    }

    #[test]
    fn decode_for_analysis_decodes_wav_to_target_rate() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("fixture.wav");
        let spec = WavSpec {
            channels: 2,
            sample_rate: 44_100,
            bits_per_sample: 32,
            sample_format: SampleFormat::Float,
        };
        let mut writer = WavWriter::create(&path, spec).unwrap();
        for _ in 0..(44_100 / 10) {
            writer.write_sample::<f32>(0.25).unwrap();
            writer.write_sample::<f32>(0.25).unwrap();
        }
        writer.finalize().unwrap();

        let decoded = decode_for_analysis(&path).unwrap();
        assert_eq!(decoded.sample_rate_used, ANALYSIS_SAMPLE_RATE);
        assert!((decoded.duration_seconds - 0.1).abs() < 0.02);
        let peak = decoded
            .mono
            .iter()
            .copied()
            .map(|v| v.abs())
            .fold(0.0, f32::max);
        assert!((peak - 1.0).abs() < 1e-6);
    }
}
