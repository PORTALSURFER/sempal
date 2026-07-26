use super::*;
use radiant::runtime::Command;
use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
    sync::Arc,
    time::SystemTime,
};

fn after_messages(
    state: &mut crate::native_app::test_support::state::NativeAppState,
    command: Command<GuiMessage>,
) -> Vec<GuiMessage> {
    let timer_count = match &command {
        Command::Timer(..) => 1,
        Command::Batch(commands) => commands
            .iter()
            .filter(|command| matches!(command, Command::Timer(..)))
            .count(),
        _ => 0,
    };
    let messages = state.take_scheduled_timer_messages();
    assert_eq!(messages.len(), timer_count);
    messages
}

fn starmap_state_with_wav_files(
    file_count: usize,
) -> crate::native_app::test_support::state::NativeAppState {
    let source_root = tempfile::tempdir().expect("source root");
    let source_path = source_root.path().to_path_buf();
    for index in 0..file_count {
        fs::write(source_path.join(format!("sample-{index:03}.wav")), [])
            .expect("write wav placeholder");
    }
    let mut state = crate::native_app::test_support::state::NativeAppStateFixture::default()
        .with_folder_browser(
            crate::native_app::test_support::state::FolderBrowserState::from_sample_sources(&[
                wavecrate::sample_sources::SampleSource::new(source_path),
            ]),
        )
        .build();
    state.ui.chrome.sample_browser_display = SampleBrowserDisplayMode::Map;
    crate::native_app::test_support::sample_browser::complete_starmap_layout_for_selected_source(
        &mut state,
    );
    crate::native_app::test_support::sample_browser::prepare_sample_browser_view(&mut state);
    state
}

fn list_state_with_wav_files(
    file_count: usize,
    selected_index: usize,
) -> (
    crate::native_app::test_support::state::NativeAppState,
    Vec<String>,
) {
    let source_root = tempfile::tempdir().expect("source root");
    let source_path = source_root.path().to_path_buf();
    let mut paths = Vec::new();
    for index in 0..file_count {
        let path = source_path.join(format!("sample-{index:03}.wav"));
        fs::write(&path, []).expect("write wav placeholder");
        paths.push(path.display().to_string());
    }
    let mut state = crate::native_app::test_support::state::NativeAppStateFixture::default()
        .with_folder_browser(
            crate::native_app::test_support::state::FolderBrowserState::from_sample_sources(&[
                wavecrate::sample_sources::SampleSource::new(source_path),
            ]),
        )
        .build();
    state.ui.chrome.sample_browser_display = SampleBrowserDisplayMode::List;
    state
        .library
        .folder_browser
        .select_file(paths[selected_index].clone());
    crate::native_app::test_support::sample_browser::prepare_sample_browser_view(&mut state);
    (state, paths)
}

fn unprepared_list_state_with_wav_files(
    file_count: usize,
    selected_index: usize,
) -> (
    crate::native_app::test_support::state::NativeAppState,
    Vec<String>,
) {
    let source_root = tempfile::tempdir().expect("source root");
    let source_path = source_root.path().to_path_buf();
    let mut paths = Vec::new();
    for index in 0..file_count {
        let path = source_path.join(format!("sample-{index:03}.wav"));
        fs::write(&path, []).expect("write wav placeholder");
        paths.push(path.display().to_string());
    }
    let mut state = crate::native_app::test_support::state::NativeAppStateFixture::default()
        .with_folder_browser(
            crate::native_app::test_support::state::FolderBrowserState::from_sample_sources(&[
                wavecrate::sample_sources::SampleSource::new(source_path),
            ]),
        )
        .build();
    state.ui.chrome.sample_browser_display = SampleBrowserDisplayMode::List;
    state
        .library
        .folder_browser
        .select_file(paths[selected_index].clone());
    (state, paths)
}

fn preview_clip_with_gain(normalized_gain: f32) -> PreviewAuditionClip {
    PreviewAuditionClip {
        path: PathBuf::from("/tmp/wavecrate-preview-gain.wav"),
        source_len: 0,
        source_modified: Some(SystemTime::UNIX_EPOCH),
        samples: Arc::from([0.25_f32, -0.5]),
        sample_rate: 44_100,
        channels: 1,
        frames: 2,
        normalized_gain,
    }
}

