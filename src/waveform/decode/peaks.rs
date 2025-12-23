use super::normalize::clamp_sample;
use crate::waveform::{WaveformDecodeError, WaveformPeaks};

pub(super) fn peak_bucket_size(frames: usize) -> usize {
    if frames >= 60_000_000 {
        8_192
    } else if frames >= 10_000_000 {
        4_096
    } else {
        2_048
    }
}

pub(super) fn build_peaks_from_float(
    reader: &mut hound::WavReader<std::io::Cursor<&[u8]>>,
    channels: usize,
) -> Result<WaveformPeaks, WaveformDecodeError> {
    let total_frames = reader.duration() as usize;
    let bucket_size_frames = peak_bucket_size(total_frames).max(1);
    let bucket_count = total_frames.div_ceil(bucket_size_frames).max(1);

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
    for frame in 0..total_frames {
        let bucket = frame / bucket_size_frames;
        let mut frame_min = 1.0_f32;
        let mut frame_max = -1.0_f32;
        for ch in 0..channels {
            let sample = iter.next().transpose()?.unwrap_or(0.0);
            let sample = clamp_sample(sample);
            frame_min = frame_min.min(sample);
            frame_max = frame_max.max(sample);
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
        *min = (*min).min(frame_min);
        *max = (*max).max(frame_max);
    }

    Ok(WaveformPeaks {
        total_frames,
        channels: channels.min(u16::MAX as usize) as u16,
        bucket_size_frames,
        mono,
        left,
        right,
    })
}

pub(super) fn build_peaks_from_int(
    reader: &mut hound::WavReader<std::io::Cursor<&[u8]>>,
    channels: usize,
    bits_per_sample: u16,
) -> Result<WaveformPeaks, WaveformDecodeError> {
    let scale = (1i64 << bits_per_sample.saturating_sub(1)).max(1) as f32;
    let total_frames = reader.duration() as usize;
    let bucket_size_frames = peak_bucket_size(total_frames).max(1);
    let bucket_count = total_frames.div_ceil(bucket_size_frames).max(1);

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
    for frame in 0..total_frames {
        let bucket = frame / bucket_size_frames;
        let mut frame_min = 1.0_f32;
        let mut frame_max = -1.0_f32;
        for ch in 0..channels {
            let sample = iter.next().transpose()?.unwrap_or(0) as f32 / scale;
            let sample = clamp_sample(sample);
            frame_min = frame_min.min(sample);
            frame_max = frame_max.max(sample);
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
        *min = (*min).min(frame_min);
        *max = (*max).max(frame_max);
    }

    Ok(WaveformPeaks {
        total_frames,
        channels: channels.min(u16::MAX as usize) as u16,
        bucket_size_frames,
        mono,
        left,
        right,
    })
}
