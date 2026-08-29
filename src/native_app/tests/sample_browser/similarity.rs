use super::*;
use std::time::{Duration, Instant};
use wavecrate_analysis::aspects::SimilarityAspect;

const SIMILARITY_TEST_SOURCE_ID: &str = "native-similarity-test";

#[test]
fn toggling_similarity_anchor_hands_keyboard_focus_to_the_sample_lane() {
    let mut state = crate::native_app::tests::gui_state_for_span_tests();
    let source_root = tempfile::tempdir().expect("source root");
    let anchor = source_root.path().join("anchor.wav");
    fs::write(&anchor, []).expect("write anchor");
    state.library.folder_browser =
        crate::native_app::test_support::state::FolderBrowserState::from_sample_sources(&[
            wavecrate::sample_sources::SampleSource::new(source_root.path().to_path_buf()),
        ]);
    state
        .library
        .folder_browser
        .focus_selected_source_for_keyboard();
    assert!(state.library.folder_browser.source_keyboard_focus_active());
    assert!(state.library.folder_browser.keyboard_focus_visible());

    state.toggle_similarity_anchor(
        anchor.display().to_string(),
        &mut radiant::prelude::UiUpdateContext::default(),
    );

    assert!(!state.library.folder_browser.source_keyboard_focus_active());
    assert!(!state.library.folder_browser.keyboard_focus_visible());
}

#[test]
fn sample_row_selection_still_works_in_similarity_mode() {
    let mut state = crate::native_app::tests::gui_state_for_span_tests();
    let source_root = tempfile::tempdir().expect("source root");
    let drums = source_root.path().join("drums");
    fs::create_dir_all(&drums).expect("create drums folder");
    let anchor = drums.join("anchor.wav");
    let near = drums.join("near.wav");
    fs::write(&anchor, []).expect("write anchor");
    fs::write(&near, []).expect("write near");
    state.library.folder_browser =
        crate::native_app::test_support::state::FolderBrowserState::from_sample_sources(&[
            wavecrate::sample_sources::SampleSource::new(source_root.path().to_path_buf()),
        ]);
    state.library.folder_browser.apply_message(
        crate::native_app::test_support::state::FolderBrowserMessage::ActivateFolder(
            drums.display().to_string(),
            Default::default(),
        ),
    );
    let anchor_id = anchor.display().to_string();
    let near_id = near.display().to_string();
    state
        .library
        .folder_browser
        .set_similarity_scores_for_tests(anchor_id, [(near_id.clone(), 0.9)].into_iter().collect());
    let mut runtime = native_runtime_for_tests(state, Vector2::new(900.0, 620.0));
    let frame = runtime.frame_with_default_theme();

    runtime.dispatch_primary_click(text_center(&frame, "near"));

    assert_eq!(
        runtime
            .bridge()
            .state()
            .library
            .folder_browser
            .selected_file_id(),
        Some(near_id.as_str())
    );
}

#[test]
fn sample_browser_renders_similarity_header_only_in_similarity_mode() {
    let mut state = crate::native_app::tests::gui_state_for_span_tests();
    let source_root = tempfile::tempdir().expect("source root");
    let drums = source_root.path().join("drums");
    fs::create_dir_all(&drums).expect("create drums folder");
    let anchor = drums.join("anchor.wav");
    fs::write(&anchor, []).expect("write anchor");
    state.library.folder_browser =
        crate::native_app::test_support::state::FolderBrowserState::from_sample_sources(&[
            wavecrate::sample_sources::SampleSource::new(source_root.path().to_path_buf()),
        ]);
    state.library.folder_browser.apply_message(
        crate::native_app::test_support::state::FolderBrowserMessage::ActivateFolder(
            drums.display().to_string(),
            Default::default(),
        ),
    );

    prepare_sample_browser_view(&mut state);
    let inactive_frame = crate::native_app::test_support::sample_browser::sample_browser(&state)
        .view_frame_at_size_with_default_theme(Vector2::new(720.0, 360.0));
    assert!(!inactive_frame.paint_plan.contains_text("Sim"));

    state.library.folder_browser.apply_message(
        crate::native_app::test_support::state::FolderBrowserMessage::ToggleSimilarityAnchor(
            anchor.display().to_string(),
        ),
    );
    prepare_sample_browser_view(&mut state);
    let active_frame = crate::native_app::test_support::sample_browser::sample_browser(&state)
        .view_frame_at_size_with_default_theme(Vector2::new(720.0, 360.0));
    assert!(active_frame.paint_plan.contains_text("Sim"));
    assert!(active_frame.paint_plan.contains_text("Weight"));
}

