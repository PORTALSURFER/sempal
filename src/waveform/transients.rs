use super::DecodedWaveform;
use crate::analysis::fft::{Complex32, FftPlan, fft_radix2_inplace_with_plan, hann_window};

const MIN_TRANSIENT_SPACING_SECONDS: f32 = 0.05;
const SMOOTH_RADIUS: usize = 1;
const MIN_THRESHOLD_WINDOW: usize = 8;
const MAX_THRESHOLD_WINDOW: usize = 64;
const PREEMPHASIS: f32 = 0.97;
const BASELINE_SECONDS: f32 = 0.15;

pub fn detect_transients(decoded: &DecodedWaveform, sensitivity: f32) -> Vec<f32> {
    let total_frames = decoded.frame_count();
    if total_frames == 0 {
        return Vec::new();
    }
    if decoded.samples.is_empty() {
        return Vec::new();
    }
    let sensitivity = sensitivity.clamp(0.0, 1.0);
    let sample_rate = decoded.sample_rate.max(1) as f32;
    let (fft_len, hop) = stft_params(decoded.sample_rate);
    let novelty = complex_domain_novelty(decoded, fft_len, hop);
    if novelty.len() < 3 {
        return Vec::new();
    }
    let novelty_smoothed = smooth_values(&novelty, SMOOTH_RADIUS);
    let window = ((BASELINE_SECONDS * sample_rate / hop as f32).round() as usize)
        .clamp(MIN_THRESHOLD_WINDOW, MAX_THRESHOLD_WINDOW);
    let thresholds = adaptive_thresholds(&novelty_smoothed, window, sensitivity);
    let global_floor = percentile(
        &novelty_smoothed,
        0.7 + (1.0 - sensitivity) * 0.2,
    );
    let min_gap_frames = ((MIN_TRANSIENT_SPACING_SECONDS * sample_rate) / hop as f32)
        .round()
        .max(1.0) as usize;
    let mut peaks: Vec<(usize, f32)> = Vec::new();
    let mut last_frame: Option<usize> = None;
    let mut last_strength = 0.0f32;
    for i in 1..novelty_smoothed.len().saturating_sub(1) {
        let strength = novelty_smoothed[i];
        if strength < thresholds[i] || strength < global_floor {
            continue;
        }
        if strength < novelty_smoothed[i - 1] || strength < novelty_smoothed[i + 1] {
            continue;
        }
        let frame = i;
        if let Some(prev_frame) = last_frame {
            let distance = frame.saturating_sub(prev_frame);
            if distance < min_gap_frames {
                if strength > last_strength {
                    if let Some((last_frame, last_strength)) = peaks.last_mut() {
                        *last_frame = frame;
                        *last_strength = strength;
                    }
                    last_frame = Some(frame);
                    last_strength = strength;
                }
                continue;
            }
        }
        peaks.push((frame, strength));
        last_frame = Some(frame);
        last_strength = strength;
    }
    let max_transients = max_transients(decoded, sensitivity);
    if peaks.len() > max_transients {
        peaks.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        peaks.truncate(max_transients);
        peaks.sort_by_key(|(frame, _)| *frame);
    }
    peaks
        .into_iter()
        .map(|(frame, _)| {
            let position = ((frame * hop + fft_len / 2) as f32) / total_frames as f32;
            position.clamp(0.0, 1.0)
        })
        .collect()
}

fn stft_params(sample_rate: u32) -> (usize, usize) {
    if sample_rate < 32_000 {
        (512, 128)
    } else {
        (1024, 256)
    }
}

fn complex_domain_novelty(decoded: &DecodedWaveform, fft_len: usize, hop: usize) -> Vec<f32> {
    let channels = decoded.channel_count().max(1);
    let frames = decoded.frame_count();
    let mut mono = Vec::with_capacity(frames);
    for frame in 0..frames {
        let idx = frame.saturating_mul(channels);
        let mut sum = 0.0f32;
        for ch in 0..channels {
            if let Some(sample) = decoded.samples.get(idx + ch) {
                sum += *sample;
            }
        }
        mono.push(sum / channels as f32);
    }
    for i in (1..mono.len()).rev() {
        mono[i] = mono[i] - PREEMPHASIS * mono[i - 1];
    }
    let window = hann_window(fft_len);
    let plan = match FftPlan::new(fft_len) {
        Ok(plan) => plan,
        Err(_) => return Vec::new(),
    };
    let bins = fft_len / 2 + 1;
    let mut prev_phase = vec![0.0f32; bins];
    let mut prev2_phase = vec![0.0f32; bins];
    let mut prev_mag = vec![0.0f32; bins];
    let mut buf = vec![Complex32::default(); fft_len];
    let mut novelty = Vec::new();
    let mut start = 0usize;
    while start < mono.len() {
        for i in 0..fft_len {
            let sample = mono.get(start + i).copied().unwrap_or(0.0);
            buf[i].re = sample * window[i];
            buf[i].im = 0.0;
        }
        if fft_radix2_inplace_with_plan(&mut buf, &plan).is_err() {
            return Vec::new();
        }
        let mut sum = 0.0f32;
        for bin in 1..bins {
            let c = buf[bin];
            let mag = (c.re * c.re + c.im * c.im).sqrt();
            let mag_log = (1.0 + mag).ln();
            let phase = c.im.atan2(c.re);
            let predicted = 2.0 * prev_phase[bin] - prev2_phase[bin];
            let expected = Complex32::from_polar(prev_mag[bin], predicted);
            let actual = Complex32::from_polar(mag_log, phase);
            sum += (actual - expected).norm();
            prev2_phase[bin] = prev_phase[bin];
            prev_phase[bin] = phase;
            prev_mag[bin] = mag_log;
        }
        novelty.push(sum);
        start += hop;
    }
    novelty
}

