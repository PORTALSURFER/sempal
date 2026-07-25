use super::*;
use radiant::runtime::{PaintPrimitive, RepaintScope};
use std::time::{Duration, Instant};

#[test]
fn folder_focus_fade_invalidates_the_cached_row_projection() {
    let mut state = gui_state_for_span_tests();
    state.library.folder_browser.show_pointer_focus();
    let started_at = Instant::now();
    let before = state.capture_frame_surface_inputs_at(started_at);

    assert_eq!(
        state.frame_scope_since_at(before, started_at + Duration::from_millis(125)),
        RepaintScope::Projection,
        "time-derived folder focus opacity must reproject cached rows while it fades"
    );
    assert_eq!(
        state.frame_scope_since_at(before, started_at + Duration::from_millis(300)),
        RepaintScope::Projection,
        "the final transparent focus frame must clear the cached focus marker"
    );
}

#[test]
fn folder_focus_fade_reprojects_during_retained_playback_frames() {
    let mut state = gui_state_for_span_tests();
    state.waveform.current.start_playback(0.25);
    state.library.folder_browser.show_pointer_focus();
    let started_at = Instant::now();
    let before = state.frame_surface_revisions();
    let guard = state.begin_frame_surface_revision_tracking();

    state
        .library
        .folder_browser
        .advance_keyboard_focus_fade(started_at + Duration::from_millis(125));
    state.finish_frame_surface_revision_tracking(guard);

    assert_eq!(
        state.frame_surface_revisions().repaint_scope_since(before),
        RepaintScope::Projection,
        "retained playback must still reproject rows while focus opacity changes"
    );
}

#[test]
fn folder_focus_fade_reprojects_during_retained_starmap_frames() {
    let mut state = gui_state_for_span_tests();
    state.ui.chrome.starmap_audition_drag =
        Some(crate::native_app::app::StarmapAuditionDragState {
            last_hit_file_id: Some(String::from("/samples/kick.wav")),
            last_position: radiant::gui::types::Point::new(50.0, 50.0),
            modifiers: radiant::widgets::PointerModifiers::default(),
        });
    state.library.folder_browser.show_pointer_focus();
    let started_at = Instant::now();
    let before = state.frame_surface_revisions();
    let guard = state.begin_frame_surface_revision_tracking();

    state
        .library
        .folder_browser
        .advance_keyboard_focus_fade(started_at + Duration::from_millis(125));
    state.finish_frame_surface_revision_tracking(guard);

    assert_eq!(
        state.frame_surface_revisions().repaint_scope_since(before),
        RepaintScope::Projection,
        "retained starmap frames must reproject rows while focus opacity changes"
    );
}

#[test]
fn playback_frame_uses_paint_only_when_only_playhead_changes() {
    let mut state = gui_state_for_span_tests();
    state.waveform.current.start_playback(0.25);

    let before = state.capture_frame_surface_inputs();
    state.advance_frame(&mut radiant::prelude::UiUpdateContext::default());

    assert!(
        state.frame_can_use_paint_only_since(before),
        "playback-only frames should not force full surface reprojection"
    );
}

#[test]
fn early_runtime_playback_handoff_keeps_transient_overlay_active() {
    let mut state = gui_state_for_span_tests();
    crate::native_app::test_support::state::seed_sample_playback_session(
        &mut state,
        String::from("kick.wav"),
        "audio_file",
    );
    state.audio.playback_progress.active = true;
    state.audio.playback_progress.progress = Some(0.25);

    assert!(
        state.should_paint_waveform_transient_overlay(),
        "descriptor/file-backed runtime playback should keep paint-only playback frames active before waveform replacement catches up"
    );
}

#[test]
fn active_starmap_drag_keeps_app_transient_overlay_active_without_waveform_overlay() {
    let mut state = gui_state_for_span_tests();
    assert!(!state.should_paint_waveform_transient_overlay());
    assert!(!state.should_paint_app_transient_overlay());

    state.ui.chrome.starmap_audition_drag =
        Some(crate::native_app::app::StarmapAuditionDragState {
            last_hit_file_id: Some(String::from("/samples/kick.wav")),
            last_position: radiant::gui::types::Point::new(50.0, 50.0),
            modifiers: radiant::widgets::PointerModifiers::default(),
        });

    assert!(
        state.should_paint_app_transient_overlay(),
        "starmap drag feedback should stay on the 60Hz paint-only overlay even before audio playback starts"
    );
    assert!(
        !state.should_paint_waveform_transient_overlay(),
        "waveform overlay predicates should remain waveform/playback-specific"
    );
}

#[test]
fn active_starmap_queue_keeps_app_transient_overlay_active_between_pointer_events() {
    let mut state = gui_state_for_span_tests();
    assert!(!state.should_paint_app_transient_overlay());

    state.ui.chrome.starmap_audition_queue.active_file_id = Some(String::from("/samples/kick.wav"));

    assert!(
        state.should_paint_app_transient_overlay(),
        "starmap active audition feedback should repaint even when the next pointer event has not arrived yet"
    );
}

#[test]
fn early_runtime_playback_handoff_still_uses_paint_only_frames() {
    let mut state = gui_state_for_span_tests();
    crate::native_app::test_support::state::seed_sample_playback_session(
        &mut state,
        String::from("kick.wav"),
        "audio_file",
    );
    state.audio.playback_progress.active = true;
    state.audio.playback_progress.progress = Some(0.25);

    let before = state.capture_frame_surface_inputs();
    state.advance_frame(&mut radiant::prelude::UiUpdateContext::default());

    assert!(
        state.frame_can_use_paint_only_since(before),
        "stable runtime playback handoff frames should not force full surface reprojection"
    );
}

#[test]
fn playback_restart_refreshes_projection_to_clear_stale_playhead_visuals() {
    let mut state = gui_state_for_span_tests();
    state.waveform.current.start_playback(0.62);

    let before = state.frame_surface_revisions();
    let guard = state.begin_frame_surface_revision_tracking();
    state.waveform.current.start_playback(0.12);
    state.finish_frame_surface_revision_tracking(guard);

    assert_eq!(
        state.frame_surface_revisions().repaint_scope_since(before),
        RepaintScope::Projection,
        "retriggering playback should reproject paint state without relayout"
    );
}

#[test]
fn starmap_drag_frame_keeps_audio_handoffs_and_progress_ticks_paint_only() {
    let mut state = gui_state_for_span_tests();
    state.ui.chrome.starmap_audition_drag =
        Some(crate::native_app::app::StarmapAuditionDragState {
            last_hit_file_id: Some(String::from("/samples/kick.wav")),
            last_position: radiant::gui::types::Point::new(50.0, 50.0),
            modifiers: radiant::widgets::PointerModifiers::default(),
        });
    state.waveform.current.start_playback(0.25);

    let before = state.frame_surface_revisions();
    let guard = state.begin_frame_surface_revision_tracking();
    state.waveform.current.stop_playback();
    state.background.progress_tick = 0.5;
    state.finish_frame_surface_revision_tracking(guard);

    assert_eq!(
        state.frame_surface_revisions().repaint_scope_since(before),
        RepaintScope::PaintOnly,
        "starmap drag handoffs should keep frame-clock work on retained paint"
    );
}

#[test]
fn starmap_drag_begin_and_release_frames_stay_paint_only() {
    let mut state = gui_state_for_span_tests();
    let before_begin = state.frame_surface_revisions();
    let begin_guard = state.begin_frame_surface_revision_tracking();
    state.ui.chrome.starmap_audition_drag =
        Some(crate::native_app::app::StarmapAuditionDragState {
            last_hit_file_id: Some(String::from("/samples/kick.wav")),
            last_position: radiant::gui::types::Point::new(50.0, 50.0),
            modifiers: radiant::widgets::PointerModifiers::default(),
        });
    state.waveform.current.start_playback(0.0);
    state.finish_frame_surface_revision_tracking(begin_guard);
    assert_eq!(
        state
            .frame_surface_revisions()
            .repaint_scope_since(before_begin),
        RepaintScope::PaintOnly,
        "the pointer-down frame must not rebuild the scene"
    );

    let before_release = state.frame_surface_revisions();
    let release_guard = state.begin_frame_surface_revision_tracking();
    state.ui.chrome.starmap_audition_drag = None;
    state.finish_frame_surface_revision_tracking(release_guard);
    assert_eq!(
        state
            .frame_surface_revisions()
            .repaint_scope_since(before_release),
        RepaintScope::PaintOnly,
        "the pointer-release frame must not rebuild the scene"
    );
}

