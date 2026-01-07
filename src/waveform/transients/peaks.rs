use super::stats::mean_std_dev;
use ordered_float::NotNan;
use std::collections::{BTreeMap, VecDeque};
use std::ops::Bound::{Excluded, Unbounded};

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

struct SlidingWindowBaseline {
    window: usize,
    entries: VecDeque<Option<NotNan<f32>>>,
    counts: BTreeMap<NotNan<f32>, usize>,
    total: usize,
}

impl SlidingWindowBaseline {
    fn new(window: usize) -> Self {
        Self {
            window,
            entries: VecDeque::with_capacity(window),
            counts: BTreeMap::new(),
            total: 0,
        }
    }

    fn push(&mut self, value: f32) {
        debug_assert!(self.window > 0);
        let sample = NotNan::new(value).ok();
        if let Some(entry) = sample {
            *self.counts.entry(entry).or_insert(0) += 1;
            self.total += 1;
        }
        self.entries.push_back(sample);
        if self.entries.len() > self.window {
            if let Some(outgoing) = self.entries.pop_front().flatten() {
                if let Some(count) = self.counts.get_mut(&outgoing) {
                    *count -= 1;
                    if *count == 0 {
                        self.counts.remove(&outgoing);
                    }
                }
                self.total -= 1;
            }
        }
    }

    fn baseline(&self) -> Option<(f32, f32)> {
        let median = self.median()?;
        let mad = self.mad(median);
        Some((median.into_inner(), mad.max(1.0e-6)))
    }

    fn median(&self) -> Option<NotNan<f32>> {
        if self.total == 0 {
            return None;
        }
        let target = self.total / 2;
        let mut cumulative = 0;
        for (&value, &count) in self.counts.iter() {
            let next = cumulative + count;
            if target < next {
                return Some(value);
            }
            cumulative = next;
        }
        None
    }

    fn mad(&self, median: NotNan<f32>) -> f32 {
        if self.total == 0 {
            return 0.0;
        }
        let target = self.total / 2;
        let zero_count = *self.counts.get(&median).unwrap_or(&0);
        if target < zero_count {
            return 0.0;
        }
        let mut remaining = target - zero_count;
        let median_value = median.into_inner();
        let median_key = median;
        let mut left_iter = self
            .counts
            .range(..median_key.clone())
            .rev()
            .map(|(&value, &count)| (value, count))
            .peekable();
        let mut right_iter = self
            .counts
            .range((Excluded(median_key), Unbounded))
            .map(|(&value, &count)| (value, count))
            .peekable();
        loop {
            match (left_iter.peek(), right_iter.peek()) {
                (Some(&(left_value, left_count)), Some(&(right_value, right_count))) => {
                    let left_diff = median_value - left_value.into_inner();
                    let right_diff = right_value.into_inner() - median_value;
                    if left_diff <= right_diff {
                        if remaining < left_count {
                            return left_diff;
                        }
                        remaining -= left_count;
                        left_iter.next();
                    } else {
                        if remaining < right_count {
                            return right_diff;
                        }
                        remaining -= right_count;
                        right_iter.next();
                    }
                }
                (Some(&(left_value, left_count)), None) => {
                    let diff = median_value - left_value.into_inner();
                    if remaining < left_count {
                        return diff;
                    }
                    remaining -= left_count;
                    left_iter.next();
                }
                (None, Some(&(right_value, right_count))) => {
                    let diff = right_value.into_inner() - median_value;
                    if remaining < right_count {
                        return diff;
                    }
                    remaining -= right_count;
                    right_iter.next();
                }
                (None, None) => break,
            }
        }
        0.0
    }
}

/// Builds per-frame baselines that pair the rolling median with a MAD scale using the previous `window` samples.
/// When no finite samples are available the signal-wide mean/std are returned so that detection stays stable
/// during the initial warm-up.
pub(crate) fn compute_baselines(values: &[f32], window: usize) -> Vec<Baseline> {
    let (global_mean, global_std) = mean_std_dev(values);
    if window == 0 {
        return vec![
            Baseline {
                median: global_mean,
                mad: global_std,
            };
            values.len()
        ];
    }
    let mut baselines = Vec::with_capacity(values.len());
    let mut tracker = SlidingWindowBaseline::new(window);
    for &value in values {
        let baseline = tracker.baseline().unwrap_or((global_mean, global_std));
        baselines.push(Baseline {
            median: baseline.0,
            mad: baseline.1,
        });
        tracker.push(value);
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
