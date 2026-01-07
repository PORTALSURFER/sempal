use super::super::test_support::{dummy_controller, sample_entry, write_test_wav};
use super::super::*;
use super::common::prepare_browser_sample;
use crate::egui_app::state::FocusContext;
use crate::waveform::DecodedWaveform;
use std::path::Path;
use std::time::{Duration, Instant};

#[test]
fn cursor_step_size_tracks_view_zoom() {
    let (mut controller, source) = dummy_controller();
    prepare_browser_sample(&mut controller, &source, "zoom.wav");
    controller.update_waveform_size(200, 10);
    controller.select_wav_by_path(Path::new("zoom.wav"));
    controller.sample_view.waveform.decoded = Some(DecodedWaveform {
        cache_token: 1,
        samples: std::sync::Arc::from(vec![0.0; 10_000]),
        peaks: None,
        duration_seconds: 1.0,
        sample_rate: 48_000,
        channels: 1,
    });
    controller.ui.waveform.playhead.position = 0.1;
    controller.ui.waveform.playhead.visible = true;
    controller.set_waveform_cursor(0.5);

    controller.move_playhead_steps(1, false, false);
    assert!((controller.ui.waveform.cursor.unwrap() - 0.66).abs() < 0.001);
    assert!((controller.ui.waveform.playhead.position - 0.1).abs() < 0.001);

    controller.zoom_waveform(true);
    controller.move_playhead_steps(1, false, false);
    assert!((controller.ui.waveform.cursor.unwrap() - 0.804).abs() < 0.001);
    assert!((controller.ui.waveform.playhead.position - 0.1).abs() < 0.001);
}

#[test]
fn batched_zoom_matches_sequential_steps() {
    let (mut batched, source_a) = dummy_controller();
    prepare_browser_sample(&mut batched, &source_a, "zoom.wav");
    batched.update_waveform_size(240, 24);
    batched.select_wav_by_path(Path::new("zoom.wav"));
    batched.ui.waveform.playhead.position = 0.4;
    batched.ui.waveform.playhead.visible = true;

    let (mut stepped, source_b) = dummy_controller();
    prepare_browser_sample(&mut stepped, &source_b, "zoom.wav");
    stepped.update_waveform_size(240, 24);
    stepped.select_wav_by_path(Path::new("zoom.wav"));
    stepped.ui.waveform.playhead.position = 0.4;
    stepped.ui.waveform.playhead.visible = true;

    batched.zoom_waveform_steps(true, 3, None);
    for _ in 0..3 {
        stepped.zoom_waveform(true);
    }

    let view_a = batched.ui.waveform.view;
    let view_b = stepped.ui.waveform.view;
    assert!((view_a.start - view_b.start).abs() < 1e-6);
    assert!((view_a.end - view_b.end).abs() < 1e-6);
}

#[test]
fn mouse_zoom_prefers_pointer_over_playhead() {
    let (mut controller, _source) = dummy_controller();
    controller.sample_view.waveform.size = [240, 24];
    controller.sample_view.waveform.decoded = Some(DecodedWaveform {
        cache_token: 1,
        samples: std::sync::Arc::from(vec![0.0; 10_000]),
        peaks: None,
        duration_seconds: 1.0,
        sample_rate: 48_000,
        channels: 1,
    });
    controller.ui.waveform.playhead.position = 0.1;
    controller.ui.waveform.playhead.visible = true;

    controller.zoom_waveform_steps_with_factor(true, 1, Some(0.8), Some(0.5), false, false);

    let center = (controller.ui.waveform.view.start + controller.ui.waveform.view.end) * 0.5;
    let playhead_dist = (center - 0.1).abs();
    let pointer_dist = (center - 0.8).abs();
    assert!(
        pointer_dist < playhead_dist,
        "zoom centered closer to playhead ({playhead_dist}) than pointer ({pointer_dist}), center {center}"
    );
    assert!(controller.ui.waveform.view.start < controller.ui.waveform.view.end);
}

#[test]
fn playhead_completion_detects_full_span_end() {
    let (controller, _source) = dummy_controller();

    assert!(!controller.playhead_completed_span_for_tests(0.5, false));
    assert!(controller.playhead_completed_span_for_tests(0.9995, false));
    assert!(!controller.playhead_completed_span_for_tests(1.0, true));
}

#[test]
fn playhead_completion_tracks_selection_end() {
    let (mut controller, _source) = dummy_controller();
    controller.ui.waveform.playhead.active_span_end = Some(0.25);

    assert!(!controller.playhead_completed_span_for_tests(0.2, false));
    assert!(controller.playhead_completed_span_for_tests(0.251, false));
}

