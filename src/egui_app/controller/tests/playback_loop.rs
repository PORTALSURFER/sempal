use super::super::test_support::{dummy_controller, write_test_wav};
use super::super::*;
use crate::selection::SelectionRange;
use std::path::PathBuf;
use tempfile::tempdir;

fn setup_looping_controller(selection: SelectionRange) -> Option<EguiController> {
    let Some(mut player) = crate::audio::AudioPlayer::playing_for_tests() else {
        return None;
    };
    let dir = tempdir().ok()?;
    let wav_path = dir.path().join("loop_drag_test.wav");
    let long_samples = vec![0.1_f32; 240];
    write_test_wav(&wav_path, &long_samples);
    let bytes = std::fs::read(&wav_path).ok()?;
    let duration = 30.0;
    player.set_audio(bytes.clone(), duration);

    let (mut controller, source) = dummy_controller();
    controller.sample_view.wav.loaded_audio = Some(LoadedAudio {
        source_id: source.id.clone(),
        relative_path: PathBuf::from("loop_drag_test.wav"),
        bytes,
        duration_seconds: duration,
        sample_rate: 8,
        channels: 1,
    });
    controller.audio.player = Some(std::rc::Rc::new(std::cell::RefCell::new(player)));
    controller.selection_state.range.set_range(Some(selection));
    controller.apply_selection(Some(selection));
    controller.ui.waveform.loop_enabled = true;
    let _ = controller.play_audio(true, None);
    if !controller.is_playing() {
        return None;
    }
    Some(controller)
}

#[test]
fn enabling_loop_while_playing_restarts_in_looped_mode() {
    let Some(mut player) = crate::audio::AudioPlayer::playing_for_tests() else {
        return;
    };

    let dir = tempdir().unwrap();
    let wav_path = dir.path().join("loop_test.wav");
    let long_samples = vec![0.1_f32; 240];
    write_test_wav(&wav_path, &long_samples);
    let bytes = std::fs::read(&wav_path).unwrap();
    player.set_audio(bytes, 30.0);
    player.play_range(0.0, 1.0, false).unwrap();

    let (mut controller, source) = dummy_controller();
    controller.sample_view.wav.loaded_audio = Some(LoadedAudio {
        source_id: source.id.clone(),
        relative_path: PathBuf::from("loop_test.wav"),
        bytes: std::fs::read(&wav_path).unwrap(),
        duration_seconds: 30.0,
        sample_rate: 8,
        channels: 1,
    });
    controller.audio.player = Some(std::rc::Rc::new(std::cell::RefCell::new(player)));

    controller.ui.waveform.loop_enabled = false;
    if !controller.is_playing() {
        // Some environments may not keep the sink alive; skip in that case.
        return;
    }

    controller.toggle_loop();

    assert!(controller.ui.waveform.loop_enabled);
    assert!(
        controller
            .audio
            .player
            .as_ref()
            .unwrap()
            .borrow()
            .is_looping()
    );
}

#[test]
fn enabling_loop_while_playing_uses_full_selection() {
    let Some(mut player) = crate::audio::AudioPlayer::playing_for_tests() else {
        return;
    };

    let dir = tempdir().unwrap();
    let wav_path = dir.path().join("loop_selection_test.wav");
    let long_samples = vec![0.1_f32; 240];
    write_test_wav(&wav_path, &long_samples);
    let bytes = std::fs::read(&wav_path).unwrap();
    let duration = 30.0;
    player.set_audio(bytes, duration);
    player.play_range(0.0, 1.0, false).unwrap();

    let (mut controller, source) = dummy_controller();
    controller.sample_view.wav.loaded_audio = Some(LoadedAudio {
        source_id: source.id.clone(),
        relative_path: PathBuf::from("loop_selection_test.wav"),
        bytes: std::fs::read(&wav_path).unwrap(),
        duration_seconds: duration,
        sample_rate: 8,
        channels: 1,
    });
    controller.audio.player = Some(std::rc::Rc::new(std::cell::RefCell::new(player)));

    if !controller.is_playing() {
        return;
    }

    let selection = SelectionRange::new(0.2, 0.6);
    controller.selection_state.range.set_range(Some(selection));
    controller.apply_selection(Some(selection));

    controller.ui.waveform.loop_enabled = false;
    controller.toggle_loop();

    let (start, end) = controller
        .audio
        .player
        .as_ref()
        .unwrap()
        .borrow()
        .play_span()
        .expect("play span set");
    let expected_start = duration * selection.start();
    let expected_end = duration * selection.end();
    assert!((start - expected_start).abs() < 1e-4);
    assert!((end - expected_end).abs() < 1e-4);
}

#[test]
fn finish_selection_drag_keeps_playing_when_playhead_inside_loop() {
    let initial_selection = SelectionRange::new(0.1, 0.4);
    let Some(mut controller) = setup_looping_controller(initial_selection) else {
        return;
    };
    let updated_selection = SelectionRange::new(0.2, 0.6);
    controller.selection_state.range.set_range(Some(updated_selection));
    controller.apply_selection(Some(updated_selection));
    controller.ui.waveform.playhead.position = 0.3;

    controller.finish_selection_drag();

    assert!((controller.ui.waveform.playhead.position - 0.3).abs() < 1e-6);
}

#[test]
fn finish_selection_drag_restarts_when_playhead_outside_loop() {
    let initial_selection = SelectionRange::new(0.1, 0.4);
    let Some(mut controller) = setup_looping_controller(initial_selection) else {
        return;
    };
    let updated_selection = SelectionRange::new(0.6, 0.8);
    controller.selection_state.range.set_range(Some(updated_selection));
    controller.apply_selection(Some(updated_selection));
    controller.ui.waveform.playhead.position = 0.2;

    controller.finish_selection_drag();

    assert!((controller.ui.waveform.playhead.position - updated_selection.start()).abs() < 1e-6);
}
