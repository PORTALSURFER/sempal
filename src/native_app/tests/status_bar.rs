use super::gui_state_for_span_tests;
use crate::native_app::test_support::state::NativeAppState;
use radiant::{
    gui::types::{Point, Rect, Vector2},
    prelude::IntoView,
    widgets::WidgetInput,
};

#[test]
fn bottom_status_bar_reports_selected_sample_count() {
    let mut state = gui_state_for_span_tests();
    let source_root = tempfile::tempdir().expect("source root");
    let sample_path = source_root.path().join("selected-count.wav");
    std::fs::write(&sample_path, []).expect("sample file");
    state.library.folder_browser =
        crate::native_app::test_support::state::FolderBrowserState::from_sample_sources(&[
            wavecrate::sample_sources::SampleSource::new(source_root.path().to_path_buf()),
        ]);
    let empty_frame = crate::native_app::test_support::status_bar::bottom_status_bar(&state)
        .view_frame_at_size_with_default_theme(Vector2::new(720.0, 30.0));
    assert!(empty_frame.paint_plan.contains_text("0 samples"));
    assert!(!empty_frame.paint_plan.contains_text("1 sample"));

    state
        .library
        .folder_browser
        .select_file(sample_path.display().to_string());
    let selected_frame = crate::native_app::test_support::status_bar::bottom_status_bar(&state)
        .view_frame_at_size_with_default_theme(Vector2::new(720.0, 30.0));

    assert!(selected_frame.paint_plan.contains_text("1 sample"));
}

#[test]
fn status_bar_reports_selected_missing_source() {
    let temp = tempfile::tempdir().expect("tempdir");
    let missing_root = temp.path().join("missing-source");
    let folder_browser =
        crate::native_app::test_support::state::FolderBrowserState::from_sample_sources_deferred(
            &[wavecrate::sample_sources::SampleSource::new(missing_root)],
        );
    let state = crate::native_app::test_support::state::NativeAppStateFixture::default()
        .with_folder_browser(folder_browser)
        .with_sample_status("Ready")
        .build();

    let model = crate::native_app::test_support::status_bar::status_bar_projection(&state);

    assert_eq!(model.status_text, "Source missing | Ready");
}

#[test]
fn bottom_status_worker_indicator_retains_only_a_transparent_pulse_anchor() {
    let mut state = NativeAppState::load_default().expect("default state loads");
    state.library.set_folder_progress_for_tests(
        crate::native_app::test_support::state::FolderScanProgress::new(
            7,
            String::from("assets"),
            String::from("Assets"),
            crate::native_app::test_support::state::FolderScanLifecycle::Scanning,
            2,
            5,
            String::from("kick.wav"),
        ),
    );
    let frame = crate::native_app::test_support::status_bar::worker_progress_bar(&state)
        .view_frame_at_size_with_default_theme(Vector2::new(20.0, 20.0));

    assert_eq!(
        frame
            .paint_plan
            .primitives
            .iter()
            .filter(|primitive| matches!(
                primitive,
                radiant::runtime::PaintPrimitive::FillPolygon(_)
            ))
            .count(),
        0,
        "the visible circle belongs exclusively to the animated overlay"
    );
    assert_eq!(frame.paint_plan.fill_rects().count(), 1);
    assert_eq!(frame.paint_plan.stroke_rects().count(), 0);
}

#[test]
fn bottom_status_bar_reports_normalization_progress() {
    let mut state = NativeAppState::load_default().expect("default state loads");
    state.ui.status.sample = String::from("Ready");
    state.background.normalization_progress = Some(
        crate::native_app::test_support::state::NormalizationProgress {
            task_id: 9,
            label: String::from("2 samples"),
            completed: 1,
            total: 2,
            work_completed: 500,
            work_total: 2_000,
            queued: 0,
            detail: String::from("snare.wav"),
        },
    );
    let frame = crate::native_app::test_support::status_bar::bottom_status_bar(&state)
        .view_frame_at_size_with_default_theme(Vector2::new(720.0, 30.0));

    assert!(frame.paint_plan.contains_text("Ready"));
    assert!(!frame.paint_plan.contains_text("Normalizing 2 samples"));
}

