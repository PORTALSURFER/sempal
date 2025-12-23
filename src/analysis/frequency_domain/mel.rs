pub(super) struct MelBank {
    dct_size: usize,
    filters: Vec<Vec<(usize, f32)>>,
}

impl MelBank {
    pub(super) fn new(
        sample_rate: u32,
        fft_len: usize,
        mel_bands: usize,
        dct_size: usize,
        f_min: f32,
        f_max: f32,
    ) -> Self {
        let bins = mel_bins(sample_rate, fft_len, mel_bands, f_min, f_max);
        let filters = build_filters(&bins, mel_bands);
        Self { dct_size, filters }
    }

    pub(super) fn mfcc_from_power(&self, power: &[f32]) -> Vec<f32> {
        let mel_energies = apply_filters(&self.filters, power);
        let log_energies: Vec<f32> = mel_energies
            .iter()
            .copied()
            .map(|e| (e.max(1e-12)).ln())
            .collect();
        dct_ii(&log_energies, self.dct_size)
    }
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

fn dct_ii(values: &[f32], count: usize) -> Vec<f32> {
    let n = values.len().max(1) as f32;
    let mut out = Vec::with_capacity(count);
    for k in 0..count {
        let mut sum = 0.0_f64;
        for (m, &v) in values.iter().enumerate() {
            let angle = std::f64::consts::PI * (k as f64) * ((m as f64) + 0.5) / n as f64;
            sum += v as f64 * angle.cos();
        }
        out.push(sum as f32);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analysis::audio::ANALYSIS_SAMPLE_RATE;

    #[test]
    fn mfcc_from_power_returns_expected_length() {
        let bank = MelBank::new(ANALYSIS_SAMPLE_RATE, 1024, 40, 20, 20.0, 16_000.0);
        let power = vec![0.0_f32; 1024 / 2 + 1];
        let mfcc = bank.mfcc_from_power(&power);
        assert_eq!(mfcc.len(), 20);
    }
}
