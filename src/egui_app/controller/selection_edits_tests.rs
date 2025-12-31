use super::*;
use super::selection_click::repair_clicks_selection;

#[test]
fn slice_frames_keeps_requested_range() {
    let samples = vec![0.1, 0.2, 0.3, 0.4, 0.5, 0.6];
    let sliced = slice_frames(&samples, 2, 1, 3);
    assert_eq!(sliced, vec![0.3, 0.4, 0.5, 0.6]);
}

#[test]
fn trim_removes_target_span() {
    let mut buffer = SelectionEditBuffer {
        samples: vec![1.0_f32; 8],
        channels: 1,
        sample_rate: 48_000,
        spec_channels: 1,
        start_frame: 2,
        end_frame: 6,
    };
    trim_buffer(&mut buffer).unwrap();
    assert_eq!(buffer.samples.len(), 4);
}

#[test]
fn directional_fade_zeroes_expected_side() {
    let mut samples = vec![1.0_f32; 6];
    apply_directional_fade(&mut samples, 1, 0, 6, FadeDirection::LeftToRight);
    assert!(samples[5].abs() < 1e-6);
    let mut samples = vec![1.0_f32; 6];
    apply_directional_fade(&mut samples, 1, 0, 6, FadeDirection::RightToLeft);
    assert!(samples[0].abs() < 1e-6);
}

#[test]
fn directional_fade_left_to_right_zeroes_tail() {
    let mut samples = vec![1.0_f32; 10];
    apply_directional_fade(&mut samples, 1, 2, 6, FadeDirection::LeftToRight);
    assert!((samples[1] - 1.0).abs() < 1e-6);
    assert!(samples[6..].iter().all(|sample| sample.abs() < 1e-6));
}

#[test]
fn directional_fade_right_to_left_zeroes_head() {
    let mut samples = vec![1.0_f32; 10];
    apply_directional_fade(&mut samples, 1, 3, 7, FadeDirection::RightToLeft);
    assert!(samples[..3].iter().all(|sample| sample.abs() < 1e-6));
    assert!((samples[9] - 1.0).abs() < 1e-6);
}

#[test]
fn mute_zeroes_selection_without_fades() {
    let mut samples = vec![1.0_f32; 10];
    apply_muted_selection(&mut samples, 1, 0, 10);
    assert!(samples.iter().all(|sample| sample.abs() < 1e-6));
}

#[test]
fn crop_keeps_only_selection_frames() {
    let mut buffer = SelectionEditBuffer {
        samples: vec![0.0, 1.0, 2.0, 3.0],
        channels: 1,
        sample_rate: 44_100,
        spec_channels: 1,
        start_frame: 1,
        end_frame: 3,
    };
    crop_buffer(&mut buffer).unwrap();
    assert_eq!(buffer.samples, vec![1.0, 2.0]);
}

#[test]
fn reverse_buffer_reverses_selection_frames_in_place() {
    let mut buffer = SelectionEditBuffer {
        samples: vec![0.0, 1.0, 2.0, 3.0, 4.0],
        channels: 1,
        sample_rate: 44_100,
        spec_channels: 1,
        start_frame: 1,
        end_frame: 4,
    };
    reverse_buffer(&mut buffer).unwrap();
    assert_eq!(buffer.samples, vec![0.0, 3.0, 2.0, 1.0, 4.0]);
}

#[test]
fn reverse_buffer_reverses_interleaved_frames() {
    let mut buffer = SelectionEditBuffer {
        // frames: (0,10), (1,11), (2,12), (3,13)
        samples: vec![0.0, 10.0, 1.0, 11.0, 2.0, 12.0, 3.0, 13.0],
        channels: 2,
        sample_rate: 44_100,
        spec_channels: 2,
        start_frame: 1,
        end_frame: 4,
    };
    reverse_buffer(&mut buffer).unwrap();
    assert_eq!(
        buffer.samples,
        vec![0.0, 10.0, 3.0, 13.0, 2.0, 12.0, 1.0, 11.0]
    );
}

#[test]
fn selection_frame_bounds_include_tail() {
    let bounds = SelectionRange::new(0.8, 1.0);
    let (start, end) = selection_frame_bounds(5, bounds);
    assert_eq!((start, end), (4, 5));
}

