use super::super::decode_audio_outcome;
use crate::app::controller::WaveformRenderMeta;
use crate::app::controller::jobs::JobMessage;
use crate::app::controller::playback::audio_loader::{AudioLoadResult, AudioVisualResult};
use crate::app::controller::state::audio::PendingAudio;
use crate::app::controller::test_support::{
    prepare_with_source_and_wav_entries, sample_entry, write_test_wav,
};
use crate::app::controller::{AudioLoadIntent, PendingPlayback};
use crate::app::state::WaveformView;
use crate::app_core::ui_projection::project_waveform_model;
use crate::sample_sources::{Rating, SourceId};
use std::path::Path;
use std::sync::Arc;

#[test]
/// Matching visual completion should publish the new sample and resolve pending playback together.
fn audio_visual_message_commits_staged_handoff_before_playback() {
    let (mut controller, source) =
        prepare_with_source_and_wav_entries(vec![sample_entry("match.wav", Rating::NEUTRAL)]);
    let relative_path = Path::new("match.wav");
    write_test_wav(&source.root.join(relative_path), &[0.0, 0.25, -0.25, 0.5]);
    controller.sample_view.wav.selected_wav = Some(relative_path.to_path_buf());
    controller.ui.waveform.loading = Some(relative_path.to_path_buf());
    controller
        .runtime
        .jobs
        .set_pending_playback(Some(PendingPlayback {
            source_id: source.id.clone(),
            relative_path: relative_path.to_path_buf(),
            looped: false,
            start_override: None,
            force_loaded_audio: false,
        }));

    let outcome = decode_audio_outcome(&controller, &source, relative_path);
    let cache_token = outcome.decoded.cache_token;
    controller.handle_audio_loaded(
        PendingAudio {
            request_id: 17,
            source_id: source.id.clone(),
            root: source.root.clone(),
            relative_path: relative_path.to_path_buf(),
            intent: AudioLoadIntent::Selection,
        },
        outcome,
    );
    assert!(controller.sample_view.wav.loaded_wav.is_none());
    assert!(!controller.is_playing());
    assert!(controller.runtime.jobs.pending_playback.is_some());

    controller.apply_background_job_message_for_tests(JobMessage::AudioLoaded(
        AudioLoadResult::Visual(AudioVisualResult {
            request_id: 17,
            source_id: source.id.clone(),
            relative_path: relative_path.to_path_buf(),
            metadata: controller
                .current_file_metadata(&source, relative_path)
                .expect("metadata"),
            cache_token,
            transients: Arc::from(vec![0.2, 0.7]),
            image: None,
            projected_image: None,
            render_meta: None,
            stretched: false,
        }),
    ));

    assert!(controller.runtime.jobs.staged_audio_handoff().is_none());
    assert_eq!(
        controller.sample_view.wav.loaded_wav.as_deref(),
        Some(relative_path)
    );
    assert_eq!(controller.ui.loaded_wav.as_deref(), Some(relative_path));
    assert!(controller.ui.waveform.loading.is_none());
    assert!(controller.runtime.jobs.pending_playback.is_none());
    assert_eq!(
        controller
            .sample_view
            .wav
            .loaded_audio
            .as_ref()
            .map(|audio| audio.relative_path.as_path()),
        Some(relative_path)
    );
}

#[test]
/// Initial async visuals are rendered for the full waveform and must be rerendered for stale views.
fn audio_visual_message_rerenders_stale_view_before_projecting_waveform_image() {
    let (mut controller, source) =
        prepare_with_source_and_wav_entries(vec![sample_entry("match.wav", Rating::NEUTRAL)]);
    let relative_path = Path::new("match.wav");
    write_test_wav(&source.root.join(relative_path), &[0.0, 0.25, -0.25, 0.5]);
    controller.sample_view.wav.selected_wav = Some(relative_path.to_path_buf());
    controller.ui.waveform.loading = Some(relative_path.to_path_buf());
    controller.ui.waveform.view = WaveformView {
        start: 0.25,
        end: 0.50,
    };
    controller.ui.waveform.selection = Some(crate::selection::SelectionRange::new(0.30, 0.40));
    controller.ui.waveform.cursor = Some(0.38);

    let outcome = decode_audio_outcome(&controller, &source, relative_path);
    let cache_token = outcome.decoded.cache_token;
    let samples_len = outcome.decoded.frame_count();
    controller.handle_audio_loaded(
        PendingAudio {
            request_id: 17,
            source_id: source.id.clone(),
            root: source.root.clone(),
            relative_path: relative_path.to_path_buf(),
            intent: AudioLoadIntent::Selection,
        },
        outcome,
    );

    controller.handle_audio_visual_loaded(AudioVisualResult {
        request_id: 17,
        source_id: source.id.clone(),
        relative_path: relative_path.to_path_buf(),
        metadata: controller
            .current_file_metadata(&source, relative_path)
            .expect("metadata"),
        cache_token,
        transients: Arc::from(vec![0.2, 0.7]),
        image: Some(crate::waveform::WaveformImage {
            size: [2, 1],
            pixels: vec![
                crate::waveform::WaveformRgba::from_rgba_unmultiplied(10, 20, 30, 255),
                crate::waveform::WaveformRgba::from_rgba_unmultiplied(11, 21, 31, 255),
            ],
        }),
        projected_image: None,
        render_meta: Some(WaveformRenderMeta {
            view_start: 0.0,
            view_end: 1.0,
            size: [2, 1],
            samples_len,
            texture_width: 2,
            channel_view: controller.ui.waveform.channel_view,
            channels: 1,
            edit_fade: None,
            transient_visual_token: Some(cache_token),
        }),
        stretched: false,
    });

    assert_eq!(
        controller.ui.waveform.view,
        WaveformView {
            start: 0.25,
            end: 0.50
        }
    );
    assert!(controller.ui.waveform.selection.is_some());
    assert_eq!(controller.ui.waveform.cursor, Some(0.38));
    assert!(
        controller
            .waveform_render_meta()
            .is_some_and(|meta| meta.matches_view_identity(controller.ui.waveform.view))
    );
    let projected = project_waveform_model(&mut controller);
    assert!(projected.waveform_image_signature.is_some());
    assert!(
        projected.waveform_image.is_some(),
        "visual should be rerendered for the current stale view before projection"
    );
}

