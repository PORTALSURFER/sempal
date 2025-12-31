#[cfg(test)]
pub(crate) fn resample_linear(samples: &[f32], input_rate: u32, output_rate: u32) -> Vec<f32> {
    let mut out = Vec::new();
    resample_linear_into(&mut out, samples, input_rate, output_rate);
    out
}

pub(crate) fn resample_linear_into(
    out: &mut Vec<f32>,
    samples: &[f32],
    input_rate: u32,
    output_rate: u32,
) {
    let input_rate = input_rate.max(1);
    let output_rate = output_rate.max(1);
    if samples.is_empty() || input_rate == output_rate {
        out.clear();
        out.extend_from_slice(samples);
        return;
    }
    let duration_seconds = samples.len() as f64 / input_rate as f64;
    let out_len = (duration_seconds * output_rate as f64).round().max(1.0) as usize;
    out.clear();
    out.reserve(out_len.saturating_sub(out.capacity()));
    for i in 0..out_len {
        let t = i as f64 / output_rate as f64;
        let pos = t * input_rate as f64;
        out.push(lerp_sample(samples, pos));
    }
}

fn lerp_sample(samples: &[f32], pos: f64) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let idx0 = pos.floor().max(0.0) as usize;
    let frac = (pos - idx0 as f64).clamp(0.0, 1.0) as f32;
    let idx1 = idx0.saturating_add(1).min(samples.len().saturating_sub(1));
    let a = samples.get(idx0).copied().unwrap_or(0.0);
    let b = samples.get(idx1).copied().unwrap_or(a);
    a + (b - a) * frac
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resample_linear_preserves_endpoints_for_ramp() {
        let input = vec![0.0_f32, 1.0];
        let out = resample_linear(&input, 1, 2);
        assert_eq!(out.len(), 4);
        assert!((out[0] - 0.0).abs() < 1e-6);
        assert!((out[out.len() - 1] - 1.0).abs() < 1e-6);
    }
}
