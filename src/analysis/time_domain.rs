use serde::{Deserialize, Serialize};

/// A compact set of time-domain features extracted from analysis-normalized mono audio.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub(crate) struct TimeDomainFeatures {
    pub(crate) duration_seconds: f32,
    pub(crate) peak: f32,
    pub(crate) rms: f32,
    pub(crate) crest_factor: f32,
    pub(crate) zero_crossing_rate: f32,
    pub(crate) attack_seconds: f32,
    pub(crate) decay_20db_seconds: f32,
    pub(crate) decay_40db_seconds: f32,
    pub(crate) onset_count: u32,
}

pub(crate) fn extract_time_domain_features(samples: &[f32], sample_rate: u32) -> TimeDomainFeatures {
    let duration_seconds = duration_seconds(samples.len(), sample_rate);
    let peak = peak(samples);
    let rms = rms(samples);
    let crest_factor = if rms > 0.0 { peak / rms } else { 0.0 };
    let zero_crossing_rate = zero_crossing_rate(samples, sample_rate);
    let envelope = rms_envelope(samples, sample_rate, 0.01);
    let (attack_seconds, decay_20db_seconds, decay_40db_seconds) =
        attack_and_decay_times(&envelope, sample_rate, 0.01);
    let onset_count = count_onsets(&envelope);

    TimeDomainFeatures {
        duration_seconds,
        peak,
        rms,
        crest_factor,
        zero_crossing_rate,
        attack_seconds,
        decay_20db_seconds,
        decay_40db_seconds,
        onset_count,
    }
}

fn duration_seconds(sample_count: usize, sample_rate: u32) -> f32 {
    if sample_rate == 0 {
        return 0.0;
    }
    sample_count as f32 / sample_rate as f32
}

fn peak(samples: &[f32]) -> f32 {
    samples
        .iter()
        .copied()
        .map(|v| v.abs())
        .fold(0.0_f32, f32::max)
        .clamp(0.0, 1.0)
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
    (mean.max(0.0).sqrt() as f32).clamp(0.0, 1.0)
}

fn zero_crossing_rate(samples: &[f32], sample_rate: u32) -> f32 {
    if samples.len() < 2 || sample_rate == 0 {
        return 0.0;
    }
    let mut crossings = 0u64;
    let mut prev = sanitize_sample(samples[0]);
    for &sample in &samples[1..] {
        let current = sanitize_sample(sample);
        let crossed = (prev >= 0.0 && current < 0.0) || (prev < 0.0 && current >= 0.0);
        if crossed && (prev != 0.0 || current != 0.0) {
            crossings += 1;
        }
        prev = current;
    }
    let duration = samples.len() as f32 / sample_rate as f32;
    if duration > 0.0 {
        crossings as f32 / duration
    } else {
        0.0
    }
}

fn sanitize_sample(sample: f32) -> f32 {
    if sample.is_finite() {
        sample.clamp(-1.0, 1.0)
    } else {
        0.0
    }
}

fn rms_envelope(samples: &[f32], sample_rate: u32, window_seconds: f32) -> Vec<f32> {
    if samples.is_empty() || sample_rate == 0 {
        return Vec::new();
    }
    let window_size = (sample_rate as f32 * window_seconds).round().max(1.0) as usize;
    let hop = window_size;
    let mut envelope = Vec::with_capacity(samples.len().div_ceil(hop).max(1));
    let mut start = 0usize;
    while start < samples.len() {
        let end = (start + window_size).min(samples.len());
        envelope.push(rms(&samples[start..end]));
        start = start.saturating_add(hop);
    }
    envelope
}