#[test]
fn starmap_audition_handoff_stays_paint_only_after_pointer_release() {
    let mut state = gui_state_for_span_tests();
    let request = crate::native_app::app::SamplePlaybackRequest::transient(
        String::from("/samples/kick.wav"),
        crate::native_app::app::SamplePlaybackIntent::StarmapDrag,
        "starmap_drag",
    );
    state.audio.pending_sample_playback = Some(request.clone());

    let before = state.frame_surface_revisions();
    let guard = state.begin_frame_surface_revision_tracking();
    state.background.progress_tick = 0.5;
    state.finish_frame_surface_revision_tracking(guard);

    assert_eq!(
        state.frame_surface_revisions().repaint_scope_since(before),
        RepaintScope::PaintOnly,
        "a queued starmap audition must retain the scene after pointer release"
    );

    state.audio.pending_sample_playback = None;
    state
        .audio
        .start_resolving_sample_playback_session(request, "audio_file");
    state.audio.sample_playback_session.as_mut().unwrap().state =
        crate::native_app::app::SamplePlaybackSessionState::AudibleTransient;
    let before_runtime = state.frame_surface_revisions();
    let runtime_guard = state.begin_frame_surface_revision_tracking();
    state.background.progress_tick = 0.75;
    state.finish_frame_surface_revision_tracking(runtime_guard);

    assert_eq!(
        state
            .frame_surface_revisions()
            .repaint_scope_since(before_runtime),
        RepaintScope::PaintOnly,
        "an audible starmap session must retain the scene even between runtime progress events"
    );
}

#[test]
fn idle_frame_uses_paint_only_when_frame_state_is_stable() {
    let mut state = gui_state_for_span_tests();

    let before = state.capture_frame_surface_inputs();
    state.advance_frame(&mut radiant::prelude::UiUpdateContext::default());

    assert!(
        state.frame_can_use_paint_only_since(before),
        "stable 60Hz idle frames should not force full surface reprojection"
    );
}

#[test]
fn loading_frame_uses_paint_only_when_progress_advances() {
    let mut state = gui_state_for_span_tests();
    state.waveform.load.label = Some(String::from("kick.wav"));
    state.waveform.load.progress = 0.25;
    state.waveform.load.target_progress = 0.8;

    let before = state.capture_frame_surface_inputs();
    state.advance_frame(&mut radiant::prelude::UiUpdateContext::default());

    assert!(
        state.frame_can_use_paint_only_since(before),
        "loading-progress-only frames should not force full surface reprojection"
    );
}

#[test]
fn loading_frame_repaints_surface_when_loading_state_changes() {
    let mut state = gui_state_for_span_tests();

    let before_start = state.capture_frame_surface_inputs();
    state.waveform.load.label = Some(String::from("kick.wav"));
    assert_eq!(
        state.frame_scope_since(before_start),
        RepaintScope::Surface,
        "starting loading changes structural overlay/input state and needs a full repaint"
    );

    let before_stop = state.capture_frame_surface_inputs();
    state.waveform.load.label = None;
    assert_eq!(
        state.frame_scope_since(before_stop),
        RepaintScope::Surface,
        "finishing loading changes structural overlay/input state and needs a full repaint"
    );
}

#[test]
fn source_cache_progress_frame_refreshes_projection_for_status_bar_animation() {
    let mut state = gui_state_for_span_tests();
    state.waveform.cache.active_folder_warm_folder_id = Some(String::from("source"));
    state.waveform.cache.active_folder_warm_total = 10;

    let before = state.capture_frame_surface_inputs();
    state.advance_frame(&mut radiant::prelude::UiUpdateContext::default());

    assert_eq!(
        state.frame_scope_since(before),
        RepaintScope::Projection,
        "source-cache status animation should reproject without relayout"
    );
}

#[test]
fn paused_source_cache_progress_does_not_force_playback_surface_frames() {
    let mut state = gui_state_for_span_tests();
    state.waveform.current.start_playback(0.25);
    state.waveform.cache.start_active_folder_warm_decode_queue(
        String::from("source"),
        vec![std::path::PathBuf::from("kick.wav")],
    );

    state.pause_active_folder_cache_warm_for_playback();

    assert!(
        state.waveform.cache.active_folder_warm_folder_id.is_none(),
        "playback pause should clear dormant source-cache progress"
    );
    assert!(
        state.waveform.cache.active_folder_warm_pending.is_empty(),
        "playback pause should clear pending source-cache work"
    );
    assert_eq!(state.waveform.cache.active_folder_warm_total, 0);

    let before = state.capture_frame_surface_inputs();
    state.advance_frame(&mut radiant::prelude::UiUpdateContext::default());

    assert!(
        state.frame_can_use_paint_only_since(before),
        "paused source-cache progress should not force full surface frames during playback"
    );
}

#[test]
fn copy_flash_frame_refreshes_projection_while_countdown_changes() {
    let (mut state, _source_root, selected_file) =
        native_app_state_with_temp_sample("copy-flash.wav");
    state
        .library
        .folder_browser
        .flash_copied_file_paths([selected_file]);

    let before = state.capture_frame_surface_inputs();
    state.advance_frame(&mut radiant::prelude::UiUpdateContext::default());

    assert_eq!(
        state.frame_scope_since(before),
        RepaintScope::Projection,
        "copy flash should refresh sample-row chrome without recomputing layout"
    );
}

#[test]
fn selection_flash_frame_refreshes_projection_while_countdown_changes() {
    let (mut state, _source_root, selected_file) =
        native_app_state_with_temp_sample("selection-flash.wav");
    state
        .library
        .folder_browser
        .flash_marked_item(selected_file);

    let before = state.capture_frame_surface_inputs();
    state.advance_frame(&mut radiant::prelude::UiUpdateContext::default());

    assert_eq!(
        state.frame_scope_since(before),
        RepaintScope::Projection,
        "selection flash should refresh browser chrome without recomputing layout"
    );
}

#[test]
fn normalization_progress_frame_uses_paint_only_when_progress_is_stable() {
    let mut state = gui_state_for_span_tests();
    state.background.normalization_progress = Some(
        crate::native_app::test_support::state::NormalizationProgress {
            task_id: 9,
            label: String::from("6000 samples"),
            completed: 800,
            total: 6000,
            work_completed: 800_000,
            work_total: 6_000_000,
            queued: 0,
            detail: String::from("kick.wav | Analyzing"),
        },
    );

    let before = state.capture_frame_surface_inputs();
    state.advance_frame(&mut radiant::prelude::UiUpdateContext::default());

    assert!(
        state.frame_can_use_paint_only_since(before),
        "determinate normalization progress should repaint only when progress messages arrive"
    );
}

#[test]
fn playback_frame_repaints_surface_when_playback_state_changes() {
    let mut state = gui_state_for_span_tests();
    state.waveform.current.start_playback(0.25);

    let before = state.frame_surface_revisions();
    let guard = state.begin_frame_surface_revision_tracking();
    state.waveform.current.stop_playback();
    state.finish_frame_surface_revision_tracking(guard);

    assert_eq!(
        state.frame_surface_revisions().repaint_scope_since(before),
        RepaintScope::Surface,
        "stopping playback changes toolbar/status surface state and needs a full repaint"
    );
}

#[test]
fn audio_output_sample_rate_change_requests_layout_refresh() {
    let mut state = gui_state_for_span_tests();
    let before = state.capture_frame_surface_inputs();
    state.audio.output_resolved = Some(crate::native_app::test_support::audio::ResolvedOutput {
        host_id: String::from("core-audio"),
        device_name: String::from("Studio"),
        sample_rate: 48_000,
        buffer_size_frames: None,
        channel_count: 2,
        used_fallback: false,
    });

    assert_eq!(
        state.frame_scope_since(before),
        RepaintScope::Layout,
        "sample-rate label changes should relayout without replacing widget identity"
    );
}

#[test]
fn audio_output_error_repaints_surface_for_top_bar_badge() {
    let mut state = gui_state_for_span_tests();

    let before = state.capture_frame_surface_inputs();
    state.audio.settings_error = Some(String::from(
        "Audio output stream error: output device disconnected",
    ));

    assert_eq!(
        state.frame_scope_since(before),
        RepaintScope::Surface,
        "audio output errors change the top bar badge and need a full repaint"
    );
}

