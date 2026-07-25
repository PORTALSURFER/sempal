use super::*;
use crate::native_app::app::{MetadataMessage, SourceFilesystemSyncResult};
use crate::native_app::app_chrome::view_models::sample_browser::prepare_sample_browser_view;
use crate::native_app::test_support::sample_browser::complete_starmap_layout_for_selected_source;
use crate::native_app::test_support::state::{FolderBrowserState, NativeAppStateFixture};
use std::{
    fs,
    sync::{Arc, atomic::Ordering},
    time::Duration,
};

#[test]
fn accepted_sidebar_scroll_arms_paint_only_without_projection_repaint() {
    let mut state = NativeAppStateFixture::default().build();
    state
        .ui
        .chrome
        .overflow_fades
        .consume_pending_visibility_sample();
    let mut context = ui::UiUpdateContext::default();

    state.apply_message(
        GuiMessage::BrowserScrollAccepted(crate::native_app::app::BrowserScrollSurface::Filter),
        &mut context,
    );

    assert!(state.ui.chrome.overflow_fades.frame_needed());
    assert_eq!(
        context.into_command().repaint_scope(),
        Some(ui::RepaintScope::PaintOnly),
        "accepted sidebar scrolling must stay on the paint-only path"
    );
}

#[test]
fn folder_tree_interior_scroll_arms_without_applying_a_window_change() {
    let mut state = NativeAppStateFixture::default().build();
    state
        .ui
        .chrome
        .overflow_fades
        .consume_pending_visibility_sample();
    let before = state.library.folder_browser.tree_view_start();
    let mut context = ui::UiUpdateContext::default();

    state.apply_message(GuiMessage::FolderTreeWindowChanged(None), &mut context);

    assert_eq!(state.library.folder_browser.tree_view_start(), before);
    assert!(state.ui.chrome.overflow_fades.frame_needed());
    assert_eq!(
        context.into_command().repaint_scope(),
        Some(ui::RepaintScope::PaintOnly)
    );
}

#[test]
fn root_dispatch_routes_metadata_messages_to_metadata_owner() {
    let mut state = NativeAppState::load_default().expect("default state loads");

    state.apply_message(
        GuiMessage::Metadata(MetadataMessage::ToggleMetadataTagLibrary),
        &mut ui::UiUpdateContext::default(),
    );

    assert!(state.metadata.tag_library_open);
}

#[test]
fn frame_messages_use_frame_budget_slow_threshold() {
    assert_eq!(
        slow_ui_message_threshold(FRAME_MESSAGE_PROFILE_LABEL),
        Duration::from_micros(16_667)
    );
    assert_eq!(
        slow_ui_message_threshold("NavigateBrowser"),
        Duration::from_millis(4)
    );
}

#[test]
fn source_processing_progress_opens_the_shared_job_details_popover() {
    let mut state = NativeAppStateFixture::default().build();
    let source_id = state
        .library
        .folder_browser
        .defer_add_source_path(std::path::PathBuf::from("/tmp/progress-source"), false)
        .expect("source registered");
    state
        .background
        .source_lifecycle_generations
        .insert(source_id.clone(), 0);
    state.background.source_processing_progress =
        Some(crate::native_app::app::SourceProcessingProgress {
            source_id: source_id.clone(),
            lifecycle_generation: 0,
            active: true,
            source_row_active: true,
            completed: 3,
            total: 10,
            stage: String::from("Preparing similarity"),
            detail: String::from("kick.wav"),
        });

    state.apply_message(
        GuiMessage::ToggleJobDetails,
        &mut ui::UiUpdateContext::default(),
    );

    assert!(state.ui.chrome.job_details_open);

    state.apply_message(
        GuiMessage::SourceProcessingHealth(crate::native_app::app::SourceProcessingHealth {
            source_id: source_id.clone(),
            lifecycle_generation: 0,
            status: crate::native_app::app::SourceProcessingHealthStatus::DegradedTerminal,
            source_generation: 4,
            readiness_revision: 5,
            stage_counts: std::collections::BTreeMap::new(),
            retry_at: None,
            failure_codes: vec![String::from("decoder_unsupported")],
        }),
        &mut ui::UiUpdateContext::default(),
    );

    state.apply_message(
        GuiMessage::SourceProcessingProgress(crate::native_app::app::SourceProcessingProgress {
            source_id: source_id.clone(),
            lifecycle_generation: 0,
            active: false,
            source_row_active: false,
            completed: 10,
            total: 10,
            stage: String::from("Building similarity layout"),
            detail: String::from("Finalizing source"),
        }),
        &mut ui::UiUpdateContext::default(),
    );

    assert!(state.background.source_processing_progress.is_none());
    assert_eq!(
        state.background.source_processing_health[&source_id].status,
        crate::native_app::app::SourceProcessingHealthStatus::DegradedTerminal
    );
    assert!(!state.ui.chrome.job_details_open);

    state.apply_message(
        GuiMessage::SourceProcessingHealth(crate::native_app::app::SourceProcessingHealth {
            source_id: source_id.clone(),
            lifecycle_generation: 1,
            status: crate::native_app::app::SourceProcessingHealthStatus::Ready,
            source_generation: 6,
            readiness_revision: 7,
            stage_counts: std::collections::BTreeMap::new(),
            retry_at: None,
            failure_codes: Vec::new(),
        }),
        &mut ui::UiUpdateContext::default(),
    );
    assert_eq!(
        state.background.source_processing_health[&source_id].status,
        crate::native_app::app::SourceProcessingHealthStatus::DegradedTerminal,
        "stale lifecycle health must not replace the retained current state"
    );
}