#[test]
fn sample_browser_similarity_controls_emit_control_messages() {
    let mut state = crate::native_app::tests::gui_state_for_span_tests();
    let source_root = tempfile::tempdir().expect("source root");
    let drums = source_root.path().join("drums");
    fs::create_dir_all(&drums).expect("create drums folder");
    let anchor = drums.join("anchor.wav");
    fs::write(&anchor, []).expect("write anchor");
    state.library.folder_browser =
        crate::native_app::test_support::state::FolderBrowserState::from_sample_sources(&[
            wavecrate::sample_sources::SampleSource::new(source_root.path().to_path_buf()),
        ]);
    state.library.folder_browser.apply_message(
        crate::native_app::test_support::state::FolderBrowserMessage::ToggleSimilarityAnchor(
            anchor.display().to_string(),
        ),
    );
    prepare_sample_browser_view(&mut state);

    let surface =
        crate::native_app::test_support::sample_browser::sample_browser(&state).into_surface();
    assert_eq!(
        surface.dispatch_widget_output(
            crate::native_app::ui::ids::AUTOMATION_SAMPLE_SIMILARITY_WEIGHTING_TOGGLE_ID,
            radiant::widgets::WidgetOutput::typed(radiant::widgets::ToggleMessage::ValueChanged {
                checked: true,
                provenance: radiant::widgets::InteractionProvenance::Programmatic,
            },),
        ),
        Some(
            crate::native_app::test_support::state::GuiMessage::SetSimilarityAspectWeightingEnabled(
                true
            )
        )
    );
    assert_eq!(
        surface.dispatch_widget_output(
            radiant::widgets::stable_widget_id(
                crate::native_app::ui::ids::AUTOMATION_SAMPLE_SIMILARITY_ASPECT_TOGGLE_SCOPE,
                "spectrum",
            ),
            radiant::widgets::WidgetOutput::typed(radiant::widgets::ToggleMessage::ValueChanged {
                checked: false,
                provenance: radiant::widgets::InteractionProvenance::Programmatic,
            },),
        ),
        Some(
            crate::native_app::test_support::state::GuiMessage::SetSimilarityAspectEnabled {
                aspect: SimilarityAspect::Spectrum,
                enabled: false,
            },
        )
    );
    assert_eq!(
        surface.dispatch_widget_output(
            radiant::widgets::stable_widget_id(
                crate::native_app::ui::ids::AUTOMATION_SAMPLE_SIMILARITY_ASPECT_WEIGHT_SCOPE,
                "spectrum",
            ),
            radiant::widgets::WidgetOutput::typed(radiant::widgets::SliderMessage::ValueChanged {
                value: 0.25
            },),
        ),
        Some(
            crate::native_app::test_support::state::GuiMessage::SetSimilarityAspectWeight {
                aspect: SimilarityAspect::Spectrum,
                weight: 0.25,
            },
        )
    );
}

