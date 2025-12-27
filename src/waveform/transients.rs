use super::DecodedWaveform;
use crate::analysis::fft::{Complex32, FftPlan, fft_radix2_inplace_with_plan, hann_window};

const MIN_TRANSIENT_SPACING_SECONDS: f32 = 0.05;
const SMOOTH_RADIUS: usize = 1;
const MIN_THRESHOLD_WINDOW: usize = 8;
const MAX_THRESHOLD_WINDOW: usize = 64;
const BASELINE_SECONDS: f32 = 0.15;
const BAND_COUNT: usize = 24;
const MIN_BAND_HZ: f32 = 40.0;

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
    let mono = mono_samples(decoded);
    let novelty = spectral_flux_novelty(&mono, fft_len, hop, decoded.sample_rate);
    if novelty.len() < 3 {
        return Vec::new();
    }
    let novelty_smoothed = smooth_values(&novelty, SMOOTH_RADIUS);
    let window = ((BASELINE_SECONDS * sample_rate / hop as f32).round() as usize)
        .clamp(MIN_THRESHOLD_WINDOW, MAX_THRESHOLD_WINDOW);
    let min_gap_frames = ((MIN_TRANSIENT_SPACING_SECONDS * sample_rate) / hop as f32)
        .round()
        .max(1.0) as usize;
    let max_transients_cap = max_transients(decoded, sensitivity);
    let long_sample = decoded.duration_seconds > 30.0;
    let floor_quantile = if long_sample {
        0.4 + (1.0 - sensitivity) * 0.1
    } else {
        0.6 + (1.0 - sensitivity) * 0.15
    };
    let mut peaks = if long_sample {
        pick_peaks_windowed(
            &novelty_smoothed,
            window,
            sensitivity,
            min_gap_frames,
            max_transients_cap,
            sample_rate,
            hop,
            floor_quantile,
        )
    } else {
        let thresholds = adaptive_thresholds(&novelty_smoothed, window, sensitivity);
        let global_floor = percentile(&novelty_smoothed, floor_quantile);
        pick_peaks(
            &novelty_smoothed,
            &thresholds,
            global_floor,
            min_gap_frames,
            max_transients_cap,
        )
    };
    let mut raw_flux = None;
    if long_sample {
        raw_flux = Some(spectral_flux_raw(&mono, fft_len, hop));
        if let Some(raw_flux) = &raw_flux {
            let raw_smoothed = smooth_values(raw_flux, SMOOTH_RADIUS);
            let raw_window = (window / 2).max(4);
            let raw_peaks = pick_peaks_windowed(
                &raw_smoothed,
                raw_window,
                1.0,
                min_gap_frames,
                max_transients_cap,
                sample_rate,
                hop,
                0.35,
            );
            peaks = merge_peaks(peaks, raw_peaks, min_gap_frames, max_transients_cap);
        }
    }
    if peaks.is_empty() && long_sample {
        peaks = pick_peaks_loose(
            &novelty_smoothed,
            min_gap_frames,
            max_transients_cap,
            0.2,
        );
    }
    if peaks.is_empty() {
        let fallback_sensitivity = 1.0;
        let fallback_thresholds =
            adaptive_thresholds(&novelty_smoothed, window, fallback_sensitivity);
        let fallback_floor = percentile(&novelty_smoothed, 0.55);
        let fallback_cap = max_transients(decoded, 0.8);
        peaks = pick_peaks(
            &novelty_smoothed,
            &fallback_thresholds,
            fallback_floor,
            min_gap_frames,
            fallback_cap,
        );
    }
    if peaks.is_empty() {
        let raw_flux = raw_flux
            .as_ref()
            .cloned()
            .unwrap_or_else(|| spectral_flux_raw(&mono, fft_len, hop));
        let raw_smoothed = smooth_values(&raw_flux, SMOOTH_RADIUS);
        let raw_window = (window / 2).max(4);
        let raw_thresholds = adaptive_thresholds(&raw_smoothed, raw_window, 1.0);
        let raw_floor = percentile(&raw_smoothed, 0.4);
        let raw_cap = max_transients(decoded, 1.0);
        peaks = pick_peaks(
            &raw_smoothed,
            &raw_thresholds,
            raw_floor,
            min_gap_frames,
            raw_cap,
        );
    }
    if peaks.is_empty() && long_sample {
        let raw_flux = raw_flux
            .as_ref()
            .cloned()
            .unwrap_or_else(|| spectral_flux_raw(&mono, fft_len, hop));
        let raw_smoothed = smooth_values(&raw_flux, SMOOTH_RADIUS);
        let raw_cap = max_transients(decoded, 1.0);
        peaks = pick_peaks_loose(&raw_smoothed, min_gap_frames, raw_cap, 0.15);
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

fn mono_samples(decoded: &DecodedWaveform) -> Vec<f32> {
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
    mono
}

fn spectral_flux_novelty(
    mono: &[f32],
    fft_len: usize,
    hop: usize,
    sample_rate: u32,
) -> Vec<f32> {
    if mono.is_empty() {
        return Vec::new();
    }
    let window = hann_window(fft_len);
    let plan = match FftPlan::new(fft_len) {
        Ok(plan) => plan,
        Err(_) => return Vec::new(),
    };
    let bins = fft_len / 2 + 1;
    let bands = band_edges(bins, sample_rate, BAND_COUNT);
    if bands.is_empty() {
        return Vec::new();
    }
    let mut band_means = vec![0.0f32; bands.len()];
    let mut prev_band = vec![0.0f32; bands.len()];
    let mut buf = vec![Complex32::default(); fft_len];
    let mut novelty = Vec::new();
    let mut start = 0usize;
    let hop_seconds = hop as f32 / sample_rate.max(1) as f32;
    let tau = BASELINE_SECONDS.max(0.05);
    let alpha = (hop_seconds / (tau + hop_seconds)).clamp(0.01, 0.2);
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
        for (band_index, (start_bin, end_bin)) in bands.iter().enumerate() {
            if *start_bin >= *end_bin || *start_bin >= bins {
                continue;
            }
            let mut band_sum = 0.0f32;
            let mut count = 0.0f32;
            for bin in *start_bin..(*end_bin).min(bins) {
                let c = buf[bin];
                let mag = (c.re * c.re + c.im * c.im).sqrt();
                let mag_log = (1.0 + 10.0 * mag).ln();
                band_sum += mag_log;
                count += 1.0;
            }
            if count == 0.0 {
                continue;
            }
            let band_value = band_sum / count;
            let mean = band_means[band_index];
            let updated_mean = if mean == 0.0 {
                band_value
            } else {
                mean + alpha * (band_value - mean)
            };
            band_means[band_index] = updated_mean;
            let normalized = band_value / (updated_mean + 1.0e-6);
            let delta = (normalized - prev_band[band_index]).max(0.0);
            prev_band[band_index] = normalized;
            let weight = ((band_index + 1) as f32 / bands.len() as f32).sqrt();
            sum += delta * weight;
        }
        novelty.push(sum);
        start += hop;
    }
    novelty
}