#[test]
fn repeated_source_scan_cancel_messages_release_visible_owner_once() {
    let root = tempfile::tempdir().expect("source root");
    fs::write(root.path().join("sample.wav"), [0_u8; 8]).expect("sample");
    let mut state = NativeAppStateFixture::default().build();
    let request = state
        .library
        .begin_add_source_path(root.path().to_path_buf(), 71)
        .expect("scan request");
    state.library.start_folder_scan(&request);
    state.ui.chrome.job_details_open = true;
    let mut context = ui::UiUpdateContext::default();

    state.apply_message(GuiMessage::CancelActiveSourceScan, &mut context);
    state.apply_message(GuiMessage::CancelActiveSourceScan, &mut context);

    assert!(state.library.folder_progress().is_none());
    assert!(!state.ui.chrome.job_details_open);
    assert_eq!(
        state.ui.status.sample,
        format!("Canceled source scan for {}", request.label)
    );
}

#[test]
fn source_processing_progress_refreshes_the_retained_details_projection() {
    let mut state = NativeAppStateFixture::default().build();
    let source_id = state
        .library
        .folder_browser
        .defer_add_source_path(std::path::PathBuf::from("/tmp/progress-source"), false)
        .expect("source registered");
    state
        .background
        .source_lifecycle_generations
        .insert(source_id.clone(), 0);
    let mut context = ui::UiUpdateContext::default();

    state.apply_message(
        GuiMessage::SourceProcessingProgress(crate::native_app::app::SourceProcessingProgress {
            source_id,
            lifecycle_generation: 0,
            active: true,
            source_row_active: true,
            completed: 4,
            total: 10,
            stage: String::from("Preparing similarity"),
            detail: String::from("snare.wav"),
        }),
        &mut context,
    );

    assert_eq!(
        context.into_command().repaint_scope(),
        Some(ui::RepaintScope::Projection),
        "background progress must refresh retained text and counters without user input"
    );
}

