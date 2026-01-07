use super::normalize::{db_to_linear, rms};
use super::{
    SILENCE_POST_ROLL_SECONDS, SILENCE_PRE_ROLL_SECONDS, SILENCE_THRESHOLD_OFF_DB,
    SILENCE_THRESHOLD_ON_DB,
};

pub(super) fn trim_silence_with_hysteresis(samples: &[f32], sample_rate: u32) -> Vec<f32> {
    if samples.is_empty() || sample_rate == 0 {
        return samples.to_vec();
    }
    let window_size = (sample_rate as f32 * 0.02).round().max(1.0) as usize; // 20ms
    let hop = window_size;
    if samples.len() <= window_size {
        return samples.to_vec();
    }

    let threshold_on = db_to_linear(SILENCE_THRESHOLD_ON_DB);
    let threshold_off = db_to_linear(SILENCE_THRESHOLD_OFF_DB);
    let pre_roll = (sample_rate as f32 * SILENCE_PRE_ROLL_SECONDS)
        .round()
        .max(0.0) as usize; // 10ms
    let post_roll = (sample_rate as f32 * SILENCE_POST_ROLL_SECONDS)
        .round()
        .max(0.0) as usize; // 5ms

    let mut active_start: Option<usize> = None;
    let mut active_end: Option<usize> = None;

    let mut active = false;
    let mut window_start = 0usize;
    while window_start < samples.len() {
        let window_end = (window_start + window_size).min(samples.len());
        let rms_value = rms(&samples[window_start..window_end]);
        if !active {
            if rms_value >= threshold_on {
                active = true;
                active_start = Some(window_start);
                active_end = Some(window_end);
            }
        } else if rms_value >= threshold_off {
            active_end = Some(window_end);
        } else {
            active = false;
        }
        window_start = window_start.saturating_add(hop);
    }

    let Some(active_start) = active_start else {
        return samples.to_vec();
    };
    let Some(active_end) = active_end else {
        return samples.to_vec();
    };

    let trimmed_start = active_start.saturating_sub(pre_roll).min(samples.len());
    let trimmed_end = (active_end + post_roll)
        .max(trimmed_start.saturating_add(1))
        .min(samples.len());
    samples[trimmed_start..trimmed_end].to_vec()
}

/// Identify contiguous non-silent ranges using RMS hysteresis thresholds.
pub(crate) fn detect_non_silent_ranges(samples: &[f32], sample_rate: u32) -> Vec<(usize, usize)> {
    if samples.is_empty() || sample_rate == 0 {
        return Vec::new();
    }
    let window_size = (sample_rate as f32 * 0.02).round().max(1.0) as usize; // 20ms
    if samples.len() <= window_size {
        return vec![(0, samples.len())];
    }
    let params = SilenceParams::new(sample_rate, window_size);
    let ranges = collect_active_ranges(samples, window_size, &params);
    expand_and_merge_ranges(samples.len(), ranges, &params)
}

struct SilenceParams {
    threshold_on: f32,
    threshold_off: f32,
    pre_roll: usize,
    post_roll: usize,
    hop: usize,
}

impl SilenceParams {
    fn new(sample_rate: u32, window_size: usize) -> Self {
        Self {
            threshold_on: db_to_linear(SILENCE_THRESHOLD_ON_DB),
            threshold_off: db_to_linear(SILENCE_THRESHOLD_OFF_DB),
            pre_roll: (sample_rate as f32 * SILENCE_PRE_ROLL_SECONDS)
                .round()
                .max(0.0) as usize,
            post_roll: (sample_rate as f32 * SILENCE_POST_ROLL_SECONDS)
                .round()
                .max(0.0) as usize,
            hop: window_size,
        }
    }
}

fn collect_active_ranges(
    samples: &[f32],
    window_size: usize,
    params: &SilenceParams,
) -> Vec<(usize, usize)> {
    let mut ranges = Vec::new();
    let mut active = false;
    let mut active_start = 0usize;
    let mut active_end = 0usize;

    let mut window_start = 0usize;
    while window_start < samples.len() {
        let window_end = (window_start + window_size).min(samples.len());
        let rms_value = rms(&samples[window_start..window_end]);
        if !active {
            if rms_value >= params.threshold_on {
                active = true;
                active_start = window_start;
                active_end = window_end;
            }
        } else if rms_value >= params.threshold_off {
            active_end = window_end;
        } else {
            active = false;
            ranges.push((active_start, active_end));
        }
        window_start = window_start.saturating_add(params.hop);
    }

    if active {
        ranges.push((active_start, active_end));
    }
    ranges
}

fn expand_and_merge_ranges(
    sample_len: usize,
    ranges: Vec<(usize, usize)>,
    params: &SilenceParams,
) -> Vec<(usize, usize)> {
    let mut expanded = Vec::with_capacity(ranges.len());
    for (start, end) in ranges {
        let trimmed_start = start.saturating_sub(params.pre_roll).min(sample_len);
        let trimmed_end = (end + params.post_roll)
            .max(trimmed_start.saturating_add(1))
            .min(sample_len);
        if trimmed_end > trimmed_start {
            expanded.push((trimmed_start, trimmed_end));
        }
    }
    expanded.sort_by_key(|(start, _)| *start);
    merge_overlapping_ranges(expanded)
}

fn merge_overlapping_ranges(ranges: Vec<(usize, usize)>) -> Vec<(usize, usize)> {
    let mut merged: Vec<(usize, usize)> = Vec::new();
    for (start, end) in ranges {
        if let Some(last) = merged.last_mut()
            && start <= last.1
        {
            last.1 = last.1.max(end);
        } else {
            merged.push((start, end));
        }
    }
    merged
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn silence_hysteresis_keeps_audio_through_off_threshold() {
        let sample_rate = 1000;
        let window_size = (sample_rate as f32 * 0.02).round() as usize;
        let on_amp = db_to_linear(SILENCE_THRESHOLD_ON_DB) * 1.1;
        let off_amp = db_to_linear(SILENCE_THRESHOLD_OFF_DB) * 1.1;

        let mut samples = Vec::new();
        samples.extend(std::iter::repeat(0.0).take(window_size * 2));
        samples.extend(std::iter::repeat(on_amp).take(window_size));
        samples.extend(std::iter::repeat(off_amp).take(window_size));
        samples.extend(std::iter::repeat(0.0).take(window_size));

        let trimmed = trim_silence_with_hysteresis(&samples, sample_rate);
        assert!(trimmed.len() >= window_size * 2);
        let max = trimmed
            .iter()
            .copied()
            .map(|v| v.abs())
            .fold(0.0_f32, f32::max);
        assert!(max >= off_amp * 0.9);
    }

    #[test]
    fn detect_non_silent_ranges_splits_on_silence() {
        let sample_rate = 1000;
        let window_size = (sample_rate as f32 * 0.02).round() as usize;
        let on_amp = db_to_linear(SILENCE_THRESHOLD_ON_DB) * 1.1;
        let mut samples = Vec::new();
        samples.extend(std::iter::repeat(0.0).take(window_size));
        samples.extend(std::iter::repeat(on_amp).take(window_size * 2));
        samples.extend(std::iter::repeat(0.0).take(window_size * 2));
        samples.extend(std::iter::repeat(on_amp).take(window_size));
        let ranges = detect_non_silent_ranges(&samples, sample_rate);
        assert_eq!(ranges.len(), 2);
        assert!(ranges[0].0 < ranges[0].1);
        assert!(ranges[1].0 < ranges[1].1);
    }
}