fn spectral_flux_raw(mono: &[f32], fft_len: usize, hop: usize) -> Vec<f32> {
    if mono.is_empty() {
        return Vec::new();
    }
    let window = hann_window(fft_len);
    let plan = match FftPlan::new(fft_len) {
        Ok(plan) => plan,
        Err(_) => return Vec::new(),
    };
    let bins = fft_len / 2 + 1;
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
            let mag_log = (1.0 + 10.0 * mag).ln();
            let delta = (mag_log - prev_mag[bin]).max(0.0);
            prev_mag[bin] = mag_log;
            sum += delta;
        }
        novelty.push(sum);
        start += hop;
    }
    novelty
}

fn band_edges(bins: usize, sample_rate: u32, bands: usize) -> Vec<(usize, usize)> {
    if bins < 4 || bands == 0 {
        return Vec::new();
    }
    let nyquist = sample_rate as f32 * 0.5;
    let min_hz = MIN_BAND_HZ.min(nyquist * 0.5);
    let max_hz = nyquist.max(min_hz + 1.0);
    let log_min = min_hz.ln();
    let log_max = max_hz.ln();
    let mut edges = Vec::with_capacity(bands);
    let mut last_bin = 1usize;
    for band in 0..bands {
        let t0 = band as f32 / bands as f32;
        let t1 = (band + 1) as f32 / bands as f32;
        let hz0 = (log_min + (log_max - log_min) * t0).exp();
        let hz1 = (log_min + (log_max - log_min) * t1).exp();
        let bin0 = ((hz0 / nyquist) * (bins as f32 - 1.0)).round() as usize;
        let bin1 = ((hz1 / nyquist) * (bins as f32 - 1.0)).round() as usize;
        let start = bin0.clamp(1, bins.saturating_sub(1));
        let end = bin1.clamp(start + 1, bins);
        let start = start.max(last_bin);
        let end = end.max(start + 1).min(bins);
        if start < end {
            edges.push((start, end));
            last_bin = end;
        }
    }
    edges
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
    let k = 2.0 + (1.0 - sensitivity.clamp(0.0, 1.0)) * 3.0;
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

fn pick_peaks_windowed(
    novelty_smoothed: &[f32],
    window: usize,
    sensitivity: f32,
    min_gap_frames: usize,
    max_transients: usize,
    sample_rate: f32,
    hop: usize,
    floor_quantile: f32,
) -> Vec<(usize, f32)> {
    let hop_seconds = hop as f32 / sample_rate.max(1.0);
    let window_seconds = 12.0;
    let overlap_seconds = 2.0;
    let window_frames =
        ((window_seconds / hop_seconds).round() as usize).max(window * 2).min(4096);
    let overlap_frames =
        ((overlap_seconds / hop_seconds).round() as usize).min(window_frames.saturating_sub(1));
    let step = window_frames.saturating_sub(overlap_frames).max(1);
    let mut peaks = Vec::new();
    let mut start = 0usize;
    while start < novelty_smoothed.len() {
        let end = (start + window_frames).min(novelty_smoothed.len());
        let slice = &novelty_smoothed[start..end];
        if slice.len() >= 3 {
            let thresholds = adaptive_thresholds(slice, window, sensitivity);
            let floor = percentile(slice, floor_quantile);
            let mut local = pick_peaks(slice, &thresholds, floor, min_gap_frames, max_transients);
            for (frame, strength) in local.drain(..) {
                peaks.push((frame + start, strength));
            }
        }
        if end == novelty_smoothed.len() {
            break;
        }
        start = start.saturating_add(step);
    }
    peaks = merge_peaks(Vec::new(), peaks, min_gap_frames, max_transients);
    if peaks.len() > max_transients {
        peaks.truncate(max_transients);
    }
    peaks.sort_by_key(|(frame, _)| *frame);
    peaks
}

fn merge_peaks(
    mut primary: Vec<(usize, f32)>,
    mut extra: Vec<(usize, f32)>,
    min_gap_frames: usize,
    max_transients: usize,
) -> Vec<(usize, f32)> {
    primary.append(&mut extra);
    if primary.is_empty() {
        return primary;
    }
    primary.sort_by_key(|(frame, _)| *frame);
    let mut merged: Vec<(usize, f32)> = Vec::with_capacity(primary.len());
    for (frame, strength) in primary {
        if let Some((last_frame, last_strength)) = merged.last_mut() {
            if frame.abs_diff(*last_frame) <= min_gap_frames {
                if strength > *last_strength {
                    *last_frame = frame;
                    *last_strength = strength;
                }
                continue;
            }
        }
        merged.push((frame, strength));
    }
    if merged.len() > max_transients {
        merged.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        merged.truncate(max_transients);
        merged.sort_by_key(|(frame, _)| *frame);
    }
    merged
}

fn pick_peaks(
    novelty_smoothed: &[f32],
    thresholds: &[f32],
    global_floor: f32,
    min_gap_frames: usize,
    max_transients: usize,
) -> Vec<(usize, f32)> {
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
    if peaks.len() > max_transients {
        peaks.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        peaks.truncate(max_transients);
        peaks.sort_by_key(|(frame, _)| *frame);
    }
    peaks
}

fn pick_peaks_loose(
    novelty_smoothed: &[f32],
    min_gap_frames: usize,
    max_transients: usize,
    floor_quantile: f32,
) -> Vec<(usize, f32)> {
    if novelty_smoothed.len() < 3 {
        return Vec::new();
    }
    let floor = percentile(novelty_smoothed, floor_quantile);
    let mut peaks: Vec<(usize, f32)> = Vec::new();
    for i in 1..novelty_smoothed.len().saturating_sub(1) {
        let strength = novelty_smoothed[i];
        if strength < floor {
            continue;
        }
        if strength < novelty_smoothed[i - 1] || strength < novelty_smoothed[i + 1] {
            continue;
        }
        peaks.push((i, strength));
    }
    merge_peaks(Vec::new(), peaks, min_gap_frames, max_transients)
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