#[test]
fn scene_frame_clock_runs_at_60hz_even_when_idle() {
    let state = gui_state_for_span_tests();
    let bridge = radiant::app(state)
        .view(crate::native_app::test_support::state::view)
        .handle_message(apply_gui_message_for_presentation_test)
        .into_bridge();
    let mut runtime = SurfaceRuntime::new(bridge, Vector2::new(900.0, 620.0));
    apply_strict_update_diagnostics(&mut runtime);

    let activity = runtime.host_animation_activity();

    assert!(activity.needs_frame_message());
    assert_eq!(activity.target_fps(), Some(60));
}

#[test]
fn scene_playback_overlay_and_frame_messages_share_native_cadence() {
    let mut state = gui_state_for_span_tests();
    state.waveform.current.start_playback(0.25);
    let bridge = radiant::app(state)
        .view(crate::native_app::test_support::state::view)
        .handle_message(apply_gui_message_for_presentation_test)
        .into_bridge();
    let mut runtime = SurfaceRuntime::new(bridge, Vector2::new(900.0, 620.0));
    apply_strict_update_diagnostics(&mut runtime);

    let activity = runtime.host_animation_activity();

    assert!(activity.needs_frame_message());
    assert_eq!(
        activity.target_fps(),
        None,
        "playback overlay paint should use the native window cadence"
    );
    assert_eq!(
        activity.frame_message_target_fps(),
        None,
        "playback frame updates should align with the native cursor paint cadence"
    );
}

#[test]
fn scene_frame_clock_queues_gui_frame_message() {
    let mut state = gui_state_for_span_tests();
    state.ui.startup.source_scan_pending = true;
    let messages = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let captured_messages = std::rc::Rc::clone(&messages);
    let bridge = radiant::app(state)
        .view(crate::native_app::test_support::state::view)
        .handle_message(move |state, message, context| {
            captured_messages.borrow_mut().push(message.clone());
            apply_gui_message_for_presentation_test(state, message, context);
        })
        .into_bridge();
    let mut runtime = SurfaceRuntime::new(bridge, Vector2::new(900.0, 620.0));
    apply_strict_update_diagnostics(&mut runtime);

    let activity = runtime.host_animation_activity();

    assert!(activity.needs_frame_message());
    assert!(runtime.host_queue_animation_frame());
    let outcome = runtime.drain_runtime_messages();
    assert_eq!(outcome.messages_dispatched, 1);
    assert_eq!(*messages.borrow(), vec![GuiMessage::Frame]);
}

#[test]
fn scene_playback_frame_uses_paint_only_repaint_scope() {
    let mut state = gui_state_for_span_tests();
    state.waveform.current.start_playback(0.25);
    let bridge = radiant::app(state)
        .view(crate::native_app::test_support::state::view)
        .handle_message(apply_gui_message_for_presentation_test)
        .into_bridge();
    let mut runtime = SurfaceRuntime::new(bridge, Vector2::new(900.0, 620.0));
    apply_strict_update_diagnostics(&mut runtime);
    let broad_observations = NativeAppState::broad_frame_revision_observations();

    assert!(runtime.host_animation_activity().needs_frame_message());
    assert!(runtime.host_queue_animation_frame());
    let outcome = runtime.drain_runtime_messages();

    assert_eq!(outcome.messages_dispatched, 1);
    assert!(outcome.paint_only_requested);
    assert_eq!(
        NativeAppState::broad_frame_revision_observations(),
        broad_observations,
        "steady playback should read the stable revision keys without rebuilding broad inputs"
    );

    assert!(runtime.host_queue_animation_frame());
    let outcome = runtime.drain_runtime_messages();

    assert_eq!(outcome.messages_dispatched, 1);
    assert!(outcome.paint_only_requested);
    assert_eq!(
        NativeAppState::broad_frame_revision_observations(),
        broad_observations,
        "repeated playback frames must remain on the revision fast path"
    );
}

#[test]
fn scene_starmap_drag_frame_uses_paint_only_repaint_scope_during_background_progress() {
    let mut state = gui_state_for_span_tests();
    state.ui.chrome.starmap_audition_drag =
        Some(crate::native_app::app::StarmapAuditionDragState {
            last_hit_file_id: Some(String::from("/samples/kick.wav")),
            last_position: radiant::gui::types::Point::new(50.0, 50.0),
            modifiers: radiant::widgets::PointerModifiers::default(),
        });
    state.waveform.cache.active_folder_warm_folder_id = Some(String::from("source"));
    state.waveform.cache.active_folder_warm_total = 10;
    let bridge = radiant::app(state)
        .view(crate::native_app::test_support::state::view)
        .handle_message(apply_gui_message_for_presentation_test)
        .into_bridge();
    let mut runtime = SurfaceRuntime::new(bridge, Vector2::new(900.0, 620.0));
    apply_strict_update_diagnostics(&mut runtime);

    assert!(runtime.host_animation_activity().needs_frame_message());
    assert!(runtime.host_queue_animation_frame());
    let outcome = runtime.drain_runtime_messages();

    assert_eq!(outcome.messages_dispatched, 1);
    assert!(
        outcome.paint_only_requested,
        "background progress during a starmap drag must retain the existing scene"
    );
    assert!(!outcome.surface_refresh_requested);
}

#[test]
fn scene_source_cache_frame_uses_projection_repaint_scope() {
    let mut state = gui_state_for_span_tests();
    state.waveform.cache.active_folder_warm_folder_id = Some(String::from("source"));
    state.waveform.cache.active_folder_warm_total = 10;
    let bridge = radiant::app(state)
        .view(crate::native_app::test_support::state::view)
        .handle_message(|state, message, _context| {
            if message == GuiMessage::Frame {
                state.advance_frame(&mut radiant::prelude::UiUpdateContext::default());
            }
        })
        .into_bridge();
    let mut runtime = SurfaceRuntime::new(bridge, Vector2::new(900.0, 620.0));
    apply_strict_update_diagnostics(&mut runtime);

    assert!(runtime.host_animation_activity().needs_frame_message());
    assert!(runtime.host_queue_animation_frame());
    let command = runtime
        .bridge_mut()
        .update(crate::native_app::test_support::state::GuiMessage::Frame);

    assert_eq!(command.repaint_scope(), Some(RepaintScope::Projection));
}

#[test]
fn scene_source_processing_frame_uses_paint_only_repaint_scope() {
    let mut state = gui_state_for_span_tests();
    state.background.source_processing_progress = Some(
        crate::native_app::test_support::state::SourceProcessingProgress {
            source_id: String::from("source"),
            lifecycle_generation: 0,
            active: true,
            source_row_active: true,
            completed: 3,
            total: 10,
            stage: String::from("Analyzing audio"),
            detail: String::from("kick.wav"),
        },
    );
    let bridge = radiant::app(state)
        .view(crate::native_app::test_support::state::view)
        .handle_message(|state, message, _context| {
            if message == GuiMessage::Frame {
                state.advance_frame(&mut radiant::prelude::UiUpdateContext::default());
            }
        })
        .into_bridge();
    let mut runtime = SurfaceRuntime::new(bridge, Vector2::new(900.0, 620.0));
    apply_strict_update_diagnostics(&mut runtime);

    assert!(runtime.host_animation_activity().needs_frame_message());
    assert!(runtime.host_queue_animation_frame());
    let command = runtime
        .bridge_mut()
        .update(crate::native_app::test_support::state::GuiMessage::Frame);

    assert_eq!(
        command.repaint_scope(),
        Some(RepaintScope::PaintOnly),
        "source-processing animation must not rebuild the full library projection"
    );
}