#[test]
fn bottom_status_bar_reports_queued_normalization_tasks() {
    let mut state = NativeAppState::load_default().expect("default state loads");
    state.ui.status.sample = String::from("Ready");
    state.background.normalization_progress = Some(
        crate::native_app::test_support::state::NormalizationProgress {
            task_id: 9,
            label: String::from("1 sample"),
            completed: 0,
            total: 1,
            work_completed: 250,
            work_total: 1_000,
            queued: 2,
            detail: String::from("kick.wav"),
        },
    );
    let frame = crate::native_app::test_support::status_bar::bottom_status_bar(&state)
        .view_frame_at_size_with_default_theme(Vector2::new(720.0, 30.0));

    assert!(frame.paint_plan.contains_text("Ready"));
    let model = crate::native_app::test_support::status_bar::status_bar_projection(&state);
    assert_eq!(
        model.job_details.expect("normalization details")[3],
        "Current: kick.wav | 2 queued"
    );
}

#[test]
fn bottom_status_progress_bar_click_opens_job_details() {
    let bounds = Rect::from_size(180.0, 10.0);
    let mut progress = radiant::widgets::ProgressBarWidget::determinate(0.4).with_activation();
    assert_eq!(
        progress.handle_input(bounds, WidgetInput::primary_press(Point::new(90.0, 5.0)),),
        None
    );

    assert_eq!(
        progress.handle_input(bounds, WidgetInput::primary_release(Point::new(90.0, 5.0)),),
        Some(radiant::widgets::ProgressBarMessage::Activate)
    );
}

#[test]
fn bottom_status_worker_indicator_keeps_one_anchor_for_unknown_totals() {
    let mut state = NativeAppState::load_default().expect("default state loads");
    state.background.progress_tick = 0.5;
    state.library.set_folder_progress_for_tests(
        crate::native_app::test_support::state::FolderScanProgress::new(
            7,
            String::from("assets"),
            String::from("Assets"),
            crate::native_app::test_support::state::FolderScanLifecycle::Scanning,
            128,
            0,
            String::from("kick.wav"),
        ),
    );
    let frame = crate::native_app::test_support::status_bar::worker_progress_bar(&state)
        .view_frame_at_size_with_default_theme(Vector2::new(20.0, 20.0));

    assert_eq!(
        frame
            .paint_plan
            .primitives
            .iter()
            .filter(|primitive| matches!(
                primitive,
                radiant::runtime::PaintPrimitive::FillPolygon(_)
            ))
            .count(),
        0,
        "unknown totals still retain only the transparent pulse anchor"
    );
    assert_eq!(frame.paint_plan.fill_rects().count(), 1);
    assert_eq!(frame.paint_plan.stroke_rects().count(), 0);
}

#[test]
fn bottom_status_source_cache_uses_one_worker_indicator_anchor() {
    let mut state = NativeAppState::load_default().expect("default state loads");
    state.background.progress_tick = 0.5;
    state.waveform.cache.active_folder_warm_folder_id = Some(String::from("source"));
    state.waveform.cache.active_folder_warm_completed = 3;
    state.waveform.cache.active_folder_warm_total = 10;
    state.waveform.cache.active_folder_warm_current = Some("kicks/kick-01.wav".into());
    state.waveform.cache.active_folder_warm_current_progress = 0.42;

    let frame = crate::native_app::test_support::status_bar::worker_progress_bar(&state)
        .view_frame_at_size_with_default_theme(Vector2::new(20.0, 20.0));

    assert_eq!(
        frame
            .paint_plan
            .primitives
            .iter()
            .filter(|primitive| matches!(
                primitive,
                radiant::runtime::PaintPrimitive::FillPolygon(_)
            ))
            .count(),
        0,
        "source-cache progress still retains only the transparent pulse anchor"
    );
    assert_eq!(frame.paint_plan.fill_rects().count(), 1);
    assert_eq!(frame.paint_plan.stroke_rects().count(), 0);
}

