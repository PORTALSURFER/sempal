pub(super) struct MelBank {
    dct_size: usize,
    filters: Vec<Vec<(usize, f32)>>,
    dct_cos: Vec<f32>,
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
        let dct_cos = build_dct_cos_table(dct_size, filters.len());
        Self {
            dct_size,
            filters,
            dct_cos,
        }
    }

    pub(super) fn mfcc_from_power(&self, power: &[f32]) -> Vec<f32> {
        let mut scratch = MelScratch::new(self.filters.len());
        let mut out = Vec::with_capacity(self.dct_size);
        self.mfcc_from_power_into(power, &mut scratch, &mut out);
        out
    }

    pub(super) fn mfcc_from_power_into(
        &self,
        power: &[f32],
        scratch: &mut MelScratch,
        out: &mut Vec<f32>,
    ) {
        apply_filters_into(&self.filters, power, &mut scratch.mel);
        for (dst, src) in scratch.log.iter_mut().zip(scratch.mel.iter()) {
            *dst = (src.max(1e-12)).ln();
        }
        dct_ii_into(&scratch.log, self.dct_size, &self.dct_cos, out);
    }

    pub(super) fn mel_bands(&self) -> usize {
        self.filters.len()
    }

    pub(super) fn dct_size(&self) -> usize {
        self.dct_size
    }
}

pub(super) struct MelScratch {
    mel: Vec<f32>,
    log: Vec<f32>,
}

impl MelScratch {
    pub(super) fn new(bands: usize) -> Self {
        Self {
            mel: vec![0.0; bands],
            log: vec![0.0; bands],
        }
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

fn build_dct_cos_table(dct_size: usize, bands: usize) -> Vec<f32> {
    if bands == 0 || dct_size == 0 {
        return Vec::new();
    }
    let n = bands as f32;
    let mut table = Vec::with_capacity(dct_size * bands);
    for k in 0..dct_size {
        for m in 0..bands {
            let angle = std::f32::consts::PI * (k as f32) * ((m as f32) + 0.5) / n;
            table.push(angle.cos());
        }
    }
    table
}

fn dct_ii_into(values: &[f32], count: usize, cos_table: &[f32], out: &mut Vec<f32>) {
    out.resize(count, 0.0);
    let bands = values.len();
    if bands == 0 || count == 0 {
        return;
    }
    let expected = count * bands;
    if cos_table.len() != expected {
        for k in 0..count {
            let mut sum = 0.0_f64;
            for (m, &v) in values.iter().enumerate() {
                let angle =
                    std::f64::consts::PI * (k as f64) * ((m as f64) + 0.5) / bands as f64;
                sum += v as f64 * angle.cos();
            }
            out[k] = sum as f32;
        }
        return;
    }
    for k in 0..count {
        let mut sum = 0.0_f64;
        let base = k * bands;
        for m in 0..bands {
            sum += values[m] as f64 * cos_table[base + m] as f64;
        }
        out[k] = sum as f32;
    }
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
