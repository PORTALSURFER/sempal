use super::DecodedWaveform;

const MAX_TRANSIENT_WINDOWS: usize = 4096;
const MIN_TRANSIENT_SPACING_SECONDS: f32 = 0.02;
const SMOOTH_RADIUS: usize = 1;
const MIN_THRESHOLD_WINDOW: usize = 8;
const MAX_THRESHOLD_WINDOW: usize = 32;

pub fn detect_transients(decoded: &DecodedWaveform, sensitivity: f32) -> Vec<f32> {
    let total_frames = decoded.frame_count();
    if total_frames == 0 {
        return Vec::new();
    }
    let sensitivity = sensitivity.clamp(0.0, 1.0);
    let windows = build_energy_windows(decoded, total_frames);
    if windows.len() < 3 {
        return Vec::new();
    }
    let energies = build_energy_series(&windows);
    let smoothed = smooth_values(&energies, SMOOTH_RADIUS);
    let log_energy = smoothed
        .iter()
        .map(|value| (1.0 + value.max(0.0)).ln())
        .collect::<Vec<f32>>();
    let deltas = build_positive_deltas(&log_energy);
    let threshold_window = (windows.len() / 64).clamp(MIN_THRESHOLD_WINDOW, MAX_THRESHOLD_WINDOW);
    let thresholds = adaptive_thresholds(&deltas, threshold_window, sensitivity);
    let min_gap_frames = min_spacing_frames(decoded, total_frames);
    let mut transients = Vec::new();
    let mut last_frame: Option<usize> = None;
    let mut last_strength = 0.0f32;
    for i in 1..deltas.len().saturating_sub(1) {
        let strength = deltas[i];
        if strength < thresholds[i] {
            continue;
        }
        if strength < deltas[i - 1] || strength < deltas[i + 1] {
            continue;
        }
        let frame = windows[i].frame;
        if let Some(prev_frame) = last_frame {
            let distance = frame.saturating_sub(prev_frame);
            if distance < min_gap_frames {
                if strength > last_strength {
                    if let Some(last_pos) = transients.last_mut() {
                        *last_pos = frame as f32 / total_frames as f32;
                    }
                    last_frame = Some(frame);
                    last_strength = strength;
                }
                continue;
            }
        }
        transients.push(frame as f32 / total_frames as f32);
        last_frame = Some(frame);
        last_strength = strength;
    }
    transients
        .into_iter()
        .map(|pos| pos.clamp(0.0, 1.0))
        .collect()
}

#[derive(Clone, Copy)]
struct EnergyWindow {
    frame: usize,
    energy: f32,
}

fn build_energy_windows(decoded: &DecodedWaveform, total_frames: usize) -> Vec<EnergyWindow> {
    let target = total_frames.min(MAX_TRANSIENT_WINDOWS).max(1);
    let bucket_size = (total_frames / target).max(1);
    if !decoded.samples.is_empty() {
        return build_windows_from_samples(decoded, total_frames, bucket_size);
    }
    if let Some(peaks) = decoded.peaks.as_deref() {
        return build_windows_from_peaks(peaks, total_frames);
    }
    Vec::new()
}

fn build_windows_from_samples(
    decoded: &DecodedWaveform,
    total_frames: usize,
    bucket_size: usize,
) -> Vec<EnergyWindow> {
    let channels = decoded.channel_count().max(1);
    let mut windows = Vec::new();
    let mut prev_samples = vec![0.0f32; channels];
    let mut start = 0usize;
    while start < total_frames {
        let end = (start + bucket_size).min(total_frames);
        let mut sum_diff = 0.0f32;
        let mut count = 0u32;
        for frame in start..end {
            let idx = frame.saturating_mul(channels);
            for ch in 0..channels {
                if let Some(sample) = decoded.samples.get(idx + ch) {
                    let diff = (*sample - prev_samples[ch]).abs();
                    prev_samples[ch] = *sample;
                    sum_diff += diff;
                    count += 1;
                }
            }
        }
        let frame_center = start + (end - start) / 2;
        windows.push(EnergyWindow {
            frame: frame_center,
            energy: if count > 0 {
                sum_diff / count as f32
            } else {
                0.0
            },
        });
        start = end;
    }
    windows
}

fn build_windows_from_peaks(
    peaks: &super::WaveformPeaks,
    total_frames: usize,
) -> Vec<EnergyWindow> {
    let bucket_size = peaks.bucket_size_frames.max(1);
    peaks
        .mono
        .iter()
        .enumerate()
        .map(|(idx, (min, max))| {
            let frame = (idx * bucket_size + bucket_size / 2).min(total_frames.saturating_sub(1));
            let energy = min.abs().max(max.abs());
            EnergyWindow { frame, energy }
        })
        .collect()
}

fn build_positive_deltas(values: &[f32]) -> Vec<f32> {
    let mut deltas = Vec::with_capacity(values.len());
    deltas.push(0.0);
    for i in 1..values.len() {
        let delta = (values[i] - values[i - 1]).max(0.0);
        deltas.push(delta);
    }
    deltas
}

fn build_energy_series(windows: &[EnergyWindow]) -> Vec<f32> {
    windows.iter().map(|window| window.energy).collect()
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
    let k = 0.6 + (1.0 - sensitivity.clamp(0.0, 1.0)) * 2.4;
    for i in 0..values.len() {
        let start = i.saturating_sub(window);
        let slice = &values[start..i];
        let (mean, std_dev) = if slice.is_empty() {
            (global_mean, global_std)
        } else {
            mean_std_dev(slice)
        };
        let threshold = mean + std_dev * k;
        thresholds.push(if threshold.is_finite() { threshold } else { global_mean });
    }
    thresholds
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
        let mut samples = vec![0.0f32; 512];
        samples[128] = 1.0;
        let decoded = DecodedWaveform {
            cache_token: 1,
            samples: Arc::from(samples.into_boxed_slice()),
            peaks: None,
            duration_seconds: 1.0,
            sample_rate: 512,
            channels: 1,
        };
        let transients = detect_transients(&decoded, 0.8);
        assert!(!transients.is_empty());
        let pos = transients[0];
        assert!(pos > 0.1 && pos < 0.4);
    }
}
