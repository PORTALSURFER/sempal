use super::normalize::clamp_sample;
use crate::waveform::{WaveformDecodeError, WaveformPeaks};

const MAX_PEAK_BUCKETS: usize = 1_000_000;
const MIN_ANALYSIS_SAMPLE_RATE: u32 = 8_000;
const MAX_ANALYSIS_SAMPLES: usize = 5_000_000;

/// Choose an envelope bucket size that caps peak bucket count.
pub(super) fn peak_bucket_size(frames: usize) -> usize {
    frames.div_ceil(MAX_PEAK_BUCKETS).max(1)
}

/// Combined peaks and decimated analysis samples for long files.
pub(super) struct PeaksAndAnalysis {
    /// Min/max envelope for waveform rendering.
    pub(super) peaks: WaveformPeaks,
    /// Decimated mono samples for analysis pipelines.
    pub(super) analysis_samples: Vec<f32>,
    /// Effective sample rate (Hz) for the analysis samples.
    pub(super) analysis_sample_rate: u32,
    /// Number of original frames represented by each analysis sample.
    pub(super) analysis_stride: usize,
}

/// Compute the frame stride for decimating long files into analysis samples.
pub(super) fn analysis_stride(sample_rate: u32, total_frames: usize) -> usize {
    let sample_rate = sample_rate.max(1);
    let min_stride = (sample_rate / MIN_ANALYSIS_SAMPLE_RATE).max(1) as usize;
    let max_samples_stride = total_frames.div_ceil(MAX_ANALYSIS_SAMPLES).max(1);
    min_stride.max(max_samples_stride).max(1)
}

/// Build waveform peaks and decimated analysis samples from float PCM.
pub(super) fn build_peaks_with_analysis_from_float(
    reader: &mut hound::WavReader<std::io::Cursor<&[u8]>>,
    channels: usize,
    sample_rate: u32,
) -> Result<PeaksAndAnalysis, WaveformDecodeError> {
    let total_frames = reader.duration() as usize;
    let bucket_size_frames = peak_bucket_size(total_frames).max(1);
    let bucket_count = total_frames.div_ceil(bucket_size_frames).max(1);
    let analysis_stride = analysis_stride(sample_rate, total_frames);
    let mut analysis_samples =
        Vec::with_capacity(total_frames.div_ceil(analysis_stride).max(1));

    let mut mono = vec![(1.0_f32, -1.0_f32); bucket_count];
    let mut left = if channels >= 2 {
        Some(vec![(1.0_f32, -1.0_f32); bucket_count])
    } else {
        None
    };
    let mut right = if channels >= 2 {
        Some(vec![(1.0_f32, -1.0_f32); bucket_count])
    } else {
        None
    };

    let mut iter = reader
        .samples::<f32>()
        .map(|s| s.map_err(|source| WaveformDecodeError::Sample { source }));
    let mut analysis_sum = 0.0f32;
    let mut analysis_count = 0usize;
    for frame in 0..total_frames {
        let bucket = frame / bucket_size_frames;
        let mut frame_min = 1.0_f32;
        let mut frame_max = -1.0_f32;
        let mut frame_count = 0usize;
        let mut frame_sum = 0.0f32;
        for ch in 0..channels {
            let sample = iter.next().transpose()?.unwrap_or(0.0);
            let sample = clamp_sample(sample);
            frame_count = frame_count.saturating_add(1);
            frame_min = frame_min.min(sample);
            frame_max = frame_max.max(sample);
            frame_sum += sample;
            if ch == 0 {
                if let Some(left_peaks) = left.as_mut() {
                    let (min, max) = &mut left_peaks[bucket];
                    *min = (*min).min(sample);
                    *max = (*max).max(sample);
                }
            } else if ch == 1 {
                if let Some(right_peaks) = right.as_mut() {
                    let (min, max) = &mut right_peaks[bucket];
                    *min = (*min).min(sample);
                    *max = (*max).max(sample);
                }
            }
        }
        let (min, max) = &mut mono[bucket];
        if frame_count == 0 {
            *min = (*min).min(0.0);
            *max = (*max).max(0.0);
        } else {
            *min = (*min).min(frame_min);
            *max = (*max).max(frame_max);
        }
        if frame_count > 0 {
            analysis_sum += frame_sum / frame_count as f32;
            analysis_count += 1;
            if analysis_count >= analysis_stride {
                analysis_samples.push(analysis_sum / analysis_count as f32);
                analysis_sum = 0.0;
                analysis_count = 0;
            }
        }
    }
    if analysis_count > 0 {
        analysis_samples.push(analysis_sum / analysis_count as f32);
    }

    let analysis_sample_rate =
        ((sample_rate as f32) / analysis_stride as f32).round().max(1.0) as u32;
    Ok(PeaksAndAnalysis {
        peaks: WaveformPeaks {
            total_frames,
            channels: channels.min(u16::MAX as usize) as u16,
            bucket_size_frames,
            mono,
            left,
            right,
        },
        analysis_samples,
        analysis_sample_rate,
        analysis_stride,
    })
}