#[test]
fn source_processing_progress_repaints_retained_worker_anchor_on_visibility_edges() {
    let mut state = NativeAppStateFixture::default().build();
    let source_id = state
        .library
        .folder_browser
        .defer_add_source_path(
            std::path::PathBuf::from("/tmp/starmap-progress-source"),
            false,
        )
        .expect("source registered");
    state
        .background
        .source_lifecycle_generations
        .insert(source_id.clone(), 0);
    state.ui.chrome.starmap_audition_drag =
        Some(crate::native_app::app::StarmapAuditionDragState {
            last_hit_file_id: Some(String::from("/samples/kick.wav")),
            last_position: radiant::gui::types::Point::new(50.0, 50.0),
            modifiers: radiant::widgets::PointerModifiers::default(),
        });
    let mut context = ui::UiUpdateContext::default();

    state.handle_message(
        GuiMessage::SourceProcessingProgress(crate::native_app::app::SourceProcessingProgress {
            source_id: source_id.clone(),
            lifecycle_generation: 0,
            active: true,
            source_row_active: true,
            completed: 4,
            total: 10,
            stage: String::from("Preparing similarity"),
            detail: String::from("snare.wav"),
        }),
        &mut context,
    );

    assert_eq!(
        context.into_command().repaint_scope(),
        Some(ui::RepaintScope::Surface),
        "starting the aggregate worker during audition must install its retained anchor"
    );

    let mut tick_context = ui::UiUpdateContext::default();
    state.handle_message(
        GuiMessage::SourceProcessingProgress(crate::native_app::app::SourceProcessingProgress {
            source_id: source_id.clone(),
            lifecycle_generation: 0,
            active: true,
            source_row_active: true,
            completed: 5,
            total: 10,
            stage: String::from("Preparing similarity"),
            detail: String::from("hat.wav"),
        }),
        &mut tick_context,
    );
    assert_eq!(
        tick_context.into_command().repaint_scope(),
        Some(ui::RepaintScope::PaintOnly),
        "steady aggregate worker ticks must remain paint-only during audition"
    );

    let mut finish_context = ui::UiUpdateContext::default();
    state.handle_message(
        GuiMessage::SourceProcessingProgress(crate::native_app::app::SourceProcessingProgress {
            source_id: source_id.clone(),
            lifecycle_generation: 0,
            active: false,
            source_row_active: false,
            completed: 10,
            total: 10,
            stage: String::from("Building similarity layout"),
            detail: String::from("Finalizing source"),
        }),
        &mut finish_context,
    );
    assert_eq!(
        finish_context.into_command().repaint_scope(),
        Some(ui::RepaintScope::Surface),
        "finishing the last aggregate worker must remove its retained anchor"
    );

    state.ui.chrome.starmap_audition_drag = None;
    let request = crate::native_app::app::SamplePlaybackRequest::transient(
        String::from("/samples/kick.wav"),
        crate::native_app::app::SamplePlaybackIntent::StarmapDrag,
        "starmap_release",
    );
    state
        .audio
        .start_resolving_sample_playback_session(request, "audio_file");
    state.audio.sample_playback_session.as_mut().unwrap().state =
        crate::native_app::app::SamplePlaybackSessionState::AudibleTransient;
    let mut active_session_context = ui::UiUpdateContext::default();
    state.handle_message(
        GuiMessage::SourceProcessingProgress(crate::native_app::app::SourceProcessingProgress {
            source_id: source_id.clone(),
            lifecycle_generation: 0,
            active: true,
            source_row_active: true,
            completed: 5,
            total: 10,
            stage: String::from("Preparing similarity"),
            detail: String::from("hat.wav"),
        }),
        &mut active_session_context,
    );
    assert_eq!(
        active_session_context.into_command().repaint_scope(),
        Some(ui::RepaintScope::Surface),
        "a worker starting during an audible starmap release must install its global anchor"
    );

    state.audio.clear_sample_playback_session();
    let mut settled_context = ui::UiUpdateContext::default();
    state.handle_message(
        GuiMessage::SourceProcessingProgress(crate::native_app::app::SourceProcessingProgress {
            source_id,
            lifecycle_generation: 0,
            active: true,
            source_row_active: true,
            completed: 6,
            total: 10,
            stage: String::from("Preparing similarity"),
            detail: String::from("clap.wav"),
        }),
        &mut settled_context,
    );
    assert_eq!(
        settled_context.into_command().repaint_scope(),
        Some(ui::RepaintScope::Projection),
        "normal progress projection must resume when the starmap session ends"
    );
}

#[test]
fn source_processing_progress_keeps_retained_repaint_paint_only_when_another_worker_is_visible() {
    let mut state = NativeAppStateFixture::default().build();
    let source_id = state
        .library
        .folder_browser
        .defer_add_source_path(
            std::path::PathBuf::from("/tmp/starmap-progress-shared-worker-source"),
            false,
        )
        .expect("source registered");
    state
        .background
        .source_lifecycle_generations
        .insert(source_id.clone(), 0);
    state.background.normalization_progress = Some(
        crate::native_app::test_support::state::NormalizationProgress {
            task_id: 1,
            label: String::from("another worker"),
            completed: 1,
            total: 2,
            work_completed: 1,
            work_total: 2,
            queued: 0,
            detail: String::from("normalizing"),
        },
    );
    state.ui.chrome.starmap_audition_drag =
        Some(crate::native_app::app::StarmapAuditionDragState {
            last_hit_file_id: Some(String::from("/samples/kick.wav")),
            last_position: radiant::gui::types::Point::new(50.0, 50.0),
            modifiers: radiant::widgets::PointerModifiers::default(),
        });

    let progress = |active, completed, detail| {
        GuiMessage::SourceProcessingProgress(crate::native_app::app::SourceProcessingProgress {
            source_id: source_id.clone(),
            lifecycle_generation: 0,
            active,
            source_row_active: active,
            completed,
            total: 10,
            stage: String::from("Preparing similarity"),
            detail: String::from(detail),
        })
    };

    let mut start_context = ui::UiUpdateContext::default();
    state.handle_message(progress(true, 1, "start.wav"), &mut start_context);
    assert_eq!(
        start_context.into_command().repaint_scope(),
        Some(ui::RepaintScope::PaintOnly),
        "a source worker starting behind another aggregate worker must stay paint-only"
    );

    let mut finish_context = ui::UiUpdateContext::default();
    state.handle_message(progress(false, 10, "finish.wav"), &mut finish_context);
    assert_eq!(
        finish_context.into_command().repaint_scope(),
        Some(ui::RepaintScope::PaintOnly),
        "a source worker finishing behind another aggregate worker must stay paint-only"
    );
}