#[test]
fn sample_browser_similarity_controls_reorder_and_mute_disabled_aspects() {
    let mut state = crate::native_app::tests::gui_state_for_span_tests();
    let source_root = tempfile::tempdir().expect("source root");
    let drums = source_root.path().join("drums");
    fs::create_dir_all(&drums).expect("create drums folder");
    let anchor = drums.join("anchor.wav");
    let raw_near = drums.join("raw_near.wav");
    let spectrum_near = drums.join("spectrum_near.wav");
    for path in [&anchor, &raw_near, &spectrum_near] {
        fs::write(path, []).expect("write sample");
    }
    state.library.folder_browser =
        crate::native_app::test_support::state::FolderBrowserState::from_sample_sources(&[
            wavecrate::sample_sources::SampleSource::new(source_root.path().to_path_buf()),
        ]);
    state.library.folder_browser.apply_message(
        crate::native_app::test_support::state::FolderBrowserMessage::ActivateFolder(
            drums.display().to_string(),
            Default::default(),
        ),
    );
    let anchor_id = anchor.display().to_string();
    let raw_near_id = raw_near.display().to_string();
    let spectrum_near_id = spectrum_near.display().to_string();
    let mut raw_near_aspects =
        crate::native_app::sample_library::folder_browser::model::EMPTY_SIMILARITY_ASPECT_STRENGTHS;
    raw_near_aspects[SimilarityAspect::Spectrum.index()] = Some(0.1);
    raw_near_aspects[SimilarityAspect::Timbre.index()] = Some(0.9);
    let mut spectrum_near_aspects =
        crate::native_app::sample_library::folder_browser::model::EMPTY_SIMILARITY_ASPECT_STRENGTHS;
    spectrum_near_aspects[SimilarityAspect::Spectrum.index()] = Some(0.95);
    spectrum_near_aspects[SimilarityAspect::Timbre.index()] = Some(0.9);
    state
        .library
        .folder_browser
        .set_similarity_scores_with_aspects(
            anchor_id.clone(),
            [(raw_near_id.clone(), 0.9), (spectrum_near_id.clone(), 0.2)]
                .into_iter()
                .collect(),
            [
                (raw_near_id.clone(), raw_near_aspects),
                (spectrum_near_id.clone(), spectrum_near_aspects),
            ]
            .into_iter()
            .collect(),
        );
    assert_eq!(
        state
            .library
            .folder_browser
            .selected_audio_files()
            .into_iter()
            .map(|file| file.id.clone())
            .collect::<Vec<_>>(),
        vec![
            anchor_id.clone(),
            raw_near_id.clone(),
            spectrum_near_id.clone()
        ]
    );

    let mut context = radiant::prelude::UiUpdateContext::default();
    for message in [
        crate::native_app::test_support::state::GuiMessage::SetSimilarityAspectWeightingEnabled(
            true,
        ),
        crate::native_app::test_support::state::GuiMessage::SetSimilarityAspectEnabled {
            aspect: SimilarityAspect::Overall,
            enabled: false,
        },
        crate::native_app::test_support::state::GuiMessage::SetSimilarityAspectEnabled {
            aspect: SimilarityAspect::Timbre,
            enabled: false,
        },
        crate::native_app::test_support::state::GuiMessage::SetSimilarityAspectEnabled {
            aspect: SimilarityAspect::Pitch,
            enabled: false,
        },
        crate::native_app::test_support::state::GuiMessage::SetSimilarityAspectEnabled {
            aspect: SimilarityAspect::Amplitude,
            enabled: false,
        },
    ] {
        state.apply_message(message, &mut context);
    }

    assert_eq!(
        state
            .library
            .folder_browser
            .selected_audio_files()
            .into_iter()
            .map(|file| file.id.clone())
            .collect::<Vec<_>>(),
        vec![anchor_id, spectrum_near_id.clone(), raw_near_id]
    );
    let strengths = state
        .library
        .folder_browser
        .similarity_aspect_display_strengths_for_file(spectrum_near_id.as_str());
    assert!(strengths[SimilarityAspect::Spectrum.index()].is_some());
    assert_eq!(strengths[SimilarityAspect::Timbre.index()], None);
}

#[test]
fn sample_browser_similarity_controls_persist_after_debounce() {
    let config_base = tempfile::tempdir().expect("config base");
    let _base_guard = wavecrate::app_dirs::ConfigBaseGuard::set(config_base.path().to_path_buf());
    let mut state = crate::native_app::tests::gui_state_for_span_tests();
    let mut context = radiant::prelude::UiUpdateContext::default();

    state.apply_message(
        crate::native_app::test_support::state::GuiMessage::SetSimilarityAspectWeightingEnabled(
            true,
        ),
        &mut context,
    );

    let loaded = wavecrate::sample_sources::config::load_or_default().expect("load config");
    assert!(!loaded.core.similarity.weighting_enabled);
    assert!(state.ui.settings.similarity_persist_deadline.is_some());

    state.ui.settings.similarity_persist_deadline = Some(Instant::now() - Duration::from_millis(1));
    state.advance_frame(&mut context);
    let command = context.into_command();
    assert_eq!(
        command.business_task_priority("gui-similarity-settings-persist"),
        Some(radiant::prelude::TaskPriority::BlockingIo)
    );
    state.apply_message(
        crate::native_app::tests::run_worker_message_for_tests(
            command,
            "gui-similarity-settings-persist",
        )
        .expect("similarity settings persist worker completion"),
        &mut radiant::prelude::UiUpdateContext::default(),
    );

    let loaded = wavecrate::sample_sources::config::load_or_default().expect("reload config");
    assert!(loaded.core.similarity.weighting_enabled);
    assert!(!state.ui.settings.similarity_persist_inflight);
}

