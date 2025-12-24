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
}
