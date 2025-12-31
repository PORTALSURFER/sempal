use super::SelectionEditBuffer;

#[derive(Clone, Copy, Debug)]
struct ClickRepairBounds {
    start_frame: usize,
    end_frame: usize,
    total_frames: usize,
    channels: usize,
}

/// Replace the selected frames with an interpolated repair to remove clicks.
pub(super) fn repair_clicks_selection(
    buffer: &mut SelectionEditBuffer,
) -> Result<(), String> {
    let bounds = selection_bounds(buffer)?;
    ensure_neighbors(&bounds)?;
    let original = buffer.samples.clone();
    for frame in bounds.start_frame..bounds.end_frame {
        for channel in 0..bounds.channels {
            if should_repair_click(&original, &bounds, frame, channel) {
                let idx = frame * bounds.channels + channel;
                buffer.samples[idx] = replacement_sample(&original, &bounds, frame, channel);
            }
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

fn should_repair_click(
    samples: &[f32],
    bounds: &ClickRepairBounds,
    frame: usize,
    channel: usize,
) -> bool {
    let prev = sample_at(samples, bounds.channels, frame - 1, channel);
    let current = sample_at(samples, bounds.channels, frame, channel);
    let next = sample_at(samples, bounds.channels, frame + 1, channel);
    let local = prev.abs().max(next.abs()).max(1e-3);
    let diff_prev = (current - prev).abs();
    let diff_next = (current - next).abs();
    let neighbors_close = (prev - next).abs() <= local * 0.5;
    if neighbors_close {
        return diff_prev > local * 2.5 && diff_next > local * 2.5;
    }
    let interp = 0.5 * (prev + next);
    let diff_interp = (current - interp).abs();
    diff_interp > local * 3.0 && diff_prev > local * 2.5 && diff_next > local * 2.5
}

fn replacement_sample(
    samples: &[f32],
    bounds: &ClickRepairBounds,
    frame: usize,
    channel: usize,
) -> f32 {
    let prev = sample_at(samples, bounds.channels, frame - 1, channel);
    let next = sample_at(samples, bounds.channels, frame + 1, channel);
    if frame >= 2 && frame + 2 < bounds.total_frames {
        let p0 = sample_at(samples, bounds.channels, frame - 2, channel);
        let p3 = sample_at(samples, bounds.channels, frame + 2, channel);
        return catmull_rom(p0, prev, next, p3, 0.5);
    }
    0.5 * (prev + next)
}

fn catmull_rom(p0: f32, p1: f32, p2: f32, p3: f32, t: f32) -> f32 {
    let t2 = t * t;
    let t3 = t2 * t;
    0.5
        * (2.0 * p1
            + (-p0 + p2) * t
            + (2.0 * p0 - 5.0 * p1 + 4.0 * p2 - p3) * t2
            + (-p0 + 3.0 * p1 - 3.0 * p2 + p3) * t3)
}