#[test]
fn late_progress_for_removed_source_is_ignored() {
    let mut state = NativeAppStateFixture::default().build();
    let mut context = ui::UiUpdateContext::default();

    state.apply_message(
        GuiMessage::SourceProcessingProgress(crate::native_app::app::SourceProcessingProgress {
            source_id: String::from("retired-source"),
            lifecycle_generation: 0,
            active: true,
            source_row_active: true,
            completed: 1,
            total: 10,
            stage: String::from("Preparing similarity"),
            detail: String::from("late.wav"),
        }),
        &mut context,
    );

    assert!(state.background.source_processing_progress.is_none());
}

#[test]
fn late_progress_from_previous_readded_source_epoch_is_ignored() {
    let mut state = NativeAppStateFixture::default().build();
    let source_id = state
        .library
        .folder_browser
        .defer_add_source_path(
            std::path::PathBuf::from("/tmp/readded-progress-source"),
            false,
        )
        .expect("source registered");
    state
        .background
        .source_lifecycle_generations
        .insert(source_id.clone(), 2);
    let mut context = ui::UiUpdateContext::default();

    state.apply_message(
        GuiMessage::SourceProcessingProgress(crate::native_app::app::SourceProcessingProgress {
            source_id: source_id.clone(),
            lifecycle_generation: 1,
            active: true,
            source_row_active: true,
            completed: 1,
            total: 10,
            stage: String::from("Preparing similarity"),
            detail: String::from("old-epoch.wav"),
        }),
        &mut context,
    );
    assert!(state.background.source_processing_progress.is_none());

    state.apply_message(
        GuiMessage::SourceProcessingProgress(crate::native_app::app::SourceProcessingProgress {
            source_id,
            lifecycle_generation: 2,
            active: true,
            source_row_active: true,
            completed: 2,
            total: 10,
            stage: String::from("Preparing similarity"),
            detail: String::from("current-epoch.wav"),
        }),
        &mut context,
    );
    assert_eq!(
        state
            .background
            .source_processing_progress
            .as_ref()
            .map(|progress| progress.lifecycle_generation),
        Some(2)
    );
}

#[test]
fn source_completions_from_previous_readded_epoch_are_ignored() {
    let directory = tempfile::tempdir().expect("completion source");
    let mut state = NativeAppStateFixture::default().build();
    let source_id = state
        .library
        .folder_browser
        .defer_add_source_path(directory.path().to_path_buf(), false)
        .expect("source registered");
    state.sync_source_watcher();
    let current_generation = state.background.source_lifecycle_generations[&source_id];
    let old_generation = current_generation.wrapping_sub(1);
    let current_permit = state
        .background
        .source_processing
        .budget_handle()
        .acquire_scan(&source_id)
        .expect("current source scan permit");
    let current_cancel = current_permit.cancel_token();
    let initial_status = state.ui.status.sample.clone();
    let mut context = ui::UiUpdateContext::default();

    state.apply_message(
        GuiMessage::SourceFilesystemSyncFinished(SourceFilesystemSyncResult {
            source_id: source_id.clone(),
            lifecycle_generation: old_generation,
            changed_count: 1,
            journal_checkpoint_event_id: None,
            cancelled: true,
            result: Err(String::from("old epoch cancelled")),
        }),
        &mut context,
    );
    assert_eq!(state.ui.status.sample, initial_status);

    state.apply_message(
        GuiMessage::SourceManifestAuditCommitted {
            source_id,
            lifecycle_generation: old_generation,
            committed_delta: wavecrate::sample_sources::scanner::CommittedSourceDelta {
                revision: 2,
                changed: vec![wavecrate::sample_sources::scanner::ManifestIdentityDelta {
                    identity: String::from("old-file"),
                    relative_path: std::path::PathBuf::from("old.wav"),
                    content_generation: String::from("old-generation"),
                    source_metadata_changed: true,
                }],
                ..Default::default()
            },
        },
        &mut context,
    );
    assert!(
        !current_cancel.load(Ordering::Acquire),
        "an old completion must not cancel the re-added source generation"
    );
    drop(current_permit);
}

