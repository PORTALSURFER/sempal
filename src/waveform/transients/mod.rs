mod odf;
mod peaks;
mod stats;

use super::{DecodedWaveform, WaveformPeaks};
use odf::{analysis_params, mono_samples, spectral_flux_superflux};
use peaks::{compute_baselines, pick_peaks_hysteresis, percentile, smooth_values, SensitivityParams};
use tracing::info;

const BASELINE_SECONDS: f32 = 0.15;
const MAX_THRESHOLD_WINDOW: usize = 64;
const MIN_THRESHOLD_WINDOW: usize = 8;
const PEAK_ENVELOPE_MAX_SAMPLES: usize = 200_000;
const SMOOTH_RADIUS: usize = 1;

#[derive(Clone, Debug)]
pub struct TransientNovelty {
    pub novelty: Vec<f32>,
    pub fft_len: usize,
    pub hop: usize,
    pub sample_rate: u32,
    pub total_frames: usize,
}

#[derive(Clone, Copy, Debug)]
pub struct TransientTuning {
    pub use_custom: bool,
    pub k_high: f32,
    pub k_low: f32,
    pub floor_quantile: f32,
    pub min_gap_seconds: f32,
}

impl Default for TransientTuning {
    fn default() -> Self {
        Self {
            use_custom: false,
            k_high: 4.2,
            k_low: 2.1,
            floor_quantile: 0.58,
            min_gap_seconds: 0.084,
        }
    }
}

pub fn detect_transients(decoded: &DecodedWaveform, sensitivity: f32) -> Vec<f32> {
    let Some(novelty) = compute_transient_novelty(decoded) else {
        if decoded.samples.is_empty() {
            if let Some(peaks) = decoded.peaks.as_deref() {
                return detect_transients_from_peaks_with_tuning(
                    peaks,
                    decoded,
                    sensitivity,
                    TransientTuning::default(),
                );
            }
        }
        return Vec::new();
    };
    pick_transients_from_novelty(&novelty, sensitivity, decoded.duration_seconds)
}

pub fn compute_transient_novelty(decoded: &DecodedWaveform) -> Option<TransientNovelty> {
    let total_frames = decoded.frame_count();
    if total_frames == 0 || decoded.samples.is_empty() {
        return None;
    }
    let mono = mono_samples(decoded);
    let params = analysis_params(decoded.sample_rate, mono.len());
    let novelty = spectral_flux_superflux(&mono, params.fft_len, params.hop, params.sample_rate);
    if novelty.len() < 3 {
        return None;
    }
    Some(TransientNovelty {
        novelty,
        fft_len: params.fft_len,
        hop: params.hop,
        sample_rate: params.sample_rate,
        total_frames,
    })
}

pub fn pick_transients_from_novelty(
    novelty: &TransientNovelty,
    sensitivity: f32,
    duration_seconds: f32,
) -> Vec<f32> {
    let tuning = TransientTuning::default();
    pick_transients_with_tuning(novelty, sensitivity, duration_seconds, tuning)
}

pub fn pick_transients_with_tuning(
    novelty: &TransientNovelty,
    sensitivity: f32,
    duration_seconds: f32,
    tuning: TransientTuning,
) -> Vec<f32> {
    let sensitivity = sensitivity.clamp(0.0, 1.0);
    let params = if tuning.use_custom {
        SensitivityParams::from_overrides(
            tuning.k_high,
            tuning.k_low,
            tuning.floor_quantile,
            tuning.min_gap_seconds,
        )
    } else {
        SensitivityParams::from_sensitivity(sensitivity)
    };
    let novelty_smoothed = smooth_values(&novelty.novelty, SMOOTH_RADIUS);
    let window = ((BASELINE_SECONDS * novelty.sample_rate as f32 / novelty.hop as f32).round()
        as usize)
        .clamp(MIN_THRESHOLD_WINDOW, MAX_THRESHOLD_WINDOW);
    let baselines = compute_baselines(&novelty_smoothed, window);
    let global_floor = percentile(&novelty_smoothed, params.floor_quantile);
    let min_gap_frames = ((params.min_gap_seconds * novelty.sample_rate as f32)
        / novelty.hop as f32)
        .round()
        .max(1.0) as usize;
    let max_transients = max_transients(duration_seconds, params.min_gap_seconds);
    if std::env::var("SEMPAL_TRANSIENT_DEBUG").is_ok() {
        let min_value = novelty_smoothed
            .iter()
            .copied()
            .fold(f32::INFINITY, f32::min);
        let max_value = novelty_smoothed
            .iter()
            .copied()
            .fold(f32::NEG_INFINITY, f32::max);
        let median = percentile(&novelty_smoothed, 0.5);
        info!(
            "transients: novelty min={:.4}, median={:.4}, max={:.4}, frames={}, hop={}",
            min_value,
            median,
            max_value,
            novelty_smoothed.len(),
            novelty.hop
        );
    }
    let mut peaks = pick_peaks_hysteresis(
        &novelty_smoothed,
        &baselines,
        params,
        global_floor,
        min_gap_frames,
        max_transients,
    );
    if peaks.is_empty() {
        let relaxed = params.relaxed();
        let relaxed_floor = percentile(&novelty_smoothed, relaxed.floor_quantile);
        peaks = pick_peaks_hysteresis(
            &novelty_smoothed,
            &baselines,
            relaxed,
            relaxed_floor,
            min_gap_frames,
            max_transients,
        );
    }
    let positions: Vec<f32> = peaks
        .into_iter()
        .map(|(frame, _)| {
            let position = ((frame * novelty.hop + novelty.fft_len / 2) as f32)
                / novelty.total_frames.max(1) as f32;
            position.clamp(0.0, 1.0)
        })
        .collect();
    if std::env::var("SEMPAL_TRANSIENT_DEBUG").is_ok() {
        info!("transients: picked {} markers", positions.len());
    }
    positions
}