#[test]
fn hiding_playhead_clears_span_target() {
    let (mut controller, _source) = dummy_controller();
    controller.ui.waveform.playhead.visible = true;
    controller.ui.waveform.playhead.active_span_end = Some(0.4);

    controller.hide_waveform_playhead_for_tests();

    assert!(!controller.ui.waveform.playhead.visible);
    assert!(controller.ui.waveform.playhead.active_span_end.is_none());
}

#[test]
fn last_start_marker_clamps_and_resets() {
    let (mut controller, source) = dummy_controller();
    prepare_browser_sample(&mut controller, &source, "marker.wav");

    controller.record_play_start(-0.25);
    assert_eq!(controller.ui.waveform.last_start_marker, Some(0.0));

    controller.record_play_start(0.75);
    assert_eq!(controller.ui.waveform.last_start_marker, Some(0.75));

    controller.clear_waveform_view();
    assert!(controller.ui.waveform.last_start_marker.is_none());
}

#[test]
fn selecting_new_sample_clears_last_start_marker() {
    let (mut controller, source) = dummy_controller();
    controller.library.sources.push(source.clone());
    write_test_wav(&source.root.join("a.wav"), &[0.0, 0.1]);
    write_test_wav(&source.root.join("b.wav"), &[0.2, -0.2]);
    controller.set_wav_entries_for_tests(vec![
        sample_entry("a.wav", SampleTag::Neutral),
        sample_entry("b.wav", SampleTag::Neutral),
    ]);
    controller.rebuild_wav_lookup();
    controller.rebuild_browser_lists();

    controller.select_wav_by_path(Path::new("a.wav"));
    controller.record_play_start(0.25);
    controller.select_wav_by_path(Path::new("b.wav"));

    assert!(controller.ui.waveform.last_start_marker.is_none());
}

#[test]
fn replay_from_last_start_requeues_pending_playback() {
    let (mut controller, source) = dummy_controller();
    prepare_browser_sample(&mut controller, &source, "marker.wav");
    controller.select_wav_by_path(Path::new("marker.wav"));
    controller.record_play_start(0.42);
    controller.ui.waveform.playhead.visible = true;
    controller.ui.waveform.playhead.position = 0.1;

    let handled = controller.replay_from_last_start();
    assert!(handled);
    let pending = controller
        .runtime
        .jobs
        .pending_playback
        .as_ref()
        .expect("pending playback request");
    assert_eq!(pending.start_override, Some(0.42));
}

#[test]
fn replay_from_last_start_falls_back_to_cursor() {
    let (mut controller, source) = dummy_controller();
    prepare_browser_sample(&mut controller, &source, "marker.wav");
    controller.select_wav_by_path(Path::new("marker.wav"));
    controller.ui.waveform.cursor = Some(0.25);
    controller.ui.waveform.last_start_marker = None;

    let handled = controller.replay_from_last_start();

    assert!(handled);
    let pending = controller
        .runtime
        .jobs
        .pending_playback
        .as_ref()
        .expect("pending playback request");
    assert_eq!(pending.start_override, Some(0.25));
    assert_eq!(controller.ui.waveform.last_start_marker, Some(0.25));
}

#[test]
fn play_from_cursor_prefers_cursor_position() {
    let (mut controller, source) = dummy_controller();
    prepare_browser_sample(&mut controller, &source, "cursor.wav");
    controller.select_wav_by_path(Path::new("cursor.wav"));
    controller.sample_view.waveform.decoded = Some(DecodedWaveform {
        cache_token: 1,
        samples: std::sync::Arc::from(vec![0.0; 10_000]),
        peaks: None,
        duration_seconds: 1.0,
        sample_rate: 48_000,
        channels: 1,
    });
    controller.ui.waveform.cursor = Some(0.33);
    controller.ui.waveform.cursor_last_navigation_at = Some(Instant::now());
    controller.ui.waveform.cursor_last_hover_at = None;
    controller.ui.waveform.last_start_marker = Some(0.1);

    let handled = controller.play_from_cursor();

    assert!(handled);
    let pending = controller
        .runtime
        .jobs
        .pending_playback
        .as_ref()
        .expect("pending playback request");
    assert_eq!(pending.start_override, Some(0.33));
    assert_eq!(controller.ui.waveform.last_start_marker, Some(0.33));
}

