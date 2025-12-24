pub(super) fn normalize_peak_in_place(samples: &mut [f32]) {
    let mut peak = 0.0_f32;
    for &sample in samples.iter() {
        peak = peak.max(sample.abs());
    }
    if !peak.is_finite() || peak <= 0.0 {
        return;
    }
    let gain = 1.0_f32 / peak;
    for sample in samples.iter_mut() {
        *sample = (*sample * gain).clamp(-1.0, 1.0);
    }
}

pub(super) fn normalize_peak_limit_in_place(samples: &mut [f32]) {
    let mut peak = 0.0_f32;
    for &sample in samples.iter() {
        peak = peak.max(sample.abs());
    }
    if !peak.is_finite() || peak <= 1.0 {
        return;
    }
    let gain = 1.0_f32 / peak;
    for sample in samples.iter_mut() {
        *sample *= gain;
    }
}

pub(super) fn normalize_rms_in_place(samples: &mut [f32], target_db: f32) {
    if samples.is_empty() {
        return;
    }
    let rms_value = rms(samples);
    if !rms_value.is_finite() || rms_value <= 0.0 {
        return;
    }
    let target = db_to_linear(target_db);
    if !target.is_finite() || target <= 0.0 {
        return;
    }
    let gain = target / rms_value;
    for sample in samples.iter_mut() {
        *sample *= gain;
    }
}

pub(crate) fn sanitize_samples_in_place(samples: &mut [f32]) {
    for sample in samples.iter_mut() {
        *sample = sanitize_sample(*sample);
    }
}

pub(super) fn rms(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let mut sum = 0.0_f64;
    for &sample in samples {
        let sample = sanitize_sample(sample) as f64;
        sum += sample * sample;
    }
    let mean = sum / samples.len() as f64;
    (mean.max(0.0).sqrt() as f32).min(1.0)
}

pub(super) fn db_to_linear(db: f32) -> f32 {
    10.0_f32.powf(db / 20.0)
}

fn sanitize_sample(sample: f32) -> f32 {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_samples_removes_nan_and_denormals() {
        let mut out = vec![0.0_f32, f32::NAN, f32::MIN_POSITIVE / 2.0];
        sanitize_samples_in_place(&mut out);
        assert_eq!(out.len(), 3);
        assert!(out.iter().all(|v| v.is_finite()));
        assert!(
            out.iter()
                .all(|v| v.abs() == 0.0 || v.abs() >= f32::MIN_POSITIVE)
        );
    }

    #[test]
    fn normalize_peak_scales_to_unit_peak() {
        let mut samples = vec![0.25_f32, -0.5, 0.125];
        normalize_peak_in_place(&mut samples);
        let peak = samples.iter().copied().map(|v| v.abs()).fold(0.0, f32::max);
        assert!((peak - 1.0).abs() < 1e-6);
    }

    #[test]
    fn normalize_rms_targets_expected_level() {
        let mut samples = vec![0.1_f32; 1000];
        let target_db = -20.0;
        normalize_rms_in_place(&mut samples, target_db);
        let measured = rms(&samples);
        let target = db_to_linear(target_db);
        assert!((measured - target).abs() < 1e-3);
    }
}
