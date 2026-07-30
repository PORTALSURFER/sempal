use super::super::decode_audio_outcome;
use crate::app::controller::AudioLoadIntent;
use crate::app::controller::playback::audio_loader::AudioVisualResult;
use crate::app::controller::state::audio::PendingAudio;
use crate::app::controller::test_support::{
    prepare_with_source_and_wav_entries, sample_entry, write_test_wav,
};
use crate::sample_sources::Rating;
use std::path::Path;
use std::sync::Arc;

#[test]
/// Selection handoff should queue one follow-loaded similarity refresh only after visuals commit.
fn audio_visual_message_queues_one_follow_loaded_similarity_refresh() {
    let (mut controller, source) =
        prepare_with_source_and_wav_entries(vec![sample_entry("match.wav", Rating::NEUTRAL)]);
    let relative_path = Path::new("match.wav");
    write_test_wav(&source.root.join(relative_path), &[0.0, 0.25, -0.25, 0.5]);
    controller.selection_state.ctx.selected_source = Some(source.id.clone());
    controller.sample_view.wav.selected_wav = Some(relative_path.to_path_buf());
    controller.ui.browser.search.sort = crate::app::state::SampleBrowserSort::Similarity;
    controller.ui.browser.search.similarity_sort_follow_loaded = true;
    controller.ui.waveform.loading = Some(relative_path.to_path_buf());

    controller.handle_audio_loaded(
        PendingAudio {
            request_id: 17,
            source_id: source.id.clone(),
            root: source.root.clone(),
            relative_path: relative_path.to_path_buf(),
            intent: AudioLoadIntent::Selection,
        },
        decode_audio_outcome(&controller, &source, relative_path),
    );
    assert!(controller.runtime.similarity.pending_loaded_query.is_none());

    let staged = controller
        .runtime
        .jobs
        .staged_audio_handoff()
        .expect("primary completion should stage the handoff");
    controller.handle_audio_visual_loaded(AudioVisualResult {
        request_id: staged.request_id,
        source_id: source.id.clone(),
        relative_path: relative_path.to_path_buf(),
        metadata: controller
            .current_file_metadata(&source, relative_path)
            .expect("metadata"),
        cache_token: staged.decoded.cache_token,
        transients: Arc::from(vec![0.2, 0.7]),
        image: None,
        projected_image: None,
        render_meta: None,
        stretched: false,
    });

    let pending = controller
        .runtime
        .similarity
        .pending_loaded_query
        .as_ref()
        .expect("follow-loaded similarity query should be queued");
    assert_eq!(pending.request_id, 1);
    assert_eq!(pending.source_id, source.id);
    assert_eq!(pending.relative_path, relative_path);
}
