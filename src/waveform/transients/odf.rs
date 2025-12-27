use super::stats::RollingMedian;
use crate::analysis::fft::{Complex32, FftPlan, fft_radix2_inplace_with_plan, hann_window};
use crate::waveform::DecodedWaveform;

const BAND_COUNT: usize = 24;
const MIN_BAND_HZ: f32 = 40.0;
const MAX_ODF_FRAMES: usize = 60_000;
const SUPERFLUX_LAG: usize = 2;
const SUPERFLUX_WINDOW: usize = 3;
const SUPERFLUX_HISTORY: usize = SUPERFLUX_LAG + SUPERFLUX_WINDOW;
const WHITEN_SECONDS: f32 = 0.25;

pub(crate) struct AnalysisParams {
    pub(crate) fft_len: usize,
    pub(crate) hop: usize,
    pub(crate) sample_rate: u32,
}

pub(crate) fn analysis_params(sample_rate: u32, frame_count: usize) -> AnalysisParams {
    let (fft_len, hop) = stft_params(sample_rate);
    let mut analysis_hop = hop.max(1);
    let frame_count = frame_count.max(1);
    let frames = frame_count / analysis_hop;
    if frames > MAX_ODF_FRAMES {
        let scale = ((frames as f32 / MAX_ODF_FRAMES as f32).ceil() as usize).max(1);
        analysis_hop = analysis_hop.saturating_mul(scale).max(1);
    }
    AnalysisParams {
        fft_len,
        hop: analysis_hop,
        sample_rate,
    }
}

pub(crate) fn mono_samples(decoded: &DecodedWaveform) -> Vec<f32> {
    let channels = decoded.channel_count().max(1);
    let frames = decoded.frame_count();
    let mut mono = Vec::with_capacity(frames);
    for frame in 0..frames {
        let idx = frame.saturating_mul(channels);
        let mut sum = 0.0f32;
        for ch in 0..channels {
            if let Some(sample) = decoded.samples.get(idx + ch) {
                sum += *sample;
            }
        }
        mono.push(sum / channels as f32);
    }
    mono
}

pub(crate) fn spectral_flux_superflux(
    mono: &[f32],
    fft_len: usize,
    hop: usize,
    sample_rate: u32,
) -> Vec<f32> {
    if mono.is_empty() || hop == 0 || fft_len == 0 {
        return Vec::new();
    }
    let window = hann_window(fft_len);
    let plan = match FftPlan::new(fft_len) {
        Ok(plan) => plan,
        Err(_) => return Vec::new(),
    };
    let bins = fft_len / 2 + 1;
    let bands = band_edges(bins, sample_rate, BAND_COUNT);
    if bands.is_empty() {
        return Vec::new();
    }
    let hop_seconds = hop as f32 / sample_rate.max(1) as f32;
    let whiten_window = ((WHITEN_SECONDS / hop_seconds).round() as usize).clamp(8, 64);
    let mut band_medians = (0..bands.len())
        .map(|_| RollingMedian::new(whiten_window))
        .collect::<Vec<_>>();
    let mut band_history = vec![vec![0.0f32; SUPERFLUX_HISTORY]; bands.len()];
    let mut history_pos = 0usize;
    let mut buf = vec![Complex32::default(); fft_len];
    let mut novelty = Vec::new();
    let mut start = 0usize;
    while start < mono.len() {
        for i in 0..fft_len {
            let sample = mono.get(start + i).copied().unwrap_or(0.0);
            buf[i].re = sample * window[i];
            buf[i].im = 0.0;
        }
        if fft_radix2_inplace_with_plan(&mut buf, &plan).is_err() {
            return Vec::new();
        }
        let mut sum = 0.0f32;
        for (band_index, (start_bin, end_bin)) in bands.iter().enumerate() {
            if *start_bin >= *end_bin || *start_bin >= bins {
                continue;
            }
            let mut band_sum = 0.0f32;
            let mut count = 0.0f32;
            for bin in *start_bin..(*end_bin).min(bins) {
                let c = buf[bin];
                let mag = (c.re * c.re + c.im * c.im).sqrt();
                let mag_log = (1.0 + 10.0 * mag).ln();
                band_sum += mag_log;
                count += 1.0;
            }
            if count == 0.0 {
                continue;
            }
            let band_value = band_sum / count;
            let median = band_medians[band_index].push(band_value);
            let normalized = band_value / (median + 1.0e-6);
            let mut prev_max = 0.0f32;
            for offset in SUPERFLUX_LAG..(SUPERFLUX_LAG + SUPERFLUX_WINDOW) {
                let idx = (history_pos + SUPERFLUX_HISTORY - offset) % SUPERFLUX_HISTORY;
                prev_max = prev_max.max(band_history[band_index][idx]);
            }
            let delta = (normalized - prev_max).max(0.0);
            band_history[band_index][history_pos] = normalized;
            let weight = ((band_index + 1) as f32 / bands.len() as f32).sqrt();
            sum += delta * weight;
        }
        novelty.push(sum);
        history_pos = (history_pos + 1) % SUPERFLUX_HISTORY;
        start = start.saturating_add(hop);
    }
    novelty
}

fn stft_params(sample_rate: u32) -> (usize, usize) {
    if sample_rate < 32_000 {
        (512, 128)
    } else {
        (1024, 256)
    }
}

fn band_edges(bins: usize, sample_rate: u32, bands: usize) -> Vec<(usize, usize)> {
    if bins < 4 || bands == 0 {
        return Vec::new();
    }
    let nyquist = sample_rate as f32 * 0.5;
    let min_hz = MIN_BAND_HZ.min(nyquist * 0.5);
    let max_hz = nyquist.max(min_hz + 1.0);
    let log_min = min_hz.ln();
    let log_max = max_hz.ln();
    let mut edges = Vec::with_capacity(bands);
    let mut last_bin = 1usize;
    for band in 0..bands {
        let t0 = band as f32 / bands as f32;
        let t1 = (band + 1) as f32 / bands as f32;
        let hz0 = (log_min + (log_max - log_min) * t0).exp();
        let hz1 = (log_min + (log_max - log_min) * t1).exp();
        let bin0 = ((hz0 / nyquist) * (bins as f32 - 1.0)).round() as usize;
        let bin1 = ((hz1 / nyquist) * (bins as f32 - 1.0)).round() as usize;
        let start = bin0.clamp(1, bins.saturating_sub(1));
        let end = bin1.clamp(start + 1, bins);
        let start = start.max(last_bin);
        let end = end.max(start + 1).min(bins);
        if start < end {
            edges.push((start, end));
            last_bin = end;
        }
    }
    edges
}
