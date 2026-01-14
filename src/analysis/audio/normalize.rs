#[cfg(target_arch = "x86_64")]
use std::arch::x86_64::*;

const PARALLEL_THRESHOLD: usize = 1_000_000;

pub(crate) fn normalize_peak_in_place(samples: &mut [f32]) {
    let peak;
    #[cfg(target_arch = "x86_64")]
    {
        if std::is_x86_feature_detected!("avx2") {
            // SAFETY: gated by runtime feature check.
            peak = unsafe { max_abs_avx2(samples) };
        } else if std::is_x86_feature_detected!("sse2") {
            // SAFETY: gated by runtime feature check.
            peak = unsafe { max_abs_sse2(samples) };
        } else {
            peak = max_abs_serial(samples);
        }
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        peak = max_abs_serial(samples);
    }

    if !peak.is_finite() || peak <= 0.0 {
        return;
    }
    let gain = 1.0_f32 / peak;

    #[cfg(target_arch = "x86_64")]
    {
        if std::is_x86_feature_detected!("avx2") {
            // SAFETY: gated by runtime feature check.
            unsafe { scale_and_clamp_avx2(samples, gain) };
            return;
        } else if std::is_x86_feature_detected!("sse2") {
            // SAFETY: gated by runtime feature check.
            unsafe { scale_and_clamp_sse2(samples, gain) };
            return;
        }
    }

    scale_and_clamp_serial(samples, gain);
}

pub(crate) fn normalize_peak_limit_in_place(samples: &mut [f32]) {
    let peak;
    #[cfg(target_arch = "x86_64")]
    {
        if std::is_x86_feature_detected!("avx2") {
            // SAFETY: gated by runtime feature check.
            peak = unsafe { max_abs_avx2(samples) };
        } else if std::is_x86_feature_detected!("sse2") {
            // SAFETY: gated by runtime feature check.
            peak = unsafe { max_abs_sse2(samples) };
        } else {
            peak = max_abs_serial(samples);
        }
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        peak = max_abs_serial(samples);
    }

    if !peak.is_finite() || peak <= 1.0 {
        return;
    }
    let gain = 1.0_f32 / peak;

    #[cfg(target_arch = "x86_64")]
    {
        if std::is_x86_feature_detected!("avx2") {
            // SAFETY: gated by runtime feature check.
            unsafe { scale_in_place_avx2(samples, gain) };
            return;
        } else if std::is_x86_feature_detected!("sse2") {
            // SAFETY: gated by runtime feature check.
            unsafe { scale_in_place_sse2(samples, gain) };
            return;
        }
    }

    scale_in_place_serial(samples, gain);
}

pub(crate) fn normalize_rms_in_place(samples: &mut [f32], target_db: f32) {
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
        if std::is_x86_feature_detected!("avx2") {
            // SAFETY: gated by runtime feature check.
            unsafe { scale_in_place_avx2(samples, gain) };
            return;
        } else if std::is_x86_feature_detected!("sse2") {
            // SAFETY: gated by runtime feature check.
            unsafe { scale_in_place_sse2(samples, gain) };
            return;
        }
    }

    scale_in_place_serial(samples, gain);
}

pub(crate) fn sanitize_samples_in_place(samples: &mut [f32]) {
    for sample in samples.iter_mut() {
        *sample = sanitize_sample(*sample);
    }
}

pub(crate) fn rms(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let sum = {
        #[cfg(target_arch = "x86_64")]
        {
            if std::is_x86_feature_detected!("avx2") {
                // SAFETY: gated by runtime feature check.
                unsafe { sum_sq_avx2(samples) }
            } else if std::is_x86_feature_detected!("sse2") {
                // SAFETY: gated by runtime feature check.
                unsafe { sum_sq_sse2(samples) }
            } else {
                sum_sq_serial(samples)
            }
        }
        #[cfg(not(target_arch = "x86_64"))]
        {
            sum_sq_serial(samples)
        }
    };

    let mean = sum / samples.len() as f64;
    (mean.max(0.0).sqrt() as f32).min(1.0)
}

fn max_abs_serial(samples: &[f32]) -> f32 {
    samples.iter().fold(0.0_f32, |m, &s| m.max(s.abs()))
}

fn scale_in_place_serial(samples: &mut [f32], gain: f32) {
    for sample in samples.iter_mut() {
        *sample *= gain;
    }
}

