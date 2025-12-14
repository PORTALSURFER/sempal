use super::super::test_support::{dummy_controller, write_test_wav};
use super::super::*;
use std::path::PathBuf;
use tempfile::tempdir;

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
    controller.wav_selection.loaded_audio = Some(LoadedAudio {
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
    assert!(controller
        .audio
        .player
        .as_ref()
        .unwrap()
        .borrow()
        .is_looping());
}
