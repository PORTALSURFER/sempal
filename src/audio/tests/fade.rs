use super::super::{
    DEFAULT_ANTI_CLIP_FADE, EdgeFade, FadeOutHandle, FadeOutOnRequest, fade_duration,
};
use super::support::ConstantSource;
use std::time::Duration;

#[test]
fn fade_duration_clamps_to_span_length() {
    let short = fade_duration(0.004, DEFAULT_ANTI_CLIP_FADE);
    assert!((short.as_secs_f32() - 0.002).abs() < 1e-6);
    let long = fade_duration(0.25, DEFAULT_ANTI_CLIP_FADE);
    assert!((long.as_secs_f32() - DEFAULT_ANTI_CLIP_FADE.as_secs_f32()).abs() < 1e-6);
    assert_eq!(
        fade_duration(0.0, DEFAULT_ANTI_CLIP_FADE),
        Duration::from_secs(0)
    );
}

#[test]
fn edge_fade_ramps_start_samples() {
    let fade = Duration::from_millis(5);
    let source = ConstantSource::new(1_000, 1, 0.02, 1.0);
    let samples: Vec<f32> = EdgeFade::new(source, fade).take(10).collect();
    assert!(samples[0] < samples[1]);
    assert!(samples[4] < 1.0);
    assert!(samples[6] > 0.9);
}

#[test]
fn edge_fade_handles_tiny_segments() {
    let span_secs = 0.002;
    let fade = fade_duration(span_secs, DEFAULT_ANTI_CLIP_FADE);
    let source = ConstantSource::new(1_000, 1, span_secs, 1.0);
    let samples: Vec<f32> = EdgeFade::new(source, fade).collect();
    assert_eq!(samples.len(), 2);
    assert!(samples[0] < samples[1]);
}

#[test]
fn fade_out_on_request_ramps_to_zero_and_stops() {
    let source = ConstantSource::new(1_000, 2, 1.0, 1.0);
    let handle = FadeOutHandle::new();
    let mut faded = FadeOutOnRequest::new(source, handle.clone());

    let mut samples: Vec<f32> = faded.by_ref().take(4).collect();
    handle.request_fade_out_frames(3);
    samples.extend(faded);

    assert_eq!(samples.len(), 10);
    // Fade begins at a frame boundary, so each stereo frame shares the same factor.
    assert_eq!(samples[4], 1.0);
    assert_eq!(samples[5], 1.0);
    assert_eq!(samples[6], 0.5);
    assert_eq!(samples[7], 0.5);
    assert_eq!(samples[8], 0.0);
    assert_eq!(samples[9], 0.0);
}

#[test]
fn edge_fade_tracks_frames_for_multichannel() {
    let source = ConstantSource::new(1_000, 2, 1.0, 1.0);
    let mut faded = EdgeFade::new(source, Duration::from_millis(100));
    let samples: Vec<f32> = faded.by_ref().collect();
    assert_eq!(samples.len(), 2_000);
    // Halfway through the clip should still be fully audible (no early fade-out).
    assert!(samples[1_000] > 0.5);
}
