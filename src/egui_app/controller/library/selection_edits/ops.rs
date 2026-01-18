use super::FadeDirection;
use crate::selection::FadeParams;
use super::buffer::SelectionEditBuffer;

pub(crate) fn crop_buffer(buffer: &mut SelectionEditBuffer) -> Result<(), String> {
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

pub(crate) fn trim_buffer(buffer: &mut SelectionEditBuffer) -> Result<(), String> {
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

pub(crate) fn mute_buffer(buffer: &mut SelectionEditBuffer) -> Result<(), String> {
    apply_muted_selection(
        &mut buffer.samples,
        buffer.channels,
        buffer.start_frame,
        buffer.end_frame,
    );
    Ok(())
}

pub(crate) fn reverse_buffer(buffer: &mut SelectionEditBuffer) -> Result<(), String> {
    let channels = buffer.channels.max(1);
    let total_frames = buffer.samples.len() / channels;
    let start = buffer.start_frame.min(total_frames);
    let end = buffer.end_frame.min(total_frames);
    if end <= start + 1 {
        return Ok(());
    }
    let mut left = start;
    let mut right = end - 1;
    while left < right {
        let left_offset = left * channels;
        let right_offset = right * channels;
        for ch in 0..channels {
            buffer.samples.swap(left_offset + ch, right_offset + ch);
        }
        left += 1;
        right = right.saturating_sub(1);
    }
    Ok(())
}

pub(crate) fn slice_frames(
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

pub(crate) fn apply_directional_fade(
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
    apply_fade_ramp(samples, channels, clamped_start, clamped_end, direction, 0.5);
    match direction {
        FadeDirection::LeftToRight => {
            apply_muted_selection(samples, channels, clamped_end, total_frames);
        }
        FadeDirection::RightToLeft => {
            apply_muted_selection(samples, channels, 0, clamped_start);
        }
    }
}

/// Apply fade-in and fade-out ramps at the edges of the selected span.
pub(crate) fn apply_edge_fades(
    samples: &mut [f32],
    channels: usize,
    start_frame: usize,
    end_frame: usize,
    fade_frames: usize,
) {
    let channels = channels.max(1);
    let total_frames = samples.len() / channels;
    let (clamped_start, clamped_end) = clamped_selection_span(total_frames, start_frame, end_frame);
    if clamped_end <= clamped_start {
        return;
    }
    let selection_frames = clamped_end - clamped_start;
    let fade_frames = fade_frames.min(selection_frames / 2);
    if fade_frames == 0 {
        return;
    }
    let denom = (fade_frames.saturating_sub(1)).max(1) as f32;
    // Use default curve of 0.5 for edge fades
    let curve = 0.5;
    for i in 0..fade_frames {
        let t = i as f32 / denom;
        let factor = fade_factor(fade_frames, t, FadeDirection::RightToLeft, curve);
        let frame = clamped_start + i;
        for ch in 0..channels {
            let idx = frame * channels + ch;
            if let Some(sample) = samples.get_mut(idx) {
                *sample *= factor;
            }
        }
    }
    for i in 0..fade_frames {
        let t = if fade_frames == 1 {
            1.0
        } else {
            i as f32 / denom
        };
        let factor = fade_factor(fade_frames, t, FadeDirection::LeftToRight, curve);
        let frame = clamped_end.saturating_sub(fade_frames) + i;
        for ch in 0..channels {
            let idx = frame * channels + ch;
            if let Some(sample) = samples.get_mut(idx) {
                *sample *= factor;
            }
        }
    }
}

/// Apply optional fade-in and fade-out ramps within the selection bounds.
pub(crate) fn apply_selection_fades(
    samples: &mut [f32],
    channels: usize,
    start_frame: usize,
    end_frame: usize,
    fade_in: Option<FadeParams>,
    fade_out: Option<FadeParams>,
) {
    let channels = channels.max(1);
    let total_frames = samples.len() / channels;
    let (clamped_start, clamped_end) = clamped_selection_span(total_frames, start_frame, end_frame);
    if clamped_end <= clamped_start {
        return;
    }
    let selection_frames = clamped_end - clamped_start;
    if let Some(fade_in) = fade_in {
        let fade_frames = ((selection_frames as f32) * fade_in.length)
            .round()
            .clamp(0.0, selection_frames as f32) as usize;
        if fade_frames > 0 {
            let fade_end = clamped_start.saturating_add(fade_frames).min(clamped_end);
            if fade_end > clamped_start {
                apply_fade_ramp(
                    samples,
                    channels,
                    clamped_start,
                    fade_end,
                    FadeDirection::RightToLeft,
                    fade_in.curve,
                );
            }
        }
    }
    if let Some(fade_out) = fade_out {
        let fade_frames = ((selection_frames as f32) * fade_out.length)
            .round()
            .clamp(0.0, selection_frames as f32) as usize;
        if fade_frames > 0 {
            let fade_start = clamped_end.saturating_sub(fade_frames).max(clamped_start);
            if fade_start < clamped_end {
                apply_fade_ramp(
                    samples,
                    channels,
                    fade_start,
                    clamped_end,
                    FadeDirection::LeftToRight,
                    fade_out.curve,
                );
            }
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
    curve: f32,
) {
    let frame_count = clamped_end - clamped_start;
    let denom = (frame_count.saturating_sub(1)).max(1) as f32;
    for i in 0..frame_count {
        let progress = i as f32 / denom;
        let factor = fade_factor(frame_count, progress, direction, curve);
        let frame = clamped_start + i;
        for ch in 0..channels {
            let idx = frame * channels + ch;
            if let Some(sample) = samples.get_mut(idx) {
                *sample *= factor;
            }
        }
    }
}

pub(crate) fn fade_factor(frame_count: usize, progress: f32, direction: FadeDirection, curve: f32) -> f32 {
    if frame_count == 1 {
        return 0.0;
    }
    let smoothed = apply_s_curve(progress.clamp(0.0, 1.0), curve);
    let factor = match direction {
        FadeDirection::LeftToRight => 1.0 - smoothed,
        FadeDirection::RightToLeft => smoothed,
    };
    factor.clamp(0.0, 1.0)
}

/// Apply S-curve interpolation with adjustable tension.
/// t: 0.0-1.0 input
/// curve: 0.0 = linear, 0.5 = medium S-curve, 1.0 = maximum S-curve
fn apply_s_curve(t: f32, curve: f32) -> f32 {
    if curve <= 0.0 {
        // Linear
        return t;
    }
    
    // Blend between linear and smootherstep based on curve value
    let smootherstep = {
        let t2 = t * t;
        let t3 = t2 * t;
        t3 * (t * (t * 6.0 - 15.0) + 10.0)
    };
    
    // Interpolate between linear and smootherstep
    t * (1.0 - curve) + smootherstep * curve
}

pub(crate) fn apply_muted_selection(
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

#[cfg(test)]
mod tests {
    use super::apply_edge_fades;

    #[test]
    fn edge_fades_ramp_selection_edges() {
        let mut samples = vec![1.0_f32; 4];
        apply_edge_fades(&mut samples, 1, 0, 4, 2);
        assert!((samples[0] - 0.0).abs() < 1e-6);
        assert!((samples[1] - 1.0).abs() < 1e-6);
        assert!((samples[2] - 1.0).abs() < 1e-6);
        assert!((samples[3] - 0.0).abs() < 1e-6);
    }
}