#[test]
fn retained_starmap_worker_plan_tracks_source_rail_and_global_anchor_visibility() {
    let (mut state, _source_root, _selected_file) =
        native_app_state_with_temp_sample("retained-worker-anchors.wav");
    state.ui.chrome.sample_browser_display = crate::native_app::app::SampleBrowserDisplayMode::Map;
    crate::native_app::test_support::sample_browser::complete_starmap_layout_for_selected_source(
        &mut state,
    );
    crate::native_app::test_support::sample_browser::prepare_sample_browser_view(&mut state);
    let source_id = state
        .library
        .folder_browser
        .selected_source_id()
        .to_string();
    state.waveform.cache.active_folder_warm_folder_id = None;
    state.waveform.cache.active_folder_warm_total = 0;
    assert!(
        !state.worker_progress_indicator_visible(),
        "fixture must start without another aggregate worker"
    );
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
    let theme = radiant::theme::ThemeTokens::default();
    let mut runtime = native_runtime_for_tests(state, Vector2::new(900.0, 620.0));
    let row_widget_id =
        crate::native_app::app_chrome::library_browser::library_sidebar::source_row_widget_id(
            &source_id,
        );
    let idle_frame = runtime.frame(&theme);
    assert!(
        idle_frame
            .paint_plan
            .first_widget_rect_by_priority([crate::native_app::ui::ids::SAMPLE_BROWSER_MAP_ID])
            .is_some(),
        "retained Starmap must have a real idle map plan before checking worker anchors"
    );
    assert!(
        idle_frame
            .paint_plan
            .first_widget_rect_by_priority([row_widget_id])
            .is_some(),
        "the source row must be present before checking its processing rail"
    );
    assert!(
        idle_frame
            .paint_plan
            .first_widget_rect_by_priority([crate::native_app::ui::ids::WORKER_PROGRESS_ROOT_ID,])
            .is_none(),
        "an idle retained Starmap must not install the global progress anchor"
    );

    let start_command = runtime.dispatch_message(
        crate::native_app::test_support::state::GuiMessage::SourceProcessingProgress(
            crate::native_app::test_support::state::SourceProcessingProgress {
                source_id: source_id.clone(),
                lifecycle_generation: 0,
                active: true,
                source_row_active: true,
                completed: 3,
                total: 10,
                stage: String::from("Analyzing audio"),
                detail: String::from("retained-worker-anchors.wav"),
            },
        ),
    );
    assert_eq!(
        start_command.surface_refresh_scope,
        Some(RepaintScope::Surface),
        "starting the first retained worker must refresh the global anchor topology"
    );

    let active_frame = runtime.frame(&theme);
    assert!(
        active_frame
            .paint_plan
            .first_widget_rect_by_priority([crate::native_app::ui::ids::WORKER_PROGRESS_ROOT_ID,])
            .is_some(),
        "an active source worker must install the global progress anchor"
    );

    let mut active_primitives = Vec::new();
    runtime
        .bridge_mut()
        .state_mut()
        .paint_source_processing_source_pulse(
            TransientOverlayContext::new(
                &active_frame.paint_plan,
                Vector2::new(900.0, 620.0),
                Duration::ZERO,
            ),
            &mut active_primitives,
        );
    assert!(
        !active_primitives.is_empty(),
        "an active source worker must paint its retained source rail"
    );

    let tick_command = runtime.dispatch_message(
        crate::native_app::test_support::state::GuiMessage::SourceProcessingProgress(
            crate::native_app::test_support::state::SourceProcessingProgress {
                source_id: source_id.clone(),
                lifecycle_generation: 0,
                active: true,
                source_row_active: true,
                completed: 4,
                total: 10,
                stage: String::from("Analyzing audio"),
                detail: String::from("retained-worker-anchors-tick.wav"),
            },
        ),
    );
    assert!(
        tick_command.paint_only_requested,
        "steady retained worker ticks must request paint-only repaint"
    );
    assert_eq!(
        tick_command.surface_refresh_scope, None,
        "steady retained worker ticks must reuse the retained plan"
    );
    let tick_frame = runtime.frame(&theme);
    assert!(
        tick_frame
            .paint_plan
            .first_widget_rect_by_priority([crate::native_app::ui::ids::WORKER_PROGRESS_ROOT_ID,])
            .is_some(),
        "a paint-only worker tick must retain the global progress anchor"
    );
    let mut tick_primitives = Vec::new();
    runtime
        .bridge_mut()
        .state_mut()
        .paint_source_processing_source_pulse(
            TransientOverlayContext::new(
                &tick_frame.paint_plan,
                Vector2::new(900.0, 620.0),
                Duration::from_millis(100),
            ),
            &mut tick_primitives,
        );
    assert!(
        !tick_primitives.is_empty(),
        "a paint-only worker tick must retain the source processing rail"
    );

    let finish_command = runtime.dispatch_message(
        crate::native_app::test_support::state::GuiMessage::SourceProcessingProgress(
            crate::native_app::test_support::state::SourceProcessingProgress {
                source_id: source_id.clone(),
                lifecycle_generation: 0,
                active: false,
                source_row_active: false,
                completed: 10,
                total: 10,
                stage: String::from("Finished"),
                detail: String::from("retained-worker-anchors.wav"),
            },
        ),
    );
    assert_eq!(
        finish_command.surface_refresh_scope,
        Some(RepaintScope::Surface),
        "finishing the last retained worker must invalidate its global anchor"
    );
    let finished_frame = runtime.frame(&theme);
    assert!(
        finished_frame
            .paint_plan
            .first_widget_rect_by_priority([crate::native_app::ui::ids::WORKER_PROGRESS_ROOT_ID,])
            .is_none(),
        "finishing the last worker must remove the global progress anchor"
    );
    let mut finished_primitives = Vec::new();
    runtime
        .bridge_mut()
        .state_mut()
        .paint_source_processing_source_pulse(
            TransientOverlayContext::new(
                &finished_frame.paint_plan,
                Vector2::new(900.0, 620.0),
                Duration::ZERO,
            ),
            &mut finished_primitives,
        );
    assert!(
        finished_primitives.is_empty(),
        "finishing the source worker must remove its retained source rail"
    );
}

#[test]
fn worker_progress_indicator_pulses_in_transient_overlay_without_input() {
    let mut state = gui_state_for_span_tests();
    state.background.source_processing_progress = Some(
        crate::native_app::test_support::state::SourceProcessingProgress {
            source_id: String::from("source"),
            lifecycle_generation: 0,
            active: true,
            source_row_active: true,
            completed: 3,
            total: 10,
            stage: String::from("Analyzing audio"),
            detail: String::from("kick.wav"),
        },
    );
    assert!(state.should_paint_app_transient_overlay());
    let theme = radiant::theme::ThemeTokens::default();
    let mut runtime = native_runtime_for_tests(state, Vector2::new(900.0, 620.0));
    let frame = runtime.frame(&theme);

    let pulse = |runtime: &mut NativeRuntimeForTests, animation_time| {
        let mut primitives = Vec::new();
        runtime
            .bridge_mut()
            .state_mut()
            .paint_worker_progress_indicator(
                TransientOverlayContext::new(
                    &frame.paint_plan,
                    Vector2::new(900.0, 620.0),
                    animation_time,
                ),
                &mut primitives,
            );
        primitives
    };

    let initial = pulse(&mut runtime, Duration::ZERO);
    let later = pulse(&mut runtime, Duration::from_millis(250));

    assert_eq!(initial.len(), 1, "worker pulse paints one breathing fill");
    assert_eq!(later.len(), 1, "worker pulse remains one circular fill");
    let path_parts = |primitive: &PaintPrimitive| match primitive {
        PaintPrimitive::FillPath(path) => match path.brush {
            radiant::runtime::PaintBrush::Solid(color) => (path.path.clone(), color.a),
            other => panic!("expected solid circular pulse fill, got {other:?}"),
        },
        other => panic!("expected circular pulse path, got {other:?}"),
    };
    let (initial_path, initial_alpha) = path_parts(&initial[0]);
    let (later_path, later_alpha) = path_parts(&later[0]);
    assert_eq!(
        initial_path, later_path,
        "worker pulse keeps one fixed circle geometry"
    );
    assert_ne!(
        initial_alpha, later_alpha,
        "paint-only worker pulse must vary only its fill intensity"
    );
}

