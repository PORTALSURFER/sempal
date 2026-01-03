use super::{PANNS_MEL_BANDS, PANNS_MEL_FMAX_HZ, PANNS_MEL_FMIN_HZ};

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

    #[test]
    fn mel_bins_clamp_min_and_max_to_nyquist() {
        let bins = mel_bins(16_000, 512, 8, -10.0, 40_000.0);
        assert_eq!(bins.first().copied(), Some(0));
        assert_eq!(bins.last().copied(), Some(512 / 2));
    }

    #[test]
    fn mel_bins_handles_fmax_below_fmin() {
        let bins = mel_bins(16_000, 512, 8, 10_000.0, 1_000.0);
        assert!(bins.iter().all(|&bin| bin <= 512 / 2));
    }

    #[test]
    fn mel_bins_clamps_min_above_nyquist() {
        let bins = mel_bins(16_000, 512, 8, 20_000.0, 30_000.0);
        assert!(bins.iter().all(|&bin| bin <= 512 / 2));
        assert_eq!(bins.first().copied(), Some(512 / 2));
        assert_eq!(bins.last().copied(), Some(512 / 2));
    }
}