fn smooth_values(values: &[f32], radius: usize) -> Vec<f32> {
    if values.is_empty() || radius == 0 {
        return values.to_vec();
    }
    let mut out = Vec::with_capacity(values.len());
    for i in 0..values.len() {
        let start = i.saturating_sub(radius);
        let end = (i + radius + 1).min(values.len());
        let mut sum = 0.0f32;
        let mut count = 0.0f32;
        for value in &values[start..end] {
            if value.is_finite() {
                sum += *value;
                count += 1.0;
            }
        }
        out.push(if count > 0.0 { sum / count } else { 0.0 });
    }
    out
}

fn adaptive_thresholds(values: &[f32], window: usize, sensitivity: f32) -> Vec<f32> {
    let (global_mean, global_std) = mean_std_dev(values);
    let mut thresholds = Vec::with_capacity(values.len());
    let k = 2.5 + (1.0 - sensitivity.clamp(0.0, 1.0)) * 4.0;
    for i in 0..values.len() {
        let start = i.saturating_sub(window);
        let slice = &values[start..i];
        let (median, mad) = if slice.is_empty() {
            (global_mean, global_std)
        } else {
            median_mad(slice)
        };
        let threshold = median + mad * k;
        thresholds.push(if threshold.is_finite() { threshold } else { global_mean });
    }
    thresholds
}

fn percentile(values: &[f32], quantile: f32) -> f32 {
    if values.is_empty() {
        return 0.0;
    }
    let mut sorted = values
        .iter()
        .copied()
        .filter(|value| value.is_finite())
        .collect::<Vec<f32>>();
    if sorted.is_empty() {
        return 0.0;
    }
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let q = quantile.clamp(0.0, 1.0);
    let idx = ((sorted.len() - 1) as f32 * q).round() as usize;
    sorted[idx]
}

fn max_transients(decoded: &DecodedWaveform, sensitivity: f32) -> usize {
    let duration = decoded.duration_seconds.max(0.01);
    let per_second = 1.5 + sensitivity.clamp(0.0, 1.0) * 2.0;
    (duration * per_second).round().max(1.0) as usize
}

fn median_mad(values: &[f32]) -> (f32, f32) {
    if values.is_empty() {
        return (0.0, 0.0);
    }
    let mut sorted = values
        .iter()
        .copied()
        .filter(|value| value.is_finite())
        .collect::<Vec<f32>>();
    if sorted.is_empty() {
        return (0.0, 0.0);
    }
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let median = sorted[sorted.len() / 2];
    let mut deviations = sorted
        .iter()
        .map(|value| (*value - median).abs())
        .collect::<Vec<f32>>();
    deviations.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let mad = deviations[deviations.len() / 2];
    (median, mad.max(1.0e-6))
}

fn mean_std_dev(values: &[f32]) -> (f32, f32) {
    if values.is_empty() {
        return (0.0, 0.0);
    }
    let mut sum = 0.0f32;
    let mut count = 0.0f32;
    for value in values {
        if value.is_finite() {
            sum += value;
            count += 1.0;
        }
    }
    if count == 0.0 {
        return (0.0, 0.0);
    }
    let mean = sum / count;
    let mut variance = 0.0f32;
    for value in values {
        if value.is_finite() {
            let diff = value - mean;
            variance += diff * diff;
        }
    }
    let std_dev = (variance / count).sqrt();
    (mean, std_dev)
}

fn min_spacing_frames(decoded: &DecodedWaveform, total_frames: usize) -> usize {
    let duration = decoded.duration_seconds;
    if duration.is_finite() && duration > 0.0 {
        let min_spacing = MIN_TRANSIENT_SPACING_SECONDS / duration;
        return ((min_spacing * total_frames as f32).round() as usize).max(1);
    }
    let sample_rate = decoded.sample_rate.max(1) as f32;
    ((MIN_TRANSIENT_SPACING_SECONDS * sample_rate).round() as usize).max(1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn detects_single_spike_transient() {
        let mut samples = vec![0.0f32; 4096];
        samples[1024] = 1.0;
        let decoded = DecodedWaveform {
            cache_token: 1,
            samples: Arc::from(samples.into_boxed_slice()),
            peaks: None,
            duration_seconds: 1.0,
            sample_rate: 48_000,
            channels: 1,
        };
        let transients = detect_transients(&decoded, 1.0);
        assert!(!transients.is_empty());
        let pos = transients[0];
        assert!(pos > 0.15 && pos < 0.4);
    }
}
