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

pub(crate) fn probe_duration_seconds(path: &Path) -> Result<Option<f32>, String> {
    if path
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("wav"))
    {
        let reader = hound::WavReader::open(path)
            .map_err(|err| format!("WAV probe failed for {}: {err}", path.display()))?;
        let spec = reader.spec();
        let sample_rate = spec.sample_rate.max(1) as f32;
        let channels = spec.channels.max(1) as f32;
        let samples = reader.duration() as f32;
        return Ok(Some((samples / channels / sample_rate).max(0.0)));
    }

    let file = File::open(path).map_err(|err| format!("Failed to open {}: {err}", path.display()))?;
    let byte_len = file.metadata().map(|meta| meta.len()).unwrap_or(0) as u64;
    let hint = path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(str::to_ascii_lowercase);
    let mut builder = Decoder::builder()
        .with_data(BufReader::new(file))
        .with_byte_len(byte_len)
        .with_seekable(false);
    if let Some(hint) = hint.as_deref() {
        builder = builder.with_hint(hint);
    }
    let decoder = builder
        .build()
        .map_err(|err| format!("Audio decode probe failed for {}: {err}", path.display()))?;
    Ok(decoder.total_duration().map(|dur| dur.as_secs_f32()))
}

fn decode_for_analysis_with_rate(path: &Path, sample_rate: u32) -> Result<AnalysisAudio, String> {
    let decoded = decode_to_interleaved_f32(path)?;
    let mono = downmix_to_mono(&decoded.samples, decoded.channels);
    let mut resampled = resample_linear(&mono, decoded.sample_rate, sample_rate);
    resampled = trim_silence_with_hysteresis(&resampled, sample_rate);
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

fn trim_silence_with_hysteresis(samples: &[f32], sample_rate: u32) -> Vec<f32> {
    if samples.is_empty() || sample_rate == 0 {
        return samples.to_vec();
    }
    let window_size = (sample_rate as f32 * 0.02).round().max(1.0) as usize; // 20ms
    let hop = window_size;
    if samples.len() <= window_size {
        return samples.to_vec();
    }

    let threshold_on = db_to_linear(-45.0);
    let threshold_off = db_to_linear(-55.0);
    let pre_roll = (sample_rate as f32 * 0.01).round().max(0.0) as usize; // 10ms
    let post_roll = (sample_rate as f32 * 0.005).round().max(0.0) as usize; // 5ms

    let mut active_start: Option<usize> = None;
    let mut active_end: Option<usize> = None;

    let mut active = false;
    let mut window_start = 0usize;
    while window_start < samples.len() {
        let window_end = (window_start + window_size).min(samples.len());
        let rms = rms(&samples[window_start..window_end]);
        if !active {
            if rms >= threshold_on {
                active = true;
                active_start = Some(window_start);
                active_end = Some(window_end);
            }
        } else {
            if rms >= threshold_off {
                active_end = Some(window_end);
            } else {
                active = false;
            }
        }
        window_start = window_start.saturating_add(hop);
    }

    let Some(active_start) = active_start else {
        return samples.to_vec();
    };
    let Some(active_end) = active_end else {
        return samples.to_vec();
    };

    let trimmed_start = active_start.saturating_sub(pre_roll).min(samples.len());
    let trimmed_end = (active_end + post_roll)
        .max(trimmed_start.saturating_add(1))
        .min(samples.len());
    samples[trimmed_start..trimmed_end].to_vec()
}

fn rms(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let mut sum = 0.0_f64;
    for &sample in samples {
        let sample = sanitize_sample(sample) as f64;
        sum += sample * sample;
    }
    let mean = sum / samples.len() as f64;
    (mean.max(0.0).sqrt() as f32).min(1.0)
}

fn db_to_linear(db: f32) -> f32 {
    10.0_f32.powf(db / 20.0)
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
    fn wav_probe_reads_duration_without_full_decode() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("probe.wav");
        let spec = WavSpec {
            channels: 1,
            sample_rate: 48_000,
            bits_per_sample: 16,
            sample_format: SampleFormat::Int,
        };
        let mut writer = WavWriter::create(&path, spec).unwrap();
        for _ in 0..48_000 {
            writer.write_sample::<i16>(0).unwrap();
        }
        writer.finalize().unwrap();
        let duration = probe_duration_seconds(&path).unwrap().unwrap();
        assert!((duration - 1.0).abs() < 1e-3);
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

    #[test]
    fn decode_for_analysis_trims_leading_and_trailing_silence() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("trim.wav");
        let sample_rate = ANALYSIS_SAMPLE_RATE;
        let spec = WavSpec {
            channels: 1,
            sample_rate,
            bits_per_sample: 32,
            sample_format: SampleFormat::Float,
        };
        let mut writer = WavWriter::create(&path, spec).unwrap();
        let silence_frames = (0.1 * sample_rate as f32).round() as usize;
        let tone_frames = (0.1 * sample_rate as f32).round() as usize;
        let tail_silence_frames = (0.2 * sample_rate as f32).round() as usize;
        for _ in 0..silence_frames {
            writer.write_sample::<f32>(0.0).unwrap();
        }
        for _ in 0..tone_frames {
            writer.write_sample::<f32>(0.25).unwrap();
        }
        for _ in 0..tail_silence_frames {
            writer.write_sample::<f32>(0.0).unwrap();
        }
        writer.finalize().unwrap();

        let decoded = decode_for_analysis(&path).unwrap();
        assert!(decoded.duration_seconds < 0.25);
        assert!(decoded.duration_seconds > 0.08);
    }

    #[test]
    fn quiet_samples_are_not_trimmed_to_empty() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("quiet.wav");
        let sample_rate = ANALYSIS_SAMPLE_RATE;
        let spec = WavSpec {
            channels: 1,
            sample_rate,
            bits_per_sample: 32,
            sample_format: SampleFormat::Float,
        };
        let mut writer = WavWriter::create(&path, spec).unwrap();
        let frames = (0.1 * sample_rate as f32).round() as usize;
        for _ in 0..frames {
            writer.write_sample::<f32>(0.001).unwrap();
        }
        writer.finalize().unwrap();

        let decoded = decode_for_analysis(&path).unwrap();
        assert!(!decoded.mono.is_empty());
        let peak = decoded
            .mono
            .iter()
            .copied()
            .map(|v| v.abs())
            .fold(0.0, f32::max);
        assert!(peak > 0.5);
    }
}
