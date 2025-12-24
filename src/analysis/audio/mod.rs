mod decode;
mod normalize;
mod resample;
mod silence;

/// Fixed sample rate used during analysis.
pub(crate) const ANALYSIS_SAMPLE_RATE: u32 = 16_000;
pub(crate) const MAX_ANALYSIS_SECONDS: f32 = 6.0;
pub(crate) const WINDOW_SECONDS: f32 = 2.0;
pub(crate) const WINDOW_HOP_SECONDS: f32 = 1.0;
pub(crate) const MIN_ANALYSIS_SECONDS: f32 = 0.1;
pub(crate) const SILENCE_THRESHOLD_ON_DB: f32 = -45.0;
pub(crate) const SILENCE_THRESHOLD_OFF_DB: f32 = -55.0;
pub(crate) const SILENCE_PRE_ROLL_SECONDS: f32 = 0.01;
pub(crate) const SILENCE_POST_ROLL_SECONDS: f32 = 0.005;
const EMBEDDING_TARGET_RMS_DB: f32 = -20.0;

pub(crate) use decode::{decode_for_analysis, probe_metadata};
pub(crate) use normalize::sanitize_samples_in_place;
pub(crate) use resample::resample_linear;

/// Decoded mono audio ready for analysis.
#[derive(Debug)]
#[allow(dead_code)]
pub(crate) struct AnalysisAudio {
    pub(crate) mono: Vec<f32>,
    pub(crate) duration_seconds: f32,
    pub(crate) sample_rate_used: u32,
}

pub(crate) fn preprocess_mono_for_embedding(samples: &[f32], sample_rate: u32) -> Vec<f32> {
    let mut trimmed = silence::trim_silence_with_hysteresis(samples, sample_rate);
    normalize::normalize_rms_in_place(&mut trimmed, EMBEDDING_TARGET_RMS_DB);
    normalize::normalize_peak_limit_in_place(&mut trimmed);
    trimmed
}
