use std::f32::consts::PI;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Complex32 {
    pub(crate) re: f32,
    pub(crate) im: f32,
}

impl Complex32 {
    pub(crate) fn new(re: f32, im: f32) -> Self {
        Self { re, im }
    }

    pub(crate) fn mul(self, other: Self) -> Self {
        Self {
            re: self.re * other.re - self.im * other.im,
            im: self.re * other.im + self.im * other.re,
        }
    }

    pub(crate) fn add(self, other: Self) -> Self {
        Self {
            re: self.re + other.re,
            im: self.im + other.im,
        }
    }

    pub(crate) fn sub(self, other: Self) -> Self {
        Self {
            re: self.re - other.re,
            im: self.im - other.im,
        }
    }
}

pub(crate) fn hann_window(length: usize) -> Vec<f32> {
    if length <= 1 {
        return vec![1.0_f32; length.max(1)];
    }
    let denom = (length - 1) as f32;
    (0..length)
        .map(|n| 0.5_f32 * (1.0 - (2.0 * PI * n as f32 / denom).cos()))
        .collect()
}

pub(crate) fn fft_radix2_inplace(buffer: &mut [Complex32]) -> Result<(), String> {
    let n = buffer.len();
    if n == 0 || !n.is_power_of_two() {
        return Err(format!("FFT length must be power-of-two, got {n}"));
    }
    bit_reverse_permute(buffer);
    let mut len = 2usize;
    while len <= n {
        let angle = -2.0_f32 * PI / len as f32;
        let wlen = Complex32::new(angle.cos(), angle.sin());
        for start in (0..n).step_by(len) {
            let mut w = Complex32::new(1.0, 0.0);
            for i in 0..(len / 2) {
                let u = buffer[start + i];
                let v = buffer[start + i + len / 2].mul(w);
                buffer[start + i] = u.add(v);
                buffer[start + i + len / 2] = u.sub(v);
                w = w.mul(wlen);
            }
        }
        len *= 2;
    }
    Ok(())
}

pub(crate) struct FftPlan {
    len: usize,
    bit_swaps: Vec<(usize, usize)>,
    twiddles: Vec<Vec<Complex32>>,
}

impl FftPlan {
    pub(crate) fn new(len: usize) -> Result<Self, String> {
        if len == 0 || !len.is_power_of_two() {
            return Err(format!("FFT length must be power-of-two, got {len}"));
        }
        Ok(Self {
            len,
            bit_swaps: build_bit_swaps(len),
            twiddles: build_twiddle_tables(len),
        })
    }
}

pub(crate) fn fft_radix2_inplace_with_plan(
    buffer: &mut [Complex32],
    plan: &FftPlan,
) -> Result<(), String> {
    if buffer.len() != plan.len {
        return Err(format!(
            "FFT length mismatch: buffer {} plan {}",
            buffer.len(),
            plan.len
        ));
    }
    apply_bit_swaps(buffer, &plan.bit_swaps);
    for stage in &plan.twiddles {
        apply_stage(buffer, stage);
    }
    Ok(())
}

fn bit_reverse_permute(buffer: &mut [Complex32]) {
    let n = buffer.len();
    let mut j = 0usize;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j ^= bit;
        if i < j {
            buffer.swap(i, j);
        }
    }
}

fn build_bit_swaps(len: usize) -> Vec<(usize, usize)> {
    let mut swaps = Vec::new();
    let mut j = 0usize;
    for i in 1..len {
        let mut bit = len >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j ^= bit;
        if i < j {
            swaps.push((i, j));
        }
    }
    swaps
}

fn apply_bit_swaps(buffer: &mut [Complex32], swaps: &[(usize, usize)]) {
    for &(i, j) in swaps {
        buffer.swap(i, j);
    }
}

fn build_twiddle_tables(len: usize) -> Vec<Vec<Complex32>> {
    let mut tables = Vec::new();
    let mut step = 2usize;
    while step <= len {
        let half = step / 2;
        let angle = -2.0_f32 * PI / step as f32;
        let mut stage = Vec::with_capacity(half);
        for i in 0..half {
            let theta = angle * i as f32;
            let (sin, cos) = theta.sin_cos();
            stage.push(Complex32::new(cos, sin));
        }
        tables.push(stage);
        step *= 2;
    }
    tables
}

fn apply_stage(buffer: &mut [Complex32], twiddles: &[Complex32]) {
    let half = twiddles.len();
    let step = half * 2;
    for start in (0..buffer.len()).step_by(step) {
        for i in 0..half {
            let u = buffer[start + i];
            let v = buffer[start + i + half].mul(twiddles[i]);
            buffer[start + i] = u.add(v);
            buffer[start + i + half] = u.sub(v);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hann_window_is_symmetric_and_zero_at_edges() {
        let w = hann_window(8);
        assert!((w[0]).abs() < 1e-6);
        assert!((w[7]).abs() < 1e-6);
        assert!((w[1] - w[6]).abs() < 1e-6);
    }

    #[test]
    fn fft_produces_expected_bin_for_constant_signal() {
        let mut buf = vec![Complex32::new(1.0, 0.0); 8];
        fft_radix2_inplace(&mut buf).unwrap();
        assert!((buf[0].re - 8.0).abs() < 1e-4);
        for bin in 1..8 {
            assert!(buf[bin].re.abs() < 1e-4);
            assert!(buf[bin].im.abs() < 1e-4);
        }
    }

    #[test]
    fn fft_plan_matches_plain_fft() {
        let mut buf = vec![Complex32::new(0.0, 0.0); 16];
        for (i, cell) in buf.iter_mut().enumerate() {
            cell.re = (i as f32 * 0.25).sin();
        }
        let mut planned = buf.clone();
        fft_radix2_inplace(&mut buf).unwrap();
        let plan = FftPlan::new(planned.len()).unwrap();
        fft_radix2_inplace_with_plan(&mut planned, &plan).unwrap();
        for i in 0..buf.len() {
            assert!((buf[i].re - planned[i].re).abs() < 1e-4);
            assert!((buf[i].im - planned[i].im).abs() < 1e-4);
        }
    }
}
