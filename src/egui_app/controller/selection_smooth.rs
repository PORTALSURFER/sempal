use super::SelectionEditBuffer;
use std::time::Duration;

pub(super) fn smooth_selection(
    buffer: &mut SelectionEditBuffer,
    fade_duration: Duration,
) -> Result<(), String> {
    let channels = buffer.channels.max(1);
    let (selection_start, selection_end, selection_frames) = selection_bounds(buffer, channels)?;
    let mut smoothed = buffer.samples[selection_start..selection_end].to_vec();
    let original = smoothed.clone();
    let fade_frames =
        smoothing_fade_frames(buffer.sample_rate.max(1), selection_frames, fade_duration)?;
    let (before_edge, after_edge) = edge_neighbors(
        &buffer.samples,
        channels,
        buffer.start_frame,
        buffer.end_frame,
        &original[..channels],
        &original[original.len().saturating_sub(channels)..],
    );
    apply_edge_smoothing(
        &mut smoothed,
        &original,
        &before_edge,
        &after_edge,
        channels,
        fade_frames,
    );
    buffer.samples[selection_start..selection_end].copy_from_slice(&smoothed);
    Ok(())
}

fn selection_bounds(
    buffer: &SelectionEditBuffer,
    channels: usize,
) -> Result<(usize, usize, usize), String> {
    let selection_start = buffer.start_frame * channels;
    let selection_end = buffer.end_frame * channels;
    if selection_end <= selection_start {
        return Err("Selection is empty".into());
    }
    let selection_frames = (selection_end - selection_start) / channels;
    if selection_frames < 2 {
        return Err("Selection is too short to smooth".into());
    }
    Ok((selection_start, selection_end, selection_frames))
}

fn smoothing_fade_frames(
    sample_rate: u32,
    selection_frames: usize,
    duration: Duration,
) -> Result<usize, String> {
    let frames = (sample_rate as f32 * duration.as_secs_f32()).round() as usize;
    let fade = frames.max(1).min(selection_frames / 2);
    if fade == 0 {
        return Err("Selection is too short to smooth".into());
    }
    Ok(fade)
}

fn edge_neighbors(
    samples: &[f32],
    channels: usize,
    start_frame: usize,
    end_frame: usize,
    first_selection_frame: &[f32],
    last_selection_frame: &[f32],
) -> (Vec<f32>, Vec<f32>) {
    let total_frames = samples.len() / channels.max(1);
    let before_frame = start_frame.checked_sub(1);
    let after_frame = if end_frame < total_frames {
        Some(end_frame)
    } else {
        None
    };
    let mut before = Vec::with_capacity(channels);
    let mut after = Vec::with_capacity(channels);
    for ch in 0..channels {
        let prev = before_frame
            .and_then(|frame| samples.get(frame * channels + ch).copied())
            .unwrap_or_else(|| first_selection_frame[ch]);
        before.push(prev);
        let next = after_frame
            .and_then(|frame| samples.get(frame * channels + ch).copied())
            .unwrap_or_else(|| last_selection_frame[ch]);
        after.push(next);
    }
    (before, after)
}

fn apply_edge_smoothing(
    smoothed: &mut [f32],
    original: &[f32],
    before: &[f32],
    after: &[f32],
    channels: usize,
    fade_frames: usize,
) {
    let channels = channels.max(1);
    let total_frames = smoothed.len() / channels;
    let fade_frames = fade_frames.min(total_frames / 2);
    if fade_frames == 0 || total_frames == 0 {
        return;
    }
    let denom = (fade_frames.saturating_sub(1)).max(1) as f32;
    apply_start_fade(smoothed, original, before, channels, fade_frames, denom);
    apply_end_fade(smoothed, original, after, channels, fade_frames, denom, total_frames);
}

fn apply_start_fade(
    smoothed: &mut [f32],
    original: &[f32],
    before: &[f32],
    channels: usize,
    fade_frames: usize,
    denom: f32,
) {
    for frame in 0..fade_frames {
        let progress = fade_progress(frame, fade_frames, denom);
        let weight = raised_cosine(progress);
        for ch in 0..channels {
            let idx = frame * channels + ch;
            smoothed[idx] = lerp(before[ch], original[idx], weight);
        }
    }
}

fn apply_end_fade(
    smoothed: &mut [f32],
    original: &[f32],
    after: &[f32],
    channels: usize,
    fade_frames: usize,
    denom: f32,
    total_frames: usize,
) {
    for frame in 0..fade_frames {
        let progress = fade_progress(frame, fade_frames, denom);
        let weight = raised_cosine(progress);
        let frame_idx = total_frames.saturating_sub(fade_frames) + frame;
        for ch in 0..channels {
            let idx = frame_idx * channels + ch;
            smoothed[idx] = lerp(original[idx], after[ch], weight);
        }
    }
}

fn fade_progress(frame: usize, fade_frames: usize, denom: f32) -> f32 {
    if fade_frames == 1 {
        return 0.5;
    }
    frame as f32 / denom
}

fn raised_cosine(t: f32) -> f32 {
    let clamped = t.clamp(0.0, 1.0);
    0.5 - 0.5 * (std::f32::consts::PI * clamped).cos()
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    let factor = t.clamp(0.0, 1.0);
    a + (b - a) * factor
}