fn write_sparse_wav_i16(path: &Path, channels: u16, frames: usize) {
    let spec = hound::WavSpec {
        channels,
        sample_rate: 48_000,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = hound::WavWriter::create(path, spec).expect("create wav");
    for frame in 0..frames {
        for channel in 0..channels {
            let sample = ((frame + usize::from(channel)) % 256) as i16;
            writer.write_sample(sample).expect("write sample");
        }
    }
    writer.finalize().expect("finalize wav");
}

#[test]
fn starmap_drag_fast_audition_prefers_original_wav_before_persistent_or_preview_cache() {
    assert_eq!(
        fast_audition_probe_order(FastAuditionOptions::starmap_drag()),
        [
            FastAuditionProbe::FileBackedWav,
            FastAuditionProbe::PersistedCache,
            FastAuditionProbe::PreviewCache,
            FastAuditionProbe::PreviewDecode,
        ]
    );
}

#[test]
fn instant_navigation_fast_audition_prefers_original_wav_before_persistent_or_preview_cache() {
    assert_eq!(
        fast_audition_probe_order(FastAuditionOptions::instant_navigation()),
        [
            FastAuditionProbe::FileBackedWav,
            FastAuditionProbe::PersistedCache,
            FastAuditionProbe::PreviewCache,
            FastAuditionProbe::PreviewDecode,
        ]
    );
}

#[test]
fn preview_decode_completion_retries_the_original_wav_before_using_the_preview_clip() {
    let options = FastAuditionOptions::preview_decode_completion("test", false);

    assert!(options.allow_file_backed_source);
    assert_eq!(
        fast_audition_probe_order(options),
        [
            FastAuditionProbe::FileBackedWav,
            FastAuditionProbe::PersistedCache,
            FastAuditionProbe::PreviewCache,
            FastAuditionProbe::PreviewDecode,
        ]
    );
}

#[test]
fn instant_navigation_fast_audition_avoids_ui_thread_sidecar_lookup() {
    assert!(
        !FastAuditionOptions::instant_navigation().allow_sidecar_lookup,
        "list and keyboard navigation should not read playback descriptor sidecars on the UI path"
    );
}

#[test]
fn hot_fast_audition_options_submit_unprobed_file_backed_sources() {
    assert!(
        FastAuditionOptions::instant_navigation().allow_file_backed_source,
        "list and keyboard navigation should enqueue the original WAV path"
    );
    assert!(
        FastAuditionOptions::starmap_drag().allow_file_backed_source,
        "starmap drag playback should enqueue the original WAV path"
    );
}

#[test]
fn hot_fast_audition_options_crossfade_previous_runtime_source() {
    assert_eq!(
        FastAuditionOptions::instant_navigation().replace_policy,
        PlaybackRuntimeReplacePolicy::FadeOutPrevious,
        "list and keyboard navigation should de-click the previous source instead of severing it"
    );
    assert_eq!(
        FastAuditionOptions::starmap_drag().replace_policy,
        PlaybackRuntimeReplacePolicy::FadeOutPrevious,
        "starmap drag playback should de-click each rapid source replacement"
    );
}

#[test]
fn starmap_drag_queues_preview_decode_only_while_audio_runtime_is_starting() {
    let source_root = tempfile::tempdir().expect("source root");
    let sample = source_root.path().join("long.wav");
    write_sparse_wav_i16(&sample, 1, 700);
    let sample_id = sample.display().to_string();
    let mut state = crate::native_app::test_support::state::NativeAppStateFixture::default()
        .with_folder_browser(
            crate::native_app::test_support::state::FolderBrowserState::from_sample_sources(&[
                wavecrate::sample_sources::SampleSource::new(source_root.path().to_path_buf()),
            ]),
        )
        .build();
    state.ui.chrome.sample_browser_display = SampleBrowserDisplayMode::Map;
    state.ui.chrome.starmap_audition_drag =
        Some(crate::native_app::app::StarmapAuditionDragState {
            last_hit_file_id: Some(sample_id.clone()),
            last_position: ui::Point::new(0.0, 0.0),
            modifiers: Default::default(),
        });
    state.ui.chrome.starmap_audition_queue.active_file_id = Some(sample_id.clone());
    let mut context = ui::UiUpdateContext::default();

    assert!(
        crate::native_app::waveform::should_use_file_backed_wav_decode(&sample),
        "all WAV files should be eligible for file-backed playback"
    );
    let outcome = state.start_fast_path_audition(
        sample_id.as_str(),
        &mut context,
        Instant::now(),
        FastAuditionOptions::starmap_drag(),
    );

    assert_eq!(
        outcome,
        InstantAuditionOutcome::AudioPending,
        "without an initialized audio runtime the fallback decode should remain pending"
    );
    assert!(
        state.background.preview_audition_task.active().is_some(),
        "preview-head decode should be tracked as the active cancellable task"
    );
    assert!(
        state.audio.sample_playback_session.is_none(),
        "the WAV path must not synchronously open the source on the UI path"
    );
}

#[test]
fn preview_clip_playback_uses_precomputed_normalized_gain() {
    let clip = preview_clip_with_gain(2.5);

    assert_eq!(preview_clip_playback_gain(&clip, true), 2.5);
    assert_eq!(preview_clip_playback_gain(&clip, false), 1.0);
    assert_eq!(
        preview_clip_playback_gain(&preview_clip_with_gain(f32::NAN), true),
        1.0
    );
    assert_eq!(
        preview_clip_playback_gain(&preview_clip_with_gain(0.0), true),
        1.0
    );
}

#[test]
fn list_preview_warm_stops_after_selected_neighborhood_budget() {
    let selected_index = 48;
    let (mut state, paths) = list_state_with_wav_files(96, selected_index);
    let mut warmed = HashSet::new();

    for _ in 0..(PREVIEW_AUDITION_LIST_VIEW_BUDGET / PREVIEW_AUDITION_WARM_BATCH) {
        let plan = state.preview_audition_warm_list_candidates();
        assert_eq!(plan.paths.len(), PREVIEW_AUDITION_WARM_BATCH);
        if warmed.is_empty() {
            assert_eq!(plan.inspected_count, PREVIEW_AUDITION_LIST_VIEW_BUDGET);
            assert_eq!(plan.candidate_count, PREVIEW_AUDITION_LIST_VIEW_BUDGET);
            assert_eq!(plan.eligible_count, PREVIEW_AUDITION_LIST_VIEW_BUDGET);
        }
        warmed.extend(plan.paths.iter().cloned());
        reserve_preview_warm_plan(&mut state, &plan);
    }

    assert_eq!(warmed.len(), PREVIEW_AUDITION_LIST_VIEW_BUDGET);
    assert!(
        warmed.contains(&paths[selected_index]),
        "list preview warming should include the selected row"
    );
    assert!(
        !warmed.contains(&paths[0]),
        "list preview warming should not crawl back to the start of a large source"
    );
    assert!(
        !warmed.contains(&paths[paths.len() - 1]),
        "list preview warming should not crawl to the end of a large source"
    );

    let exhausted_plan = state.preview_audition_warm_list_candidates();

    assert_eq!(
        exhausted_plan.paths.len(),
        0,
        "list preview warming should stop after the selected-row neighborhood budget"
    );
    assert_eq!(exhausted_plan.inspected_count, 0);
    assert_eq!(exhausted_plan.candidate_count, 0);
    assert_eq!(exhausted_plan.eligible_count, 0);
}

#[test]
fn list_preview_warm_skips_until_visible_window_is_prepared() {
    let (mut state, _paths) = unprepared_list_state_with_wav_files(256, 128);
    let cache_len_before = state
        .library
        .folder_browser
        .selected_audio_projection_cache_len_for_tests();

    let plan = state.preview_audition_warm_list_candidates();

    assert_eq!(
        plan.paths.len(),
        0,
        "preview warming should not build the list projection before the visible list is prepared"
    );
    assert_eq!(
        state
            .library
            .folder_browser
            .selected_audio_projection_cache_len_for_tests(),
        cache_len_before,
        "preview warming must stay opportunistic instead of filling the projection cache on the UI frame"
    );
}

#[test]
fn list_preview_warm_exhausts_sparse_view_after_attempted_candidates() {
    let (mut state, _) = list_state_with_wav_files(PREVIEW_AUDITION_WARM_BATCH / 2, 0);
    let first_plan = state.preview_audition_warm_list_candidates();
    assert!(
        !first_plan.paths.is_empty(),
        "sparse list fixture should still have warmable candidates"
    );
    assert!(
        first_plan.paths.len() < PREVIEW_AUDITION_LIST_VIEW_BUDGET,
        "fixture must leave budget remaining after the first sparse warm"
    );
    reserve_preview_warm_plan(&mut state, &first_plan);
    state.waveform.cache.finish_preview_audition_warm_schedule(
        &first_plan.paths,
        &first_plan.paths,
        &[],
    );

    let exhausted_plan = state.preview_audition_warm_list_candidates();

    assert_eq!(exhausted_plan.paths.len(), 0);
    assert_eq!(
        exhausted_plan.list_remaining_budget,
        Some(0),
        "already-attempted sparse list views should not be re-planned forever"
    );

    let repeated_plan = state.preview_audition_warm_list_candidates();

    assert_eq!(
        repeated_plan.inspected_count, 0,
        "exhausted list warm views should skip candidate inspection"
    );
}

#[test]
fn list_preview_warm_cancel_releases_view_budget_for_retry() {
    let (mut state, _) = list_state_with_wav_files(PREVIEW_AUDITION_LIST_VIEW_BUDGET + 8, 0);
    let first_plan = state.preview_audition_warm_list_candidates();
    assert_eq!(first_plan.paths.len(), PREVIEW_AUDITION_WARM_BATCH);
    reserve_preview_warm_plan(&mut state, &first_plan);

    state.waveform.cache.cancel_preview_audition_warm_schedule();
    let retry_plan = state.preview_audition_warm_list_candidates();

    assert_eq!(
        retry_plan.list_remaining_budget,
        Some(PREVIEW_AUDITION_LIST_VIEW_BUDGET),
        "cancelled list warm work should not consume the finite viewport budget"
    );
    assert_eq!(
        retry_plan.paths, first_plan.paths,
        "cancelled list warm work should retry the same nearest candidates once idle"
    );
}

#[test]
fn legacy_preview_preference_keeps_preview_decode_before_file_backed_wav() {
    let options = FastAuditionOptions {
        origin: "test",
        record_history: false,
        allow_sidecar_lookup: false,
        queue_preview_decode: true,
        prefer_preview_decode: true,
        allow_file_backed_source: false,
        replace_policy: PlaybackRuntimeReplacePolicy::ClearPrevious,
    };

    assert_eq!(
        fast_audition_probe_order(options),
        [
            FastAuditionProbe::PreviewCache,
            FastAuditionProbe::PersistedCache,
            FastAuditionProbe::PreviewDecode,
            FastAuditionProbe::FileBackedWav,
        ]
    );
}

#[test]
fn preview_decode_completion_uses_active_starmap_target_during_drag() {
    let mut state = starmap_state_with_wav_files(2);
    let files = state
        .library
        .folder_browser
        .selected_source_audio_files()
        .into_iter()
        .map(|file| file.id.clone())
        .collect::<Vec<_>>();
    let active = files[0].clone();
    let selected = files[1].clone();
    state.ui.chrome.starmap_audition_drag =
        Some(crate::native_app::app::StarmapAuditionDragState {
            last_hit_file_id: Some(active.clone()),
            last_position: ui::Point::new(0.0, 0.0),
            modifiers: Default::default(),
        });
    state.ui.chrome.starmap_audition_queue.active_file_id = Some(active.clone());
    state.library.folder_browser.select_file(selected);

    assert!(
        state.preview_audition_decode_matches_current_target(active.as_str()),
        "active starmap drag target should be allowed to finish even if browser selection has already moved"
    );
}

#[test]
fn preview_decode_completion_rejects_replaced_starmap_target() {
    let mut state = starmap_state_with_wav_files(2);
    let files = state
        .library
        .folder_browser
        .selected_source_audio_files()
        .into_iter()
        .map(|file| file.id.clone())
        .collect::<Vec<_>>();
    let stale = files[0].clone();
    let active = files[1].clone();
    state.ui.chrome.starmap_audition_drag =
        Some(crate::native_app::app::StarmapAuditionDragState {
            last_hit_file_id: Some(active.clone()),
            last_position: ui::Point::new(0.0, 0.0),
            modifiers: Default::default(),
        });
    state.ui.chrome.starmap_audition_queue.active_file_id = Some(active);
    state.library.folder_browser.select_file(stale.clone());

    assert!(
        !state.preview_audition_decode_matches_current_target(stale.as_str()),
        "stale starmap preview decode must not play just because the browser selection still points at it"
    );
}

fn reserve_preview_warm_plan(
    state: &mut crate::native_app::test_support::state::NativeAppState,
    plan: &PreviewAuditionWarmPlan,
) {
    state
        .waveform
        .cache
        .mark_preview_audition_warm_scheduled(&plan.paths);
    if let Some(signature) = plan.starmap_signature {
        state
            .waveform
            .cache
            .reserve_starmap_preview_warm_batch(signature, plan.paths.len());
    }
    if let Some(signature) = plan.list_signature {
        state
            .waveform
            .cache
            .reserve_list_preview_warm_batch(signature, plan.paths.len());
    }
}

#[test]
fn starmap_preview_warm_stops_after_view_budget_until_view_changes() {
    let mut state = starmap_state_with_wav_files(PREVIEW_AUDITION_STARMAP_VIEW_BUDGET + 48);
    let mut warmed = HashSet::new();

    for _ in 0..(PREVIEW_AUDITION_STARMAP_VIEW_BUDGET / PREVIEW_AUDITION_WARM_BATCH) {
        let plan = state.preview_audition_warm_starmap_candidates();
        assert_eq!(plan.paths.len(), PREVIEW_AUDITION_WARM_BATCH);
        if warmed.is_empty() {
            assert_eq!(
                plan.starmap_remaining_budget,
                Some(PREVIEW_AUDITION_STARMAP_VIEW_BUDGET)
            );
            assert!(plan.eligible_count >= PREVIEW_AUDITION_WARM_BATCH);
        }
        warmed.extend(plan.paths.iter().cloned());
        reserve_preview_warm_plan(&mut state, &plan);
    }
    assert_eq!(warmed.len(), PREVIEW_AUDITION_STARMAP_VIEW_BUDGET);

    let exhausted_plan = state.preview_audition_warm_starmap_candidates();

    assert_eq!(
        exhausted_plan.paths.len(),
        0,
        "starmap preview warming should not keep crawling a dense unchanged map forever"
    );
    assert_eq!(exhausted_plan.starmap_remaining_budget, Some(0));

    state.ui.chrome.starmap_viewport.center_x += 0.25;
    let changed_view_plan = state.preview_audition_warm_starmap_candidates();

    assert_eq!(
        changed_view_plan.paths.len(),
        PREVIEW_AUDITION_WARM_BATCH,
        "a meaningful starmap viewport change should open a fresh warm budget"
    );
    assert!(
        changed_view_plan
            .paths
            .iter()
            .all(|path| !warmed.contains(path))
    );
}

#[test]
fn starmap_preview_warm_budget_survives_selection_changes() {
    let mut state = starmap_state_with_wav_files(PREVIEW_AUDITION_STARMAP_VIEW_BUDGET + 48);
    let files = state
        .library
        .folder_browser
        .selected_source_audio_files()
        .into_iter()
        .map(|file| file.id.clone())
        .collect::<Vec<_>>();

    for _ in 0..(PREVIEW_AUDITION_STARMAP_VIEW_BUDGET / PREVIEW_AUDITION_WARM_BATCH) {
        let plan = state.preview_audition_warm_starmap_candidates();
        assert_eq!(plan.paths.len(), PREVIEW_AUDITION_WARM_BATCH);
        reserve_preview_warm_plan(&mut state, &plan);
    }

    for selected in files.iter().take(6) {
        state.library.folder_browser.select_file(selected.clone());
        let plan = state.preview_audition_warm_starmap_candidates();
        assert_eq!(
            plan.paths.len(),
            0,
            "starmap audition selection changes must not reset the finite viewport warm budget"
        );
    }
}

#[test]
fn starmap_preview_warm_exhausts_sparse_view_after_attempted_candidates() {
    let mut state = starmap_state_with_wav_files(PREVIEW_AUDITION_WARM_BATCH / 2);
    let first_plan = state.preview_audition_warm_starmap_candidates();
    assert!(
        !first_plan.paths.is_empty(),
        "sparse starmap fixture should still have warmable candidates"
    );
    assert!(
        first_plan.paths.len() < PREVIEW_AUDITION_STARMAP_VIEW_BUDGET,
        "fixture must leave budget remaining after the first sparse warm"
    );
    reserve_preview_warm_plan(&mut state, &first_plan);
    state.waveform.cache.finish_preview_audition_warm_schedule(
        &first_plan.paths,
        &first_plan.paths,
        &[],
    );

    let exhausted_plan = state.preview_audition_warm_starmap_candidates();

    assert_eq!(exhausted_plan.paths.len(), 0);
    assert_eq!(
        exhausted_plan.starmap_remaining_budget,
        Some(0),
        "already-attempted sparse starmap views should not be re-planned forever"
    );

    let repeated_plan = state.preview_audition_warm_starmap_candidates();

    assert_eq!(
        repeated_plan.inspected_count, 0,
        "exhausted starmap warm views should skip candidate inspection"
    );
}

#[test]
fn starmap_preview_warm_cancel_releases_view_budget_for_retry() {
    let mut state = starmap_state_with_wav_files(PREVIEW_AUDITION_STARMAP_VIEW_BUDGET + 48);
    let first_plan = state.preview_audition_warm_starmap_candidates();
    assert_eq!(first_plan.paths.len(), PREVIEW_AUDITION_WARM_BATCH);
    reserve_preview_warm_plan(&mut state, &first_plan);

    state.waveform.cache.cancel_preview_audition_warm_schedule();
    let retry_plan = state.preview_audition_warm_starmap_candidates();

    assert_eq!(
        retry_plan.starmap_remaining_budget,
        Some(PREVIEW_AUDITION_STARMAP_VIEW_BUDGET),
        "cancelled starmap warm work should not consume the finite viewport budget"
    );
    assert_eq!(
        retry_plan.paths, first_plan.paths,
        "cancelled starmap warm work should retry the same viewport candidates once idle"
    );
}

#[test]
fn starmap_preview_warm_partial_finish_consumes_only_attempted_budget() {
    let mut state = starmap_state_with_wav_files(PREVIEW_AUDITION_STARMAP_VIEW_BUDGET + 48);
    let first_plan = state.preview_audition_warm_starmap_candidates();
    assert_eq!(first_plan.paths.len(), PREVIEW_AUDITION_WARM_BATCH);
    let attempted = first_plan.paths[0].clone();
    reserve_preview_warm_plan(&mut state, &first_plan);
    state.waveform.cache.finish_preview_audition_warm_schedule(
        &first_plan.paths,
        std::slice::from_ref(&attempted),
        &[],
    );

    let next_plan = state.preview_audition_warm_starmap_candidates();

    assert_eq!(
        next_plan.starmap_remaining_budget,
        Some(PREVIEW_AUDITION_STARMAP_VIEW_BUDGET - 1),
        "unattempted starmap warm tails should not be charged against the viewport budget"
    );
    assert!(
        next_plan.paths.iter().any(|path| path != &attempted),
        "partial completion should leave later candidates available for warming"
    );
}

#[test]
fn starmap_drag_instant_audition_schedules_stable_target_promotion() {
    let mut state =
        crate::native_app::test_support::state::NativeAppStateFixture::default().build();
    let mut context = ui::UiUpdateContext::default();

    state.maybe_schedule_starmap_audition_promotion(
        "/tmp/starmap-target.wav",
        "starmap_drag",
        &mut context,
    );
    let delayed = after_messages(&mut state, context.into_command());

    assert!(delayed.iter().any(|message| matches!(
        message,
        GuiMessage::PromoteStarmapAudition {
            path,
            ..
        } if path == "/tmp/starmap-target.wav"
    )));
}

#[test]
fn non_starmap_instant_audition_does_not_schedule_stable_target_promotion() {
    let mut state =
        crate::native_app::test_support::state::NativeAppStateFixture::default().build();
    let mut context = ui::UiUpdateContext::default();

    state.maybe_schedule_starmap_audition_promotion(
        "/tmp/browser-target.wav",
        "instant_audition",
        &mut context,
    );

    assert!(after_messages(&mut state, context.into_command()).is_empty());
}