#[test]
fn sample_browser_similarity_controls_persist_after_debounce_while_playback_is_active() {
    let config_base = tempfile::tempdir().expect("config base");
    let _base_guard = wavecrate::app_dirs::ConfigBaseGuard::set(config_base.path().to_path_buf());
    let mut state = crate::native_app::tests::gui_state_for_span_tests();
    state.waveform.current.start_playback(0.2);
    let mut context = radiant::prelude::UiUpdateContext::default();

    state.apply_message(
        crate::native_app::test_support::state::GuiMessage::SetSimilarityAspectEnabled {
            aspect: SimilarityAspect::Spectrum,
            enabled: false,
        },
        &mut context,
    );

    let loaded = wavecrate::sample_sources::config::load_or_default().expect("load config");
    assert!(
        loaded
            .core
            .similarity
            .aspect_enabled(SimilarityAspect::Spectrum)
    );
    assert!(state.ui.settings.similarity_persist_deadline.is_some());

    state.ui.settings.similarity_persist_deadline = Some(Instant::now() - Duration::from_millis(1));
    state.advance_frame(&mut context);

    assert!(
        state.playback_visual_activity_active(),
        "test setup should keep playback visually active during the persist frame"
    );
    let command = context.into_command();
    assert_eq!(
        command.business_task_priority("gui-similarity-settings-persist"),
        Some(radiant::prelude::TaskPriority::BlockingIo)
    );
    state.apply_message(
        crate::native_app::tests::run_worker_message_for_tests(
            command,
            "gui-similarity-settings-persist",
        )
        .expect("similarity settings persist worker completion during playback"),
        &mut radiant::prelude::UiUpdateContext::default(),
    );

    let loaded = wavecrate::sample_sources::config::load_or_default().expect("reload config");
    assert!(
        !loaded
            .core
            .similarity
            .aspect_enabled(SimilarityAspect::Spectrum)
    );
    assert!(!state.ui.settings.similarity_persist_inflight);
}

#[test]
fn sample_browser_similarity_anchor_resolves_production_scores() {
    let (mut state, _source_root, anchor_id, near_id, far_id, missing_id) =
        similarity_state_with_embeddings();
    state.library.folder_browser.apply_message(
        crate::native_app::test_support::state::FolderBrowserMessage::ToggleSimilarityAnchor(
            anchor_id.clone(),
        ),
    );
    let mut context = radiant::prelude::UiUpdateContext::default();

    state.queue_similarity_score_resolution(anchor_id.clone(), &mut context);
    super::super::run_command_for_tests(&mut state, context.into_command());

    let ordered_ids = state
        .library
        .folder_browser
        .selected_audio_files()
        .into_iter()
        .map(|file| file.id.clone())
        .collect::<Vec<_>>();
    assert_eq!(
        ordered_ids,
        vec![
            anchor_id.clone(),
            near_id.clone(),
            far_id,
            missing_id.clone()
        ]
    );
    assert_eq!(
        state
            .library
            .folder_browser
            .similarity_display_strength_for_file(anchor_id.as_str()),
        Some(1.0)
    );
    assert!(
        state
            .library
            .folder_browser
            .similarity_display_strength_for_file(near_id.as_str())
            .is_some()
    );
    assert!(
        state
            .library
            .folder_browser
            .similarity_aspect_display_strengths_for_file(near_id.as_str())
            [wavecrate_analysis::aspects::SimilarityAspect::Spectrum.index()]
        .is_some_and(|strength| (strength - 1.0).abs() < 1e-5)
    );
    assert!(
        state
            .library
            .folder_browser
            .similarity_display_strength_for_file(missing_id.as_str())
            .is_none()
    );
    assert_eq!(state.ui.status.sample, "Resolved 2 similar samples");
}

