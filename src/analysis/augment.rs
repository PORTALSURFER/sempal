use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};

#[derive(Debug, Clone)]
pub struct AugmentOptions {
    pub enabled: bool,
    pub copies_per_sample: usize,
    pub gain_jitter_db: f32,
    pub noise_std: f32,
    pub pitch_semitones: f32,
    pub time_stretch_pct: f32,
    pub seed: u64,
}

impl AugmentOptions {
    pub fn rng(&self) -> StdRng {
        StdRng::seed_from_u64(self.seed)
    }
}

pub fn augment_waveform(samples: &[f32], rng: &mut StdRng, options: &AugmentOptions) -> Vec<f32> {
    if samples.is_empty() {
        return Vec::new();
    }
    let mut out = samples.to_vec();

    let gain_db = jitter_range(rng, options.gain_jitter_db);
    let gain = 10.0_f32.powf(gain_db / 20.0);
    for v in &mut out {
        *v *= gain;
    }

    if options.noise_std > 0.0 {
        for v in &mut out {
            let noise = rng.random_range(-1.0_f32..=1.0_f32) * options.noise_std;
            *v += noise;
        }
    }

    let pitch = jitter_range(rng, options.pitch_semitones);
    let pitch_factor = 2.0_f32.powf(pitch / 12.0);
    let stretch = jitter_range(rng, options.time_stretch_pct);
    let stretch_factor = 1.0 + stretch;
    let total_factor = (pitch_factor * stretch_factor).max(0.25).min(4.0);

    let mut stretched = resample_linear(&out, total_factor);
    for v in &mut stretched {
        if !v.is_finite() {
            *v = 0.0;
        } else {
            *v = v.clamp(-1.0, 1.0);
        }
    }
    stretched
}

fn jitter_range(rng: &mut StdRng, range: f32) -> f32 {
    if range <= 0.0 {
        0.0
    } else {
        rng.random_range(-range..=range)
    }
}

fn resample_linear(samples: &[f32], speed: f32) -> Vec<f32> {
    if samples.is_empty() || speed <= 0.0 {
        return Vec::new();
    }
    let new_len = ((samples.len() as f32) / speed).round().max(1.0) as usize;
    let mut out = vec![0.0f32; new_len];
    let max_idx = samples.len().saturating_sub(1) as f32;
    for i in 0..new_len {
        let pos = (i as f32) * speed;
        let idx = pos.floor() as usize;
        let frac = pos - (idx as f32);
        let idx1 = idx.min(samples.len().saturating_sub(1));
        let idx2 = (idx + 1).min(samples.len().saturating_sub(1));
        let a = samples[idx1];
        let b = samples[idx2];
        out[i] = a + (b - a) * frac;
    }
    if !out.is_empty() {
        let last = out.len() - 1;
        out[last] = samples.get(max_idx as usize).copied().unwrap_or(out[last]);
    }
    out
}