fn scale_and_clamp_serial(samples: &mut [f32], gain: f32) {
    for sample in samples.iter_mut() {
        *sample = (*sample * gain).clamp(-1.0, 1.0);
    }
}

fn sum_sq_serial(samples: &[f32]) -> f64 {
    samples.iter().fold(0.0_f64, |acc, &s| {
        let s = sanitize_sample(s) as f64;
        acc + s * s
    })
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
unsafe fn max_abs_avx2(samples: &[f32]) -> f32 {
    use rayon::prelude::*;

    if samples.len() >= PARALLEL_THRESHOLD {
        return samples
            .par_chunks(PARALLEL_THRESHOLD)
            .map(|chunk| unsafe { max_abs_avx2_impl(chunk) })
            .reduce(|| 0.0, |a, b| a.max(b));
    }
    unsafe { max_abs_avx2_impl(samples) }
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
unsafe fn max_abs_avx2_impl(samples: &[f32]) -> f32 {
    unsafe {
        let mut max_v = _mm256_set1_ps(0.0);
        let sign_mask = _mm256_castsi256_ps(_mm256_set1_epi32(0x7fffffff_u32 as i32));
        let chunks = samples.chunks_exact(8);
        let rem = chunks.remainder();
        for chunk in chunks {
            let v = _mm256_loadu_ps(chunk.as_ptr());
            let abs = _mm256_and_ps(v, sign_mask);
            max_v = _mm256_max_ps(max_v, abs);
        }
        let mut tmp = [0.0_f32; 8];
        _mm256_storeu_ps(tmp.as_mut_ptr(), max_v);
        let mut max = tmp.iter().fold(0.0_f32, |m, &v| m.max(v));
        for &val in rem {
            max = max.max(val.abs());
        }
        max
    }
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "sse2")]
unsafe fn max_abs_sse2(samples: &[f32]) -> f32 {
    use rayon::prelude::*;

    if samples.len() >= PARALLEL_THRESHOLD {
        return samples
            .par_chunks(PARALLEL_THRESHOLD)
            .map(|chunk| unsafe { max_abs_sse2_impl(chunk) })
            .reduce(|| 0.0, |a, b| a.max(b));
    }
    unsafe { max_abs_sse2_impl(samples) }
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "sse2")]
unsafe fn max_abs_sse2_impl(samples: &[f32]) -> f32 {
    unsafe {
        let mut max_v = _mm_set1_ps(0.0);
        let sign_mask = _mm_castsi128_ps(_mm_set1_epi32(0x7fffffff_u32 as i32));
        let chunks = samples.chunks_exact(4);
        let rem = chunks.remainder();
        for chunk in chunks {
            let v = _mm_loadu_ps(chunk.as_ptr());
            let abs = _mm_and_ps(v, sign_mask);
            max_v = _mm_max_ps(max_v, abs);
        }
        let mut tmp = [0.0_f32; 4];
        _mm_storeu_ps(tmp.as_mut_ptr(), max_v);
        let mut max = tmp.iter().fold(0.0_f32, |m, &v| m.max(v));
        for &val in rem {
            max = max.max(val.abs());
        }
        max
    }
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
unsafe fn scale_in_place_avx2(samples: &mut [f32], gain: f32) {
    use rayon::prelude::*;

    if samples.len() >= PARALLEL_THRESHOLD {
        samples.par_chunks_mut(PARALLEL_THRESHOLD).for_each(|chunk| {
            unsafe { scale_in_place_avx2_impl(chunk, gain) };
        });
        return;
    }
    unsafe { scale_in_place_avx2_impl(samples, gain) };
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
unsafe fn scale_in_place_avx2_impl(samples: &mut [f32], gain: f32) {
    unsafe {
        let gain_v = _mm256_set1_ps(gain);
        let chunk_count = samples.len() / 8;
        let (chunk_part, remainder_part) = samples.split_at_mut(chunk_count * 8);
        
        for chunk in chunk_part.chunks_exact_mut(8) {
            let v = _mm256_loadu_ps(chunk.as_ptr());
            let scaled = _mm256_mul_ps(v, gain_v);
            _mm256_storeu_ps(chunk.as_mut_ptr(), scaled);
        }
        for sample in remainder_part {
            *sample *= gain;
        }
    }
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "sse2")]
unsafe fn scale_in_place_sse2(samples: &mut [f32], gain: f32) {
    use rayon::prelude::*;

    if samples.len() >= PARALLEL_THRESHOLD {
        samples.par_chunks_mut(PARALLEL_THRESHOLD).for_each(|chunk| {
            unsafe { scale_in_place_sse2_impl(chunk, gain) };
        });
        return;
    }
    unsafe { scale_in_place_sse2_impl(samples, gain) };
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "sse2")]
unsafe fn scale_in_place_sse2_impl(samples: &mut [f32], gain: f32) {
    unsafe {
        let gain_v = _mm_set1_ps(gain);
        let chunk_count = samples.len() / 4;
        let (chunk_part, remainder_part) = samples.split_at_mut(chunk_count * 4);
        
        for chunk in chunk_part.chunks_exact_mut(4) {
            let v = _mm_loadu_ps(chunk.as_ptr());
            let scaled = _mm_mul_ps(v, gain_v);
            _mm_storeu_ps(chunk.as_mut_ptr(), scaled);
        }
        for sample in remainder_part {
            *sample *= gain;
        }
    }
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
unsafe fn scale_and_clamp_avx2(samples: &mut [f32], gain: f32) {
    use rayon::prelude::*;

    if samples.len() >= PARALLEL_THRESHOLD {
        samples.par_chunks_mut(PARALLEL_THRESHOLD).for_each(|chunk| {
            unsafe { scale_and_clamp_avx2_impl(chunk, gain) };
        });
        return;
    }
    unsafe { scale_and_clamp_avx2_impl(samples, gain) };
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
unsafe fn scale_and_clamp_avx2_impl(samples: &mut [f32], gain: f32) {
    unsafe {
        let gain_v = _mm256_set1_ps(gain);
        let min_v = _mm256_set1_ps(-1.0);
        let max_v = _mm256_set1_ps(1.0);
        let chunk_count = samples.len() / 8;
        let (chunk_part, remainder_part) = samples.split_at_mut(chunk_count * 8);
        
        for chunk in chunk_part.chunks_exact_mut(8) {
            let v = _mm256_loadu_ps(chunk.as_ptr());
            let scaled = _mm256_mul_ps(v, gain_v);
            let clamped = _mm256_min_ps(_mm256_max_ps(scaled, min_v), max_v);
            _mm256_storeu_ps(chunk.as_mut_ptr(), clamped);
        }
        for sample in remainder_part {
            *sample = (*sample * gain).clamp(-1.0, 1.0);
        }
    }
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "sse2")]
unsafe fn scale_and_clamp_sse2(samples: &mut [f32], gain: f32) {
    use rayon::prelude::*;

    if samples.len() >= PARALLEL_THRESHOLD {
        samples.par_chunks_mut(PARALLEL_THRESHOLD).for_each(|chunk| {
            unsafe { scale_and_clamp_sse2_impl(chunk, gain) };
        });
        return;
    }
    unsafe { scale_and_clamp_sse2_impl(samples, gain) };
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "sse2")]
unsafe fn scale_and_clamp_sse2_impl(samples: &mut [f32], gain: f32) {
    unsafe {
        let gain_v = _mm_set1_ps(gain);
        let min_v = _mm_set1_ps(-1.0);
        let max_v = _mm_set1_ps(1.0);
        let chunk_count = samples.len() / 4;
        let (chunk_part, remainder_part) = samples.split_at_mut(chunk_count * 4);
        
        for chunk in chunk_part.chunks_exact_mut(4) {
            let v = _mm_loadu_ps(chunk.as_ptr());
            let scaled = _mm_mul_ps(v, gain_v);
            let clamped = _mm_min_ps(_mm_max_ps(scaled, min_v), max_v);
            _mm_storeu_ps(chunk.as_mut_ptr(), clamped);
        }
        for sample in remainder_part {
            *sample = (*sample * gain).clamp(-1.0, 1.0);
        }
    }
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
unsafe fn sum_sq_avx2(samples: &[f32]) -> f64 {
    use rayon::prelude::*;

    if samples.len() >= PARALLEL_THRESHOLD {
        return samples
            .par_chunks(PARALLEL_THRESHOLD)
            .map(|chunk| unsafe { sum_sq_avx2_impl(chunk) })
            .sum::<f64>();
    }
    unsafe { sum_sq_avx2_impl(samples) }
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
unsafe fn sum_sq_avx2_impl(samples: &[f32]) -> f64 {
    unsafe {
        // Use 4 separate accumulators to reduce dependency chains and improve precision
        let mut sum_v0 = _mm256_set1_ps(0.0);
        let mut sum_v1 = _mm256_set1_ps(0.0);
        let mut sum_v2 = _mm256_set1_ps(0.0);
        let mut sum_v3 = _mm256_set1_ps(0.0);
        
        let mut chunks = samples.chunks_exact(32);
        for chunk in &mut chunks {
            let v0 = _mm256_loadu_ps(chunk[0..8].as_ptr());
            let v1 = _mm256_loadu_ps(chunk[8..16].as_ptr());
            let v2 = _mm256_loadu_ps(chunk[16..24].as_ptr());
            let v3 = _mm256_loadu_ps(chunk[24..32].as_ptr());
            
            sum_v0 = _mm256_add_ps(sum_v0, _mm256_mul_ps(v0, v0));
            sum_v1 = _mm256_add_ps(sum_v1, _mm256_mul_ps(v1, v1));
            sum_v2 = _mm256_add_ps(sum_v2, _mm256_mul_ps(v2, v2));
            sum_v3 = _mm256_add_ps(sum_v3, _mm256_mul_ps(v3, v3));
        }
        
        let sum_v = _mm256_add_ps(_mm256_add_ps(sum_v0, sum_v1), _mm256_add_ps(sum_v2, sum_v3));
        let mut tmp = [0.0_f32; 8];
        _mm256_storeu_ps(tmp.as_mut_ptr(), sum_v);
        
        let mut sum = tmp.iter().map(|&v| v as f64).sum::<f64>();
        for &val in chunks.remainder() {
            let val = val as f64;
            sum += val * val;
        }
        sum
    }
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "sse2")]
unsafe fn sum_sq_sse2(samples: &[f32]) -> f64 {
    use rayon::prelude::*;

    if samples.len() >= PARALLEL_THRESHOLD {
        return samples
            .par_chunks(PARALLEL_THRESHOLD)
            .map(|chunk| unsafe { sum_sq_sse2_impl(chunk) })
            .sum::<f64>();
    }
    unsafe { sum_sq_sse2_impl(samples) }
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "sse2")]
unsafe fn sum_sq_sse2_impl(samples: &[f32]) -> f64 {
    unsafe {
        let mut sum_v = _mm_set1_ps(0.0);
        let chunks = samples.chunks_exact(4);
        let rem = chunks.remainder();
        for chunk in chunks {
            let v = _mm_loadu_ps(chunk.as_ptr());
            let sq = _mm_mul_ps(v, v);
            sum_v = _mm_add_ps(sum_v, sq);
        }
        let mut tmp = [0.0_f32; 4];
        _mm_storeu_ps(tmp.as_mut_ptr(), sum_v);
        let mut sum = tmp.iter().map(|&v| v as f64).sum::<f64>();
        for &val in rem {
            let val = val as f64;
            sum += val * val;
        }
        sum
    }
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

    #[test]
    fn normalize_large_parallel_correctness() {
        // Use 1.5M samples to trigger PARALLEL_THRESHOLD (1M)
        let count = 1_500_000;
        let mut samples = vec![0.0_f32; count];
        for (i, s) in samples.iter_mut().enumerate() {
            *s = (i as f32).sin() * 0.5;
        }
        
        // Save original peak for verification
        let mut peak = 0.0_f32;
        for &s in &samples {
             peak = peak.max(s.abs());
        }

        normalize_peak_in_place(&mut samples);
        
        let new_peak = samples.iter().copied().map(|v| v.abs()).fold(0.0, f32::max);
        assert!((new_peak - 1.0).abs() < 1e-5, "Peak should be 1.0, got {}", new_peak);
        
        // Also verify RMS on large buffer
        let target_db = -15.0;
        normalize_rms_in_place(&mut samples, target_db);
        let measured = rms(&samples);
        let target = db_to_linear(target_db);
        assert!((measured - target).abs() < 1e-4, "RMS should be {}, got {}", target, measured);
    }
}