#[test]
fn source_cache_warm_advances_activity_tick_on_frame() {
    let mut state = NativeAppState::load_default().expect("default state loads");
    state.waveform.cache.active_folder_warm_folder_id = Some(String::from("source"));
    state.waveform.cache.active_folder_warm_total = 10;

    state.advance_frame(&mut radiant::prelude::UiUpdateContext::default());

    assert_eq!(state.background.progress_tick, 0.035);
}

#[test]
fn source_processing_advances_activity_tick_on_frame() {
    let mut state = NativeAppState::load_default().expect("default state loads");
    state.background.source_processing_progress = Some(
        crate::native_app::test_support::state::SourceProcessingProgress {
            source_id: String::from("source"),
            lifecycle_generation: 0,
            active: true,
            presentation:
                crate::native_app::source_processing::SourceProcessingPresentation::UserRelevant,
            source_row_active: true,
            completed: 3,
            total: 10,
            stage: String::from("Analyzing audio"),
            detail: String::from("kick.wav"),
        },
    );

    state.advance_frame(&mut radiant::prelude::UiUpdateContext::default());

    assert_eq!(state.background.progress_tick, 0.035);
}

#[test]
fn source_processing_keeps_measured_progress_behind_one_worker_indicator() {
    let mut state = NativeAppState::load_default().expect("default state loads");
    state.background.source_processing_progress = Some(
        crate::native_app::test_support::state::SourceProcessingProgress {
            source_id: String::from("source"),
            lifecycle_generation: 0,
            active: true,
            presentation:
                crate::native_app::source_processing::SourceProcessingPresentation::UserRelevant,
            source_row_active: true,
            completed: 20_625,
            total: 40_658,
            stage: String::from("Analyzing audio"),
            detail: String::from("kick.wav"),
        },
    );

    let frame = crate::native_app::test_support::status_bar::worker_progress_bar(&state)
        .view_frame_at_size_with_default_theme(Vector2::new(20.0, 20.0));

    assert_eq!(
        frame
            .paint_plan
            .primitives
            .iter()
            .filter(|primitive| matches!(
                primitive,
                radiant::runtime::PaintPrimitive::FillPolygon(_)
            ))
            .count(),
        0,
        "source processing must not leave a static retained indicator behind"
    );
    assert_eq!(frame.paint_plan.fill_rects().count(), 1);
    assert_eq!(frame.paint_plan.stroke_rects().count(), 0);
}

#[test]
fn source_processing_discovery_uses_compact_activity_feedback() {
    let mut state = NativeAppState::load_default().expect("default state loads");
    state.background.source_processing_progress = Some(
        crate::native_app::test_support::state::SourceProcessingProgress {
            source_id: String::from("Projects"),
            lifecycle_generation: 0,
            active: true,
            presentation:
                crate::native_app::source_processing::SourceProcessingPresentation::UserRelevant,
            source_row_active: true,
            completed: 0,
            total: 0,
            stage: String::from("Inspecting source manifest"),
            detail: String::from("Reading eligible files"),
        },
    );

    let model = crate::native_app::test_support::status_bar::status_bar_projection(&state);

    assert_eq!(
        model.worker_progress.expect("worker activity"),
        crate::native_app::test_support::status_bar::WorkerProgressProjection {
            completed: 0,
            total: 0,
            current_fraction: None,
            active_animation: false,
            compact_activity: true,
        }
    );
    assert_eq!(
        model.job_details.expect("activity details"),
        [
            String::from("Type: Source processing"),
            String::from("Source: Projects"),
            String::from("Progress: Active (total not available)"),
            String::from("Current: Inspecting source manifest | Reading eligible files"),
        ]
    );
}

#[test]
fn normalization_progress_does_not_advance_activity_tick_on_frame() {
    let mut state = NativeAppState::load_default().expect("default state loads");
    state.background.normalization_progress = Some(
        crate::native_app::test_support::state::NormalizationProgress {
            task_id: 9,
            label: String::from("6000 samples"),
            completed: 256,
            total: 6000,
            work_completed: 256_000,
            work_total: 6_000_000,
            queued: 0,
            detail: String::from("kick.wav | Analyzing"),
        },
    );

    state.advance_frame(&mut radiant::prelude::UiUpdateContext::default());

    assert_eq!(state.background.progress_tick, 0.0);
}