/// Build waveform peaks and decimated analysis samples from integer PCM.
pub(super) fn build_peaks_with_analysis_from_int(
    reader: &mut hound::WavReader<std::io::Cursor<&[u8]>>,
    channels: usize,
    bits_per_sample: u16,
    sample_rate: u32,
) -> Result<PeaksAndAnalysis, WaveformDecodeError> {
    let scale = (1i64 << bits_per_sample.saturating_sub(1)).max(1) as f32;
    let total_frames = reader.duration() as usize;
    let bucket_size_frames = peak_bucket_size(total_frames).max(1);
    let bucket_count = total_frames.div_ceil(bucket_size_frames).max(1);
    let analysis_stride = analysis_stride(sample_rate, total_frames);
    let mut analysis_samples =
        Vec::with_capacity(total_frames.div_ceil(analysis_stride).max(1));

    let mut mono = vec![(1.0_f32, -1.0_f32); bucket_count];
    let mut left = if channels >= 2 {
        Some(vec![(1.0_f32, -1.0_f32); bucket_count])
    } else {
        None
    };
    let mut right = if channels >= 2 {
        Some(vec![(1.0_f32, -1.0_f32); bucket_count])
    } else {
        None
    };

    let mut iter = reader
        .samples::<i32>()
        .map(|s| s.map_err(|source| WaveformDecodeError::Sample { source }));
    let mut analysis_sum = 0.0f32;
    let mut analysis_count = 0usize;
    for frame in 0..total_frames {
        let bucket = frame / bucket_size_frames;
        let mut frame_min = 1.0_f32;
        let mut frame_max = -1.0_f32;
        let mut frame_count = 0usize;
        let mut frame_sum = 0.0f32;
        for ch in 0..channels {
            let sample = iter.next().transpose()?.unwrap_or(0) as f32 / scale;
            let sample = clamp_sample(sample);
            frame_count = frame_count.saturating_add(1);
            frame_min = frame_min.min(sample);
            frame_max = frame_max.max(sample);
            frame_sum += sample;
            if ch == 0 {
                if let Some(left_peaks) = left.as_mut() {
                    let (min, max) = &mut left_peaks[bucket];
                    *min = (*min).min(sample);
                    *max = (*max).max(sample);
                }
            } else if ch == 1 {
                if let Some(right_peaks) = right.as_mut() {
                    let (min, max) = &mut right_peaks[bucket];
                    *min = (*min).min(sample);
                    *max = (*max).max(sample);
                }
            }
        }
        let (min, max) = &mut mono[bucket];
        if frame_count == 0 {
            *min = (*min).min(0.0);
            *max = (*max).max(0.0);
        } else {
            *min = (*min).min(frame_min);
            *max = (*max).max(frame_max);
        }
        if frame_count > 0 {
            analysis_sum += frame_sum / frame_count as f32;
            analysis_count += 1;
            if analysis_count >= analysis_stride {
                analysis_samples.push(analysis_sum / analysis_count as f32);
                analysis_sum = 0.0;
                analysis_count = 0;
            }
        }
    }
    if analysis_count > 0 {
        analysis_samples.push(analysis_sum / analysis_count as f32);
    }

    let analysis_sample_rate =
        ((sample_rate as f32) / analysis_stride as f32).round().max(1.0) as u32;
    Ok(PeaksAndAnalysis {
        peaks: WaveformPeaks {
            total_frames,
            channels: channels.min(u16::MAX as usize) as u16,
            bucket_size_frames,
            mono,
            left,
            right,
        },
        analysis_samples,
        analysis_sample_rate,
        analysis_stride,
    })
}