fn attack_and_decay_times(
    envelope: &[f32],
    sample_rate: u32,
    window_seconds: f32,
) -> (f32, f32, f32) {
    if envelope.is_empty() || sample_rate == 0 {
        return (0.0, 0.0, 0.0);
    }
    let peak_env = envelope
        .iter()
        .copied()
        .fold(0.0_f32, f32::max)
        .clamp(0.0, 1.0);
    if peak_env <= 0.0 {
        return (0.0, 0.0, 0.0);
    }

    let peak_idx = envelope
        .iter()
        .enumerate()
        .max_by(|a, b| a.1.total_cmp(b.1))
        .map(|(idx, _)| idx)
        .unwrap_or(0);

    let attack_threshold = (peak_env * 0.1).max(1e-6);
    let mut attack_idx = 0usize;
    for (idx, &value) in envelope.iter().enumerate().take(peak_idx + 1) {
        if value >= attack_threshold {
            attack_idx = idx;
            break;
        }
    }
    let attack_seconds = (peak_idx.saturating_sub(attack_idx) as f32 * window_seconds).max(0.0);

    let decay_20_threshold = peak_env * db_to_linear(-20.0);
    let decay_40_threshold = peak_env * db_to_linear(-40.0);
    let mut decay_20_idx: Option<usize> = None;
    let mut decay_40_idx: Option<usize> = None;
    for (idx, &value) in envelope.iter().enumerate().skip(peak_idx) {
        if decay_20_idx.is_none() && value <= decay_20_threshold {
            decay_20_idx = Some(idx);
        }
        if decay_40_idx.is_none() && value <= decay_40_threshold {
            decay_40_idx = Some(idx);
        }
        if decay_20_idx.is_some() && decay_40_idx.is_some() {
            break;
        }
    }
    let end_idx = envelope.len().saturating_sub(1);
    let decay_20_idx = decay_20_idx.unwrap_or(end_idx);
    let decay_40_idx = decay_40_idx.unwrap_or(end_idx);
    let decay_20_seconds = (decay_20_idx.saturating_sub(peak_idx) as f32 * window_seconds).max(0.0);
    let decay_40_seconds = (decay_40_idx.saturating_sub(peak_idx) as f32 * window_seconds).max(0.0);
    let _ = sample_rate;
    (attack_seconds, decay_20_seconds, decay_40_seconds)
}

fn count_onsets(envelope: &[f32]) -> u32 {
    if envelope.is_empty() {
        return 0;
    }
    let peak_env = envelope
        .iter()
        .copied()
        .fold(0.0_f32, f32::max)
        .clamp(0.0, 1.0);
    if peak_env <= 0.0 {
        return 0;
    }
    let on_threshold = peak_env * db_to_linear(-20.0);
    let off_threshold = peak_env * db_to_linear(-30.0);
    let mut armed = true;
    let mut count = 0u32;
    for &value in envelope {
        if armed {
            if value >= on_threshold {
                count = count.saturating_add(1);
                armed = false;
            }
        } else if value <= off_threshold {
            armed = true;
        }
    }
    count
}

fn db_to_linear(db: f32) -> f32 {
    10.0_f32.powf(db / 20.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analysis::audio::ANALYSIS_SAMPLE_RATE;

    #[test]
    fn constant_signal_has_expected_features() {
        let sr = ANALYSIS_SAMPLE_RATE;
        let samples = vec![1.0_f32; sr as usize / 10];
        let feats = extract_time_domain_features(&samples, sr);
        assert!((feats.peak - 1.0).abs() < 1e-6);
        assert!((feats.rms - 1.0).abs() < 1e-6);
        assert!((feats.crest_factor - 1.0).abs() < 1e-6);
        assert!(feats.zero_crossing_rate.abs() < 1e-6);
        assert!(feats.onset_count >= 1);
    }

    #[test]
    fn alternating_signal_has_high_zero_crossing_rate() {
        let sr = ANALYSIS_SAMPLE_RATE;
        let mut samples = Vec::with_capacity(sr as usize / 10);
        for i in 0..samples.capacity() {
            samples.push(if i % 2 == 0 { 1.0 } else { -1.0 });
        }
        let feats = extract_time_domain_features(&samples, sr);
        assert!(feats.zero_crossing_rate > sr as f32 * 0.4);
    }

    #[test]
    fn multiple_pulses_count_as_multiple_onsets() {
        let sr = ANALYSIS_SAMPLE_RATE;
        let win = (sr as f32 * 0.01).round() as usize;
        let mut samples = vec![0.0_f32; win * 30];
        for pulse in [2usize, 10usize, 20usize] {
            let start = pulse * win;
            for sample in &mut samples[start..start + win] {
                *sample = 1.0;
            }
        }
        let feats = extract_time_domain_features(&samples, sr);
        assert_eq!(feats.onset_count, 3);
    }
}