#[test]
fn job_details_popover_reports_active_scan_progress() {
    let progress = crate::native_app::test_support::state::FolderScanProgress::new(
        7,
        String::from("assets"),
        String::from("Assets"),
        crate::native_app::test_support::state::FolderScanLifecycle::Scanning,
        2,
        5,
        String::from("kick.wav"),
    );
    let frame = crate::native_app::test_support::status_bar::job_details_popover(&progress)
        .view_frame_at_size_with_default_theme(Vector2::new(360.0, 180.0));

    assert!(frame.paint_plan.contains_text("Job Details"));
    assert!(frame.paint_plan.contains_text("Type: Source scan"));
    assert!(frame.paint_plan.contains_text("Progress: Scanning — 2/5"));
    assert!(frame.paint_plan.contains_text("Current: kick.wav"));
}

#[test]
fn status_bar_view_model_prioritizes_active_worker_progress() {
    let mut state = NativeAppState::load_default().expect("default state loads");
    state.ui.status.sample = String::from("Ready");
    state.library.set_folder_progress_for_tests(
        crate::native_app::test_support::state::FolderScanProgress::new(
            7,
            String::from("assets"),
            String::from("Assets"),
            crate::native_app::test_support::state::FolderScanLifecycle::Scanning,
            2,
            5,
            String::from("kick.wav"),
        ),
    );
    state.background.normalization_progress = Some(
        crate::native_app::test_support::state::NormalizationProgress {
            task_id: 9,
            label: String::from("2 samples"),
            completed: 1,
            total: 2,
            work_completed: 500,
            work_total: 2_000,
            queued: 0,
            detail: String::from("snare.wav"),
        },
    );

    let model = crate::native_app::test_support::status_bar::status_bar_projection(&state);

    assert_eq!(model.selected_sample_count, 0);
    assert_eq!(model.status_text, "Ready");
    assert_eq!(
        model.worker_progress.expect("worker progress"),
        crate::native_app::test_support::status_bar::WorkerProgressProjection {
            completed: 2,
            total: 5,
            current_fraction: None,
            active_animation: false,
            compact_activity: false,
        }
    );
}

#[test]
fn status_bar_routes_source_processing_through_worker_progress_and_job_details() {
    let mut state = gui_state_for_span_tests();
    let source_root = tempfile::tempdir().expect("source root");
    state.library.folder_browser =
        crate::native_app::test_support::state::FolderBrowserState::from_sample_sources(&[
            wavecrate::sample_sources::SampleSource::new(source_root.path().to_path_buf()),
        ]);
    let source_id = state
        .library
        .folder_browser
        .selected_source_id()
        .to_string();
    let source_label = state
        .library
        .folder_browser
        .source_label(source_id.as_str())
        .expect("selected source label")
        .to_string();
    state.background.source_processing_progress = Some(
        crate::native_app::test_support::state::SourceProcessingProgress {
            source_id,
            lifecycle_generation: 0,
            active: true,
            presentation:
                crate::native_app::source_processing::SourceProcessingPresentation::UserRelevant,
            source_row_active: true,
            completed: 313,
            total: 9_985,
            stage: String::from("Preparing similarity"),
            detail: String::from("017/bounce/kick.wav"),
        },
    );

    let model = crate::native_app::test_support::status_bar::status_bar_projection(&state);

    assert_eq!(
        model.worker_progress.expect("worker progress"),
        crate::native_app::test_support::status_bar::WorkerProgressProjection {
            completed: 313,
            total: 9_985,
            current_fraction: None,
            active_animation: false,
            compact_activity: true,
        }
    );
    assert_eq!(
        model.job_details.expect("job details"),
        [
            String::from("Type: Source processing"),
            format!("Source: {source_label}"),
            String::from("Progress: 313/9985"),
            String::from("Current: Preparing similarity | 017/bounce/kick.wav"),
        ]
    );
}