#[test]
fn source_processing_source_pulse_uses_animation_time_not_frame_count() {
    let (mut state, _source_root, _selected_file) =
        native_app_state_with_temp_sample("processing-pulse.wav");
    let source_id = state
        .library
        .folder_browser
        .selected_source_id()
        .to_string();
    state.background.source_processing_progress = Some(
        crate::native_app::test_support::state::SourceProcessingProgress {
            source_id,
            lifecycle_generation: 0,
            active: true,
            source_row_active: true,
            completed: 3,
            total: 10,
            stage: String::from("Analyzing audio"),
            detail: String::from("processing-pulse.wav"),
        },
    );
    let theme = radiant::theme::ThemeTokens::default();
    let mut runtime = native_runtime_for_tests(state, Vector2::new(900.0, 620.0));
    let frame = runtime.frame(&theme);

    let processing_rail = |runtime: &mut NativeRuntimeForTests, animation_time| {
        let mut primitives = Vec::new();
        runtime
            .bridge_mut()
            .state_mut()
            .paint_source_processing_source_pulse(
                TransientOverlayContext::new(
                    &frame.paint_plan,
                    Vector2::new(900.0, 620.0),
                    animation_time,
                ),
                &mut primitives,
            );
        primitives
    };

    let initial = processing_rail(&mut runtime, Duration::ZERO);
    runtime.bridge_mut().state_mut().background.progress_tick = 0.5;
    let after_frame_count_change = processing_rail(&mut runtime, Duration::ZERO);
    let later = processing_rail(&mut runtime, Duration::from_millis(250));

    assert_eq!(
        after_frame_count_change, initial,
        "playback frame cadence must not change the pulse phase"
    );
    assert_eq!(initial.len(), 2, "processing paints a track and gradient");
    assert_eq!(
        initial[0], later[0],
        "the quiet processing track stays fixed beneath the activity segment"
    );
    let gradient = |primitive: &PaintPrimitive| match primitive {
        PaintPrimitive::FillPath(fill) => match fill.brush {
            radiant::runtime::PaintBrush::LinearGradient(gradient) => gradient,
            other => panic!("expected gradient source-processing rail, got {other:?}"),
        },
        other => panic!("expected source-processing gradient path, got {other:?}"),
    };
    let initial_gradient = gradient(&initial[1]);
    assert_eq!(initial_gradient.start_color.a, 0);
    assert!(initial_gradient.end_color.a > initial_gradient.start_color.a);
    assert_eq!(
        later.len(),
        3,
        "the segment splits across both track edges while wrapping"
    );
    assert_ne!(
        gradient(&later[2]).end.x,
        initial_gradient.end.x,
        "the bright head must continue moving from monotonic animation time"
    );
}

#[test]
fn source_processing_gradient_wraps_without_respawning() {
    let track = radiant::prelude::Rect::from_min_max(
        radiant::prelude::Point::new(10.0, 20.0),
        radiant::prelude::Point::new(110.0, 22.0),
    );
    let before_wrap =
        crate::native_app::app_chrome::status_bar::source_processing_gradient_slices_for_tests(
            track, 0.98,
        );
    let after_wrap =
        crate::native_app::app_chrome::status_bar::source_processing_gradient_slices_for_tests(
            track, 0.02,
        );

    assert_eq!(before_wrap.len(), 1);
    assert_eq!(after_wrap.len(), 2);
    assert_eq!(before_wrap[0].1.start_color.a, 0);
    assert_eq!(after_wrap[0].1.start_color.a, 0);
    assert_eq!(before_wrap[0].1.end_color, after_wrap[0].1.end_color);
    assert_eq!(after_wrap[0].0.max.x, track.max.x);
    assert_eq!(after_wrap[1].0.min.x, track.min.x);
}

#[test]
fn manifest_maintenance_does_not_paint_source_row_pulse_overlay() {
    let (mut state, _source_root, _selected_file) =
        native_app_state_with_temp_sample("maintenance.wav");
    let source_id = state
        .library
        .folder_browser
        .selected_source_id()
        .to_string();
    state.background.source_processing_progress = Some(
        crate::native_app::test_support::state::SourceProcessingProgress {
            source_id: source_id.clone(),
            lifecycle_generation: 0,
            active: true,
            source_row_active: false,
            completed: 1,
            total: 1,
            stage: String::from("Scanning source changes"),
            detail: String::from("Checking the source manifest"),
        },
    );
    let theme = radiant::theme::ThemeTokens::default();
    let mut runtime = native_runtime_for_tests(state, Vector2::new(900.0, 620.0));
    let frame = runtime.frame(&theme);
    let row_widget_id =
        crate::native_app::app_chrome::library_browser::library_sidebar::source_row_widget_id(
            &source_id,
        );
    assert!(
        frame
            .paint_plan
            .first_widget_rect_by_priority([row_widget_id])
            .is_some(),
        "the source row must be present so the overlay assertion is not vacuous"
    );
    let mut primitives = Vec::new();

    runtime
        .bridge_mut()
        .state_mut()
        .paint_source_processing_source_pulse(
            TransientOverlayContext::new(
                &frame.paint_plan,
                Vector2::new(900.0, 620.0),
                Duration::ZERO,
            ),
            &mut primitives,
        );

    assert!(
        primitives.is_empty(),
        "manifest maintenance must remain visible in job details without pulsing a source row"
    );
}

#[test]
fn scene_installs_playback_cursor_transient_overlay() {
    let mut state = gui_state_for_span_tests();
    state.waveform.current.start_playback(0.25);
    let theme = radiant::theme::ThemeTokens::default();
    let bridge = radiant::app(state)
        .view(crate::native_app::test_support::state::view)
        .handle_message(apply_gui_message_for_presentation_test)
        .into_bridge();
    let mut runtime = SurfaceRuntime::new(bridge, Vector2::new(900.0, 620.0));
    apply_strict_update_diagnostics(&mut runtime);
    let frame = runtime.frame(&theme);
    let mut primitives = Vec::new();

    assert!(runtime.has_transient_overlay_host());
    runtime.host_paint_transient_overlay(
        TransientOverlayContext::new(
            &frame.paint_plan,
            Vector2::new(900.0, 620.0),
            Duration::ZERO,
        ),
        &mut primitives,
    );

    assert!(
        primitives
            .iter()
            .filter_map(|primitive| primitive.fill_rect())
            .any(is_playback_cursor_fill),
        "root scene should install the paint-only playback cursor overlay"
    );
}

#[test]
fn scene_installs_starmap_active_audition_transient_overlay() {
    let (mut state, _source_root, selected_file) = native_app_state_with_temp_sample("kick.wav");
    state.ui.chrome.sample_browser_display = crate::native_app::app::SampleBrowserDisplayMode::Map;
    crate::native_app::test_support::sample_browser::complete_starmap_layout_for_selected_source(
        &mut state,
    );
    crate::native_app::test_support::sample_browser::prepare_sample_browser_view(&mut state);
    state.ui.chrome.starmap_audition_queue.active_file_id = Some(selected_file);
    assert!(state.active_starmap_audition_file_id().is_some());
    assert!(
        state
            .library
            .folder_browser
            .cached_starmap_projection()
            .is_some()
    );
    let theme = radiant::theme::ThemeTokens::default();
    let bridge = radiant::app(state)
        .view(crate::native_app::test_support::state::view)
        .handle_message(apply_gui_message_for_presentation_test)
        .into_bridge();
    let mut runtime = SurfaceRuntime::new(bridge, Vector2::new(900.0, 620.0));
    apply_strict_update_diagnostics(&mut runtime);
    let frame = runtime.frame(&theme);
    let mut primitives = Vec::new();

    assert!(runtime.has_transient_overlay_host());
    assert!(
        frame
            .paint_plan
            .first_widget_rect_by_priority([crate::native_app::ui::ids::SAMPLE_BROWSER_MAP_ID])
            .is_some()
    );
    runtime.host_paint_transient_overlay(
        TransientOverlayContext::new(
            &frame.paint_plan,
            Vector2::new(900.0, 620.0),
            Duration::ZERO,
        ),
        &mut primitives,
    );

    assert!(
        primitives.iter().any(|primitive| matches!(
            primitive,
            radiant::runtime::PaintPrimitive::FillPolygon(fill)
                if fill.widget_id == crate::native_app::ui::ids::SAMPLE_BROWSER_MAP_ID
                    && fill.color.a == 255
                    && fill.points.len() == 4
        )),
        "root scene should paint the active Starmap audition overlay from app chrome"
    );
}