#[test]
fn sample_browser_similarity_anchor_delegates_missing_artifacts_to_readiness_worker() {
    let mut state = crate::native_app::tests::gui_state_for_span_tests();
    let source_root = tempfile::tempdir().expect("source root");
    let drums = source_root.path().join("drums");
    fs::create_dir_all(&drums).expect("create drums folder");
    let anchor = drums.join("anchor.wav");
    let near = drums.join("near.wav");
    write_similarity_test_wav(&anchor, 220.0);
    write_similarity_test_wav(&near, 440.0);
    seed_source_scan_row(source_root.path(), "drums/anchor.wav");
    seed_source_scan_row(source_root.path(), "drums/near.wav");

    state.library.folder_browser =
        crate::native_app::test_support::state::FolderBrowserState::from_sample_sources(&[
            wavecrate::sample_sources::SampleSource::new_with_id(
                wavecrate::sample_sources::SourceId::from_string(SIMILARITY_TEST_SOURCE_ID),
                source_root.path().to_path_buf(),
            ),
        ]);
    state.library.folder_browser.apply_message(
        crate::native_app::test_support::state::FolderBrowserMessage::ActivateFolder(
            drums.display().to_string(),
            Default::default(),
        ),
    );
    let anchor_id = anchor.display().to_string();
    let near_id = near.display().to_string();
    let mut context = radiant::prelude::UiUpdateContext::default();

    state.apply_message(
        crate::native_app::test_support::state::GuiMessage::FolderBrowser(
            crate::native_app::test_support::state::FolderBrowserMessage::ToggleSimilarityAnchor(
                anchor_id.clone(),
            ),
        ),
        &mut context,
    );

    super::super::run_command_for_tests(&mut state, context.into_command());

    assert_eq!(
        source_jobs_by_status(source_root.path(), "wav_metadata_v1", "pending"),
        0,
        "anchor activation must not launch the competing legacy whole-source prep queue"
    );
    assert_eq!(
        source_jobs_by_status(source_root.path(), "embedding_backfill_v1", "pending"),
        0,
        "the revisioned readiness worker owns embedding generation"
    );
    assert_eq!(source_artifact_rows(source_root.path(), "embeddings"), 0);
    assert_eq!(
        source_artifact_rows(source_root.path(), "similarity_aspect_descriptors"),
        0
    );
    assert_eq!(
        state
            .library
            .folder_browser
            .similarity_display_strength_for_file(anchor_id.as_str()),
        Some(1.0)
    );
    assert_eq!(
        state
            .library
            .folder_browser
            .similarity_display_strength_for_file(near_id.as_str()),
        None,
        "anchor activation must wait for readiness before resolving missing similarity artifacts"
    );
    assert_eq!(state.ui.status.sample, "Similarity data not ready yet");
}

#[test]
fn sample_browser_similarity_anchor_scores_only_active_folder_scope() {
    let mut state = crate::native_app::tests::gui_state_for_span_tests();
    let source_root = tempfile::tempdir().expect("source root");
    let drums = source_root.path().join("drums");
    let loops = source_root.path().join("loops");
    fs::create_dir_all(&drums).expect("create drums folder");
    fs::create_dir_all(&loops).expect("create loops folder");
    let anchor = drums.join("anchor.wav");
    let local_near = drums.join("local_near.wav");
    let outside_near = loops.join("outside_near.wav");
    for path in [&anchor, &local_near, &outside_near] {
        fs::write(path, []).expect("write sample");
    }
    seed_similarity_embedding(source_root.path(), "drums/anchor.wav", &[1.0, 0.0]);
    seed_similarity_embedding(source_root.path(), "drums/local_near.wav", &[0.8, 0.6]);
    seed_similarity_embedding(source_root.path(), "loops/outside_near.wav", &[0.99, 0.01]);
    state.library.folder_browser =
        crate::native_app::test_support::state::FolderBrowserState::from_sample_sources(&[
            wavecrate::sample_sources::SampleSource::new_with_id(
                wavecrate::sample_sources::SourceId::from_string(SIMILARITY_TEST_SOURCE_ID),
                source_root.path().to_path_buf(),
            ),
        ]);
    state.library.folder_browser.apply_message(
        crate::native_app::test_support::state::FolderBrowserMessage::ActivateFolder(
            drums.display().to_string(),
            Default::default(),
        ),
    );
    let anchor_id = anchor.display().to_string();
    let local_near_id = local_near.display().to_string();
    let outside_near_id = outside_near.display().to_string();
    state.library.folder_browser.apply_message(
        crate::native_app::test_support::state::FolderBrowserMessage::ToggleSimilarityAnchor(
            anchor_id.clone(),
        ),
    );
    let mut context = radiant::prelude::UiUpdateContext::default();

    state.queue_similarity_score_resolution(anchor_id.clone(), &mut context);
    super::super::run_command_for_tests(&mut state, context.into_command());

    assert!(
        state
            .library
            .folder_browser
            .similarity_display_strength_for_file(local_near_id.as_str())
            .is_some(),
        "active-folder candidates should receive similarity scores"
    );
    assert_eq!(
        state
            .library
            .folder_browser
            .similarity_display_strength_for_file(outside_near_id.as_str()),
        None,
        "similarity scores must not bleed into other folders in the same source"
    );

    state.library.folder_browser.apply_message(
        crate::native_app::test_support::state::FolderBrowserMessage::ActivateFolder(
            loops.display().to_string(),
            Default::default(),
        ),
    );

    assert_eq!(
        state
            .library
            .folder_browser
            .similarity_display_strength_for_file(outside_near_id.as_str()),
        None,
        "changing folders should not reveal scores from an anchor scoped elsewhere"
    );
}

