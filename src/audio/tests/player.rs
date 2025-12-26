use super::super::AudioPlayer;
use super::support::{fixtures, silent_wav_bytes, test_player};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};

#[test]
fn remaining_loop_duration_reports_time_left_in_cycle() {
    let Ok(stream) = rodio::OutputStreamBuilder::open_default_stream() else {
        return;
    };
    let started_at = Instant::now() - Duration::from_secs_f32(0.75);
    let player = test_player(
        stream,
        Some(8.0),
        Some(started_at),
        Some((1.0, 3.0)),
        true,
        None,
        None,
    );

    let remaining = player.remaining_loop_duration().unwrap();
    assert!((remaining.as_secs_f32() - 1.25).abs() < 0.1);
}

#[test]
fn remaining_loop_duration_accounts_for_full_track_offset() {
    let Ok(stream) = rodio::OutputStreamBuilder::open_default_stream() else {
        return;
    };
    let started_at = Instant::now() - Duration::from_secs_f32(0.5);
    let player = test_player(
        stream,
        Some(8.0),
        Some(started_at),
        Some((0.0, 8.0)),
        true,
        Some(2.0),
        None,
    );

    let remaining = player.remaining_loop_duration().unwrap();
    assert!((remaining.as_secs_f32() - 5.5).abs() < 0.1);
}

#[test]
fn remaining_loop_duration_none_when_not_looping() {
    let Ok(stream) = rodio::OutputStreamBuilder::open_default_stream() else {
        return;
    };
    let player = test_player(
        stream,
        Some(8.0),
        Some(Instant::now()),
        Some((1.0, 3.0)),
        false,
        None,
        None,
    );

    assert!(player.remaining_loop_duration().is_none());
}

#[test]
fn progress_wraps_full_loop_from_offset() {
    let Ok(stream) = rodio::OutputStreamBuilder::open_default_stream() else {
        return;
    };
    let player = test_player(
        stream,
        Some(10.0),
        Some(Instant::now() - Duration::from_secs_f32(2.0)),
        Some((0.0, 10.0)),
        true,
        Some(7.0),
        None,
    );

    let progress = player.progress().unwrap();
    assert!((progress - 0.9).abs() < 0.05);
}

#[test]
fn progress_uses_elapsed_override_for_looping() {
    let Ok(stream) = rodio::OutputStreamBuilder::open_default_stream() else {
        return;
    };
    let player = test_player(
        stream,
        Some(10.0),
        Some(Instant::now()),
        Some((2.0, 4.0)),
        true,
        Some(0.5),
        Some(Duration::from_secs_f32(1.25)),
    );

    let progress = player.progress().unwrap();
    assert!((progress - 0.375).abs() < 0.01);
}

#[test]
fn remaining_loop_duration_uses_elapsed_override() {
    let Ok(stream) = rodio::OutputStreamBuilder::open_default_stream() else {
        return;
    };
    let player = test_player(
        stream,
        Some(8.0),
        Some(Instant::now()),
        Some((1.0, 3.0)),
        true,
        None,
        Some(Duration::from_secs_f32(0.4)),
    );

    let remaining = player.remaining_loop_duration().unwrap();
    assert!((remaining.as_secs_f32() - 1.6).abs() < 0.01);
}

#[test]
fn play_range_at_track_end_expands_backwards() {
    let Ok(mut player) = AudioPlayer::new() else {
        return;
    };
    let duration = 0.5;
    player.set_audio(silent_wav_bytes(duration, 44_100, 1), duration);

    assert!(player.play_range(1.0, 1.0, false).is_ok());
    let (start, end) = player.play_span().expect("play span set");

    assert!(start < duration);
    assert!((end - duration).abs() < 0.0005);
    assert!((start - (duration - 0.01)).abs() < 0.002);
}

