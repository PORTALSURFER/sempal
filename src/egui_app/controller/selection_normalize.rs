use super::SelectionEditBuffer;
use std::time::Duration;

pub(super) fn normalize_selection(
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
    let original = buffer.samples[start..end].to_vec();
    let peak = original
        .iter()
        .fold(0.0_f32, |acc, sample| acc.max(sample.abs()));
    if peak <= f32::EPSILON {
        return Err("Cannot normalize silent selection".into());
    }
    let scale = 1.0 / peak;
    for sample in &mut buffer.samples[start..end] {
        *sample = (*sample * scale).clamp(-1.0, 1.0);
    }
    let fade_frames =
        fade_frame_count(buffer.sample_rate.max(1), selection_frames, fade_duration);
    apply_edge_crossfades(
        &mut buffer.samples[start..end],
        &original,
        channels,
        fade_frames,
    );
    Ok(())
}

fn fade_frame_count(sample_rate: u32, selection_frames: usize, duration: Duration) -> usize {
    if selection_frames == 0 {
        return 0;
    }
    let frames = (sample_rate as f32 * duration.as_secs_f32()).round() as usize;
    frames.min(selection_frames / 2)
}

fn apply_edge_crossfades(
    selection: &mut [f32],
    original: &[f32],
    channels: usize,
    fade_frames: usize,
) {
    if selection.is_empty() || fade_frames == 0 {
        return;
    }
    debug_assert_eq!(selection.len(), original.len());
    let channels = channels.max(1);
    let total_frames = selection.len() / channels;
    if total_frames == 0 {
        return;
    }
    let fade_frames = fade_frames.min(total_frames / 2);
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
    for frame in 0..fade_frames {
        let t = if fade_frames == 1 {
            1.0
        } else {
            frame as f32 / denom
        };
        let frame_idx = total_frames - fade_frames + frame;
        for ch in 0..channels {
            let idx = frame_idx * channels + ch;
            selection[idx] = lerp(selection[idx], original[idx], t);
        }
    }
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t.clamp(0.0, 1.0)
}
