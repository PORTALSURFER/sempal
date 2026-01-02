use crate::analysis::fft::{Complex32, FftPlan, fft_radix2_inplace_with_plan, hann_window};

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

pub(super) fn fill_windowed(
    target: &mut [Complex32],
    samples: &[f32],
    start: usize,
    window: &[f32],
) {
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

pub(super) fn power_spectrum_into(fft: &[Complex32], out: &mut [f32]) {
    let bins = fft.len() / 2 + 1;
    for bin in 0..bins {
        let c = fft[bin];
        if let Some(slot) = out.get_mut(bin) {
            *slot = (c.re * c.re + c.im * c.im).max(0.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analysis::panns_preprocess::{PANNS_STFT_HOP, PANNS_STFT_N_FFT};

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
    fn power_spectrum_matches_bins() {
        let fft = vec![Complex32::default(); PANNS_STFT_N_FFT];
        let spectrum = power_spectrum(&fft);
        assert_eq!(spectrum.len(), PANNS_STFT_N_FFT / 2 + 1);
    }
}
