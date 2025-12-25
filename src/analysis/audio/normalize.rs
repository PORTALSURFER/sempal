pub(super) fn normalize_peak_in_place(samples: &mut [f32]) {
    let mut peak = 0.0_f32;
    #[cfg(target_arch = "x86_64")]
    {
        if std::is_x86_feature_detected!("sse2") && samples.iter().all(|s| s.is_finite()) {
            // SAFETY: gated by runtime feature check.
            peak = unsafe { max_abs_sse2(samples) };
        } else {
            for &sample in samples.iter() {
                peak = peak.max(sample.abs());
            }
        }
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        for &sample in samples.iter() {
            peak = peak.max(sample.abs());
        }
    }
    if !peak.is_finite() || peak <= 0.0 {
        return;
    }
    let gain = 1.0_f32 / peak;
    #[cfg(target_arch = "x86_64")]
    {
        if std::is_x86_feature_detected!("sse2") {
            // SAFETY: gated by runtime feature check.
            unsafe { scale_and_clamp_sse2(samples, gain) };
            return;
        }
    }
    for sample in samples.iter_mut() {
        *sample = (*sample * gain).clamp(-1.0, 1.0);
    }
}

pub(super) fn normalize_peak_limit_in_place(samples: &mut [f32]) {
    let mut peak = 0.0_f32;
    #[cfg(target_arch = "x86_64")]
    {
        if std::is_x86_feature_detected!("sse2") && samples.iter().all(|s| s.is_finite()) {
            // SAFETY: gated by runtime feature check.
            peak = unsafe { max_abs_sse2(samples) };
        } else {
            for &sample in samples.iter() {
                peak = peak.max(sample.abs());
            }
        }
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        for &sample in samples.iter() {
            peak = peak.max(sample.abs());
        }
    }
    if !peak.is_finite() || peak <= 1.0 {
        return;
    }
    let gain = 1.0_f32 / peak;
    #[cfg(target_arch = "x86_64")]
    {
        if std::is_x86_feature_detected!("sse2") {
            // SAFETY: gated by runtime feature check.
            unsafe { scale_in_place_sse2(samples, gain) };
            return;
        }
    }
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
    #[cfg(target_arch = "x86_64")]
    {
        if std::is_x86_feature_detected!("sse2") {
            // SAFETY: gated by runtime feature check.
            unsafe { scale_in_place_sse2(samples, gain) };
            return;
        }
    }
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
    #[cfg(target_arch = "x86_64")]
    {
        if std::is_x86_feature_detected!("sse2")
            && samples
                .iter()
                .all(|s| s.is_finite() && (s.abs() == 0.0 || s.abs() >= f32::MIN_POSITIVE))
        {
            // SAFETY: gated by runtime feature check; finiteness checked above.
            return unsafe { rms_sse2(samples) };
        }
    }
    let mut sum = 0.0_f64;
    for &sample in samples {
        let sample = sanitize_sample(sample) as f64;
        sum += sample * sample;
    }
    let mean = sum / samples.len() as f64;
    (mean.max(0.0).sqrt() as f32).min(1.0)
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "sse2")]
unsafe fn max_abs_sse2(samples: &[f32]) -> f32 {
    use std::arch::x86_64::*;
    let mut max_v = _mm_set1_ps(0.0);
    let sign_mask = _mm_castsi128_ps(_mm_set1_epi32(0x7fffffff_u32 as i32));
    let mut chunks = samples.chunks_exact(4);
    for chunk in &mut chunks {
        let v = unsafe { _mm_loadu_ps(chunk.as_ptr()) };
        let abs = _mm_and_ps(v, sign_mask);
        max_v = _mm_max_ps(max_v, abs);
    }
    let mut max = 0.0_f32;
    let mut tmp = [0.0_f32; 4];
    unsafe { _mm_storeu_ps(tmp.as_mut_ptr(), max_v) };
    for &val in &tmp {
        max = max.max(val);
    }
    for &val in chunks.remainder() {
        max = max.max(val.abs());
    }
    max
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "sse2")]
unsafe fn scale_in_place_sse2(samples: &mut [f32], gain: f32) {
    use std::arch::x86_64::*;
    let gain_v = _mm_set1_ps(gain);
    let mut chunks = samples.chunks_exact_mut(4);
    for chunk in &mut chunks {
        let v = unsafe { _mm_loadu_ps(chunk.as_ptr()) };
        let scaled = _mm_mul_ps(v, gain_v);
        unsafe { _mm_storeu_ps(chunk.as_mut_ptr(), scaled) };
    }
    for sample in chunks.into_remainder() {
        *sample *= gain;
    }
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "sse2")]
unsafe fn scale_and_clamp_sse2(samples: &mut [f32], gain: f32) {
    use std::arch::x86_64::*;
    let gain_v = _mm_set1_ps(gain);
    let min_v = _mm_set1_ps(-1.0);
    let max_v = _mm_set1_ps(1.0);
    let mut chunks = samples.chunks_exact_mut(4);
    for chunk in &mut chunks {
        let v = unsafe { _mm_loadu_ps(chunk.as_ptr()) };
        let scaled = _mm_mul_ps(v, gain_v);
        let clamped = _mm_min_ps(_mm_max_ps(scaled, min_v), max_v);
        unsafe { _mm_storeu_ps(chunk.as_mut_ptr(), clamped) };
    }
    for sample in chunks.into_remainder() {
        *sample = (*sample * gain).clamp(-1.0, 1.0);
    }
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "sse2")]
unsafe fn rms_sse2(samples: &[f32]) -> f32 {
    use std::arch::x86_64::*;
    let mut sum_v = _mm_set1_ps(0.0);
    let mut chunks = samples.chunks_exact(4);
    for chunk in &mut chunks {
        let v = unsafe { _mm_loadu_ps(chunk.as_ptr()) };
        let sq = _mm_mul_ps(v, v);
        sum_v = _mm_add_ps(sum_v, sq);
    }
    let mut tmp = [0.0_f32; 4];
    unsafe { _mm_storeu_ps(tmp.as_mut_ptr(), sum_v) };
    let mut sum = (tmp[0] + tmp[1] + tmp[2] + tmp[3]) as f64;
    for &val in chunks.remainder() {
        let val = val as f64;
        sum += val * val;
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