#[test]
fn source_processing_job_details_use_truthful_discovery_units() {
    let mut state = gui_state_for_span_tests();
    let source_root = tempfile::tempdir().expect("source root");
    state.library.folder_browser =
        crate::native_app::test_support::state::FolderBrowserState::from_sample_sources(&[
            wavecrate::sample_sources::SampleSource::new(source_root.path().to_path_buf()),
        ]);
    let source_id = state
        .library
        .folder_browser
        .selected_source_id()
        .to_string();

    for (stage, completed, total, detail, expected) in [
        (
            "Preparing readiness targets",
            7_232,
            7_232,
            "7232 / 7232 files prepared",
            "Progress: 7232/7232 files prepared",
        ),
        (
            "Comparing source readiness",
            17_435,
            21_697,
            "17435 / 21697 readiness targets compared",
            "Progress: 17435/21697 readiness targets compared",
        ),
        (
            "Queueing unfinished work",
            14_460,
            21_697,
            "14460 / 21697 readiness targets checked",
            "Progress: 14460/21697 readiness targets checked",
        ),
    ] {
        state.background.source_processing_progress = Some(
            crate::native_app::test_support::state::SourceProcessingProgress {
                source_id: source_id.clone(),
                lifecycle_generation: 0,
                active: true,
                presentation:
                    crate::native_app::source_processing::SourceProcessingPresentation::UserRelevant,
                source_row_active: true,
                completed,
                total,
                stage: String::from(stage),
                detail: String::from(detail),
            },
        );

        let model = crate::native_app::test_support::status_bar::status_bar_projection(&state);

        assert_eq!(model.job_details.expect("job details")[2], expected);
    }
}

#[test]
fn status_bar_view_model_uses_normalization_work_progress_for_worker_bar() {
    let mut state = NativeAppState::load_default().expect("default state loads");
    state.ui.status.sample = String::from("Ready");
    state.background.normalization_progress = Some(
        crate::native_app::test_support::state::NormalizationProgress {
            task_id: 9,
            label: String::from("1 sample"),
            completed: 0,
            total: 1,
            work_completed: 420,
            work_total: 1_000,
            queued: 0,
            detail: String::from("kick.wav | Writing"),
        },
    );

    let model = crate::native_app::test_support::status_bar::status_bar_projection(&state);

    assert_eq!(model.status_text, "Ready");
    assert_eq!(
        model.worker_progress.expect("worker progress"),
        crate::native_app::test_support::status_bar::WorkerProgressProjection {
            completed: 420,
            total: 1_000,
            current_fraction: None,
            active_animation: false,
            compact_activity: false,
        }
    );
}

#[test]
fn status_bar_view_model_reports_file_move_progress() {
    let mut state = NativeAppState::load_default().expect("default state loads");
    state.ui.status.sample = String::from("Ready");
    state.background.file_move_progress =
        Some(crate::native_app::test_support::state::FileMoveProgress {
            task_id: 11,
            label: String::from("Moving 3 files"),
            completed: 2,
            total: 4,
            detail: String::from("Updating metadata"),
        });

    let model = crate::native_app::test_support::status_bar::status_bar_projection(&state);

    assert_eq!(model.status_text, "Ready");
    assert_eq!(
        model.worker_progress.expect("worker progress"),
        crate::native_app::test_support::status_bar::WorkerProgressProjection {
            completed: 2,
            total: 4,
            current_fraction: None,
            active_animation: false,
            compact_activity: false,
        }
    );
}

#[test]
fn status_bar_view_model_reports_source_cache_warm_progress() {
    let mut state = NativeAppState::load_default().expect("default state loads");
    state.ui.status.sample = String::from("Ready");
    state.waveform.cache.active_folder_warm_folder_id = Some(String::from("source"));
    state.waveform.cache.active_folder_warm_completed = 3;
    state.waveform.cache.active_folder_warm_total = 10;
    state.waveform.cache.active_folder_warm_current = Some("kicks/kick-01.wav".into());

    let model = crate::native_app::test_support::status_bar::status_bar_projection(&state);

    assert_eq!(model.status_text, "Ready");
    assert_eq!(
        model.worker_progress.expect("worker progress"),
        crate::native_app::test_support::status_bar::WorkerProgressProjection {
            completed: 3,
            total: 10,
            current_fraction: Some(0.0),
            active_animation: true,
            compact_activity: false,
        }
    );
}

