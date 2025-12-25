const QUERY_WINDOW_SECONDS: f32 = 2.0;
const QUERY_HOP_SECONDS: f32 = 1.0;
const QUERY_MAX_WINDOWS: usize = 24;

pub(super) fn query_window_ranges(sample_len: usize, sample_rate: u32) -> Vec<(usize, usize)> {
    let window_len = (QUERY_WINDOW_SECONDS * sample_rate as f32).round() as usize;
    let hop_len = (QUERY_HOP_SECONDS * sample_rate as f32).round() as usize;
    if window_len == 0 || sample_len == 0 {
        return Vec::new();
    }
    if sample_len <= window_len {
        return vec![(0, sample_len)];
    }
    let hop_len = hop_len.max(1);
    let mut ranges = Vec::new();
    let max_start = sample_len.saturating_sub(window_len);
    let mut start = 0;
    while start <= max_start {
        ranges.push((start, start + window_len));
        start += hop_len;
    }
    if ranges.len() > QUERY_MAX_WINDOWS {
        let stride = (ranges.len() as f32 / QUERY_MAX_WINDOWS as f32).ceil() as usize;
        ranges = ranges
            .into_iter()
            .step_by(stride)
            .take(QUERY_MAX_WINDOWS)
            .collect();
    }
    ranges
}