#[test]
fn play_from_cursor_ignores_hover_cursor_when_replaying() {
    let (mut controller, source) = dummy_controller();
    prepare_browser_sample(&mut controller, &source, "cursor.wav");
    controller.select_wav_by_path(Path::new("cursor.wav"));
    controller.sample_view.waveform.decoded = Some(DecodedWaveform {
        cache_token: 1,
        samples: std::sync::Arc::from(vec![0.0; 10_000]),
        peaks: None,
        duration_seconds: 1.0,
        sample_rate: 48_000,
        channels: 1,
    });
    controller.ui.waveform.cursor = Some(0.33);
    controller.ui.waveform.cursor_last_navigation_at =
        Some(Instant::now() - Duration::from_secs(5));
    controller.ui.waveform.cursor_last_hover_at = Some(Instant::now());
    controller.ui.waveform.last_start_marker = Some(0.1);

    let handled = controller.play_from_cursor();

    assert!(handled);
    let pending = controller
        .runtime
        .jobs
        .pending_playback
        .as_ref()
        .expect("pending playback request");
    assert_eq!(pending.start_override, Some(0.1));
    assert_eq!(controller.ui.waveform.last_start_marker, Some(0.1));
}

#[test]
fn cursor_alpha_fades_before_reset() {
    let (mut controller, source) = dummy_controller();
    prepare_browser_sample(&mut controller, &source, "cursor.wav");
    controller.sample_view.waveform.decoded = Some(DecodedWaveform {
        cache_token: 1,
        samples: std::sync::Arc::from(vec![0.0; 10_000]),
        peaks: None,
        duration_seconds: 1.0,
        sample_rate: 48_000,
        channels: 1,
    });
    controller.ui.waveform.cursor = Some(0.4);
    controller.ui.waveform.cursor_last_navigation_at =
        Some(Instant::now() - Duration::from_millis(250));

    let alpha = controller.waveform_cursor_alpha(false);

    assert!((alpha - 0.5).abs() < 0.15);
    assert_eq!(controller.ui.waveform.cursor, Some(0.4));
}

#[test]
fn cursor_alpha_resets_after_idle_timeout() {
    let (mut controller, source) = dummy_controller();
    prepare_browser_sample(&mut controller, &source, "cursor.wav");
    controller.sample_view.waveform.decoded = Some(DecodedWaveform {
        cache_token: 1,
        samples: std::sync::Arc::from(vec![0.0; 10_000]),
        peaks: None,
        duration_seconds: 1.0,
        sample_rate: 48_000,
        channels: 1,
    });
    controller.ui.waveform.cursor = Some(0.4);
    controller.ui.waveform.cursor_last_navigation_at =
        Some(Instant::now() - Duration::from_millis(600));

    let alpha = controller.waveform_cursor_alpha(false);

    assert!(alpha <= f32::EPSILON);
    assert_eq!(controller.ui.waveform.cursor, Some(0.0));
}

#[test]
fn cursor_does_not_fade_when_waveform_focused() {
    let (mut controller, source) = dummy_controller();
    prepare_browser_sample(&mut controller, &source, "cursor.wav");
    controller.sample_view.waveform.decoded = Some(DecodedWaveform {
        cache_token: 1,
        samples: std::sync::Arc::from(vec![0.0; 10_000]),
        peaks: None,
        duration_seconds: 1.0,
        sample_rate: 48_000,
        channels: 1,
    });
    controller.ui.waveform.cursor = Some(0.4);
    controller.ui.waveform.cursor_last_navigation_at =
        Some(Instant::now() - Duration::from_millis(800));
    controller.ui.focus.context = FocusContext::Waveform;

    let alpha = controller.waveform_cursor_alpha(false);

    assert_eq!(alpha, 1.0);
    assert_eq!(controller.ui.waveform.cursor, Some(0.4));
}

#[test]
fn navigation_steps_anchor_to_cursor_instead_of_playhead() {
    let (mut controller, source) = dummy_controller();
    prepare_browser_sample(&mut controller, &source, "nav.wav");
    controller.update_waveform_size(200, 10);
    controller.select_wav_by_path(Path::new("nav.wav"));
    controller.sample_view.waveform.decoded = Some(DecodedWaveform {
        cache_token: 1,
        samples: std::sync::Arc::from(vec![0.0; 10_000]),
        peaks: None,
        duration_seconds: 1.0,
        sample_rate: 48_000,
        channels: 1,
    });
    controller.ui.waveform.playhead.position = 0.7;
    controller.ui.waveform.playhead.visible = true;
    controller.ui.waveform.cursor = Some(0.2);
    controller.ui.waveform.last_start_marker = Some(0.2);

    controller.move_playhead_steps(1, false, false);

    let cursor = controller.ui.waveform.cursor.expect("cursor set");
    assert!((cursor - 0.36).abs() < 0.001);
    assert!((controller.ui.waveform.playhead.position - 0.7).abs() < 0.001);
}