#[test]
fn shortcut_help_modal_suppresses_waveform_transient_overlay() {
    let mut state = gui_state_for_span_tests();
    state.waveform.current.start_playback(0.25);
    state.ui.chrome.shortcut_help_open = true;
    let bridge = radiant::app(state)
        .view(crate::native_app::test_support::state::view)
        .handle_message(apply_gui_message_for_presentation_test)
        .into_bridge();
    let mut runtime = SurfaceRuntime::new(bridge, Vector2::new(900.0, 620.0));
    apply_strict_update_diagnostics(&mut runtime);
    let theme = radiant::theme::ThemeTokens::default();
    let frame = runtime.frame(&theme);
    let mut primitives = Vec::new();

    runtime.host_paint_transient_overlay(
        TransientOverlayContext::new(
            &frame.paint_plan,
            Vector2::new(900.0, 620.0),
            Duration::ZERO,
        ),
        &mut primitives,
    );
    assert!(
        !primitives
            .iter()
            .filter_map(|primitive| primitive.fill_rect())
            .any(|fill| {
                fill.widget_id == crate::native_app::test_support::waveform::WAVEFORM_WIDGET_ID
                    && fill.color.r == 71
                    && fill.color.g == 220
                    && fill.color.b == 255
            }),
        "shortcut help should keep live playback cursor overlays behind the modal"
    );
}

#[test]
fn waveform_context_menu_suppresses_waveform_transient_overlay() {
    let mut state = gui_state_for_span_tests();
    state.waveform.current.start_playback(0.25);
    state.ui.browser_interaction.waveform_context_menu = Some(
        crate::native_app::test_support::context_menu::WaveformContextMenu {
            anchor: radiant::gui::types::Point::new(240.0, 160.0),
            title: String::from("Playmark Selection"),
            extract_to_harvest_destination: false,
        },
    );
    let bridge = radiant::app(state)
        .view(crate::native_app::test_support::state::view)
        .handle_message(apply_gui_message_for_presentation_test)
        .into_bridge();
    let mut runtime = SurfaceRuntime::new(bridge, Vector2::new(900.0, 620.0));
    apply_strict_update_diagnostics(&mut runtime);
    let theme = radiant::theme::ThemeTokens::default();
    let frame = runtime.frame(&theme);
    let mut primitives = Vec::new();

    runtime.host_paint_transient_overlay(
        TransientOverlayContext::new(
            &frame.paint_plan,
            Vector2::new(900.0, 620.0),
            Duration::ZERO,
        ),
        &mut primitives,
    );
    assert!(
        !primitives
            .iter()
            .filter_map(|primitive| primitive.fill_rect())
            .any(|fill| {
                fill.widget_id == crate::native_app::test_support::waveform::WAVEFORM_WIDGET_ID
                    && fill.color.r == 71
                    && fill.color.g == 220
                    && fill.color.b == 255
            }),
        "waveform context menus should keep live playback cursor overlays behind menu chrome"
    );
}

#[test]
fn waveform_context_menu_layers_above_stopped_playhead_surface_marker() {
    let mut state = gui_state_for_span_tests();
    state.waveform.current.start_playback(0.25);
    state.waveform.current.stop_playback();
    state.waveform.current.set_playhead_ratio(0.25);
    let menu = crate::native_app::test_support::context_menu::WaveformContextMenu {
        anchor: radiant::gui::types::Point::new(240.0, 160.0),
        title: String::from("Playmark Selection"),
        extract_to_harvest_destination: false,
    };
    let menu_rect = crate::native_app::app_chrome::waveform_context_menu::overlay_rect(&menu);
    state.ui.browser_interaction.waveform_context_menu = Some(menu);
    let theme = radiant::theme::ThemeTokens::default();
    let runtime = native_runtime_for_tests(state, Vector2::new(900.0, 620.0));
    let frame = runtime.frame(&theme);
    let stopped_playhead_fills = frame
        .paint_plan
        .fill_rects_for_widget(crate::native_app::test_support::waveform::WAVEFORM_WIDGET_ID)
        .filter(|fill| is_playback_cursor_fill(fill))
        .collect::<Vec<_>>();
    assert!(
        !stopped_playhead_fills.is_empty(),
        "waveform context menus should keep the stopped playhead visible outside menu chrome"
    );
    assert!(
        stopped_playhead_fills
            .iter()
            .any(|fill| rects_overlap(fill.rect, menu_rect)),
        "the context-menu overlay should cover the stopped surface marker by paint order"
    );
}

fn apply_gui_message_for_presentation_test(
    state: &mut NativeAppState,
    message: GuiMessage,
    context: &mut ui::UiUpdateContext<GuiMessage>,
) {
    let frame_message = matches!(message, GuiMessage::Frame);
    state.handle_message(message, context);
    if !frame_message {
        context.request_repaint();
    }
}

fn rects_overlap(a: radiant::gui::types::Rect, b: radiant::gui::types::Rect) -> bool {
    a.min.x < b.max.x && a.max.x > b.min.x && a.min.y < b.max.y && a.max.y > b.min.y
}

#[test]
fn playback_cursor_paints_as_transient_overlay() {
    let mut state = gui_state_for_span_tests();
    state.waveform.current.start_playback(0.25);
    let theme = radiant::theme::ThemeTokens::default();
    let mut runtime = native_runtime_for_tests(state, Vector2::new(900.0, 620.0));
    let frame = runtime.frame(&theme);

    assert!(
        !frame
            .paint_plan
            .fill_rects_for_widget(crate::native_app::test_support::waveform::WAVEFORM_WIDGET_ID)
            .any(is_playback_cursor_fill),
        "live playback cursor should not be baked into the cached surface"
    );

    let mut primitives = Vec::new();
    runtime.bridge_mut().state_mut().paint_playback_overlay(
        TransientOverlayContext::new(
            &frame.paint_plan,
            Vector2::new(900.0, 620.0),
            Duration::ZERO,
        ),
        &mut primitives,
    );

    assert!(
        primitives
            .iter()
            .filter_map(|primitive| primitive.fill_rect())
            .any(is_playback_cursor_fill),
        "paint-only playback overlay should append the live cursor"
    );
}

#[test]
fn playback_cursor_transient_overlay_keeps_subpixel_position() {
    let mut state = gui_state_for_span_tests();
    state.waveform.current.start_playback(0.12345);
    let theme = radiant::theme::ThemeTokens::default();
    let mut runtime = native_runtime_for_tests(state, Vector2::new(900.0, 620.0));
    let frame = runtime.frame(&theme);
    let mut primitives = Vec::new();

    runtime.bridge_mut().state_mut().paint_playback_overlay(
        TransientOverlayContext::new(
            &frame.paint_plan,
            Vector2::new(900.0, 620.0),
            Duration::ZERO,
        ),
        &mut primitives,
    );

    let cursor = primitives
        .iter()
        .filter_map(|primitive| primitive.fill_rect())
        .find(|fill| is_playback_cursor_fill(fill))
        .expect("paint-only playback overlay should append the live cursor");
    assert!(
        cursor.rect.min.x.fract().abs() > 0.001,
        "live playback cursor should keep subpixel positioning instead of snapping to whole pixels"
    );
}

#[test]
fn playback_cursor_overlay_progresses_smoothly_across_timed_frames() {
    let mut state = state_with_runtime_playback(0.10, (0.0, 1.0), false);
    assert_active_playback_frame_is_paint_only(&mut state);
    state
        .audio
        .playback_visual_progress
        .as_mut()
        .expect("visual progress")
        .anchor_at = std::time::Instant::now() - Duration::from_millis(80);
    let theme = radiant::theme::ThemeTokens::default();
    let mut runtime = native_runtime_for_tests(state, Vector2::new(900.0, 620.0));
    let frame = runtime.frame(&theme);
    let base_time = Duration::from_secs(30);

    let delayed_first_paint_x =
        playback_cursor_x_for_frame(&mut runtime, &frame.paint_plan, base_time)
            .expect("first cursor paint after delay");
    runtime
        .bridge_mut()
        .state_mut()
        .audio
        .set_playback_progress(wavecrate::audio::PlaybackRuntimeProgress {
            active: true,
            elapsed: Some(Duration::from_millis(8)),
            looping: false,
            progress: Some(0.105),
            error: None,
        });
    let cursor_xs = [
        delayed_first_paint_x,
        playback_cursor_x_for_frame(
            &mut runtime,
            &frame.paint_plan,
            base_time + Duration::from_millis(16),
        )
        .expect("cursor paint after stale progress snapshot"),
        playback_cursor_x_for_frame(
            &mut runtime,
            &frame.paint_plan,
            base_time + Duration::from_millis(32),
        )
        .expect("cursor paint on next frame"),
        playback_cursor_x_for_frame(
            &mut runtime,
            &frame.paint_plan,
            base_time + Duration::from_millis(48),
        )
        .expect("cursor paint on later frame"),
    ];

    assert!(
        delayed_first_paint_x > waveform_cursor_x_from_ratio(&frame.paint_plan, 0.10) + 35.0,
        "delayed first paint should include unpainted runtime progress before the first overlay frame"
    );
    assert_cursor_xs_monotonic_and_bounded("non-looping playback", &cursor_xs, 24.0);
}

