mod odf;
mod peaks;
mod stats;

use super::DecodedWaveform;
use odf::{analysis_params, mono_samples, spectral_flux_superflux};
use peaks::{
    SensitivityParams, compute_baselines, percentile, pick_peaks_hysteresis, smooth_values,
};
use tracing::info;

const BASELINE_SECONDS: f32 = 0.15;
const MAX_THRESHOLD_WINDOW: usize = 64;
const MIN_THRESHOLD_WINDOW: usize = 8;
const SMOOTH_RADIUS: usize = 1;

/// ODF novelty data and timing metadata used for peak picking.
#[derive(Clone, Debug)]
pub struct TransientNovelty {
    /// Novelty curve values (one per analysis frame).
    pub novelty: Vec<f32>,
    /// FFT length used for the analysis signal.
    pub fft_len: usize,
    /// Hop length in analysis samples.
    pub hop: usize,
    /// Sample rate of the analysis signal.
    pub sample_rate: u32,
    /// Total number of frames in the original audio.
    pub total_frames: usize,
    /// Number of original frames represented by each analysis sample.
    pub analysis_stride: usize,
}

/// Detect normalized transient positions for a decoded waveform.
pub fn detect_transients(decoded: &DecodedWaveform, sensitivity: f32) -> Vec<f32> {
    let Some(novelty) = compute_transient_novelty(decoded) else {
        return Vec::new();
    };
    pick_transients_from_novelty(&novelty, sensitivity, decoded.duration_seconds)
}

/// Compute the transient novelty curve for the decoded waveform.
///
/// Uses full samples when available and falls back to the decimated analysis
/// buffer for long files.
pub fn compute_transient_novelty(decoded: &DecodedWaveform) -> Option<TransientNovelty> {
    let total_frames = decoded.frame_count();
    let (mono, sample_rate, analysis_stride) = if !decoded.samples.is_empty() {
        (mono_samples(decoded), decoded.sample_rate, 1usize)
    } else if !decoded.analysis_samples.is_empty() {
        (
            decoded.analysis_samples.to_vec(),
            decoded.analysis_sample_rate.max(1),
            decoded.analysis_stride.max(1),
        )
    } else {
        return None;
    };
    let total_frames = if total_frames == 0 {
        mono.len().saturating_mul(analysis_stride).max(1)
    } else {
        total_frames
    };
    let params = analysis_params(sample_rate, mono.len());
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
        analysis_stride,
    })
}

/// Pick transient markers from a precomputed novelty curve.
pub fn pick_transients_from_novelty(
    novelty: &TransientNovelty,
    sensitivity: f32,
    duration_seconds: f32,
) -> Vec<f32> {
    let sensitivity = sensitivity.clamp(0.0, 1.0);
    let params = SensitivityParams::from_sensitivity(sensitivity);
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
            "transients: novelty min={:.4}, median={:.4}, max={:.4}, frames={}, hop={}, stride={}",
            min_value,
            median,
            max_value,
            novelty_smoothed.len(),
            novelty.hop,
            novelty.analysis_stride
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
    let hop_frames = novelty.hop.saturating_mul(novelty.analysis_stride).max(1);
    let fft_len_frames = novelty
        .fft_len
        .saturating_mul(novelty.analysis_stride)
        .max(1);
    let positions: Vec<f32> = peaks
        .into_iter()
        .map(|(frame, _)| {
            let position = ((frame * hop_frames + fft_len_frames / 2) as f32)
                / novelty.total_frames.max(1) as f32;
            position.clamp(0.0, 1.0)
        })
        .collect();
    if std::env::var("SEMPAL_TRANSIENT_DEBUG").is_ok() {
        info!("transients: picked {} markers", positions.len());
    }
    positions
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
            analysis_samples: Arc::from(Vec::new()),
            analysis_sample_rate: 0,
            analysis_stride: 1,
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
            analysis_samples: Arc::from(Vec::new()),
            analysis_sample_rate: 0,
            analysis_stride: 1,
            peaks: None,
            duration_seconds: 1.0,
            sample_rate: 48_000,
            channels: 1,
        };
        let transients = detect_transients(&decoded, 1.0);
        assert!(transients.len() >= 2);
    }

    #[test]
    fn detects_transients_from_analysis_samples() {
        let mut samples = vec![0.0f32; 4096];
        for sample in samples.iter_mut().skip(256).take(16) {
            *sample = 1.0;
        }
        let decoded = DecodedWaveform {
            cache_token: 3,
            samples: Arc::from(Vec::new()),
            analysis_samples: Arc::from(samples.into_boxed_slice()),
            analysis_sample_rate: 48_000,
            analysis_stride: 1,
            peaks: None,
            duration_seconds: 1.0,
            sample_rate: 48_000,
            channels: 1,
        };
        let novelty = compute_transient_novelty(&decoded).expect("analysis novelty");
        assert!(novelty.total_frames > 0);
        assert!(!novelty.novelty.is_empty());
    }
}
