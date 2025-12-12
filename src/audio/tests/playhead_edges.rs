use super::*;

#[test]
fn normalized_progress_handles_tiny_selection_near_end() {
    let duration = 0.05;
    let span = (duration - 0.002, duration);
    let elapsed = 1.0;

    let progress = normalized_progress(Some(span), duration, elapsed, true).unwrap();
    assert!(progress >= span.0 / duration);
    assert!(progress < 1.0);
}

#[test]
fn remaining_loop_duration_stays_within_span_on_long_elapsed() {
    let Ok(stream) = rodio::OutputStreamBuilder::open_default_stream() else {
        return;
    };
    let span = (1.0_f32, 1.1_f32);
    let span_length = span.1 - span.0;
    let player = AudioPlayer {
        stream,
        sink: None,
        current_audio: None,
        track_duration: Some(8.0),
        started_at: Some(Instant::now() - Duration::from_secs(60 * 60)),
        play_span: Some(span),
        looping: true,
        loop_offset: None,
        volume: 1.0,
        output: ResolvedOutput::default(),
    };

    let remaining = player.remaining_loop_duration().unwrap().as_secs_f32();
    assert!(remaining > 0.0);
    assert!(remaining <= span_length + 0.05);
}

#[test]
fn progress_math_is_stable_for_long_running_full_track_loops() {
    let Ok(stream) = rodio::OutputStreamBuilder::open_default_stream() else {
        return;
    };
    let duration = 8.0_f32;
    let offset = 2.0_f32;
    let elapsed = 60.0_f32 * 60.0_f32 * 5.0_f32 + 0.25_f32;

    let player = AudioPlayer {
        stream,
        sink: None,
        current_audio: None,
        track_duration: Some(duration),
        started_at: Some(Instant::now() - Duration::from_secs_f32(elapsed)),
        play_span: Some((0.0, duration)),
        looping: true,
        loop_offset: Some(offset),
        volume: 1.0,
        output: ResolvedOutput::default(),
    };

    let progress = player.progress().unwrap();
    let expected = ((offset as f64 + elapsed as f64) % duration as f64) / duration as f64;
    assert!((progress as f64 - expected).abs() < 0.01);
}