#[test]
fn played_range_rail_grows_on_paint_only_playback_frames() {
    let base_time = Duration::from_secs(30);
    let mut state = state_with_runtime_playback(0.10, (0.0, 1.0), false);
    state
        .audio
        .playback_visual_progress
        .as_mut()
        .expect("visual progress")
        .anchor_animation_time = Some(base_time);
    let theme = radiant::theme::ThemeTokens::default();
    let mut runtime = native_runtime_for_tests(state, Vector2::new(900.0, 620.0));
    let frame = runtime.frame(&theme);

    let first_right = played_range_rail_right_x_for_frame(
        &mut runtime,
        &frame.paint_plan,
        base_time + Duration::from_millis(16),
    )
    .expect("played rail on first paint-only frame");
    let later_right = played_range_rail_right_x_for_frame(
        &mut runtime,
        &frame.paint_plan,
        base_time + Duration::from_millis(64),
    )
    .expect("played rail on later paint-only frame");

    assert!(
        later_right > first_right,
        "played rail should advance with the interpolated playhead without a retained surface rebuild"
    );
}

#[test]
fn playback_cursor_uses_live_playmark_drag_state_for_control_occlusion() {
    let mut state = state_with_runtime_playback(0.40, (0.0, 1.0), false);
    state
        .waveform
        .current
        .apply_interaction(WaveformInteraction::BeginSelection {
            kind: WaveformSelectionKind::Play,
            visible_ratio: 0.20,
        });
    state
        .waveform
        .current
        .apply_interaction(WaveformInteraction::UpdateSelection {
            visible_ratio: 0.60,
        });
    assert_eq!(
        state.waveform.current.play_selection(),
        Some(wavecrate::selection::SelectionRange::new(0.20, 0.60))
    );

    let theme = radiant::theme::ThemeTokens::default();
    let mut runtime = native_runtime_for_tests(state, Vector2::new(900.0, 620.0));
    let frame = runtime.frame(&theme);
    let label_rect = frame
        .paint_plan
        .text_runs()
        .find(|text| text.text.as_str() == "400 ms")
        .map(|text| text.rect)
        .expect("live playmark label");
    let mut primitives = Vec::new();
    runtime.bridge_mut().state_mut().paint_playback_overlay(
        TransientOverlayContext::new(
            &frame.paint_plan,
            Vector2::new(900.0, 620.0),
            Duration::ZERO,
        ),
        &mut primitives,
    );

    let cursor_segments = primitives
        .iter()
        .filter_map(PaintPrimitive::fill_rect)
        .filter(|fill| is_playback_cursor_fill(fill))
        .collect::<Vec<_>>();
    assert_eq!(cursor_segments.len(), 2);
    assert!(cursor_segments.iter().all(|fill| {
        fill.rect.max.x <= label_rect.min.x
            || fill.rect.min.x >= label_rect.max.x
            || fill.rect.max.y <= label_rect.min.y
            || fill.rect.min.y >= label_rect.max.y
    }));
}

#[test]
fn preview_audition_cursor_and_rail_grow_before_full_source_handoff() {
    let mut state = gui_state_for_span_tests();
    let path = state.waveform.current.path().display().to_string();
    crate::native_app::test_support::state::seed_sample_playback_session(
        &mut state,
        path,
        "preview_samples",
    );
    let session = state
        .audio
        .sample_playback_session
        .as_mut()
        .expect("preview session");
    session.state = crate::native_app::app::SamplePlaybackSessionState::AudibleTransient;
    session.audible_started_at = Some(std::time::Instant::now() - Duration::from_millis(20));
    state.audio.playback_progress = wavecrate::audio::PlaybackRuntimeProgress {
        active: true,
        elapsed: Some(Duration::ZERO),
        looping: false,
        progress: Some(0.0),
        error: None,
    };
    let theme = radiant::theme::ThemeTokens::default();
    let mut runtime = native_runtime_for_tests(state, Vector2::new(900.0, 620.0));
    let frame = runtime.frame(&theme);

    let first_right =
        played_range_rail_right_x_for_frame(&mut runtime, &frame.paint_plan, Duration::ZERO)
            .expect("preview rail on first frame");
    runtime
        .bridge_mut()
        .state_mut()
        .audio
        .sample_playback_session
        .as_mut()
        .expect("preview session")
        .audible_started_at = Some(std::time::Instant::now() - Duration::from_millis(80));
    let later_right = played_range_rail_right_x_for_frame(
        &mut runtime,
        &frame.paint_plan,
        Duration::from_millis(16),
    )
    .expect("preview rail on later frame");

    assert!(
        later_right > first_right,
        "preview audition should paint the same growing rail before the full source is ready"
    );
    assert_eq!(
        runtime
            .bridge()
            .state()
            .waveform
            .current
            .played_ranges()
            .first()
            .map(wavecrate::selection::SelectionRange::start),
        Some(0.0)
    );
}

#[test]
fn fallback_player_cursor_overlay_uses_seeded_visual_clock() {
    let base_time = Duration::from_secs(30);
    let mut state = gui_state_for_span_tests();
    state.waveform.current.start_playback(0.20);
    state.audio.current_playback_span = Some((0.0, 1.0));
    state.audio.reset_playback_visual_progress(0.20, false);
    state
        .audio
        .playback_visual_progress
        .as_mut()
        .expect("visual progress")
        .anchor_animation_time = Some(base_time);
    assert!(
        !state.audio.playback_progress.active,
        "fallback/player paint coverage should not rely on runtime progress"
    );
    assert_active_playback_frame_is_paint_only(&mut state);

    let theme = radiant::theme::ThemeTokens::default();
    let mut runtime = native_runtime_for_tests(state, Vector2::new(900.0, 620.0));
    let frame = runtime.frame(&theme);
    let cursor_xs = [
        playback_cursor_x_for_frame(
            &mut runtime,
            &frame.paint_plan,
            base_time + Duration::from_millis(16),
        )
        .expect("fallback cursor paint on first frame"),
        playback_cursor_x_for_frame(
            &mut runtime,
            &frame.paint_plan,
            base_time + Duration::from_millis(32),
        )
        .expect("fallback cursor paint on next frame"),
        playback_cursor_x_for_frame(
            &mut runtime,
            &frame.paint_plan,
            base_time + Duration::from_millis(48),
        )
        .expect("fallback cursor paint on later frame"),
    ];

    assert!(
        cursor_xs[0] > waveform_cursor_x_from_ratio(&frame.paint_plan, 0.20),
        "fallback/player cursor should advance from the seeded visual clock"
    );
    assert_cursor_xs_monotonic_and_bounded("fallback/player playback", &cursor_xs, 24.0);
}

#[test]
fn looped_playback_cursor_overlay_stays_smooth_inside_span() {
    let base_time = Duration::from_secs(30);
    let mut state = state_with_runtime_playback(0.32, (0.25, 0.75), true);
    assert_active_playback_frame_is_paint_only(&mut state);
    {
        let visual_progress = state
            .audio
            .playback_visual_progress
            .as_mut()
            .expect("visual progress");
        visual_progress.anchor_at = std::time::Instant::now() - Duration::from_millis(400);
        visual_progress.anchor_animation_time = Some(base_time);
    }
    let theme = radiant::theme::ThemeTokens::default();
    let mut runtime = native_runtime_for_tests(state, Vector2::new(900.0, 620.0));
    let frame = runtime.frame(&theme);
    let cursor_xs = [
        playback_cursor_x_for_frame(&mut runtime, &frame.paint_plan, base_time)
            .expect("looped cursor paint"),
        playback_cursor_x_for_frame(
            &mut runtime,
            &frame.paint_plan,
            base_time + Duration::from_millis(16),
        )
        .expect("looped cursor paint on next frame"),
        playback_cursor_x_for_frame(
            &mut runtime,
            &frame.paint_plan,
            base_time + Duration::from_millis(32),
        )
        .expect("looped cursor paint on later frame"),
    ];

    assert_cursor_xs_monotonic_and_bounded("looped playback before wrap", &cursor_xs, 24.0);
}