#[test]
fn sample_browser_similarity_ignores_stale_score_results() {
    let (mut state, _source_root, anchor_id, near_id, _far_id, _missing_id) =
        similarity_state_with_embeddings();
    state.library.folder_browser.apply_message(
        crate::native_app::test_support::state::FolderBrowserMessage::ToggleSimilarityAnchor(
            anchor_id.clone(),
        ),
    );
    let mut context = radiant::prelude::UiUpdateContext::default();

    state.queue_similarity_score_resolution(anchor_id, &mut context);
    state.library.folder_browser.apply_message(
        crate::native_app::test_support::state::FolderBrowserMessage::ToggleSimilarityAnchor(
            near_id.clone(),
        ),
    );
    super::super::run_command_for_tests(&mut state, context.into_command());

    assert_eq!(
        state.library.folder_browser.similarity_anchor_id(),
        Some(near_id.as_str())
    );
    assert_eq!(
        state
            .library
            .folder_browser
            .similarity_display_strength_for_file(near_id.as_str()),
        Some(1.0)
    );
}

#[test]
/// Confirms source selection observes state without directly scheduling heavy work.
fn source_selection_ui_path_does_not_directly_enqueue_heavy_readiness_work() {
    let mut state = crate::native_app::tests::gui_state_for_span_tests();
    let source_root = tempfile::tempdir().expect("source root");
    let drums = source_root.path().join("drums");
    fs::create_dir_all(&drums).expect("create drums folder");
    let first = drums.join("first.wav");
    let second = drums.join("second.wav");
    write_similarity_test_wav(&first, 220.0);
    write_similarity_test_wav(&second, 440.0);
    seed_source_scan_row(source_root.path(), "drums/first.wav");
    seed_source_scan_row(source_root.path(), "drums/second.wav");

    state.library.folder_browser =
        crate::native_app::test_support::state::FolderBrowserState::from_sample_sources(&[
            wavecrate::sample_sources::SampleSource::new_with_id(
                wavecrate::sample_sources::SourceId::from_string(SIMILARITY_TEST_SOURCE_ID),
                source_root.path().to_path_buf(),
            ),
        ]);
    state.library.folder_browser.apply_message(
        crate::native_app::test_support::state::FolderBrowserMessage::ActivateFolder(
            drums.display().to_string(),
            Default::default(),
        ),
    );
    let mut context = radiant::prelude::UiUpdateContext::default();

    state.queue_selected_source_prep(
        crate::native_app::sample_library::folder_browser_actions::SOURCE_SELECTION_PREP_INTENTS,
        crate::native_app::sample_library::folder_browser_actions::SOURCE_SELECTION_PREP_REASON,
        &mut context,
    );

    assert_eq!(state.waveform.cache.active_folder_warm_total, 0);
    assert!(
        state
            .waveform
            .cache
            .active_folder_warm_plan_task
            .active()
            .is_none(),
        "the UI/read path should not directly launch source-wide cache warming"
    );

    super::super::run_command_for_tests(&mut state, context.into_command());

    assert_ne!(
        state.ui.status.sample, "Similarity ready",
        "the UI/read path should not claim convergence before the coordinator runs"
    );
    assert_eq!(
        source_jobs_by_status(source_root.path(), "wav_metadata_v1", "pending"),
        0
    );
    assert_eq!(
        source_jobs_by_status(source_root.path(), "embedding_backfill_v1", "pending"),
        0
    );
    assert_eq!(
        source_jobs_by_status(source_root.path(), "wav_metadata_v1", "done"),
        0
    );
    assert_eq!(
        source_jobs_by_status(source_root.path(), "embedding_backfill_v1", "done"),
        0
    );
    assert_eq!(source_artifact_rows(source_root.path(), "features"), 0);
    assert_eq!(source_artifact_rows(source_root.path(), "embeddings"), 0);
    assert_eq!(
        source_artifact_rows(source_root.path(), "similarity_aspect_descriptors"),
        0
    );
}

