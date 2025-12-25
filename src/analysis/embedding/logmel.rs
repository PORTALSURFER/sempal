use crate::analysis::audio;
use crate::analysis::panns_preprocess::{
    log_mel_frames_with_scratch, PannsPreprocessScratch, PANNS_MEL_BANDS, PANNS_STFT_HOP,
};

/// Target sample rate for PANNs inference.
pub(crate) const PANNS_SAMPLE_RATE: u32 = 16_000;
const PANNS_INPUT_SECONDS: f32 = 10.0;
/// Target input length in samples for PANNs inference.
pub(crate) const PANNS_INPUT_SAMPLES: usize =
    (PANNS_SAMPLE_RATE as f32 * PANNS_INPUT_SECONDS) as usize;
/// Target input length in frames for PANNs inference.
pub(crate) const PANNS_INPUT_FRAMES: usize =
    (PANNS_SAMPLE_RATE as f32 * PANNS_INPUT_SECONDS / PANNS_STFT_HOP as f32) as usize;
/// Flat log-mel length for a single PANNs input window.
pub(crate) const PANNS_LOGMEL_LEN: usize = PANNS_MEL_BANDS * PANNS_INPUT_FRAMES;

/// Scratch buffers used while preparing log-mel inputs for PANNs.
pub(crate) struct PannsLogMelScratch {
    pub(super) resample_scratch: Vec<f32>,
    pub(super) wave_scratch: Vec<f32>,
    pub(super) preprocess_scratch: PannsPreprocessScratch,
}

impl Default for PannsLogMelScratch {
    fn default() -> Self {
        Self {
            resample_scratch: Vec::new(),
            wave_scratch: Vec::new(),
            preprocess_scratch: PannsPreprocessScratch::new(),
        }
    }
}

/// Build a log-mel frame buffer suitable for PANNs inference.
pub(crate) fn build_panns_logmel_into(
    samples: &[f32],
    sample_rate: u32,
    out: &mut [f32],
    scratch: &mut PannsLogMelScratch,
) -> Result<(), String> {
    if out.len() != PANNS_LOGMEL_LEN {
        return Err(format!(
            "PANNs log-mel buffer has wrong length: expected {PANNS_LOGMEL_LEN}, got {}",
            out.len()
        ));
    }
    prepare_panns_logmel(
        &mut scratch.resample_scratch,
        &mut scratch.wave_scratch,
        &mut scratch.preprocess_scratch,
        out,
        samples,
        sample_rate,
    )
}

pub(super) fn prepare_panns_logmel(
    resample_scratch: &mut Vec<f32>,
    wave_scratch: &mut Vec<f32>,
    preprocess_scratch: &mut PannsPreprocessScratch,
    out: &mut [f32],
    samples: &[f32],
    sample_rate: u32,
) -> Result<(), String> {
    if sample_rate != PANNS_SAMPLE_RATE {
        audio::resample_linear_into(resample_scratch, samples, sample_rate, PANNS_SAMPLE_RATE);
        audio::sanitize_samples_in_place(resample_scratch.as_mut_slice());
        repeat_pad_into(wave_scratch, resample_scratch.as_slice(), PANNS_INPUT_SAMPLES);
    } else {
        repeat_pad_into(wave_scratch, samples, PANNS_INPUT_SAMPLES);
        audio::sanitize_samples_in_place(wave_scratch.as_mut_slice());
    }
    let frames = log_mel_frames_with_scratch(wave_scratch, PANNS_SAMPLE_RATE, preprocess_scratch)?;
    out.fill(0.0);
    for (frame_idx, frame) in frames.iter().take(PANNS_INPUT_FRAMES).enumerate() {
        for (mel_idx, value) in frame.iter().enumerate().take(PANNS_MEL_BANDS) {
            let idx = frame_idx * PANNS_MEL_BANDS + mel_idx;
            out[idx] = *value;
        }
    }
    Ok(())
}

/// Repeat-pad a sample buffer up to the target length.
pub(crate) fn repeat_pad_into(out: &mut Vec<f32>, samples: &[f32], target_len: usize) {
    out.clear();
    out.resize(target_len, 0.0);
    if samples.is_empty() || target_len == 0 {
        return;
    }
    if samples.len() >= target_len {
        out[..target_len].copy_from_slice(&samples[..target_len]);
        return;
    }
    let mut offset = 0usize;
    while offset < target_len {
        let remaining = target_len - offset;
        let take = remaining.min(samples.len());
        out[offset..offset + take].copy_from_slice(&samples[..take]);
        offset += take;
    }
}