#[test]
fn restarted_playback_cursor_overlay_begins_new_smooth_sequence() {
    let state = state_with_runtime_playback(0.62, (0.0, 1.0), false);
    let theme = radiant::theme::ThemeTokens::default();
    let mut runtime = native_runtime_for_tests(state, Vector2::new(900.0, 620.0));
    let frame = runtime.frame(&theme);
    let base_time = Duration::from_secs(30);

    let old_cursor_x = playback_cursor_x_for_frame(&mut runtime, &frame.paint_plan, base_time)
        .expect("initial cursor paint");
    runtime
        .bridge_mut()
        .state_mut()
        .waveform
        .current
        .stop_playback();
    assert!(
        playback_cursor_x_for_frame(
            &mut runtime,
            &frame.paint_plan,
            base_time + Duration::from_millis(16)
        )
        .is_none(),
        "stopped playback should remove the live cursor overlay instead of retaining stale paint"
    );

    {
        let state = runtime.bridge_mut().state_mut();
        state.waveform.current.start_playback(0.12);
        state.audio.current_playback_span = Some((0.0, 1.0));
        state
            .audio
            .set_started_playback_progress(wavecrate::audio::PlaybackRuntimeProgress {
                active: true,
                elapsed: Some(Duration::ZERO),
                looping: false,
                progress: Some(0.12),
                error: None,
            });
        let visual_progress = state
            .audio
            .playback_visual_progress
            .as_mut()
            .expect("restarted visual progress");
        visual_progress.anchor_at = std::time::Instant::now() - Duration::from_millis(500);
        visual_progress.anchor_animation_time = Some(base_time + Duration::from_millis(32));
    }
    let restarted_cursor_x = playback_cursor_x_for_frame(
        &mut runtime,
        &frame.paint_plan,
        base_time + Duration::from_millis(32),
    )
    .expect("restarted cursor paint");
    let cursor_xs = [
        restarted_cursor_x,
        playback_cursor_x_for_frame(
            &mut runtime,
            &frame.paint_plan,
            base_time + Duration::from_millis(48),
        )
        .expect("restarted cursor paint on next frame"),
        playback_cursor_x_for_frame(
            &mut runtime,
            &frame.paint_plan,
            base_time + Duration::from_millis(64),
        )
        .expect("restarted cursor paint on later frame"),
    ];

    assert!(
        restarted_cursor_x < old_cursor_x,
        "restart should begin at the new playback anchor instead of continuing the old cursor path"
    );
    assert_cursor_xs_monotonic_and_bounded("restarted playback", &cursor_xs, 24.0);
}

#[test]
fn loading_progress_paints_as_transient_overlay() {
    let mut state = gui_state_for_span_tests();
    state.waveform.load.label = Some(String::from("kick.wav"));
    state.waveform.load.progress = 0.5;
    let theme = radiant::theme::ThemeTokens::default();
    let mut runtime = native_runtime_for_tests(state, Vector2::new(900.0, 620.0));
    let frame = runtime.frame(&theme);

    assert!(
        !frame
            .paint_plan
            .fill_rects_for_widget(crate::native_app::test_support::waveform::WAVEFORM_WIDGET_ID)
            .any(|fill| { fill.color.r == 174 && fill.color.g == 178 && fill.color.b == 181 }),
        "live loading progress should not be baked into the cached surface"
    );

    let mut primitives = Vec::new();
    runtime
        .bridge_mut()
        .state_mut()
        .paint_waveform_transient_overlay(
            TransientOverlayContext::new(
                &frame.paint_plan,
                Vector2::new(900.0, 620.0),
                Duration::ZERO,
            ),
            &mut primitives,
        );

    assert!(
        primitives
            .iter()
            .filter_map(|primitive| primitive.fill_rect())
            .any(|fill| {
                fill.widget_id == crate::native_app::test_support::waveform::WAVEFORM_WIDGET_ID
                    && fill.color.r == 174
                    && fill.color.g == 178
                    && fill.color.b == 181
            }),
        "paint-only loading overlay should append the live progress fill"
    );
}

// Capture the actual cyan cursor primitive produced by the transient overlay.
// These helpers keep jitter regressions visible at the same output level users see.
fn state_with_runtime_playback(
    anchor_ratio: f32,
    span: (f32, f32),
    looping: bool,
) -> NativeAppState {
    let mut state = gui_state_for_span_tests();
    state.waveform.current.start_playback(anchor_ratio);
    state.audio.current_playback_span = Some(span);
    state
        .audio
        .set_playback_progress(wavecrate::audio::PlaybackRuntimeProgress {
            active: true,
            elapsed: Some(Duration::ZERO),
            looping,
            progress: Some(anchor_ratio),
            error: None,
        });
    state
}

fn assert_active_playback_frame_is_paint_only(state: &mut NativeAppState) {
    let before = state.capture_frame_surface_inputs();
    state.advance_frame(&mut radiant::prelude::UiUpdateContext::default());
    assert!(
        state.frame_can_use_paint_only_since(before),
        "active playback cursor frames should not require retained scene rebuilds"
    );
}

fn playback_cursor_x_for_frame(
    runtime: &mut NativeRuntimeForTests,
    paint_plan: &radiant::runtime::SurfacePaintPlan,
    animation_time: Duration,
) -> Option<f32> {
    let mut primitives = Vec::new();
    runtime.bridge_mut().state_mut().paint_playback_overlay(
        TransientOverlayContext::new(paint_plan, Vector2::new(900.0, 620.0), animation_time),
        &mut primitives,
    );
    primitives
        .iter()
        .filter_map(|primitive| primitive.fill_rect())
        .find(|fill| is_playback_cursor_fill(fill))
        .map(|fill| fill.rect.center().x)
}

fn played_range_rail_right_x_for_frame(
    runtime: &mut NativeRuntimeForTests,
    paint_plan: &radiant::runtime::SurfacePaintPlan,
    animation_time: Duration,
) -> Option<f32> {
    let mut primitives = Vec::new();
    runtime.bridge_mut().state_mut().paint_playback_overlay(
        TransientOverlayContext::new(paint_plan, Vector2::new(900.0, 620.0), animation_time),
        &mut primitives,
    );
    primitives
        .iter()
        .filter_map(|primitive| primitive.fill_rect())
        .filter(|fill| {
            fill.widget_id == crate::native_app::test_support::waveform::WAVEFORM_WIDGET_ID
                && (fill.color.r, fill.color.g, fill.color.b, fill.color.a) == (98, 102, 106, 255)
        })
        .map(|fill| fill.rect.max.x)
        .max_by(f32::total_cmp)
}

fn waveform_cursor_x_from_ratio(
    paint_plan: &radiant::runtime::SurfacePaintPlan,
    ratio: f32,
) -> f32 {
    let bounds = paint_plan
        .first_widget_rect_by_priority([
            crate::native_app::test_support::waveform::WAVEFORM_SIGNAL_WIDGET_ID,
            crate::native_app::test_support::waveform::WAVEFORM_WIDGET_ID,
        ])
        .expect("waveform bounds");
    bounds.min.x + bounds.width() * ratio
}

fn is_playback_cursor_fill(fill: &radiant::runtime::PaintFillRect) -> bool {
    let accent = crate::native_app::app_chrome::palette::ACCENT;
    fill.widget_id == crate::native_app::test_support::waveform::WAVEFORM_WIDGET_ID
        && fill.color.r == accent.r
        && fill.color.g == accent.g
        && fill.color.b == accent.b
}

fn assert_cursor_xs_monotonic_and_bounded(label: &str, cursor_xs: &[f32], max_delta: f32) {
    let mut advanced = false;
    for window in cursor_xs.windows(2) {
        let previous = window[0];
        let next = window[1];
        let delta = next - previous;
        advanced |= delta > 0.25;
        assert!(
            next + 0.25 >= previous,
            "{label} cursor moved backward: {previous:.3} -> {next:.3}"
        );
        assert!(
            delta <= max_delta,
            "{label} cursor jumped too far in one frame: {previous:.3} -> {next:.3}"
        );
    }
    assert!(
        advanced,
        "{label} cursor never advanced across sampled timed frames: {cursor_xs:?}"
    );
}
