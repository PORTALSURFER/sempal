use super::normalize::{normalize_peak_in_place, rms};
use super::silence::trim_silence_with_hysteresis;
use super::{
    AnalysisAudio, MAX_ANALYSIS_SECONDS, MIN_ANALYSIS_SECONDS, WINDOW_HOP_SECONDS, WINDOW_SECONDS,
};

pub(crate) fn prepare_mono_for_analysis(samples: Vec<f32>, sample_rate: u32) -> AnalysisAudio {
    prepare_mono_for_analysis_from_slice(&samples, sample_rate)
}

pub(crate) fn prepare_mono_for_analysis_from_slice(
    samples: &[f32],
    sample_rate: u32,
) -> AnalysisAudio {
    let mut processed = trim_silence_with_hysteresis(samples, sample_rate);
    processed = apply_energy_windowing(&processed, sample_rate);
    pad_to_min_duration(&mut processed, sample_rate);
    normalize_peak_in_place(&mut processed);
    let duration_seconds = duration_seconds(processed.len(), sample_rate);
    AnalysisAudio {
        mono: processed,
        duration_seconds,
        sample_rate_used: sample_rate,
    }
}

pub(crate) fn downmix_to_mono_into(out: &mut Vec<f32>, samples: &[f32], channels: u16) {
    let channels = channels.max(1) as usize;
    if channels == 1 {
        out.clear();
        out.reserve(samples.len().saturating_sub(out.capacity()));
        for sample in samples.iter().copied() {
            out.push(sanitize_sample(sample));
        }
        return;
    }
    let frames = samples.len() / channels;
    out.clear();
    out.reserve(frames.saturating_sub(out.capacity()));
    for frame in 0..frames {
        let start = frame * channels;
        let end = start + channels;
        let slice = &samples[start..end.min(samples.len())];
        let mut sum = 0.0_f32;
        for &sample in slice {
            sum += sanitize_sample(sample);
        }
        out.push(sum / channels as f32);
    }
}

fn downmix_to_mono(samples: &[f32], channels: u16) -> Vec<f32> {
    let mut out = Vec::new();
    downmix_to_mono_into(&mut out, samples, channels);
    out
}

fn sanitize_sample(sample: f32) -> f32 {
    if !sample.is_finite() {
        return 0.0;
    }
    let clamped = sample.clamp(-1.0, 1.0);
    if clamped != 0.0 && clamped.abs() < f32::MIN_POSITIVE {
        0.0
    } else {
        clamped
    }
}

fn duration_seconds(sample_count: usize, sample_rate: u32) -> f32 {
    if sample_rate == 0 {
        return 0.0;
    }
    sample_count as f32 / sample_rate as f32
}

fn apply_energy_windowing(samples: &[f32], sample_rate: u32) -> Vec<f32> {
    if samples.is_empty() || sample_rate == 0 {
        return samples.to_vec();
    }
    let max_len = (MAX_ANALYSIS_SECONDS * sample_rate as f32).round() as usize;
    if samples.len() <= max_len || max_len == 0 {
        return samples.to_vec();
    }

    let window_len = (WINDOW_SECONDS * sample_rate as f32).round() as usize;
    let hop_len = (WINDOW_HOP_SECONDS * sample_rate as f32).round() as usize;
    if window_len == 0 || hop_len == 0 || window_len > samples.len() {
        return samples.to_vec();
    }

    let mut windows: Vec<(f32, usize)> = Vec::new();
    let mut start = 0usize;
    while start + window_len <= samples.len() {
        let end = start + window_len;
        let energy = rms(&samples[start..end]);
        windows.push((energy, start));
        start = start.saturating_add(hop_len);
    }
    windows.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));

    let target_windows = (MAX_ANALYSIS_SECONDS / WINDOW_SECONDS).floor().max(1.0) as usize;
    let mut selected: Vec<usize> = Vec::new();
    for (_, start) in windows {
        if selected.len() >= target_windows {
            break;
        }
        let overlaps = selected.iter().any(|&s| {
            let a0 = s;
            let a1 = s.saturating_add(window_len);
            let b0 = start;
            let b1 = start.saturating_add(window_len);
            a0 < b1 && b0 < a1
        });
        if !overlaps {
            selected.push(start);
        }
    }

    if selected.len() < target_windows {
        let candidates = [
            0usize,
            samples.len().saturating_sub(window_len) / 2,
            samples.len().saturating_sub(window_len),
        ];
        for &start in &candidates {
            if selected.len() >= target_windows {
                break;
            }
            let overlaps = selected.iter().any(|&s| {
                let a0 = s;
                let a1 = s.saturating_add(window_len);
                let b0 = start;
                let b1 = start.saturating_add(window_len);
                a0 < b1 && b0 < a1
            });
            if !overlaps {
                selected.push(start);
            }
        }
    }

    if selected.is_empty() {
        return samples.to_vec();
    }

    selected.sort_unstable();
    let mut out = Vec::with_capacity(window_len * selected.len());
    for start in selected {
        let end = start.saturating_add(window_len).min(samples.len());
        out.extend_from_slice(&samples[start..end]);
    }
    out
}

fn pad_to_min_duration(samples: &mut Vec<f32>, sample_rate: u32) {
    if sample_rate == 0 {
        return;
    }
    let min_len = (MIN_ANALYSIS_SECONDS * sample_rate as f32).round() as usize;
    if samples.len() < min_len {
        samples.resize(min_len, 0.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analysis::audio::{ANALYSIS_SAMPLE_RATE, MAX_ANALYSIS_SECONDS, WINDOW_SECONDS};

    #[test]
    fn downmix_averages_channels() {
        let stereo = vec![1.0_f32, -1.0, 0.5, 0.25];
        let mono = downmix_to_mono(&stereo, 2);
        assert_eq!(mono.len(), 2);
        assert!((mono[0] - 0.0).abs() < 1e-6);
        assert!((mono[1] - 0.375).abs() < 1e-6);
    }

    #[test]
    fn energy_windowing_limits_long_samples() {
        let sample_rate = ANALYSIS_SAMPLE_RATE;
        let total_len = (sample_rate as usize) * 10;
        let mut samples = vec![0.0_f32; total_len];
        let window_len = (WINDOW_SECONDS * sample_rate as f32).round() as usize;
        let max_len = (MAX_ANALYSIS_SECONDS * sample_rate as f32).round() as usize;
        for i in 0..window_len.min(samples.len()) {
            samples[i] = 0.2;
        }
        let mid_start = samples.len() / 2;
        for i in mid_start..(mid_start + window_len).min(samples.len()) {
            samples[i] = 0.6;
        }
        let tail_start = samples.len().saturating_sub(window_len);
        for i in tail_start..samples.len() {
            samples[i] = 0.4;
        }

        let windowed = apply_energy_windowing(&samples, sample_rate);
        assert_eq!(windowed.len(), max_len);
        assert!(windowed.iter().copied().any(|v| v.abs() > 0.5));
    }

    #[test]
    fn pad_to_min_duration_extends_short_samples() {
        let sample_rate = ANALYSIS_SAMPLE_RATE;
        let mut samples = vec![0.1_f32; 10];
        pad_to_min_duration(&mut samples, sample_rate);
        let min_len = (MIN_ANALYSIS_SECONDS * sample_rate as f32).round() as usize;
        assert_eq!(samples.len(), min_len);
    }
}
