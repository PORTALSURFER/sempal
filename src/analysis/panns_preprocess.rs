use crate::analysis::fft::{Complex32, FftPlan, fft_radix2_inplace_with_plan, hann_window};

pub(crate) const PANNS_STFT_N_FFT: usize = 512;
pub(crate) const PANNS_STFT_HOP: usize = 160;
pub(crate) const PANNS_MEL_BANDS: usize = 64;
pub(crate) const PANNS_MEL_FMIN_HZ: f32 = 50.0;
pub(crate) const PANNS_MEL_FMAX_HZ: f32 = 8_000.0;

/// Compute power spectra (0..=Nyquist) with Hann windowing for PANNs preprocessing.
pub(crate) fn stft_power_frames(
    samples: &[f32],
    n_fft: usize,
    hop: usize,
) -> Result<Vec<Vec<f32>>, String> {
    let n_fft = n_fft.max(1);
    let hop = hop.max(1);
    let frame_len = n_fft / 2 + 1;
    let frames_len = if samples.is_empty() {
        1
    } else {
        (samples.len().saturating_sub(1) / hop).saturating_add(1)
    };
    let mut flat = vec![0.0_f32; frames_len * frame_len];
    let written = stft_power_frames_into_flat(samples, n_fft, hop, &mut flat, frames_len)?;
    let mut frames = Vec::with_capacity(written);
    for frame in flat[..written * frame_len].chunks(frame_len) {
        frames.push(frame.to_vec());
    }
    Ok(frames)
}

/// Compute power spectra (0..=Nyquist) into a flat buffer.
pub(crate) fn stft_power_frames_into_flat(
    samples: &[f32],
    n_fft: usize,
    hop: usize,
    out: &mut [f32],
    max_frames: usize,
) -> Result<usize, String> {
    if max_frames == 0 {
        return Ok(0);
    }
    let n_fft = n_fft.max(1);
    let hop = hop.max(1);
    let frame_len = n_fft / 2 + 1;
    let needed = max_frames.saturating_mul(frame_len);
    if out.len() < needed {
        return Err(format!(
            "stft power output buffer too small: need {needed}, got {}",
            out.len()
        ));
    }
    let window = hann_window(n_fft);
    let plan = FftPlan::new(n_fft)?;
    let mut buf = vec![Complex32::default(); n_fft];
    if samples.is_empty() {
        out[..frame_len].fill(0.0);
        return Ok(1);
    }
    let mut start = 0usize;
    let mut frame_idx = 0usize;
    while start < samples.len() && frame_idx < max_frames {
        let offset = frame_idx * frame_len;
        fill_windowed(&mut buf, samples, start, &window);
        fft_radix2_inplace_with_plan(&mut buf, &plan)?;
        power_spectrum_into(&buf, &mut out[offset..offset + frame_len]);
        start = start.saturating_add(hop);
        frame_idx += 1;
    }
    Ok(frame_idx)
}

pub(crate) struct PannsMelBank {
    filters: Vec<Vec<(usize, f32)>>,
}

impl PannsMelBank {
    pub(crate) fn new(sample_rate: u32, fft_len: usize) -> Self {
        let bins = mel_bins(
            sample_rate,
            fft_len,
            PANNS_MEL_BANDS,
            PANNS_MEL_FMIN_HZ,
            PANNS_MEL_FMAX_HZ,
        );
        let filters = build_filters(&bins, PANNS_MEL_BANDS);
        Self { filters }
    }

    pub(crate) fn mel_from_power(&self, power: &[f32]) -> Vec<f32> {
        apply_filters(&self.filters, power)
    }

    pub(crate) fn mel_from_power_into(&self, power: &[f32], out: &mut [f32]) {
        apply_filters_into(&self.filters, power, out);
    }
}

pub(crate) struct PannsPreprocessor {
    sample_rate: u32,
    n_fft: usize,
    hop: usize,
    window: Vec<f32>,
    plan: FftPlan,
    mel_bank: PannsMelBank,
    fft_buf: Vec<Complex32>,
    power_buf: Vec<f32>,
    mel_buf: Vec<f32>,
}

impl PannsPreprocessor {
    pub(crate) fn new(sample_rate: u32, n_fft: usize, hop: usize) -> Result<Self, String> {
        let n_fft = n_fft.max(1);
        let hop = hop.max(1);
        let window = hann_window(n_fft);
        let plan = FftPlan::new(n_fft)?;
        let mel_bank = PannsMelBank::new(sample_rate, n_fft);
        Ok(Self {
            sample_rate,
            n_fft,
            hop,
            window,
            plan,
            mel_bank,
            fft_buf: vec![Complex32::default(); n_fft],
            power_buf: vec![0.0_f32; n_fft / 2 + 1],
            mel_buf: vec![0.0_f32; PANNS_MEL_BANDS],
        })
    }