fn similarity_state_with_embeddings() -> (
    crate::native_app::test_support::state::NativeAppState,
    tempfile::TempDir,
    String,
    String,
    String,
    String,
) {
    let mut state = crate::native_app::tests::gui_state_for_span_tests();
    let source_root = tempfile::tempdir().expect("source root");
    let drums = source_root.path().join("drums");
    fs::create_dir_all(&drums).expect("create drums folder");
    let anchor = drums.join("anchor.wav");
    let near = drums.join("near.wav");
    let far = drums.join("far.wav");
    let missing = drums.join("missing.wav");
    for path in [&anchor, &near, &far, &missing] {
        fs::write(path, []).expect("write sample");
    }
    seed_similarity_embedding(source_root.path(), "drums/anchor.wav", &[1.0, 0.0]);
    seed_similarity_embedding(source_root.path(), "drums/near.wav", &[0.8, 0.6]);
    seed_similarity_embedding(source_root.path(), "drums/far.wav", &[0.0, 1.0]);
    seed_similarity_aspects(source_root.path(), "drums/anchor.wav");
    seed_similarity_aspects(source_root.path(), "drums/near.wav");
    state.library.folder_browser =
        crate::native_app::test_support::state::FolderBrowserState::from_sample_sources(&[
            wavecrate::sample_sources::SampleSource::new_with_id(
                wavecrate::sample_sources::SourceId::from_string(SIMILARITY_TEST_SOURCE_ID),
                source_root.path().to_path_buf(),
            ),
        ]);
    state.library.folder_browser.apply_message(
        crate::native_app::test_support::state::FolderBrowserMessage::ActivateFolder(
            drums.display().to_string(),
            Default::default(),
        ),
    );
    (
        state,
        source_root,
        anchor.display().to_string(),
        near.display().to_string(),
        far.display().to_string(),
        missing.display().to_string(),
    )
}

fn seed_source_scan_row(source_root: &std::path::Path, relative_path: &str) {
    let db =
        wavecrate::sample_sources::SourceDatabase::open_for_test_fixture_source_write(source_root)
            .expect("source db");
    let size = fs::metadata(source_root.join(relative_path))
        .expect("source sample metadata")
        .len();
    db.upsert_file(std::path::Path::new(relative_path), size, 10)
        .expect("file row");
    db.set_metadata(
        wavecrate::sample_sources::db::META_LAST_SCAN_COMPLETED_AT,
        "20",
    )
    .expect("scan timestamp");
}

fn write_similarity_test_wav(path: &std::path::Path, frequency_hz: f32) {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: wavecrate_analysis::ANALYSIS_SAMPLE_RATE,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = hound::WavWriter::create(path, spec).expect("create wav");
    for index in 0..wavecrate_analysis::ANALYSIS_SAMPLE_RATE / 10 {
        let phase = index as f32 / wavecrate_analysis::ANALYSIS_SAMPLE_RATE as f32;
        let sample = (phase * frequency_hz * std::f32::consts::TAU).sin() * i16::MAX as f32 * 0.2;
        writer
            .write_sample(sample as i16)
            .expect("write wav sample");
    }
    writer.finalize().expect("finalize wav");
}

