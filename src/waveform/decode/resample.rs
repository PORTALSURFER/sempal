#[allow(dead_code)]
pub(super) fn resample_linear(
    samples: &[f32],
    channels: usize,
    src_rate: u32,
    dst_rate: u32,
) -> Vec<f32> {
    let channels = channels.max(1);
    if samples.is_empty() || src_rate == 0 || dst_rate == 0 {
        return Vec::new();
    }
    let frames = samples.len() / channels;
    if frames == 0 {
        return Vec::new();
    }
    if frames == 1 {
        return samples.to_vec();
    }
    if src_rate == dst_rate {
        return samples.to_vec();
    }

    let target_frames = ((frames as f64) * (dst_rate as f64) / (src_rate as f64))
        .round()
        .max(1.0) as usize;
    if target_frames == 1 {
        return samples[..channels].to_vec();
    }

    let max_src = (frames.saturating_sub(1)) as f64;
    let step = max_src / (target_frames.saturating_sub(1)) as f64;
    let mut out = Vec::with_capacity(target_frames.saturating_mul(channels));
    for i in 0..target_frames {
        let src_pos = step * i as f64;
        let idx = src_pos.floor() as usize;
        let frac = (src_pos - idx as f64) as f32;
        let next = (idx + 1).min(frames.saturating_sub(1));
        for ch in 0..channels {
            let base = idx.saturating_mul(channels).saturating_add(ch);
            let next_idx = next.saturating_mul(channels).saturating_add(ch);
            let s0 = samples.get(base).copied().unwrap_or(0.0);
            let s1 = samples.get(next_idx).copied().unwrap_or(s0);
            out.push(s0 + (s1 - s0) * frac);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::resample_linear;

    #[test]
    fn resample_empty_returns_empty() {
        let out = resample_linear(&[], 1, 48_000, 44_100);
        assert!(out.is_empty());
    }

    #[test]
    fn resample_single_frame_returns_same() {
        let out = resample_linear(&[0.25, -0.25], 2, 48_000, 96_000);
        assert_eq!(out, vec![0.25, -0.25]);
    }

    #[test]
    fn resample_downsample_preserves_endpoints() {
        let samples = [0.0, 0.5, 0.25, -1.0];
        let out = resample_linear(&samples, 1, 4, 2);
        assert_eq!(out.len(), 2);
        assert!((out[0] - 0.0).abs() < 1e-6);
        assert!((out[1] + 1.0).abs() < 1e-6);
    }

    #[test]
    fn resample_upsample_interpolates() {
        let samples = [0.0, 1.0];
        let out = resample_linear(&samples, 1, 2, 4);
        assert_eq!(out.len(), 4);
        let expected = [0.0, 1.0 / 3.0, 2.0 / 3.0, 1.0];
        for (got, exp) in out.iter().zip(expected) {
            assert!((got - exp).abs() < 1e-5, "got {got} expected {exp}");
        }
    }
}