    pub(crate) fn set_config(
        &mut self,
        sample_rate: u32,
        n_fft: usize,
        hop: usize,
    ) -> Result<(), String> {
        let n_fft = n_fft.max(1);
        let hop = hop.max(1);
        if self.sample_rate == sample_rate && self.n_fft == n_fft && self.hop == hop {
            return Ok(());
        }
        self.sample_rate = sample_rate;
        self.n_fft = n_fft;
        self.hop = hop;
        self.window = hann_window(n_fft);
        self.plan = FftPlan::new(n_fft)?;
        self.mel_bank = PannsMelBank::new(sample_rate, n_fft);
        self.fft_buf.resize(n_fft, Complex32::default());
        self.power_buf.resize(n_fft / 2 + 1, 0.0);
        if self.mel_buf.len() != PANNS_MEL_BANDS {
            self.mel_buf.resize(PANNS_MEL_BANDS, 0.0);
        }
        Ok(())
    }

    pub(crate) fn log_mel_frames_into_flat(
        &mut self,
        samples: &[f32],
        out: &mut [f32],
        max_frames: usize,
    ) -> Result<usize, String> {
        if max_frames == 0 {
            return Ok(0);
        }
        let frame_len = PANNS_MEL_BANDS;
        let needed = max_frames.saturating_mul(frame_len);
        if out.len() < needed {
            return Err(format!(
                "log-mel output buffer too small: need {needed}, got {}",
                out.len()
            ));
        }
        if samples.is_empty() {
            self.log_mel_silence_into(&mut out[..frame_len]);
            return Ok(1);
        }
        let mut start = 0usize;
        let mut frame_idx = 0usize;
        while start < samples.len() && frame_idx < max_frames {
            let offset = frame_idx * frame_len;
            self.log_mel_frame_into(samples, start, &mut out[offset..offset + frame_len])?;
            start = start.saturating_add(self.hop);
            frame_idx += 1;
        }
        Ok(frame_idx)
    }

    fn log_mel_frame_into(
        &mut self,
        samples: &[f32],
        start: usize,
        out_frame: &mut [f32],
    ) -> Result<(), String> {
        fill_windowed(&mut self.fft_buf, samples, start, &self.window);
        fft_radix2_inplace_with_plan(&mut self.fft_buf, &self.plan)?;
        power_spectrum_into(&self.fft_buf, &mut self.power_buf);
        self.mel_bank
            .mel_from_power_into(&self.power_buf, &mut self.mel_buf);
        write_log_mel(&self.mel_buf, out_frame);
        Ok(())
    }

    fn log_mel_silence_into(&mut self, out_frame: &mut [f32]) {
        self.power_buf.fill(0.0);
        self.mel_bank
            .mel_from_power_into(&self.power_buf, &mut self.mel_buf);
        write_log_mel(&self.mel_buf, out_frame);
    }
}

/// Compute log-mel frames using PANNs defaults (log10 with epsilon).
pub(crate) fn log_mel_frames(samples: &[f32], sample_rate: u32) -> Result<Vec<Vec<f32>>, String> {
    let mut preprocessor =
        PannsPreprocessor::new(sample_rate, PANNS_STFT_N_FFT, PANNS_STFT_HOP)?;
    let frames_len = if samples.is_empty() {
        1
    } else {
        (samples.len().saturating_sub(1) / PANNS_STFT_HOP.max(1)).saturating_add(1)
    };
    let mut flat = vec![0.0_f32; frames_len * PANNS_MEL_BANDS];
    let written = preprocessor.log_mel_frames_into_flat(samples, &mut flat, frames_len)?;
    let mut out = Vec::with_capacity(written);
    for frame in flat[..written * PANNS_MEL_BANDS].chunks(PANNS_MEL_BANDS) {
        out.push(frame.to_vec());
    }
    Ok(out)
}

fn fill_windowed(target: &mut [Complex32], samples: &[f32], start: usize, window: &[f32]) {
    for (i, cell) in target.iter_mut().enumerate() {
        let src = samples.get(start + i).copied().unwrap_or(0.0);
        let win = window.get(i).copied().unwrap_or(1.0);
        *cell = Complex32::new(sanitize(src) * win, 0.0);
    }
}

