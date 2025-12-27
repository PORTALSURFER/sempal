use super::stats::{mean_std_dev, median_mad};

#[derive(Clone, Copy, Debug)]
pub(crate) struct SensitivityParams {
    pub(crate) k_high: f32,
    pub(crate) k_low: f32,
    pub(crate) floor_quantile: f32,
    pub(crate) min_gap_seconds: f32,
}

impl SensitivityParams {
    pub(crate) fn from_sensitivity(sensitivity: f32) -> Self {
        let sensitivity = sensitivity.clamp(0.0, 1.0);
        let k_high = 6.0 - 3.0 * sensitivity;
        let k_low = k_high * 0.5;
        let floor_quantile = 0.5 + (1.0 - sensitivity) * 0.2;
        let min_gap_seconds = 0.06 + (1.0 - sensitivity) * 0.06;
        Self {
            k_high,
            k_low,
            floor_quantile,
            min_gap_seconds,
        }
    }

    pub(crate) fn from_overrides(
        k_high: f32,
        k_low: f32,
        floor_quantile: f32,
        min_gap_seconds: f32,
    ) -> Self {
        Self {
            k_high,
            k_low,
            floor_quantile,
            min_gap_seconds,
        }
    }

    pub(crate) fn relaxed(self) -> Self {
        Self {
            k_high: (self.k_high * 0.75).max(1.0),
            k_low: (self.k_low * 0.75).max(0.5),
            floor_quantile: (self.floor_quantile - 0.1).max(0.1),
            min_gap_seconds: self.min_gap_seconds,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Baseline {
    pub(crate) median: f32,
    pub(crate) mad: f32,
}

pub(crate) fn compute_baselines(values: &[f32], window: usize) -> Vec<Baseline> {
    let (global_mean, global_std) = mean_std_dev(values);
    let mut baselines = Vec::with_capacity(values.len());
    for i in 0..values.len() {
        let start = i.saturating_sub(window);
        let slice = &values[start..i];
        let (median, mad) = if slice.is_empty() {
            (global_mean, global_std)
        } else {
            median_mad(slice)
        };
        baselines.push(Baseline { median, mad });
    }
    baselines
}

pub(crate) fn pick_peaks_hysteresis(
    novelty: &[f32],
    baselines: &[Baseline],
    params: SensitivityParams,
    global_floor: f32,
    min_gap_frames: usize,
    max_transients: usize,
) -> Vec<(usize, f32)> {
    let mut peaks: Vec<(usize, f32)> = Vec::new();
    let mut last_frame: Option<usize> = None;
    let mut last_strength = 0.0f32;
    let mut armed = true;
    for i in 1..novelty.len().saturating_sub(1) {
        let strength = novelty[i];
        let baseline = baselines.get(i).copied().unwrap_or(Baseline {
            median: 0.0,
            mad: 1.0,
        });
        let high = baseline.median + baseline.mad * params.k_high;
        let low = baseline.median + baseline.mad * params.k_low;
        if strength < low {
            armed = true;
        }
        if !armed {
            continue;
        }
        if strength < global_floor || strength < high {
            continue;
        }
        if strength < novelty[i - 1] || strength < novelty[i + 1] {
            continue;
        }
        let frame = i;
        if let Some(prev_frame) = last_frame {
            let distance = frame.saturating_sub(prev_frame);
            if distance < min_gap_frames {
                if strength > last_strength {
                    if let Some((last_frame, last_strength)) = peaks.last_mut() {
                        *last_frame = frame;
                        *last_strength = strength;
                    }
                    last_frame = Some(frame);
                    last_strength = strength;
                }
                continue;
            }
        }
        peaks.push((frame, strength));
        last_frame = Some(frame);
        last_strength = strength;
        armed = false;
    }
    if peaks.len() > max_transients {
        peaks.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        peaks.truncate(max_transients);
        peaks.sort_by_key(|(frame, _)| *frame);
    }
    peaks
}

pub(crate) fn smooth_values(values: &[f32], radius: usize) -> Vec<f32> {
    if values.is_empty() || radius == 0 {
        return values.to_vec();
    }
    let mut out = Vec::with_capacity(values.len());
    for i in 0..values.len() {
        let start = i.saturating_sub(radius);
        let end = (i + radius + 1).min(values.len());
        let mut sum = 0.0f32;
        let mut count = 0.0f32;
        for value in &values[start..end] {
            if value.is_finite() {
                sum += *value;
                count += 1.0;
            }
        }
        out.push(if count > 0.0 { sum / count } else { 0.0 });
    }
    out
}

pub(crate) fn percentile(values: &[f32], quantile: f32) -> f32 {
    if values.is_empty() {
        return 0.0;
    }
    let mut sorted = values
        .iter()
        .copied()
        .filter(|value| value.is_finite())
        .collect::<Vec<f32>>();
    if sorted.is_empty() {
        return 0.0;
    }
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let q = quantile.clamp(0.0, 1.0);
    let idx = ((sorted.len() - 1) as f32 * q).round() as usize;
    sorted[idx]
}
