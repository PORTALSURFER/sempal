use super::SelectionEditBuffer;

#[derive(Clone, Copy, Debug)]
struct ClickRepairBounds {
    start_frame: usize,
    end_frame: usize,
    frames: usize,
    total_frames: usize,
    channels: usize,
}

/// Replace the selected frames with an interpolated repair to remove clicks.
pub(super) fn repair_clicks_selection(
    buffer: &mut SelectionEditBuffer,
) -> Result<(), String> {
    let bounds = selection_bounds(buffer)?;
    ensure_neighbors(&bounds)?;
    let use_cubic = bounds.frames > 1
        && bounds.start_frame >= 2
        && bounds.end_frame + 1 < bounds.total_frames;
    for channel in 0..bounds.channels {
        let before = sample_at(&buffer.samples, bounds.channels, bounds.start_frame - 1, channel);
        let after = sample_at(&buffer.samples, bounds.channels, bounds.end_frame, channel);
        if use_cubic {
            let p0 =
                sample_at(&buffer.samples, bounds.channels, bounds.start_frame - 2, channel);
            let p3 = sample_at(&buffer.samples, bounds.channels, bounds.end_frame + 1, channel);
            fill_selection_cubic(&mut buffer.samples, &bounds, channel, p0, before, after, p3);
        } else {
            fill_selection_linear(&mut buffer.samples, &bounds, channel, before, after);
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
        frames: end_frame - start_frame,
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

fn fill_selection_linear(
    samples: &mut [f32],
    bounds: &ClickRepairBounds,
    channel: usize,
    before: f32,
    after: f32,
) {
    let denom = (bounds.frames + 1) as f32;
    for frame in 0..bounds.frames {
        let t = (frame + 1) as f32 / denom;
        let idx = (bounds.start_frame + frame) * bounds.channels + channel;
        samples[idx] = lerp(before, after, t);
    }
}

fn fill_selection_cubic(
    samples: &mut [f32],
    bounds: &ClickRepairBounds,
    channel: usize,
    p0: f32,
    p1: f32,
    p2: f32,
    p3: f32,
) {
    let denom = (bounds.frames + 1) as f32;
    for frame in 0..bounds.frames {
        let t = (frame + 1) as f32 / denom;
        let idx = (bounds.start_frame + frame) * bounds.channels + channel;
        samples[idx] = catmull_rom(p0, p1, p2, p3, t);
    }
}

fn sample_at(samples: &[f32], channels: usize, frame: usize, channel: usize) -> f32 {
    samples[frame * channels + channel]
}

fn catmull_rom(p0: f32, p1: f32, p2: f32, p3: f32, t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    let t2 = t * t;
    let t3 = t2 * t;
    0.5
        * (2.0 * p1
            + (-p0 + p2) * t
            + (2.0 * p0 - 5.0 * p1 + 4.0 * p2 - p3) * t2
            + (-p0 + 3.0 * p1 - 3.0 * p2 + p3) * t3)
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    a + (b - a) * t
}
