use super::super::decode_audio_outcome;
use crate::app::controller::AudioLoadIntent;
use crate::app::controller::jobs::JobMessage;
use crate::app::controller::playback::audio_loader::AudioLoadResult;
use crate::app::controller::state::audio::PendingAudio;
use crate::app::controller::test_support::{
    prepare_with_source_and_wav_entries, sample_entry, write_test_wav,
};
use crate::sample_sources::{Rating, SourceId};
use std::path::Path;

#[test]
/// Primary audio completions should ignore stale requests and keep loading active until visuals arrive.
fn audio_primary_message_ignores_stale_completion_then_applies_matching_result() {
    let (mut controller, source) =
        prepare_with_source_and_wav_entries(vec![sample_entry("match.wav", Rating::NEUTRAL)]);
    let relative_path = Path::new("match.wav");
    write_test_wav(&source.root.join(relative_path), &[0.0, 0.25, -0.25, 0.5]);
    controller.sample_view.wav.selected_wav = Some(relative_path.to_path_buf());
    controller.ui.waveform.loading = Some(relative_path.to_path_buf());
    controller
        .runtime
        .jobs
        .set_pending_audio(Some(PendingAudio {
            request_id: 17,
            source_id: source.id.clone(),
            root: source.root.clone(),
            relative_path: relative_path.to_path_buf(),
            intent: AudioLoadIntent::Selection,
        }));

    controller.apply_background_job_message_for_tests(JobMessage::AudioLoaded(
        AudioLoadResult::Primary {
            request_id: 18,
            source_id: source.id.clone(),
            relative_path: relative_path.to_path_buf(),
            result: Ok(decode_audio_outcome(&controller, &source, relative_path)),
        },
    ));

    let pending = controller.runtime.jobs.pending_audio();
    assert!(pending.is_some(), "stale completion should stay pending");
    assert_eq!(
        controller.ui.waveform.loading.as_deref(),
        Some(relative_path)
    );
    assert!(controller.sample_view.wav.loaded_audio.is_none());
    assert!(controller.runtime.jobs.staged_audio_handoff().is_none());

    controller.apply_background_job_message_for_tests(JobMessage::AudioLoaded(
        AudioLoadResult::Primary {
            request_id: 17,
            source_id: source.id.clone(),
            relative_path: relative_path.to_path_buf(),
            result: Ok(decode_audio_outcome(&controller, &source, relative_path)),
        },
    ));

    assert!(controller.runtime.jobs.pending_audio().is_none());
    assert_eq!(
        controller.ui.waveform.loading.as_deref(),
        Some(relative_path)
    );
    assert!(controller.sample_view.wav.loaded_wav.is_none());
    let staged = controller
        .runtime
        .jobs
        .staged_audio_handoff()
        .expect("matching primary completion should stage the handoff");
    assert_eq!(staged.source_id, source.id);
    assert_eq!(staged.relative_path, relative_path);
}

#[test]
/// A primary completion for the old selection must not stage audio after focus moves.
fn audio_primary_message_ignores_completion_for_old_selection() {
    let (mut controller, source) = prepare_with_source_and_wav_entries(vec![
        sample_entry("old.wav", Rating::NEUTRAL),
        sample_entry("new.wav", Rating::NEUTRAL),
    ]);
    let old_path = Path::new("old.wav");
    let new_path = Path::new("new.wav");
    write_test_wav(&source.root.join(old_path), &[0.0, 0.25, -0.25, 0.5]);
    controller.selection_state.ctx.selected_source = Some(source.id.clone());
    controller.sample_view.wav.selected_wav = Some(old_path.to_path_buf());
    controller
        .runtime
        .jobs
        .set_pending_audio(Some(PendingAudio {
            request_id: 17,
            source_id: source.id.clone(),
            root: source.root.clone(),
            relative_path: old_path.to_path_buf(),
            intent: AudioLoadIntent::Selection,
        }));

    controller.selection_state.ctx.selected_source = Some(SourceId::from_string("new-source"));
    controller.sample_view.wav.selected_wav = Some(new_path.to_path_buf());
    controller.apply_background_job_message_for_tests(JobMessage::AudioLoaded(
        AudioLoadResult::Primary {
            request_id: 17,
            source_id: source.id.clone(),
            relative_path: old_path.to_path_buf(),
            result: Ok(decode_audio_outcome(&controller, &source, old_path)),
        },
    ));

    assert!(controller.runtime.jobs.pending_audio().is_some());
    assert!(controller.runtime.jobs.staged_audio_handoff().is_none());
    assert!(controller.sample_view.wav.loaded_wav.is_none());
    assert!(controller.sample_view.wav.loaded_audio.is_none());
}
