use super::mel::MelBank;
use crate::analysis::fft::{Complex32, fft_radix2_inplace, hann_window};

pub(super) struct FrameSet {
    pub(super) spectral: Vec<SpectralFrame>,
    pub(super) bands: Vec<BandFrame>,
    pub(super) mfcc: Vec<Vec<f32>>,
}

#[derive(Clone, Copy)]
pub(super) struct SpectralFrame {
    pub(super) centroid_hz: f32,
    pub(super) rolloff_hz: f32,
    pub(super) flatness: f32,
    pub(super) bandwidth_hz: f32,
}

#[derive(Clone, Copy)]
pub(super) struct BandFrame {
    pub(super) sub: f32,
    pub(super) low: f32,
    pub(super) mid: f32,
    pub(super) high: f32,
    pub(super) air: f32,
}

const ROLLOFF_FRACTION: f32 = 0.85;

pub(super) fn compute_frames(
    samples: &[f32],
    sample_rate: u32,
    frame_size: usize,
    hop_size: usize,
    mel: &MelBank,
) -> FrameSet {
    let frame_size = frame_size.max(1);
    let hop_size = hop_size.max(1);
    let window = hann_window(frame_size);
    let mut complex = vec![Complex32::default(); frame_size];
    let mut spectral = Vec::new();
    let mut bands = Vec::new();
    let mut mfcc = Vec::new();
    let mut start = 0usize;
    while start < samples.len() {
        if !process_frame(
            &mut complex,
            &window,
            samples,
            start,
            sample_rate,
            frame_size,
            mel,
            &mut spectral,
            &mut bands,
            &mut mfcc,
        ) {
            break;
        }
        start = start.saturating_add(hop_size);
        if samples.len() <= frame_size {
            break;
        }
    }

    ensure_minimum_frame(&mut spectral, &mut bands, &mut mfcc);
    FrameSet {
        spectral,
        bands,
        mfcc,
    }
}

fn process_frame(
    complex: &mut [Complex32],
    window: &[f32],
    samples: &[f32],
    start: usize,
    sample_rate: u32,
    frame_size: usize,
    mel: &MelBank,
    spectral: &mut Vec<SpectralFrame>,
    bands: &mut Vec<BandFrame>,
    mfcc: &mut Vec<Vec<f32>>,
) -> bool {
    fill_windowed(complex, samples, start, window);
    if fft_radix2_inplace(complex).is_err() {
        return false;
    }
    let power = power_spectrum(complex);
    spectral.push(spectral_from_power(&power, sample_rate, frame_size));
    bands.push(bands_from_power(&power, sample_rate, frame_size));
    mfcc.push(mel.mfcc_from_power(&power));
    true
}

fn ensure_minimum_frame(
    spectral: &mut Vec<SpectralFrame>,
    bands: &mut Vec<BandFrame>,
    mfcc: &mut Vec<Vec<f32>>,
) {
    if !spectral.is_empty() {
        return;
    }
    spectral.push(SpectralFrame {
        centroid_hz: 0.0,
        rolloff_hz: 0.0,
        flatness: 0.0,
        bandwidth_hz: 0.0,
    });
    bands.push(BandFrame {
        sub: 0.0,
        low: 0.0,
        mid: 0.0,
        high: 0.0,
        air: 0.0,
    });
    mfcc.push(vec![0.0_f32; 20]);
}

fn fill_windowed(target: &mut [Complex32], samples: &[f32], start: usize, window: &[f32]) {
    for (i, cell) in target.iter_mut().enumerate() {
        let src = samples.get(start + i).copied().unwrap_or(0.0);
        let win = window.get(i).copied().unwrap_or(1.0);
        *cell = Complex32::new(sanitize(src) * win, 0.0);
    }
}

fn sanitize(sample: f32) -> f32 {
    if sample.is_finite() {
        sample.clamp(-1.0, 1.0)
    } else {
        0.0
    }
}

fn power_spectrum(fft: &[Complex32]) -> Vec<f32> {
    let bins = fft.len() / 2 + 1;
    #[cfg(target_arch = "x86_64")]
    {
        if std::is_x86_feature_detected!("sse3") {
            // SAFETY: gated by runtime feature check.
            return unsafe { power_spectrum_sse3(fft, bins) };
        }
    }
    let mut power = Vec::with_capacity(bins);
    for bin in 0..bins {
        let c = fft[bin];
        power.push((c.re * c.re + c.im * c.im).max(0.0));
    }
    power
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "sse3")]
unsafe fn power_spectrum_sse3(fft: &[Complex32], bins: usize) -> Vec<f32> {
    use std::arch::x86_64::*;
    let mut power = vec![0.0_f32; bins];
    let ptr = fft.as_ptr() as *const f32;
    let mut bin = 0usize;
    while bin + 4 <= bins {
        let base = bin * 2;
        let v0 = unsafe { _mm_loadu_ps(ptr.add(base)) };
        let v1 = unsafe { _mm_loadu_ps(ptr.add(base + 4)) };
        let v0_sq = _mm_mul_ps(v0, v0);
        let v1_sq = _mm_mul_ps(v1, v1);
        let sum = _mm_hadd_ps(v0_sq, v1_sq);
        unsafe { _mm_storeu_ps(power.as_mut_ptr().add(bin), sum) };
        bin += 4;
    }
    for i in bin..bins {
        let c = fft[i];
        power[i] = (c.re * c.re + c.im * c.im).max(0.0);
    }
    power
}

