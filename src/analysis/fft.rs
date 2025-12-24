use std::f32::consts::PI;

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
}
