use crate::analysis::audio::normalize_peak_in_place;
use super::buffer::SelectionEditBuffer;
use std::time::Duration;

pub(crate) fn normalize_selection(
    buffer: &mut SelectionEditBuffer,
    fade_duration: Duration,
) -> Result<(), String> {
    let channels = buffer.channels.max(1);
    let start = buffer.start_frame * channels;
    let end = buffer.end_frame * channels;
    if end <= start {
        return Err("Selection is empty".into());
    }
    let selection_frames = (end - start) / channels;
    let fade_frames = fade_frame_count(buffer.sample_rate.max(1), selection_frames, fade_duration);
    
    // Only clone the portions needed for crossfading.
    let start_fade_len = fade_frames * channels;
    let end_fade_len = fade_frames * channels;
    
    let original_start = if start_fade_len > 0 {
        Some(buffer.samples[start..start + start_fade_len].to_vec())
    } else {
        None
    };
    
    let original_end = if end_fade_len > 0 {
        Some(buffer.samples[end - end_fade_len..end].to_vec())
    } else {
        None
    };

    // Use optimized SIMD normalization.
    normalize_peak_in_place(&mut buffer.samples[start..end]);

    // Apply crossfades using the small cached clones.
    if let Some(orig_start) = original_start {
        apply_edge_start_crossfade(
            &mut buffer.samples[start..start + start_fade_len],
            &orig_start,
            channels,
            fade_frames,
        );
    }
    
    if let Some(orig_end) = original_end {
        apply_edge_end_crossfade(
            &mut buffer.samples[end - end_fade_len..end],
            &orig_end,
            channels,
            fade_frames,
        );
    }

    Ok(())
}

fn fade_frame_count(sample_rate: u32, selection_frames: usize, duration: Duration) -> usize {
    if selection_frames == 0 {
        return 0;
    }
    let frames = (sample_rate as f32 * duration.as_secs_f32()).round() as usize;
    frames.min(selection_frames / 2)
}

fn apply_edge_start_crossfade(
    selection: &mut [f32],
    original: &[f32],
    channels: usize,
    fade_frames: usize,
) {
    if fade_frames == 0 {
        return;
    }
    let denom = (fade_frames.saturating_sub(1)).max(1) as f32;
    for frame in 0..fade_frames {
        let t = frame as f32 / denom;
        for ch in 0..channels {
            let idx = frame * channels + ch;
            selection[idx] = lerp(original[idx], selection[idx], t);
        }
    }
}

fn apply_edge_end_crossfade(
    selection: &mut [f32],
    original: &[f32],
    channels: usize,
    fade_frames: usize,
) {
    if fade_frames == 0 {
        return;
    }
    let denom = (fade_frames.saturating_sub(1)).max(1) as f32;
    for frame in 0..fade_frames {
        let t = if fade_frames == 1 {
            1.0
        } else {
            frame as f32 / denom
        };
        for ch in 0..channels {
            let idx = frame * channels + ch;
            selection[idx] = lerp(selection[idx], original[idx], t);
        }
    }
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t.clamp(0.0, 1.0)
}