fn spectral_from_power(power: &[f32], sample_rate: u32, fft_len: usize) -> SpectralFrame {
    let (sum, centroid_hz) = centroid(power, sample_rate, fft_len);
    SpectralFrame {
        centroid_hz,
        rolloff_hz: rolloff(power, sample_rate, fft_len, sum),
        flatness: flatness(power),
        bandwidth_hz: bandwidth(power, sample_rate, fft_len, sum, centroid_hz),
    }
}

fn centroid(power: &[f32], sample_rate: u32, fft_len: usize) -> (f32, f32) {
    let mut sum = 0.0_f64;
    let mut sum_freq = 0.0_f64;
    let sr = sample_rate.max(1) as f64;
    for (bin, &p) in power.iter().enumerate() {
        let p = p.max(0.0) as f64;
        sum += p;
        sum_freq += p * (bin as f64 * sr / fft_len as f64);
    }
    if sum <= 0.0 {
        return (0.0, 0.0);
    }
    (sum as f32, (sum_freq / sum) as f32)
}

fn rolloff(power: &[f32], sample_rate: u32, fft_len: usize, sum_power: f32) -> f32 {
    let total = sum_power.max(0.0) as f64;
    if total <= 0.0 {
        return 0.0;
    }
    let target = total * ROLLOFF_FRACTION as f64;
    let sr = sample_rate.max(1) as f64;
    let mut cum = 0.0_f64;
    for (bin, &p) in power.iter().enumerate() {
        cum += p.max(0.0) as f64;
        if cum >= target {
            return (bin as f64 * sr / fft_len as f64) as f32;
        }
    }
    (sample_rate as f32) * 0.5
}

fn flatness(power: &[f32]) -> f32 {
    if power.is_empty() {
        return 0.0;
    }
    let eps = 1e-12_f64;
    let mut log_sum = 0.0_f64;
    let mut arith = 0.0_f64;
    for &p in power {
        let p = (p.max(0.0) as f64) + eps;
        log_sum += p.ln();
        arith += p;
    }
    let n = power.len() as f64;
    let geom = (log_sum / n).exp();
    let arith = arith / n;
    if arith <= 0.0 {
        0.0
    } else {
        (geom / arith) as f32
    }
}

fn bandwidth(
    power: &[f32],
    sample_rate: u32,
    fft_len: usize,
    sum_power: f32,
    centroid_hz: f32,
) -> f32 {
    let total = sum_power.max(0.0) as f64;
    if total <= 0.0 {
        return 0.0;
    }
    let sr = sample_rate.max(1) as f64;
    let centroid = centroid_hz.max(0.0) as f64;
    let mut num = 0.0_f64;
    for (bin, &p) in power.iter().enumerate() {
        let p = p.max(0.0) as f64;
        let freq = bin as f64 * sr / fft_len as f64;
        let diff = freq - centroid;
        num += diff * diff * p;
    }
    (num / total).sqrt() as f32
}

fn bands_from_power(power: &[f32], sample_rate: u32, fft_len: usize) -> BandFrame {
    let total: f64 = power.iter().copied().map(|v| v.max(0.0) as f64).sum();
    if total <= 0.0 {
        return BandFrame {
            sub: 0.0,
            low: 0.0,
            mid: 0.0,
            high: 0.0,
            air: 0.0,
        };
    }
    let sub = band_energy(power, sample_rate, fft_len, 20.0, 80.0) / total;
    let low = band_energy(power, sample_rate, fft_len, 80.0, 200.0) / total;
    let mid = band_energy(power, sample_rate, fft_len, 200.0, 2_000.0) / total;
    let high = band_energy(power, sample_rate, fft_len, 2_000.0, 8_000.0) / total;
    let air = band_energy(power, sample_rate, fft_len, 8_000.0, 16_000.0) / total;
    BandFrame {
        sub: sub as f32,
        low: low as f32,
        mid: mid as f32,
        high: high as f32,
        air: air as f32,
    }
}

fn band_energy(power: &[f32], sample_rate: u32, fft_len: usize, lo: f32, hi: f32) -> f64 {
    let lo_bin = freq_to_bin(lo, sample_rate, fft_len);
    let hi_bin = freq_to_bin(hi, sample_rate, fft_len).max(lo_bin + 1);
    power[lo_bin..hi_bin.min(power.len())]
        .iter()
        .copied()
        .map(|v| v.max(0.0) as f64)
        .sum()
}

fn freq_to_bin(freq_hz: f32, sample_rate: u32, fft_len: usize) -> usize {
    let nyquist = sample_rate.max(1) as f32 * 0.5;
    let freq = freq_hz.clamp(0.0, nyquist);
    (((freq * fft_len as f32) / sample_rate.max(1) as f32).floor() as usize).min(fft_len / 2)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analysis::audio::ANALYSIS_SAMPLE_RATE;
    use crate::analysis::frequency_domain::{STFT_FRAME_SIZE, STFT_HOP_SIZE};

    #[test]
    fn compute_frames_returns_at_least_one_frame() {
        let mel = MelBank::new(
            ANALYSIS_SAMPLE_RATE,
            STFT_FRAME_SIZE,
            40,
            20,
            20.0,
            16_000.0,
        );
        let frames = compute_frames(
            &[],
            ANALYSIS_SAMPLE_RATE,
            STFT_FRAME_SIZE,
            STFT_HOP_SIZE,
            &mel,
        );
        assert_eq!(frames.spectral.len(), 1);
        assert_eq!(frames.bands.len(), 1);
        assert_eq!(frames.mfcc.len(), 1);
        assert_eq!(frames.mfcc[0].len(), 20);
    }
}
