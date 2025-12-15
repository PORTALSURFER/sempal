use crate::egui_app::state::{FadingPlayheadTrail, PlayheadState, PlayheadTrailSample};
use std::time::{Duration, Instant};

const TRAIL_DURATION: Duration = Duration::from_millis(1250);
const TRAIL_FADE: Duration = Duration::from_millis(450);
const MAX_TRAIL_SAMPLES: usize = 384;
const MAX_FADING_TRAILS: usize = 2;
const POSITION_EPS: f32 = 0.0005;
const JUMP_THRESHOLD: f32 = 0.02;
const MIN_SAMPLE_DT: Duration = Duration::from_millis(8); // ~120Hz

pub(super) fn start_or_seek_trail(playhead: &mut PlayheadState, position: f32, is_seek: bool) {
    let now = Instant::now();
    if is_seek {
        stash_active_trail(playhead);
    }
    playhead.trail.clear();
    let position = position.clamp(0.0, 1.0);
    playhead.trail.push_back(PlayheadTrailSample { position, time: now });
    // Seed a second sample so the gradient can render immediately even before the next tick.
    playhead.trail.push_back(PlayheadTrailSample {
        position,
        time: now + Duration::from_millis(1),
    });
}

pub(super) fn stash_active_trail(playhead: &mut PlayheadState) {
    if playhead.trail.is_empty() {
        return;
    }
    let samples = std::mem::take(&mut playhead.trail);
    playhead.fading_trails.push(FadingPlayheadTrail {
        started_at: Instant::now(),
        samples,
    });
    while playhead.fading_trails.len() > MAX_FADING_TRAILS {
        playhead.fading_trails.remove(0);
    }
}

pub(super) fn tick_playhead_trail(
    playhead: &mut PlayheadState,
    position: f32,
    is_looping: bool,
    is_playing: bool,
) {
    let now = Instant::now();
    playhead
        .fading_trails
        .retain(|trail| now.saturating_duration_since(trail.started_at) < TRAIL_FADE);

    if !is_playing {
        if !playhead.trail.is_empty() {
            stash_active_trail(playhead);
        }
        return;
    }

    let position = position.clamp(0.0, 1.0);
    let discontinuity = match playhead.trail.back() {
        Some(last) => {
            let delta = (position - last.position).abs();
            let backwards = position + POSITION_EPS < last.position;
            backwards || (!is_looping && delta > JUMP_THRESHOLD)
        }
        None => false,
    };

    if discontinuity {
        stash_active_trail(playhead);
        playhead.trail.push_back(PlayheadTrailSample { position, time: now });
        return;
    }

    let should_push = match playhead.trail.back() {
        Some(last) => {
            (position - last.position).abs() >= POSITION_EPS
                || now.saturating_duration_since(last.time) >= MIN_SAMPLE_DT
        }
        None => true,
    };
    if should_push {
        playhead.trail.push_back(PlayheadTrailSample { position, time: now });
    }

    while let Some(front) = playhead.trail.front() {
        if now.saturating_duration_since(front.time) > TRAIL_DURATION {
            playhead.trail.pop_front();
        } else {
            break;
        }
    }
    while playhead.trail.len() > MAX_TRAIL_SAMPLES {
        playhead.trail.pop_front();
    }
}