#[test]
fn idle_map_frame_reuses_prepared_sample_browser_projection() {
    let (_root, mut state, _sample_id) = prepared_map_state("idle-map-frame.wav");
    let prepared = state
        .library
        .folder_browser
        .cached_starmap_projection()
        .expect("prepared starmap projection");

    state.handle_message(GuiMessage::Frame, &mut ui::UiUpdateContext::default());

    let after_frame = state
        .library
        .folder_browser
        .cached_starmap_projection()
        .expect("starmap projection remains prepared");
    assert!(
        Arc::ptr_eq(&prepared, &after_frame),
        "paint-only frame traffic must not rebuild an unchanged starmap projection"
    );
}

#[test]
fn playback_frame_starts_pending_starmap_layout_load() {
    let root = tempfile::tempdir().expect("source root");
    let sample = root.path().join("playing-map-load.wav");
    fs::write(&sample, [0_u8; 8]).expect("write sample");
    let sample_id = sample.to_string_lossy().into_owned();
    let mut state = NativeAppStateFixture::default()
        .with_folder_browser(FolderBrowserState::from_root(root.path().to_path_buf()))
        .build();
    state.ui.chrome.sample_browser_display = crate::native_app::app::SampleBrowserDisplayMode::Map;
    prepare_sample_browser_view(&mut state);
    crate::native_app::test_support::state::seed_sample_playback_session(
        &mut state,
        sample_id,
        "loaded_sample",
    );
    assert!(state.playback_visual_activity_active());
    let mut context = ui::UiUpdateContext::default();

    state.handle_message(GuiMessage::Frame, &mut context);

    assert!(
        state.playback_visual_activity_active(),
        "loading the starmap must not interrupt active playback"
    );
    assert_eq!(
        context
            .into_command()
            .business_task_priority("gui-starmap-layout-load"),
        Some(ui::TaskPriority::Idle),
        "opening the starmap during playback must not defer its layout until transport stops"
    );
}

#[test]
fn map_frame_rebuilds_projection_when_copy_flash_expires() {
    let (_root, mut state, sample_id) = prepared_map_state("copy-flash-map-frame.wav");
    state
        .library
        .folder_browser
        .flash_copied_file_paths([sample_id.as_str()]);
    prepare_sample_browser_view(&mut state);
    let flashed = state
        .library
        .folder_browser
        .cached_starmap_projection()
        .expect("prepared flashed starmap projection");
    assert!(flashed[0].copy_flash);

    while state.library.folder_browser.copy_flash_frames() > 1 {
        state.handle_message(GuiMessage::Frame, &mut ui::UiUpdateContext::default());
        let during_flash = state
            .library
            .folder_browser
            .cached_starmap_projection()
            .expect("starmap projection remains prepared during flash");
        assert!(Arc::ptr_eq(&flashed, &during_flash));
    }
    state.handle_message(GuiMessage::Frame, &mut ui::UiUpdateContext::default());

    let after_flash = state
        .library
        .folder_browser
        .cached_starmap_projection()
        .expect("starmap projection refreshed after flash");
    assert!(!Arc::ptr_eq(&flashed, &after_flash));
    assert!(!after_flash[0].copy_flash);
}

fn prepared_map_state(file_name: &str) -> (tempfile::TempDir, NativeAppState, String) {
    let root = tempfile::tempdir().expect("source root");
    let sample = root.path().join(file_name);
    fs::write(&sample, [0_u8; 8]).expect("write sample");
    let sample_id = sample.to_string_lossy().into_owned();
    let mut state = NativeAppStateFixture::default()
        .with_folder_browser(FolderBrowserState::from_root(root.path().to_path_buf()))
        .build();
    state.ui.chrome.sample_browser_display = crate::native_app::app::SampleBrowserDisplayMode::Map;
    complete_starmap_layout_for_selected_source(&mut state);
    prepare_sample_browser_view(&mut state);
    (root, state, sample_id)
}
