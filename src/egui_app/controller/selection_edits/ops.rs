use super::FadeDirection;
use super::buffer::SelectionEditBuffer;

pub(super) fn crop_buffer(buffer: &mut SelectionEditBuffer) -> Result<(), String> {
    let cropped = slice_frames(
        &buffer.samples,
        buffer.channels,
        buffer.start_frame,
        buffer.end_frame,
    );
    if cropped.is_empty() {
        return Err("Selection has no audio to crop".into());
    }
    buffer.samples = cropped;
    Ok(())
}

pub(super) fn trim_buffer(buffer: &mut SelectionEditBuffer) -> Result<(), String> {
    let total_frames = buffer.samples.len() / buffer.channels;
    if buffer.start_frame == 0 && buffer.end_frame >= total_frames {
        return Err("Cannot trim the entire file; crop instead".into());
    }
    let prefix_end = buffer.start_frame * buffer.channels;
    let suffix_start = buffer.end_frame * buffer.channels;
    let mut trimmed = Vec::with_capacity(
        buffer
            .samples
            .len()
            .saturating_sub(suffix_start - prefix_end),
    );
    trimmed.extend_from_slice(&buffer.samples[..prefix_end]);
    trimmed.extend_from_slice(&buffer.samples[suffix_start..]);
    if trimmed.is_empty() {
        return Err("Trim removed all audio; crop instead".into());
    }
    buffer.samples = trimmed;
    Ok(())
}

pub(super) fn mute_buffer(buffer: &mut SelectionEditBuffer) -> Result<(), String> {
    apply_muted_selection(
        &mut buffer.samples,
        buffer.channels,
        buffer.start_frame,
        buffer.end_frame,
    );
    Ok(())
}

pub(super) fn slice_frames(
    samples: &[f32],
    channels: usize,
    start_frame: usize,
    end_frame: usize,
) -> Vec<f32> {
    let mut cropped = Vec::with_capacity((end_frame - start_frame) * channels);
    for frame in start_frame..end_frame {
        let offset = frame * channels;
        cropped.extend_from_slice(&samples[offset..offset + channels]);
    }
    cropped
}

pub(super) fn apply_directional_fade(
    samples: &mut [f32],
    channels: usize,
    start_frame: usize,
    end_frame: usize,
    direction: FadeDirection,
) {
    let channels = channels.max(1);
    let total_frames = samples.len() / channels;
    let (clamped_start, clamped_end) = clamped_selection_span(total_frames, start_frame, end_frame);
    if clamped_end <= clamped_start {
        return;
    }
    apply_fade_ramp(samples, channels, clamped_start, clamped_end, direction);
    match direction {
        FadeDirection::LeftToRight => {
            apply_muted_selection(samples, channels, clamped_end, total_frames);
        }
        FadeDirection::RightToLeft => {
            apply_muted_selection(samples, channels, 0, clamped_start);
        }
    }
}

fn clamped_selection_span(
    total_frames: usize,
    start_frame: usize,
    end_frame: usize,
) -> (usize, usize) {
    let clamped_start = start_frame.min(total_frames);
    let clamped_end = end_frame.min(total_frames);
    (clamped_start, clamped_end)
}

fn apply_fade_ramp(
    samples: &mut [f32],
    channels: usize,
    clamped_start: usize,
    clamped_end: usize,
    direction: FadeDirection,
) {
    let frame_count = clamped_end - clamped_start;
    let denom = (frame_count.saturating_sub(1)).max(1) as f32;
    for i in 0..frame_count {
        let progress = i as f32 / denom;
        let factor = fade_factor(frame_count, progress, direction);
        let frame = clamped_start + i;
        for ch in 0..channels {
            let idx = frame * channels + ch;
            if let Some(sample) = samples.get_mut(idx) {
                *sample *= factor;
            }
        }
    }
}

pub(super) fn fade_factor(frame_count: usize, progress: f32, direction: FadeDirection) -> f32 {
    if frame_count == 1 {
        return 0.0;
    }
    let curve = smootherstep(progress.clamp(0.0, 1.0));
    let factor = match direction {
        FadeDirection::LeftToRight => 1.0 - curve,
        FadeDirection::RightToLeft => curve,
    };
    factor.clamp(0.0, 1.0)
}

fn smootherstep(t: f32) -> f32 {
    // 6t^5 - 15t^4 + 10t^3: smooth S-curve with zero slope at endpoints.
    let t2 = t * t;
    let t3 = t2 * t;
    t3 * (t * (t * 6.0 - 15.0) + 10.0)
}

pub(super) fn apply_muted_selection(
    samples: &mut [f32],
    channels: usize,
    start_frame: usize,
    end_frame: usize,
) {
    if end_frame <= start_frame {
        return;
    }
    let channels = channels.max(1);
    let total_frames = samples.len() / channels;
    let clamped_start = start_frame.min(total_frames);
    let clamped_end = end_frame.min(total_frames);
    for frame in clamped_start..clamped_end {
        let offset = frame * channels;
        let frame_end = (offset + channels).min(samples.len());
        for sample in &mut samples[offset..frame_end] {
            *sample = 0.0;
        }
    }
}
