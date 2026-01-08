use super::buffer::SelectionEditBuffer;

#[derive(Clone, Copy, Debug)]
struct ClickRepairBounds {
    start_frame: usize,
    end_frame: usize,
    total_frames: usize,
    channels: usize,
}

/// Replace the selected frames with an interpolated repair to remove clicks.
pub(crate) fn repair_clicks_selection(buffer: &mut SelectionEditBuffer) -> Result<(), String> {
    let bounds = selection_bounds(buffer)?;
    ensure_neighbors(&bounds)?;
    let original = buffer.samples.clone();
    let selection_len = bounds.end_frame - bounds.start_frame;
    for channel in 0..bounds.channels {
        let left = sample_at(&original, bounds.channels, bounds.start_frame - 1, channel);
        let right = sample_at(&original, bounds.channels, bounds.end_frame, channel);
        for offset in 0..selection_len {
            let frame = bounds.start_frame + offset;
            let t = (offset + 1) as f32 / (selection_len + 1) as f32;
            let value = left + (right - left) * smoothstep(t);
            let idx = frame * bounds.channels + channel;
            buffer.samples[idx] = value;
        }
    }
    Ok(())
}

fn selection_bounds(buffer: &SelectionEditBuffer) -> Result<ClickRepairBounds, String> {
    let channels = buffer.channels.max(1);
    let total_frames = buffer.samples.len() / channels;
    let start_frame = buffer.start_frame.min(total_frames);
    let end_frame = buffer.end_frame.min(total_frames);
    if end_frame <= start_frame {
        return Err("Selection is empty".into());
    }
    Ok(ClickRepairBounds {
        start_frame,
        end_frame,
        total_frames,
        channels,
    })
}

fn ensure_neighbors(bounds: &ClickRepairBounds) -> Result<(), String> {
    if bounds.start_frame == 0 || bounds.end_frame >= bounds.total_frames {
        return Err("Selection needs audio on both sides".into());
    }
    Ok(())
}

fn sample_at(samples: &[f32], channels: usize, frame: usize, channel: usize) -> f32 {
    samples[frame * channels + channel]
}

fn smoothstep(t: f32) -> f32 {
    let clamped = t.clamp(0.0, 1.0);
    clamped * clamped * (3.0 - 2.0 * clamped)
}