#[test]
fn directional_fade_with_single_frame_zeroes_sample() {
    let mut samples = vec![0.5_f32, 1.0];
    apply_directional_fade(&mut samples, 1, 1, 2, FadeDirection::LeftToRight);
    assert!(samples[1].abs() < 1e-6);
}

#[test]
fn fade_factor_uses_soft_s_curve() {
    let left_to_right = fade_factor(10, 0.25, FadeDirection::LeftToRight);
    let right_to_left = fade_factor(10, 0.25, FadeDirection::RightToLeft);
    // For a softer curve, early fade is gentler than linear.
    assert!(
        left_to_right > 0.8,
        "expected softer fade, got {left_to_right}"
    );
    assert!(
        right_to_left < 0.2,
        "expected softer fade, got {right_to_left}"
    );
}

#[test]
fn mute_respects_selection_bounds() {
    let mut samples = vec![0.5_f32; 6];
    apply_muted_selection(&mut samples, 1, 2, 4);
    assert!((samples[0] - 0.5).abs() < 1e-6);
    assert!((samples[1] - 0.5).abs() < 1e-6);
    assert!(samples[2].abs() < 1e-6);
    assert!(samples[3].abs() < 1e-6);
    assert!((samples[4] - 0.5).abs() < 1e-6);
}

#[test]
fn click_repair_interpolates_single_sample_linearly() {
    let mut buffer = SelectionEditBuffer {
        samples: vec![0.0_f32, 1.0, 8.0, -1.0, 0.0],
        channels: 1,
        sample_rate: 48_000,
        spec_channels: 1,
        start_frame: 2,
        end_frame: 3,
    };

    repair_clicks_selection(&mut buffer).unwrap();

    assert!(buffer.samples[2].abs() < 1e-6);
}

#[test]
fn click_repair_interpolates_multichannel_linearly() {
    let mut buffer = SelectionEditBuffer {
        samples: vec![
            0.2_f32, -0.2, // frame 0 (before)
            3.0, -3.0, // frame 1 (selection)
            0.6, -0.6, // frame 2 (after)
        ],
        channels: 2,
        sample_rate: 48_000,
        spec_channels: 2,
        start_frame: 1,
        end_frame: 2,
    };

    repair_clicks_selection(&mut buffer).unwrap();

    assert!((buffer.samples[2] - 0.4).abs() < 1e-6);
    assert!((buffer.samples[3] + 0.4).abs() < 1e-6);
}

#[test]
fn click_repair_leaves_smooth_peak_untouched() {
    let mut buffer = SelectionEditBuffer {
        samples: vec![0.0_f32, 0.5, 1.0, 0.5, 0.0],
        channels: 1,
        sample_rate: 48_000,
        spec_channels: 1,
        start_frame: 2,
        end_frame: 3,
    };

    repair_clicks_selection(&mut buffer).unwrap();

    assert!((buffer.samples[2] - 1.0).abs() < 1e-6);
}

#[test]
fn normalize_selection_scales_and_blends_edges() {
    let mut samples = vec![0.0_f32; 20];
    let selection_values = vec![
        0.1, 0.2, 0.3, 0.35, 0.4, 0.6, 0.8, 0.6, 0.4, 0.3, 0.25, 0.2, 0.15, 0.1, 0.05,
    ];
    let start_frame = 2;
    for (i, value) in selection_values.iter().enumerate() {
        samples[start_frame + i] = *value;
    }
    let before_selection = samples[start_frame - 1];
    let end_frame = start_frame + selection_values.len();
    let mut buffer = SelectionEditBuffer {
        samples,
        channels: 1,
        sample_rate: 1_000,
        spec_channels: 1,
        start_frame,
        end_frame,
    };

    normalize_selection(&mut buffer, Duration::from_millis(5)).unwrap();

    let peak_index = start_frame + 6;
    assert!((buffer.samples[peak_index] - 1.0).abs() < 1e-6);
    assert!((buffer.samples[start_frame] - selection_values[0]).abs() < 1e-6);
    let last_index = end_frame - 1;
    assert!((buffer.samples[last_index] - *selection_values.last().unwrap()).abs() < 1e-6);
    assert!((buffer.samples[start_frame - 1] - before_selection).abs() < 1e-6);
    assert!(buffer.samples[end_frame].abs() < 1e-6);
}