fn sanitize(sample: f32) -> f32 {
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

fn power_spectrum(fft: &[Complex32]) -> Vec<f32> {
    let bins = fft.len() / 2 + 1;
    let mut power = Vec::with_capacity(bins);
    for bin in 0..bins {
        let c = fft[bin];
        power.push((c.re * c.re + c.im * c.im).max(0.0));
    }
    power
}

fn power_spectrum_into(fft: &[Complex32], out: &mut [f32]) {
    let bins = fft.len() / 2 + 1;
    for bin in 0..bins {
        let c = fft[bin];
        if let Some(slot) = out.get_mut(bin) {
            *slot = (c.re * c.re + c.im * c.im).max(0.0);
        }
    }
}

fn write_log_mel(input: &[f32], out: &mut [f32]) {
    for (src, dst) in input.iter().zip(out.iter_mut()) {
        *dst = log_mel(*src);
    }
}

fn log_mel(value: f32) -> f32 {
    const EPS: f32 = 1e-10;
    let v = value.max(EPS);
    let out = 10.0 * v.log10();
    if out.is_finite() { out } else { 0.0 }
}

fn mel_bins(
    sample_rate: u32,
    fft_len: usize,
    mel_bands: usize,
    f_min: f32,
    f_max: f32,
) -> Vec<usize> {
    let sr = sample_rate.max(1) as f32;
    let nyquist = sr * 0.5;
    let f_max = f_max.min(nyquist).max(f_min);
    let mel_min = hz_to_mel(f_min);
    let mel_max = hz_to_mel(f_max);
    let mut hz_points = Vec::with_capacity(mel_bands + 2);
    for i in 0..(mel_bands + 2) {
        let t = i as f32 / (mel_bands + 1) as f32;
        hz_points.push(mel_to_hz(mel_min + (mel_max - mel_min) * t));
    }
    hz_points
        .into_iter()
        .map(|hz| freq_to_bin(hz, sample_rate, fft_len))
        .collect()
}

fn build_filters(bins: &[usize], mel_bands: usize) -> Vec<Vec<(usize, f32)>> {
    let mut filters = Vec::with_capacity(mel_bands);
    for m in 0..mel_bands {
        let left = bins[m];
        let center = bins[m + 1];
        let right = bins[m + 2].max(center + 1);
        filters.push(build_tri_filter(left, center, right));
    }
    filters
}

fn apply_filters(filters: &[Vec<(usize, f32)>], power: &[f32]) -> Vec<f32> {
    let mut out = Vec::with_capacity(filters.len());
    for filter in filters {
        let mut sum = 0.0_f64;
        for &(bin, weight) in filter {
            let p = power.get(bin).copied().unwrap_or(0.0).max(0.0) as f64;
            sum += p * weight as f64;
        }
        out.push(sum as f32);
    }
    out
}

fn apply_filters_into(filters: &[Vec<(usize, f32)>], power: &[f32], out: &mut [f32]) {
    for (idx, filter) in filters.iter().enumerate() {
        let mut sum = 0.0_f64;
        for &(bin, weight) in filter {
            let p = power.get(bin).copied().unwrap_or(0.0).max(0.0) as f64;
            sum += p * weight as f64;
        }
        if let Some(slot) = out.get_mut(idx) {
            *slot = sum as f32;
        }
    }
}
fn build_tri_filter(left: usize, center: usize, right: usize) -> Vec<(usize, f32)> {
    let mut weights = Vec::new();
    if right <= left {
        return weights;
    }
    for bin in left..=right {
        let w = if bin < center {
            if center == left {
                0.0
            } else {
                (bin as f32 - left as f32) / (center as f32 - left as f32)
            }
        } else if right == center {
            0.0
        } else {
            (right as f32 - bin as f32) / (right as f32 - center as f32)
        };
        if w > 0.0 {
            weights.push((bin, w));
        }
    }
    weights
}

fn freq_to_bin(freq_hz: f32, sample_rate: u32, fft_len: usize) -> usize {
    let nyquist = sample_rate.max(1) as f32 * 0.5;
    let freq = freq_hz.clamp(0.0, nyquist);
    (((freq * fft_len as f32) / sample_rate.max(1) as f32).floor() as usize).min(fft_len / 2)
}

fn hz_to_mel(hz: f32) -> f32 {
    2595.0_f32 * (1.0 + hz / 700.0).log10()
}

fn mel_to_hz(mel: f32) -> f32 {
    700.0_f32 * (10.0_f32.powf(mel / 2595.0) - 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[test]
    fn stft_power_frames_outputs_expected_shape() {
        let samples = vec![0.1_f32; PANNS_STFT_N_FFT + PANNS_STFT_HOP];
        let frames = stft_power_frames(&samples, PANNS_STFT_N_FFT, PANNS_STFT_HOP).unwrap();
        assert_eq!(frames.len(), 5);
        assert_eq!(frames[0].len(), PANNS_STFT_N_FFT / 2 + 1);
    }

    #[test]
    fn stft_power_frames_zero_pads_last_frame() {
        let samples = vec![1.0_f32; 1000];
        let frames = stft_power_frames(&samples, PANNS_STFT_N_FFT, PANNS_STFT_HOP).unwrap();
        assert_eq!(frames.len(), 7);
        assert!(frames.iter().all(|f| f.iter().all(|v| v.is_finite())));
    }

    #[test]
    fn stft_power_frames_empty_input_is_silence() {
        let frames = stft_power_frames(&[], PANNS_STFT_N_FFT, PANNS_STFT_HOP).unwrap();
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].len(), PANNS_STFT_N_FFT / 2 + 1);
        assert!(frames[0].iter().all(|&v| v == 0.0));
    }

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
}
