use super::{BandEnergyRatios, Mfcc20Stats, SpectralAggregates, Stats};
use super::stft::{BandFrame, SpectralFrame};

pub(super) fn early_late_ranges(frame_count: usize) -> (std::ops::Range<usize>, std::ops::Range<usize>) {
    if frame_count == 0 {
        return (0..0, 0..0);
    }
    let quarter = (frame_count as f32 * 0.25).round().max(1.0) as usize;
    let early_end = quarter.min(frame_count);
    let late_start = frame_count.saturating_sub(quarter).min(frame_count);
    (0..early_end, late_start..frame_count)
}

pub(super) fn spectral_aggregates(
    frames: &[SpectralFrame],
    early: std::ops::Range<usize>,
    late: std::ops::Range<usize>,
) -> SpectralAggregates {
    let centroid = collect(frames, |f| f.centroid_hz);
    let rolloff = collect(frames, |f| f.rolloff_hz);
    let flatness = collect(frames, |f| f.flatness);
    let bandwidth = collect(frames, |f| f.bandwidth_hz);
    SpectralAggregates {
        centroid_hz: stats_f32(&centroid),
        rolloff_hz: stats_f32(&rolloff),
        flatness: stats_f32(&flatness),
        bandwidth_hz: stats_f32(&bandwidth),
        centroid_hz_early: stats_f32(&collect(&frames[early.clone()], |f| f.centroid_hz)),
        rolloff_hz_early: stats_f32(&collect(&frames[early.clone()], |f| f.rolloff_hz)),
        flatness_early: stats_f32(&collect(&frames[early.clone()], |f| f.flatness)),
        bandwidth_hz_early: stats_f32(&collect(&frames[early], |f| f.bandwidth_hz)),
        centroid_hz_late: stats_f32(&collect(&frames[late.clone()], |f| f.centroid_hz)),
        rolloff_hz_late: stats_f32(&collect(&frames[late.clone()], |f| f.rolloff_hz)),
        flatness_late: stats_f32(&collect(&frames[late.clone()], |f| f.flatness)),
        bandwidth_hz_late: stats_f32(&collect(&frames[late], |f| f.bandwidth_hz)),
    }
}

pub(super) fn band_aggregates(
    frames: &[BandFrame],
    early: std::ops::Range<usize>,
    late: std::ops::Range<usize>,
) -> BandEnergyRatios {
    BandEnergyRatios {
        sub: stats_f32(&collect(frames, |f| f.sub)),
        low: stats_f32(&collect(frames, |f| f.low)),
        mid: stats_f32(&collect(frames, |f| f.mid)),
        high: stats_f32(&collect(frames, |f| f.high)),
        air: stats_f32(&collect(frames, |f| f.air)),
        sub_early: stats_f32(&collect(&frames[early.clone()], |f| f.sub)),
        low_early: stats_f32(&collect(&frames[early.clone()], |f| f.low)),
        mid_early: stats_f32(&collect(&frames[early.clone()], |f| f.mid)),
        high_early: stats_f32(&collect(&frames[early.clone()], |f| f.high)),
        air_early: stats_f32(&collect(&frames[early], |f| f.air)),
        sub_late: stats_f32(&collect(&frames[late.clone()], |f| f.sub)),
        low_late: stats_f32(&collect(&frames[late.clone()], |f| f.low)),
        mid_late: stats_f32(&collect(&frames[late.clone()], |f| f.mid)),
        high_late: stats_f32(&collect(&frames[late.clone()], |f| f.high)),
        air_late: stats_f32(&collect(&frames[late], |f| f.air)),
    }
}

pub(super) fn mfcc_aggregates(
    frames: &[Vec<f32>],
    early: std::ops::Range<usize>,
    late: std::ops::Range<usize>,
) -> Mfcc20Stats {
    Mfcc20Stats {
        mean: mean_vec(frames),
        std: std_vec(frames),
        mean_early: mean_vec(&frames[early.clone()]),
        std_early: std_vec(&frames[early]),
        mean_late: mean_vec(&frames[late.clone()]),
        std_late: std_vec(&frames[late]),
    }
}

fn stats_f32(values: &[f32]) -> Stats {
    if values.is_empty() {
        return Stats { mean: 0.0, std: 0.0 };
    }
    let mean = values.iter().copied().sum::<f32>() / values.len() as f32;
    let mut var = 0.0_f64;
    for &v in values {
        let d = v as f64 - mean as f64;
        var += d * d;
    }
    Stats { mean, std: (var / values.len() as f64).sqrt() as f32 }
}

fn collect<T>(frames: &[T], f: impl Fn(&T) -> f32) -> Vec<f32> {
    frames.iter().map(f).collect()
}

fn mean_vec(frames: &[Vec<f32>]) -> Vec<f32> {
    let len = frames.first().map(|v| v.len()).unwrap_or(20);
    if frames.is_empty() || len == 0 {
        return vec![0.0; 20];
    }
    let mut sum = vec![0.0_f64; len];
    for frame in frames {
        for (i, &v) in frame.iter().enumerate() {
            sum[i] += v as f64;
        }
    }
    sum.into_iter()
        .map(|v| (v / frames.len() as f64) as f32)
        .collect()
}

fn std_vec(frames: &[Vec<f32>]) -> Vec<f32> {
    let mean = mean_vec(frames);
    let len = mean.len();
    if frames.is_empty() || len == 0 {
        return vec![0.0; 20];
    }
    let mut var = vec![0.0_f64; len];
    for frame in frames {
        for (i, &v) in frame.iter().enumerate() {
            let d = v as f64 - mean[i] as f64;
            var[i] += d * d;
        }
    }
    var.into_iter()
        .map(|v| (v / frames.len() as f64).sqrt() as f32)
        .collect()
}