#[test]
/// A visual completion for the old selection must not publish or finalize its staged handoff.
fn audio_visual_message_ignores_completion_for_old_selection() {
    let (mut controller, source) = prepare_with_source_and_wav_entries(vec![
        sample_entry("old.wav", Rating::NEUTRAL),
        sample_entry("new.wav", Rating::NEUTRAL),
    ]);
    let old_path = Path::new("old.wav");
    let new_path = Path::new("new.wav");
    write_test_wav(&source.root.join(old_path), &[0.0, 0.25, -0.25, 0.5]);
    controller.selection_state.ctx.selected_source = Some(source.id.clone());
    controller.sample_view.wav.selected_wav = Some(old_path.to_path_buf());

    let outcome = decode_audio_outcome(&controller, &source, old_path);
    let cache_token = outcome.decoded.cache_token;
    controller.handle_audio_loaded(
        PendingAudio {
            request_id: 17,
            source_id: source.id.clone(),
            root: source.root.clone(),
            relative_path: old_path.to_path_buf(),
            intent: AudioLoadIntent::Selection,
        },
        outcome,
    );
    controller.selection_state.ctx.selected_source = Some(SourceId::from_string("new-source"));
    controller.sample_view.wav.selected_wav = Some(new_path.to_path_buf());

    controller.handle_audio_visual_loaded(AudioVisualResult {
        request_id: 17,
        source_id: source.id.clone(),
        relative_path: old_path.to_path_buf(),
        metadata: controller
            .current_file_metadata(&source, old_path)
            .expect("metadata"),
        cache_token,
        transients: Arc::from(vec![0.2, 0.7]),
        image: None,
        projected_image: None,
        render_meta: None,
        stretched: false,
    });
    controller.finalize_staged_audio_handoff(cache_token);

    let staged = controller
        .runtime
        .jobs
        .staged_audio_handoff()
        .expect("stale visual must leave the old handoff untouched");
    assert_eq!(staged.relative_path, old_path);
    assert!(controller.sample_view.wav.loaded_wav.is_none());
    assert!(controller.sample_view.wav.loaded_audio.is_none());
    assert!(controller.sample_view.waveform.decoded.is_none());
    assert!(controller.ui.waveform.image.is_none());
    assert!(controller.ui.waveform.transients.is_empty());
}

#[test]
/// Explicit compare-anchor playback may load a target that is not browser-selected.
fn compare_anchor_visual_completion_preserves_explicit_playback_target() {
    let (mut controller, source) = prepare_with_source_and_wav_entries(vec![
        sample_entry("anchor.wav", Rating::NEUTRAL),
        sample_entry("current.wav", Rating::NEUTRAL),
    ]);
    let anchor_path = Path::new("anchor.wav");
    let current_path = Path::new("current.wav");
    write_test_wav(&source.root.join(anchor_path), &[0.0, 0.25, -0.25, 0.5]);
    controller.selection_state.ctx.selected_source = Some(source.id.clone());
    controller.sample_view.wav.selected_wav = Some(current_path.to_path_buf());
    controller
        .runtime
        .jobs
        .set_pending_playback(Some(PendingPlayback {
            source_id: source.id.clone(),
            relative_path: anchor_path.to_path_buf(),
            looped: false,
            start_override: None,
            force_loaded_audio: true,
        }));

    let outcome = decode_audio_outcome(&controller, &source, anchor_path);
    let cache_token = outcome.decoded.cache_token;
    controller.handle_audio_loaded(
        PendingAudio {
            request_id: 17,
            source_id: source.id.clone(),
            root: source.root.clone(),
            relative_path: anchor_path.to_path_buf(),
            intent: AudioLoadIntent::Selection,
        },
        outcome,
    );
    controller.handle_audio_visual_loaded(AudioVisualResult {
        request_id: 17,
        source_id: source.id.clone(),
        relative_path: anchor_path.to_path_buf(),
        metadata: controller
            .current_file_metadata(&source, anchor_path)
            .expect("metadata"),
        cache_token,
        transients: Arc::from(vec![0.2, 0.7]),
        image: None,
        projected_image: None,
        render_meta: None,
        stretched: false,
    });

    assert_eq!(
        controller.sample_view.wav.loaded_wav.as_deref(),
        Some(anchor_path)
    );
    assert_eq!(controller.ui.loaded_wav.as_deref(), Some(anchor_path));
    assert_eq!(
        controller.sample_view.wav.selected_wav.as_deref(),
        Some(current_path)
    );
    assert!(controller.runtime.jobs.pending_playback().is_none());
}