fn source_jobs_by_status(source_root: &std::path::Path, job_type: &str, status: &str) -> i64 {
    let conn = wavecrate::sample_sources::SourceDatabase::open_connection_with_role(
        source_root,
        wavecrate::sample_sources::SourceDatabaseConnectionRole::JobWorker,
    )
    .expect("source db connection");
    conn.query_row(
        "SELECT COUNT(*) FROM analysis_jobs WHERE job_type = ?1 AND status = ?2",
        [job_type, status],
        |row| row.get(0),
    )
    .expect("job count")
}

fn source_artifact_rows(source_root: &std::path::Path, table: &str) -> i64 {
    let conn = wavecrate::sample_sources::SourceDatabase::open_connection_with_role(
        source_root,
        wavecrate::sample_sources::SourceDatabaseConnectionRole::JobWorker,
    )
    .expect("source db connection");
    let table = match table {
        "features" => "features",
        "embeddings" => "embeddings",
        "similarity_aspect_descriptors" => "similarity_aspect_descriptors",
        _ => panic!("unexpected source artifact table"),
    };
    conn.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
        row.get(0)
    })
    .expect("artifact row count")
}

fn seed_similarity_embedding(source_root: &std::path::Path, relative_path: &str, values: &[f32]) {
    let _db =
        wavecrate::sample_sources::SourceDatabase::open_for_test_fixture_source_write(source_root)
            .expect("source db");
    let conn = wavecrate::sample_sources::SourceDatabase::open_connection_with_role(
        source_root,
        wavecrate::sample_sources::SourceDatabaseConnectionRole::JobWorker,
    )
    .expect("source db connection");
    let sample_id = format!("{SIMILARITY_TEST_SOURCE_ID}::{relative_path}");
    let blob = wavecrate_analysis::vector::encode_f32_le_blob(values);
    conn.execute(
        "INSERT OR REPLACE INTO embeddings
         (sample_id, model_id, dim, dtype, l2_normed, vec, created_at)
         VALUES (?1, ?2, ?3, 'f32', 1, ?4, 0)",
        rusqlite::params![
            sample_id,
            wavecrate_analysis::similarity::SIMILARITY_MODEL_ID,
            values.len() as i64,
            blob
        ],
    )
    .expect("insert embedding");
}

fn seed_similarity_aspects(source_root: &std::path::Path, relative_path: &str) {
    let _db =
        wavecrate::sample_sources::SourceDatabase::open_for_test_fixture_source_write(source_root)
            .expect("source db");
    let conn = wavecrate::sample_sources::SourceDatabase::open_connection_with_role(
        source_root,
        wavecrate::sample_sources::SourceDatabaseConnectionRole::JobWorker,
    )
    .expect("source db connection");
    let sample_id = format!("{SIMILARITY_TEST_SOURCE_ID}::{relative_path}");
    let mut features = vec![0.0_f32; wavecrate_analysis::FEATURE_VECTOR_LEN_V1];
    for (index, value) in features.iter_mut().enumerate() {
        *value = index as f32 + 1.0;
    }
    let descriptors =
        wavecrate_analysis::aspects::AspectDescriptorSet::from_feature_vector_v1(&features)
            .expect("aspect descriptors");
    let blob = wavecrate_analysis::vector::encode_f32_le_blob(descriptors.packed());
    conn.execute(
        "INSERT OR IGNORE INTO samples (sample_id, content_hash, size, mtime_ns)
         VALUES (?1, 'test-hash', 0, 0)",
        rusqlite::params![sample_id],
    )
    .expect("insert sample row");
    conn.execute(
        "INSERT OR REPLACE INTO similarity_aspect_descriptors
         (sample_id, model_id, dim, dtype, l2_normed, valid_mask, vec, created_at)
         VALUES (?1, ?2, ?3, ?4, 1, ?5, ?6, 0)",
        rusqlite::params![
            sample_id,
            wavecrate_analysis::aspects::ASPECT_DESCRIPTOR_MODEL_ID,
            wavecrate_analysis::aspects::ASPECT_DESCRIPTOR_DIM as i64,
            wavecrate_analysis::aspects::ASPECT_DESCRIPTOR_DTYPE_F32,
            descriptors.valid_mask() as i64,
            blob
        ],
    )
    .expect("insert aspect descriptors");
}