#[test]
fn status_bar_view_model_reports_source_cache_plan_progress() {
    let mut state = NativeAppState::load_default().expect("default state loads");
    state.ui.status.sample = String::from("Ready");
    state.waveform.cache.active_folder_warm_plan_task.begin();
    state.waveform.cache.active_folder_warm_folder_id = Some(String::from("source"));
    state.waveform.cache.active_folder_warm_completed = 42;
    state.waveform.cache.active_folder_warm_total = 100;
    state.waveform.cache.active_folder_warm_current = Some("kicks/plan-target.wav".into());
    state.waveform.cache.active_folder_warm_current_progress = 0.42;
    state.waveform.cache.active_folder_warm_current_stage =
        Some(crate::native_app::test_support::state::ActiveFolderCacheWarmStage::CheckingCache);

    let model = crate::native_app::test_support::status_bar::status_bar_projection(&state);

    assert_eq!(model.status_text, "Ready");
    assert_eq!(
        model.worker_progress.expect("worker progress"),
        crate::native_app::test_support::status_bar::WorkerProgressProjection {
            completed: 42,
            total: 100,
            current_fraction: Some(0.42),
            active_animation: true,
            compact_activity: false,
        }
    );
}

#[test]
fn status_bar_view_model_preserves_playback_status_while_source_cache_progress_runs() {
    let mut state = NativeAppState::load_default().expect("default state loads");
    state.ui.status.sample = String::from("Playing kick.wav");
    state.waveform.cache.active_folder_warm_folder_id = Some(String::from("source"));
    state.waveform.cache.active_folder_warm_completed = 3;
    state.waveform.cache.active_folder_warm_total = 10;
    state.waveform.cache.active_folder_warm_current = Some("kicks/cache-target.wav".into());
    state.waveform.cache.active_folder_warm_current_progress = 0.51;
    state.waveform.cache.active_folder_warm_current_stage =
        Some(crate::native_app::test_support::state::ActiveFolderCacheWarmStage::Decoding);

    let model = crate::native_app::test_support::status_bar::status_bar_projection(&state);

    assert_eq!(model.status_text, "Playing kick.wav");
    assert!(
        model.job_details.expect("cache details")[3].contains("decoding 51%"),
        "job details should retain per-file cache progress"
    );
    assert_eq!(
        model.worker_progress.expect("worker progress"),
        crate::native_app::test_support::status_bar::WorkerProgressProjection {
            completed: 3,
            total: 10,
            current_fraction: Some(0.51),
            active_animation: true,
            compact_activity: false,
        }
    );
}

#[test]
fn status_bar_view_model_keeps_normalization_priority_over_source_cache_warm() {
    let mut state = NativeAppState::load_default().expect("default state loads");
    state.ui.status.sample = String::from("Ready");
    state.waveform.cache.active_folder_warm_folder_id = Some(String::from("source"));
    state.waveform.cache.active_folder_warm_completed = 3;
    state.waveform.cache.active_folder_warm_total = 10;
    state.background.normalization_progress = Some(
        crate::native_app::test_support::state::NormalizationProgress {
            task_id: 9,
            label: String::from("1 sample"),
            completed: 0,
            total: 1,
            work_completed: 420,
            work_total: 1_000,
            queued: 0,
            detail: String::from("kick.wav | Writing"),
        },
    );

    let model = crate::native_app::test_support::status_bar::status_bar_projection(&state);

    assert_eq!(model.status_text, "Ready");
    assert_eq!(
        model.worker_progress.expect("worker progress"),
        crate::native_app::test_support::status_bar::WorkerProgressProjection {
            completed: 420,
            total: 1_000,
            current_fraction: None,
            active_animation: false,
            compact_activity: false,
        }
    );
}
