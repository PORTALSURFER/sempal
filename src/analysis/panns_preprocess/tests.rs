use super::*;
use serde::Deserialize;

#[test]
fn panns_mel_bank_outputs_expected_length() {
    let bank = PannsMelBank::new(16_000, PANNS_STFT_N_FFT);
    let power = vec![0.0_f32; PANNS_STFT_N_FFT / 2 + 1];
    let mel = bank.mel_from_power(&power);
    assert_eq!(mel.len(), PANNS_MEL_BANDS);
}

#[test]
fn log_mel_frames_are_finite() {
    let samples = vec![0.0_f32; PANNS_STFT_N_FFT];
    let frames = log_mel_frames(&samples, 16_000).unwrap();
    assert!(!frames.is_empty());
    assert!(frames.iter().all(|f| f.iter().all(|v| v.is_finite())));
}

#[test]
fn log_mel_frames_empty_input_is_silence() {
    let frames = log_mel_frames(&[], 16_000).unwrap();
    assert_eq!(frames.len(), 1);
    assert_eq!(frames[0].len(), PANNS_MEL_BANDS);
    assert!(frames[0].iter().all(|v| v.is_finite()));
}

#[test]
fn log_mel_frames_sanitizes_non_finite_samples() {
    let samples = vec![f32::NAN, f32::INFINITY, f32::NEG_INFINITY, 0.25];
    let frames = log_mel_frames(&samples, 16_000).unwrap();
    assert!(!frames.is_empty());
    assert!(frames.iter().all(|f| f.iter().all(|v| v.is_finite())));
}

#[test]
fn preprocessor_respects_config_changes() {
    let samples = vec![0.1_f32; 320];
    let mut preprocessor =
        PannsPreprocessor::new(16_000, PANNS_STFT_N_FFT, PANNS_STFT_HOP).unwrap();
    let mut flat = vec![0.0_f32; 10 * PANNS_MEL_BANDS];
    let frames_default = preprocessor
        .log_mel_frames_into_flat(&samples, &mut flat, 10)
        .unwrap();
    assert_eq!(frames_default, 2);

    preprocessor
        .set_config(16_000, PANNS_STFT_N_FFT, PANNS_STFT_HOP / 2)
        .unwrap();
    let frames_faster = preprocessor
        .log_mel_frames_into_flat(&samples, &mut flat, 10)
        .unwrap();
    assert_eq!(frames_faster, 4);
}

#[test]
fn preprocessor_matches_log_mel_frames() {
    let samples = vec![0.1_f32; PANNS_STFT_N_FFT + PANNS_STFT_HOP];
    let frames = log_mel_frames(&samples, 16_000).unwrap();
    let mut preprocessor =
        PannsPreprocessor::new(16_000, PANNS_STFT_N_FFT, PANNS_STFT_HOP).unwrap();
    let mut flat = vec![0.0_f32; frames.len() * PANNS_MEL_BANDS];
    let written = preprocessor
        .log_mel_frames_into_flat(&samples, &mut flat, frames.len())
        .unwrap();
    assert_eq!(written, frames.len());
    for (frame_idx, frame) in frames.iter().enumerate() {
        for (mel_idx, value) in frame.iter().enumerate() {
            let idx = frame_idx * PANNS_MEL_BANDS + mel_idx;
            assert!((flat[idx] - value).abs() < 1e-6);
        }
    }
}

#[derive(Deserialize)]
struct GoldenMel {
    sample_rate: u32,
    n_fft: usize,
    hop_length: usize,
    n_mels: usize,
    fmin: f32,
    fmax: f32,
    tone_hz: f32,
    tone_amp: f32,
    tone_seconds: f32,
    target_seconds: f32,
    mel_frames: Vec<Vec<f32>>,
}

#[test]
fn golden_log_mel_matches_python() {
    let path = match std::env::var("SEMPAL_PANNS_GOLDEN_PATH") {
        Ok(path) if !path.trim().is_empty() => path,
        _ => return,
    };
    let payload = std::fs::read_to_string(path).expect("read golden json");
    let golden: GoldenMel = serde_json::from_str(&payload).expect("parse golden json");
    assert_eq!(golden.n_fft, PANNS_STFT_N_FFT);
    assert_eq!(golden.hop_length, PANNS_STFT_HOP);
    assert_eq!(golden.n_mels, PANNS_MEL_BANDS);
    assert!((golden.fmin - PANNS_MEL_FMIN_HZ).abs() < 1e-3);
    assert!((golden.fmax - PANNS_MEL_FMAX_HZ).abs() < 1e-3);

    let tone_len = (golden.sample_rate as f32 * golden.tone_seconds).round() as usize;
    let mut tone = Vec::with_capacity(tone_len);
    for i in 0..tone_len {
        let t = i as f32 / golden.sample_rate.max(1) as f32;
        let sample = (2.0 * std::f32::consts::PI * golden.tone_hz * t).sin() * golden.tone_amp;
        tone.push(sample);
    }
    let target_len = (golden.sample_rate as f32 * golden.target_seconds).round() as usize;
    let padded = repeat_pad_for_test(&tone, target_len);
    let frames = log_mel_frames(&padded, golden.sample_rate).expect("log-mel frames");
    assert_eq!(frames.len(), golden.mel_frames.len());
    assert!(!frames.is_empty());
    assert_eq!(frames[0].len(), golden.n_mels);

    let mut max_diff = 0.0_f32;
    for (frame, golden_frame) in frames.iter().zip(golden.mel_frames.iter()) {
        assert_eq!(frame.len(), golden_frame.len());
        for (&a, &b) in frame.iter().zip(golden_frame.iter()) {
            max_diff = max_diff.max((a - b).abs());
        }
    }
    const MAX_DIFF: f32 = 1e-3;
    assert!(
        max_diff <= MAX_DIFF,
        "max diff {max_diff} exceeds {MAX_DIFF}"
    );
}

fn repeat_pad_for_test(samples: &[f32], target_len: usize) -> Vec<f32> {
    if samples.is_empty() || target_len == 0 {
        return Vec::new();
    }
    if samples.len() >= target_len {
        return samples[..target_len].to_vec();
    }
    let mut out = Vec::with_capacity(target_len);
    while out.len() < target_len {
        let remaining = target_len - out.len();
        let take = remaining.min(samples.len());
        out.extend_from_slice(&samples[..take]);
    }
    out
}
