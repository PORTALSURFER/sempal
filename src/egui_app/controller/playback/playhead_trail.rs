use crate::egui_app::state::{FadingPlayheadTrail, PlayheadState, PlayheadTrailSample};
use std::time::{Duration, Instant};

const TRAIL_DURATION: Duration = Duration::from_millis(1250);
const TRAIL_FADE: Duration = Duration::from_millis(450);
const MAX_TRAIL_SAMPLES: usize = 384;
const MAX_FADING_TRAILS: usize = 2;
const POSITION_EPS: f32 = 0.0005;
const MIN_SAMPLE_DT: Duration = Duration::from_millis(8); // ~120Hz

fn seed_trail(playhead: &mut PlayheadState, position: f32, now: Instant) {
    let position = position.clamp(0.0, 1.0);
    playhead.trail.clear();
    playhead.trail.push_back(PlayheadTrailSample {
        position,
        time: now,
    });
    playhead.trail.push_back(PlayheadTrailSample {
        position,
        time: now + Duration::from_millis(1),
    });
}

pub(super) fn start_or_seek_trail(playhead: &mut PlayheadState, position: f32, is_seek: bool) {
    let now = Instant::now();
    if is_seek {
        stash_active_trail(playhead);
    }
    seed_trail(playhead, position, now);
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
    _is_looping: bool,
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

    let mut position = position.clamp(0.0, 1.0);
    let discontinuity = match playhead.trail.back() {
        Some(last) => {
            let backwards = position + POSITION_EPS < last.position;
            backwards
        }
        None => false,
    };

    if discontinuity {
        stash_active_trail(playhead);
        seed_trail(playhead, position, now);
        return;
    }

    if let Some(last) = playhead.trail.back() {
        if position < last.position {
            position = last.position;
        }
    }

    let should_push = match playhead.trail.back() {
        Some(last) => {
            (position - last.position).abs() >= POSITION_EPS
                || now.saturating_duration_since(last.time) >= MIN_SAMPLE_DT
        }
        None => true,
    };
    if should_push {
        playhead.trail.push_back(PlayheadTrailSample {
            position,
            time: now,
        });
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

#[cfg(test)]
mod tests {
    use super::tick_playhead_trail;
    use crate::egui_app::state::{PlayheadState, PlayheadTrailSample};
    use std::time::{Duration, Instant};

    #[test]
    fn tick_playhead_trail_clamps_tiny_backwards_jitter() {
        let mut playhead = PlayheadState::default();
        playhead.trail.push_back(PlayheadTrailSample {
            position: 0.5,
            time: Instant::now() - Duration::from_secs(1),
        });

        tick_playhead_trail(&mut playhead, 0.4999, false, true);

        assert!(playhead.trail.len() >= 1);
        let last = playhead.trail.back().unwrap();
        assert!((last.position - 0.5).abs() < 1e-6);
    }

    #[test]
    fn tick_playhead_trail_allows_large_forward_deltas_for_short_audio() {
        let mut playhead = PlayheadState::default();
        playhead.trail.push_back(PlayheadTrailSample {
            position: 0.10,
            time: Instant::now() - Duration::from_millis(50),
        });

        tick_playhead_trail(&mut playhead, 0.30, false, true);

        assert!(playhead.fading_trails.is_empty());
        assert!(playhead.trail.len() >= 2);
        assert!((playhead.trail.back().unwrap().position - 0.30).abs() < 1e-6);
    }
}