#[test]
fn play_looped_range_from_keeps_full_span() {
    let Ok(mut player) = AudioPlayer::new() else {
        return;
    };
    let duration = 2.0;
    player.set_audio(silent_wav_bytes(duration, 44_100, 1), duration);

    assert!(player.play_looped_range_from(0.25, 0.75, 0.5).is_ok());
    let (start, end) = player.play_span().expect("play span set");

    assert!((start - 0.5).abs() < 0.01);
    assert!((end - 1.5).abs() < 0.02);
    assert!(player.is_looping());
}

#[test]
fn set_audio_prefers_provided_duration() {
    let Ok(mut player) = AudioPlayer::new() else {
        return;
    };
    let bytes = silent_wav_bytes(2.0, 44_100, 2);
    player.set_audio(bytes, 2.0);
    let duration = player.track_duration().expect("duration set");
    assert!((duration - 2.0).abs() < 0.01);
}

#[test]
fn set_audio_falls_back_to_header() {
    let Ok(mut player) = AudioPlayer::new() else {
        return;
    };
    let bytes = silent_wav_bytes(2.0, 44_100, 2);
    player.set_audio(bytes, 0.0);
    let duration = player.track_duration().expect("duration set");
    assert!((duration - 2.0).abs() < 0.01);
}

#[test]
fn span_pipeline_preserves_sample_count() {
    let bytes = Arc::from(silent_wav_bytes(4.0, 1_000, 2));
    let (count, sample_rate, channels) =
        AudioPlayer::span_sample_count(bytes, 0.0, 4.0).expect("span count");
    let expected = (4.0 * sample_rate as f32 * channels as f32) as usize;
    let delta = (count as isize - expected as isize).abs();
    assert!(delta <= 2, "count {count}, expected {expected}");
}

#[test]
fn play_range_accepts_zero_width_request() {
    let Ok(stream) = rodio::OutputStreamBuilder::open_default_stream() else {
        return;
    };
    let mut player = test_player(stream, None, None, None, false, None, None);
    let bytes = vec![
        0x52, 0x49, 0x46, 0x46, 0x24, 0x80, 0x00, 0x00, 0x57, 0x41, 0x56, 0x45, 0x66, 0x6D, 0x74,
        0x20, 0x10, 0x00, 0x00, 0x00, 0x01, 0x00, 0x01, 0x00, 0x44, 0xAC, 0x00, 0x00, 0x88, 0x58,
        0x01, 0x00, 0x02, 0x00, 0x10, 0x00, 0x64, 0x61, 0x74, 0x61, 0x00, 0x80, 0x00, 0x00, 0x00,
        0x00,
    ];
    player.set_audio(bytes, 1.0);
    assert!(player.play_range(0.5, 0.5, false).is_ok());
}

#[test]
fn span_sample_count_tracks_requested_window() {
    let spec = fixtures::ToneSpec::new(22_050, 2, 0.8).with_pulse(fixtures::TonePulse {
        start_seconds: 0.6,
        duration_seconds: 0.05,
        amplitude: 0.5,
    });
    let fixture = fixtures::build_fixture(spec);
    let start = 0.25 * fixture.spec.duration_seconds;
    let end = 0.75 * fixture.spec.duration_seconds;
    let bytes = Arc::from(fixture.bytes.clone());

    let (count, sample_rate, channels) =
        AudioPlayer::span_sample_count(bytes, start, end).expect("span count");
    let expected_frames = ((end - start) * sample_rate as f32) as usize;
    let expected_samples = expected_frames * channels as usize;
    let delta = (count as isize - expected_samples as isize).abs();
    assert!(
        delta <= 2,
        "count {count}, expected {expected_samples} (frames {expected_frames})"
    );
}

#[test]
fn aligned_span_seconds_snaps_to_frames() {
    let span_length = 0.3333;
    let sample_rate = 48_000;
    let aligned = AudioPlayer::aligned_span_seconds_for_tests(span_length, sample_rate);
    let frames = aligned * sample_rate as f32;
    let rounded = frames.round();
    assert!((frames - rounded).abs() < 1e-3);
    assert!(aligned <= span_length);
    assert!(aligned > 0.0);
}