pub fn detect_transients_from_peaks_with_tuning(
    peaks: &WaveformPeaks,
    decoded: &DecodedWaveform,
    sensitivity: f32,
    tuning: TransientTuning,
) -> Vec<f32> {
    if peaks.mono.is_empty() {
        return Vec::new();
    }
    let bucket = peaks.bucket_size_frames.max(1) as f32;
    let sample_rate = decoded.sample_rate.max(1) as f32;
    let params = if tuning.use_custom {
        SensitivityParams::from_overrides(
            tuning.k_high,
            tuning.k_low,
            tuning.floor_quantile,
            tuning.min_gap_seconds,
        )
    } else {
        SensitivityParams::from_sensitivity(sensitivity)
    };
    let mut envelope = Vec::with_capacity(peaks.mono.len());
    for (min, max) in &peaks.mono {
        let amp = min.abs().max(max.abs());
        envelope.push((1.0 + 10.0 * amp).ln());
    }
    let (envelope, stride) = if envelope.len() > PEAK_ENVELOPE_MAX_SAMPLES {
        let stride = envelope.len().div_ceil(PEAK_ENVELOPE_MAX_SAMPLES).max(1);
        let mut reduced = Vec::with_capacity(envelope.len().div_ceil(stride));
        let mut start = 0usize;
        while start < envelope.len() {
            let end = (start + stride).min(envelope.len());
            let mut sum = 0.0f32;
            let mut count = 0.0f32;
            for value in &envelope[start..end] {
                if value.is_finite() {
                    sum += *value;
                    count += 1.0;
                }
            }
            let avg = if count > 0.0 { sum / count } else { 0.0 };
            reduced.push(avg);
            start = end;
        }
        (reduced, stride)
    } else {
        (envelope, 1)
    };
    if envelope.len() < 3 {
        return Vec::new();
    }
    let mut novelty = Vec::with_capacity(envelope.len());
    let mut prev = envelope[0];
    novelty.push(0.0);
    for &value in &envelope[1..] {
        let delta = (value - prev).max(0.0);
        novelty.push(delta);
        prev = value;
    }
    let novelty_smoothed = smooth_values(&novelty, SMOOTH_RADIUS);
    let bucket_stride = bucket * stride as f32;
    let min_gap_frames = ((params.min_gap_seconds * sample_rate) / bucket_stride)
        .round()
        .max(1.0) as usize;
    let max_transients = max_transients(decoded.duration_seconds, params.min_gap_seconds);
    let window = ((BASELINE_SECONDS * sample_rate) / bucket_stride)
        .round() as usize;
    let window = window.clamp(MIN_THRESHOLD_WINDOW, MAX_THRESHOLD_WINDOW);
    let baselines = compute_baselines(&novelty_smoothed, window);
    let global_floor = percentile(&novelty_smoothed, params.floor_quantile);
    let mut peaks = pick_peaks_hysteresis(
        &novelty_smoothed,
        &baselines,
        params,
        global_floor,
        min_gap_frames,
        max_transients,
    );
    if peaks.is_empty() {
        let relaxed = params.relaxed();
        let relaxed_floor = percentile(&novelty_smoothed, relaxed.floor_quantile);
        peaks = pick_peaks_hysteresis(
            &novelty_smoothed,
            &baselines,
            relaxed,
            relaxed_floor,
            min_gap_frames,
            max_transients,
        );
    }
    peaks
        .into_iter()
        .map(|(bucket_index, _)| {
            let frame = bucket_index as f32 * bucket_stride;
            let position = frame / decoded.frame_count().max(1) as f32;
            position.clamp(0.0, 1.0)
        })
        .collect()
}

fn max_transients(duration_seconds: f32, min_gap_seconds: f32) -> usize {
    let duration = duration_seconds.max(0.01);
    let max_by_gap = (duration / min_gap_seconds.max(0.01)).ceil();
    max_by_gap.max(1.0) as usize
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

    #[test]
    fn detects_two_spikes() {
        let mut samples = vec![0.0f32; 8192];
        samples[1024] = 1.0;
        samples[6144] = 1.0;
        let decoded = DecodedWaveform {
            cache_token: 2,
            samples: Arc::from(samples.into_boxed_slice()),
            peaks: None,
            duration_seconds: 1.0,
            sample_rate: 48_000,
            channels: 1,
        };
        let transients = detect_transients(&decoded, 1.0);
        assert!(transients.len() >= 2);
    }
}
