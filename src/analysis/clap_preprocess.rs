use crate::analysis::fft::{Complex32, fft_radix2_inplace, hann_window};

pub(crate) const CLAP_STFT_N_FFT: usize = 1024;
pub(crate) const CLAP_STFT_HOP: usize = 480;

/// Compute power spectra (0..=Nyquist) with Hann windowing for CLAP preprocessing.
pub(crate) fn stft_power_frames(
    samples: &[f32],
    n_fft: usize,
    hop: usize,
) -> Result<Vec<Vec<f32>>, String> {
    let n_fft = n_fft.max(1);
    let hop = hop.max(1);
    let window = hann_window(n_fft);
    let mut frames = Vec::new();
    let mut buf = vec![Complex32::default(); n_fft];
    let mut start = 0usize;
    while start < samples.len() {
        fill_windowed(&mut buf, samples, start, &window);
        fft_radix2_inplace(&mut buf)?;
        frames.push(power_spectrum(&buf));
        start = start.saturating_add(hop);
    }
    if frames.is_empty() {
        frames.push(vec![0.0_f32; n_fft / 2 + 1]);
    }
    Ok(frames)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stft_power_frames_outputs_expected_shape() {
        let samples = vec![0.1_f32; CLAP_STFT_N_FFT + CLAP_STFT_HOP];
        let frames = stft_power_frames(&samples, CLAP_STFT_N_FFT, CLAP_STFT_HOP).unwrap();
        assert_eq!(frames.len(), 2);
        assert_eq!(frames[0].len(), CLAP_STFT_N_FFT / 2 + 1);
    }

    #[test]
    fn stft_power_frames_zero_pads_last_frame() {
        let samples = vec![1.0_f32; 1000];
        let frames = stft_power_frames(&samples, CLAP_STFT_N_FFT, CLAP_STFT_HOP).unwrap();
        assert_eq!(frames.len(), 3);
        assert!(frames.iter().all(|f| f.iter().all(|v| v.is_finite())));
    }
}
