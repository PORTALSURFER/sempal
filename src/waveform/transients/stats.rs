#[derive(Clone, Debug)]
pub(crate) struct RollingMedian {
    values: Vec<f32>,
    pos: usize,
    filled: usize,
}

impl RollingMedian {
    pub(crate) fn new(size: usize) -> Self {
        let size = size.max(1);
        Self {
            values: vec![0.0; size],
            pos: 0,
            filled: 0,
        }
    }

    pub(crate) fn push(&mut self, value: f32) -> f32 {
        if self.values.is_empty() {
            return value;
        }
        self.values[self.pos] = value;
        self.pos = (self.pos + 1) % self.values.len();
        if self.filled < self.values.len() {
            self.filled += 1;
        }
        median_from_slice(&self.values[..self.filled])
    }
}

pub(crate) fn mean_std_dev(values: &[f32]) -> (f32, f32) {
    if values.is_empty() {
        return (0.0, 0.0);
    }
    let mut sum = 0.0f32;
    let mut count = 0.0f32;
    for value in values {
        if value.is_finite() {
            sum += value;
            count += 1.0;
        }
    }
    if count == 0.0 {
        return (0.0, 0.0);
    }
    let mean = sum / count;
    let mut variance = 0.0f32;
    for value in values {
        if value.is_finite() {
            let diff = value - mean;
            variance += diff * diff;
        }
    }
    let std_dev = (variance / count).sqrt();
    (mean, std_dev.max(1.0e-6))
}

fn median_from_slice(values: &[f32]) -> f32 {
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
    sorted[sorted.len() / 2]
}
