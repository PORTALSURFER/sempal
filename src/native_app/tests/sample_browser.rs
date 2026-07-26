use radiant::{
    gui::list::dense_row_palette_from_style,
    gui::types::{Point, Rect, Rgba8, Vector2},
    prelude::{IntoView, ThemeTokens, WidgetStyle, WidgetTone},
    runtime::{Command, Event, PaintPrimitive, SurfaceFrame, SurfacePaintPlan, UiSurface},
    widgets::{PointerButton, PointerModifiers, Widget, WidgetInput, WidgetOutput},
};
use std::{collections::HashMap, fs, path::PathBuf, sync::Arc};

use super::{
    focus_metadata_tag_input, native_app_state_with_temp_sample, native_runtime_for_tests,
    run_command_for_tests, write_sparse_test_wav_i16, write_test_wav_i16,
};

fn sample_hit_target(
    selected: bool,
    drag_active: bool,
    drag_source: bool,
    cached: bool,
) -> UiSurface<crate::native_app::test_support::state::GuiMessage> {
    crate::native_app::test_support::sample_browser::sample_file_hit_target(
        String::from("sample.wav"),
        selected,
        drag_active,
        drag_source,
        cached,
    )
}

fn sample_hit_target_input(
    target: &mut UiSurface<crate::native_app::test_support::state::GuiMessage>,
    bounds: Rect,
    input: WidgetInput,
) -> Option<WidgetOutput> {
    target.dispatch_widget_input(
        crate::native_app::test_support::sample_browser::SAMPLE_FILE_HIT_TARGET_TEST_ID,
        bounds,
        input,
    )
}

fn sample_hit_target_message(
    target: &UiSurface<crate::native_app::test_support::state::GuiMessage>,
    output: WidgetOutput,
) -> Option<crate::native_app::test_support::state::GuiMessage> {
    target.dispatch_widget_output(
        crate::native_app::test_support::sample_browser::SAMPLE_FILE_HIT_TARGET_TEST_ID,
        output,
    )
}

fn sample_hit_target_plan(
    target: &UiSurface<crate::native_app::test_support::state::GuiMessage>,
    bounds: Rect,
) -> SurfacePaintPlan {
    target
        .find_widget(
            crate::native_app::test_support::sample_browser::SAMPLE_FILE_HIT_TARGET_TEST_ID,
        )
        .expect("sample hit-target widget")
        .widget()
        .paint_plan_with_defaults(bounds)
}

fn sample_hit_target_widget(
    target: &UiSurface<crate::native_app::test_support::state::GuiMessage>,
) -> &dyn Widget {
    target
        .find_widget(
            crate::native_app::test_support::sample_browser::SAMPLE_FILE_HIT_TARGET_TEST_ID,
        )
        .expect("sample hit-target widget")
        .widget()
}

fn sync_sample_hit_target_from_previous(
    current: &mut UiSurface<crate::native_app::test_support::state::GuiMessage>,
    previous: &UiSurface<crate::native_app::test_support::state::GuiMessage>,
) {
    let previous = previous
        .find_widget(
            crate::native_app::test_support::sample_browser::SAMPLE_FILE_HIT_TARGET_TEST_ID,
        )
        .expect("previous sample hit-target widget")
        .widget();
    let current = current
        .find_widget_mut(
            crate::native_app::test_support::sample_browser::SAMPLE_FILE_HIT_TARGET_TEST_ID,
        )
        .expect("current sample hit-target widget")
        .widget_mut();
    current.synchronize_from_previous(previous);
}

fn fill_rects_with_color(frame: &SurfaceFrame, color: Rgba8) -> Vec<Rect> {
    let mut rects = frame
        .paint_plan
        .fill_rects()
        .filter_map(|fill| (fill.color == color).then_some(fill.rect))
        .collect::<Vec<_>>();
    for primitive in &frame.paint_plan.primitives {
        if let PaintPrimitive::FillRectBatch(batch) = primitive
            && batch.color == color
        {
            rects.extend(batch.rects.iter().copied());
        }
    }
    rects
}

fn folder_drop_target_fill() -> Rgba8 {
    dense_row_palette_from_style(
        &ThemeTokens::default(),
        WidgetStyle::subtle(WidgetTone::Accent),
    )
    .active_target
    .expect("sidebar dense-row active target fill")
}

fn text_center(frame: &SurfaceFrame, label: &str) -> Point {
    frame
        .paint_plan
        .text_runs()
        .find(|text| text.text.as_str() == label)
        .map(|text| text.rect.center())
        .unwrap_or_else(|| panic!("{label} should paint"))
}

fn prepare_sample_browser_view(state: &mut crate::native_app::test_support::state::NativeAppState) {
    crate::native_app::test_support::sample_browser::prepare_sample_browser_view(state);
}

fn last_fixed_sample_browser_row_scroll(
    command: Command<crate::native_app::test_support::state::GuiMessage>,
) -> Option<(usize, i32)> {
    match command {
        Command::Batch(commands) => commands
            .into_iter()
            .filter_map(last_fixed_sample_browser_row_scroll)
            .last(),
        Command::ScrollFixedRowIntoView {
            node_id,
            row_index,
            direction,
            ..
        } if node_id == crate::native_app::sample_library::sample_list::SAMPLE_BROWSER_LIST_ID => {
            Some((row_index, direction))
        }
        _ => None,
    }
}

mod column_headers;
mod column_reorder;
mod drag_drop;
mod row_activation;
mod rows;
mod similarity;

#[test]
fn recording_sample_last_played_updates_row_and_persists_source_history() {
    let source_root = tempfile::tempdir().expect("source root");
    let sample_path = source_root.path().join("played.wav");
    fs::write(&sample_path, []).expect("write sample");
    let sample_path_string = sample_path.display().to_string();
    let db = wavecrate::sample_sources::SourceDatabase::open_for_test_fixture_source_write(
        source_root.path(),
    )
    .expect("db");
    db.upsert_file(std::path::Path::new("played.wav"), 0, 1)
        .expect("upsert sample");

    let mut state = crate::native_app::test_support::state::NativeAppStateFixture::default()
        .with_folder_browser(
            crate::native_app::test_support::state::FolderBrowserState::from_sample_sources(&[
                wavecrate::sample_sources::SampleSource::new(source_root.path().to_path_buf()),
            ]),
        )
        .build();
    state
        .library
        .folder_browser
        .select_file(sample_path_string.clone());
    let projection_cache_before = state
        .library
        .folder_browser
        .selected_audio_projection_cache_len_for_tests();
    assert_eq!(
        projection_cache_before, 1,
        "selecting the visible file should lazily warm only the selected projection"
    );
    let mut context = radiant::prelude::UiUpdateContext::default();

    state.record_sample_last_played(sample_path_string.clone(), &mut context);

    assert_eq!(
        state
            .library
            .folder_browser
            .selected_audio_projection_cache_len_for_tests(),
        projection_cache_before,
        "last-played metadata should not invalidate navigation projections unless it changes ordering"
    );
    assert_eq!(
        state
            .library
            .folder_browser
            .selected_audio_files()
            .into_iter()
            .find(|file| file.id == sample_path_string)
            .map(|file| file.modified.as_str()),
        Some("Today")
    );

    let delayed =
        run_first_after(&mut state, context.into_command()).expect("last played delayed command");
    let mut context = radiant::prelude::UiUpdateContext::default();
    state.apply_message(delayed, &mut context);
    let message = run_first_perform(context.into_command()).expect("last played persist command");
    state.apply_message(message, &mut radiant::prelude::UiUpdateContext::default());

    assert!(
        db.last_played_at_for_path(std::path::Path::new("played.wav"))
            .expect("read last played")
            .is_some()
    );
}

#[test]
fn last_played_sort_invalidates_projection_when_history_changes() {
    let source_root = tempfile::tempdir().expect("source root");
    let first_path = source_root.path().join("first.wav");
    let second_path = source_root.path().join("second.wav");
    fs::write(&first_path, []).expect("write first sample");
    fs::write(&second_path, []).expect("write second sample");
    let first_path_string = first_path.display().to_string();

    let mut state = crate::native_app::test_support::state::NativeAppStateFixture::default()
        .with_folder_browser(
            crate::native_app::test_support::state::FolderBrowserState::from_sample_sources(&[
                wavecrate::sample_sources::SampleSource::new(source_root.path().to_path_buf()),
            ]),
        )
        .build();
    state
        .library
        .folder_browser
        .sort_file_column(String::from("modified"));
    let _ = state.library.folder_browser.selected_audio_files();
    assert!(
        state
            .library
            .folder_browser
            .selected_audio_projection_cache_len_for_tests()
            > 0,
        "history-sorted projection should be cached before playback"
    );

    state.record_sample_last_played(
        first_path_string,
        &mut radiant::prelude::UiUpdateContext::default(),
    );

    assert_eq!(
        state
            .library
            .folder_browser
            .selected_audio_projection_cache_len_for_tests(),
        0,
        "history-sorted projection must invalidate when last-played metadata changes ordering"
    );
}

#[test]
fn selecting_missing_sample_prunes_row_without_queueing_load() {
    let (mut state, source_root, selected_file) = native_app_state_with_temp_sample("missing.wav");
    fs::remove_file(source_root.path().join("missing.wav")).expect("remove sample");
    let mut context = radiant::prelude::UiUpdateContext::default();

    state.apply_message(
        crate::native_app::test_support::state::GuiMessage::SelectSampleWithModifiers {
            path: selected_file.clone(),
            modifiers: PointerModifiers::default(),
        },
        &mut context,
    );
    run_command_for_tests(&mut state, context.into_command());

    assert!(
        state
            .library
            .folder_browser
            .selected_audio_files()
            .is_empty(),
        "selecting a missing listed sample should remove it from the visible file list"
    );
    assert_eq!(state.library.folder_browser.selected_file_id(), None);
    assert!(
        state
            .background
            .deferred_sample_load_task
            .active()
            .is_none()
            && state.active_sample_load_task().is_none(),
        "missing sample selection should not queue a foreground load after validation"
    );
    assert!(state.ui.status.sample.contains("Removed missing"));
}

#[test]
fn row_click_selection_validates_without_preview_audition() {
    let source_root = tempfile::tempdir().expect("source root");
    let sample = source_root.path().join("large-row-selection.wav");
    write_sparse_test_wav_i16(&sample, 1, 700);
    let sample_id = sample.display().to_string();
    let mut state = crate::native_app::test_support::state::NativeAppStateFixture::default()
        .with_folder_browser(
            crate::native_app::test_support::state::FolderBrowserState::from_sample_sources(&[
                wavecrate::sample_sources::SampleSource::new(source_root.path().to_path_buf()),
            ]),
        )
        .build();
    let mut context = radiant::prelude::UiUpdateContext::default();

    state.apply_message(
        crate::native_app::test_support::state::GuiMessage::SelectSampleWithModifiers {
            path: sample_id.clone(),
            modifiers: PointerModifiers::default(),
        },
        &mut context,
    );
    let command = context.into_command();

    assert_eq!(
        state.library.folder_browser.selected_file_id(),
        Some(sample_id.as_str()),
        "row selection should update visible focus before validation or decoding finishes"
    );
    assert_eq!(
        command.business_task_priority("gui-preview-audition-decode"),
        None,
        "intentional row selection should not start the tiny preview-head decode"
    );
    assert_eq!(
        command.business_task_priority("gui-sample-load-validate"),
        Some(radiant::prelude::TaskPriority::Interactive),
        "row selection should still validate the path for missing-file recovery"
    );
    assert!(
        state.background.preview_audition_task.active().is_none(),
        "row selection should leave preview audition available only to transient navigation"
    );
    assert!(
        state
            .background
            .sample_load_validation_task
            .active()
            .is_some(),
        "validation should remain active after fast preview scheduling"
    );
}

#[test]
fn rapid_row_click_selection_does_not_queue_preview_auditions() {
    let source_root = tempfile::tempdir().expect("source root");
    let first = source_root.path().join("first-large-selection.wav");
    let second = source_root.path().join("second-large-selection.wav");
    write_sparse_test_wav_i16(&first, 1, 700);
    write_sparse_test_wav_i16(&second, 1, 700);
    let first_id = first.display().to_string();
    let second_id = second.display().to_string();
    let mut state = crate::native_app::test_support::state::NativeAppStateFixture::default()
        .with_folder_browser(
            crate::native_app::test_support::state::FolderBrowserState::from_sample_sources(&[
                wavecrate::sample_sources::SampleSource::new(source_root.path().to_path_buf()),
            ]),
        )
        .build();

    state.apply_message(
        crate::native_app::test_support::state::GuiMessage::SelectSampleWithModifiers {
            path: first_id.clone(),
            modifiers: PointerModifiers::default(),
        },
        &mut radiant::prelude::UiUpdateContext::default(),
    );
    assert!(
        state.background.preview_audition_task.active().is_none(),
        "first intentional row click should not queue preview decode"
    );

    state.apply_message(
        crate::native_app::test_support::state::GuiMessage::SelectSampleWithModifiers {
            path: second_id.clone(),
            modifiers: PointerModifiers::default(),
        },
        &mut radiant::prelude::UiUpdateContext::default(),
    );

    assert_eq!(
        state.library.folder_browser.selected_file_id(),
        Some(second_id.as_str()),
        "rapid row clicks should keep the latest explicit selection"
    );
    assert!(
        state.background.preview_audition_task.active().is_none(),
        "rapid row clicks should not leave a transient preview decode active"
    );
    assert!(
        state
            .background
            .sample_load_validation_task
            .active()
            .is_some(),
        "latest row click should still validate the selected path for full playback"
    );
}

#[test]
fn keyboard_navigation_cold_wav_queues_preview_audition_before_full_load() {
    let source_root = tempfile::tempdir().expect("source root");
    let first = source_root.path().join("a-first-keyboard.wav");
    let second = source_root.path().join("b-second-keyboard.wav");
    write_sparse_test_wav_i16(&first, 1, 700);
    write_sparse_test_wav_i16(&second, 1, 700);
    let first_id = first.display().to_string();
    let second_id = second.display().to_string();
    let mut state = crate::native_app::test_support::state::NativeAppStateFixture::default()
        .with_folder_browser(
            crate::native_app::test_support::state::FolderBrowserState::from_sample_sources(&[
                wavecrate::sample_sources::SampleSource::new(source_root.path().to_path_buf()),
            ]),
        )
        .build();
    state.library.folder_browser.select_file(first_id);
    let mut context = radiant::prelude::UiUpdateContext::default();

    state.apply_message(
        crate::native_app::test_support::state::GuiMessage::NavigateBrowser {
            delta: 1,
            extend: false,
            preserve_selection: false,
        },
        &mut context,
    );
    let command = context.into_command();

    assert_eq!(
        state.library.folder_browser.selected_file_id(),
        Some(second_id.as_str()),
        "keyboard navigation should update selection immediately"
    );
    assert_eq!(
        command.business_task_priority("gui-preview-audition-decode"),
        Some(radiant::prelude::TaskPriority::Interactive),
        "keyboard navigation should start the tiny preview-head decode immediately"
    );
    assert_eq!(
        command.business_task_priority("gui-sample-load"),
        Some(radiant::prelude::TaskPriority::Background),
        "full waveform display should stay behind the fast preview path"
    );
    assert!(
        state.background.preview_audition_task.active().is_some(),
        "preview audition decode should be active after keyboard navigation"
    );
    assert!(
        state
            .background
            .settled_sample_promotion_task
            .active()
            .is_some(),
        "keyboard preview audition should schedule a settled full-playback promotion"
    );
    assert!(
        state.active_sample_load_task().is_some(),
        "keyboard navigation should still queue a background display load"
    );
}

#[test]
fn keyboard_navigation_stops_previous_preview_tail_before_cold_target_is_ready() {
    let source_root = tempfile::tempdir().expect("source root");
    let first = source_root.path().join("a-first-keyboard.wav");
    let second = source_root.path().join("b-second-keyboard.wav");
    write_sparse_test_wav_i16(&first, 1, 700);
    write_sparse_test_wav_i16(&second, 1, 700);
    let first_id = first.display().to_string();
    let second_id = second.display().to_string();
    let mut state = crate::native_app::test_support::state::NativeAppStateFixture::default()
        .with_folder_browser(
            crate::native_app::test_support::state::FolderBrowserState::from_sample_sources(&[
                wavecrate::sample_sources::SampleSource::new(source_root.path().to_path_buf()),
            ]),
        )
        .build();
    state.library.folder_browser.select_file(first_id.clone());
    crate::native_app::test_support::state::seed_sample_playback_session(
        &mut state,
        first_id,
        "preview_samples",
    );
    state.audio.current_playback_span = Some((0.0, 1.0));
    state.audio.playback_progress = wavecrate::audio::PlaybackRuntimeProgress {
        active: true,
        elapsed: Some(std::time::Duration::from_millis(90)),
        looping: false,
        progress: Some(0.25),
        error: None,
    };
    let mut context = radiant::prelude::UiUpdateContext::default();

    state.apply_message(
        crate::native_app::test_support::state::GuiMessage::NavigateBrowser {
            delta: 1,
            extend: false,
            preserve_selection: false,
        },
        &mut context,
    );

    assert_eq!(
        state.library.folder_browser.selected_file_id(),
        Some(second_id.as_str()),
        "keyboard navigation should update selection immediately"
    );
    assert_eq!(
        state.audio.sample_playback_session, None,
        "rapid keyboard/list navigation should stop the previous preview before the cold target is ready"
    );
    assert_eq!(state.audio.current_playback_span, None);
    assert_eq!(
        state.audio.playback_progress,
        wavecrate::audio::PlaybackRuntimeProgress::default(),
        "previous preview playback should not remain visually active while the next preview decodes"
    );
    assert!(
        state.background.preview_audition_task.active().is_some(),
        "the next cold target should still use the shared preview-decode path"
    );
}

#[test]
fn keyboard_navigation_ignores_stale_preview_audition_decode_after_rapid_navigation() {
    let source_root = tempfile::tempdir().expect("source root");
    let first = source_root.path().join("a-first-keyboard.wav");
    let second = source_root.path().join("b-second-keyboard.wav");
    let third = source_root.path().join("c-third-keyboard.wav");
    write_sparse_test_wav_i16(&first, 1, 700);
    write_sparse_test_wav_i16(&second, 1, 700);
    write_sparse_test_wav_i16(&third, 1, 700);
    let first_id = first.display().to_string();
    let second_id = second.display().to_string();
    let third_id = third.display().to_string();
    let mut state = crate::native_app::test_support::state::NativeAppStateFixture::default()
        .with_folder_browser(
            crate::native_app::test_support::state::FolderBrowserState::from_sample_sources(&[
                wavecrate::sample_sources::SampleSource::new(source_root.path().to_path_buf()),
            ]),
        )
        .build();
    state.library.folder_browser.select_file(first_id);

    state.apply_message(
        crate::native_app::test_support::state::GuiMessage::NavigateBrowser {
            delta: 1,
            extend: false,
            preserve_selection: false,
        },
        &mut radiant::prelude::UiUpdateContext::default(),
    );
    let stale_ticket = state
        .background
        .preview_audition_task
        .active()
        .expect("first keyboard navigation should queue preview decode");
    let stale_promotion_ticket = state
        .background
        .settled_sample_promotion_task
        .active()
        .expect("first keyboard navigation should schedule settled promotion");

    state.apply_message(
        crate::native_app::test_support::state::GuiMessage::NavigateBrowser {
            delta: 1,
            extend: false,
            preserve_selection: false,
        },
        &mut radiant::prelude::UiUpdateContext::default(),
    );
    let latest_ticket = state
        .background
        .preview_audition_task
        .active()
        .expect("second keyboard navigation should replace preview decode");
    assert_ne!(
        stale_ticket, latest_ticket,
        "rapid keyboard navigation should replace the active preview decode ticket"
    );
    let latest_promotion_ticket = state
        .background
        .settled_sample_promotion_task
        .active()
        .expect("second keyboard navigation should replace settled promotion");
    assert_ne!(
        stale_promotion_ticket, latest_promotion_ticket,
        "rapid keyboard navigation should coalesce settled promotion to the latest row"
    );

    state.apply_message(
        crate::native_app::test_support::state::GuiMessage::PreviewAuditionDecoded {
            completion: radiant::prelude::TaskCompletion {
                ticket: stale_ticket,
                output: crate::native_app::app::PreviewAuditionResult {
                    path: second_id.clone(),
                    clip: Ok(crate::native_app::waveform::PreviewAuditionClip {
                        path: PathBuf::from(&second_id),
                        source_len: 2048,
                        source_modified: Some(std::time::SystemTime::UNIX_EPOCH),
                        samples: Arc::from([0.25_f32, -0.25, 0.0, 0.125]),
                        sample_rate: 44_100,
                        channels: 1,
                        frames: 4,
                        normalized_gain: 1.0,
                    }),
                },
            },
            started_at: std::time::Instant::now(),
        },
        &mut radiant::prelude::UiUpdateContext::default(),
    );

    assert_eq!(
        state.library.folder_browser.selected_file_id(),
        Some(third_id.as_str()),
        "stale keyboard preview decode completion must not move selection back"
    );
    assert_eq!(
        state.background.preview_audition_task.active(),
        Some(latest_ticket),
        "stale keyboard preview decode completion must not consume the latest decode ticket"
    );
    assert!(
        state
            .waveform
            .cache
            .preview_audition_clip(std::path::Path::new(&second_id))
            .is_none(),
        "stale keyboard preview decode completion must not cache an old target"
    );
    assert_ne!(
        state.audio.active_sample_playback_path(),
        Some(second_id.as_str()),
        "stale keyboard preview decode completion must not start early playback for the old target"
    );
}

#[test]
fn map_mode_keyboard_navigation_centers_newly_selected_sample_node() {
    let source_root = tempfile::tempdir().expect("source root");
    let first = source_root.path().join("a.wav");
    let second = source_root.path().join("b.wav");
    write_test_wav_i16(&first, &[0, 100, -100]);
    write_test_wav_i16(&second, &[0, 120, -120]);
    let first_id = first.display().to_string();
    let second_id = second.display().to_string();
    let mut state = crate::native_app::test_support::state::NativeAppStateFixture::default()
        .with_folder_browser(
            crate::native_app::test_support::state::FolderBrowserState::from_sample_sources(&[
                wavecrate::sample_sources::SampleSource::new(source_root.path().to_path_buf()),
            ]),
        )
        .build();
    state.library.folder_browser.select_file(first_id.clone());
    state.ui.chrome.sample_browser_display = crate::native_app::app::SampleBrowserDisplayMode::Map;
    state.ui.chrome.starmap_viewport.zoom = 4.0;
    state
        .library
        .folder_browser
        .prepare_starmap_layout(&state.metadata.tags_by_file);
    let layout_request = state
        .library
        .folder_browser
        .take_starmap_layout_load_request(&state.metadata.tags_by_file)
        .expect("starmap layout request");
    state
        .library
        .folder_browser
        .apply_starmap_layout_load_result(wavecrate::sample_sources::StarmapLayoutLoadResult {
            signature: layout_request.signature,
            result: Ok(HashMap::from([
                (
                    first_id,
                    wavecrate::sample_sources::StarmapLayoutPoint {
                        x: 0.50,
                        y: 0.35,
                        cluster_id: None,
                    },
                ),
                (
                    second_id.clone(),
                    wavecrate::sample_sources::StarmapLayoutPoint {
                        x: 0.50,
                        y: 0.70,
                        cluster_id: None,
                    },
                ),
            ])),
        });

    state.apply_message(
        crate::native_app::test_support::state::GuiMessage::NavigateBrowser {
            delta: 1,
            extend: false,
            preserve_selection: false,
        },
        &mut radiant::prelude::UiUpdateContext::default(),
    );

    assert_eq!(
        state.library.folder_browser.selected_file_id(),
        Some(second_id.as_str())
    );
    let expected = state
        .library
        .folder_browser
        .selected_starmap_position(
            crate::native_app::sample_library::folder_browser::starmap::StarmapProjection {
                tags_by_file: &state.metadata.tags_by_file,
                instant_audition_sample_paths: &state.waveform.cache.instant_audition_sample_paths,
                preview_audition_sample_paths: state.waveform.cache.preview_audition_sample_paths(),
            },
        )
        .expect("selected sample should have a map position");
    assert_starmap_viewport_reveals(
        state.ui.chrome.starmap_viewport,
        expected,
        "map viewport should reveal the selected node after keyboard navigation",
    );
}

#[test]
fn leaving_starmap_mode_reveals_selected_list_row() {
    let source_root = tempfile::tempdir().expect("source root");
    let first = source_root.path().join("a.wav");
    let second = source_root.path().join("b.wav");
    let third = source_root.path().join("c.wav");
    write_test_wav_i16(&first, &[0, 100, -100]);
    write_test_wav_i16(&second, &[0, 120, -120]);
    write_test_wav_i16(&third, &[0, 140, -140]);
    let third_id = third.display().to_string();
    let mut state = crate::native_app::test_support::state::NativeAppStateFixture::default()
        .with_folder_browser(
            crate::native_app::test_support::state::FolderBrowserState::from_sample_sources(&[
                wavecrate::sample_sources::SampleSource::new(source_root.path().to_path_buf()),
            ]),
        )
        .build();
    state.library.folder_browser.select_file(third_id.clone());
    state.ui.chrome.sample_browser_display = crate::native_app::app::SampleBrowserDisplayMode::Map;
    let mut context = radiant::prelude::UiUpdateContext::default();

    state.apply_message(
        crate::native_app::test_support::state::GuiMessage::ToggleSampleBrowserMapView,
        &mut context,
    );

    assert_eq!(
        state.ui.chrome.sample_browser_display,
        crate::native_app::app::SampleBrowserDisplayMode::List
    );
    assert_eq!(
        state.library.folder_browser.selected_file_id(),
        Some(third_id.as_str())
    );
    assert_eq!(
        last_fixed_sample_browser_row_scroll(context.into_command()),
        Some((2, 0)),
        "switching back to list mode should reveal the selected row"
    );
}

#[test]
fn leaving_starmap_mode_materializes_selected_list_row_immediately() {
    let source_root = tempfile::tempdir().expect("source root");
    let files = (0..120)
        .map(|index| source_root.path().join(format!("sample_{index:03}.wav")))
        .collect::<Vec<_>>();
    for file in &files {
        write_test_wav_i16(file, &[0, 100, -100]);
    }
    let selected_index = 96;
    let selected_id = files[selected_index].display().to_string();
    let mut state = crate::native_app::test_support::state::NativeAppStateFixture::default()
        .with_folder_browser(
            crate::native_app::test_support::state::FolderBrowserState::from_sample_sources(&[
                wavecrate::sample_sources::SampleSource::new(source_root.path().to_path_buf()),
            ]),
        )
        .build();
    state
        .library
        .folder_browser
        .select_file(selected_id.clone());
    state.ui.chrome.sample_browser_display = crate::native_app::app::SampleBrowserDisplayMode::Map;
    let mut context = radiant::prelude::UiUpdateContext::default();

    state.apply_message(
        crate::native_app::test_support::state::GuiMessage::ToggleSampleBrowserMapView,
        &mut context,
    );

    assert_eq!(
        state.ui.chrome.sample_browser_display,
        crate::native_app::app::SampleBrowserDisplayMode::List
    );
    assert_eq!(
        state.library.folder_browser.selected_file_id(),
        Some(selected_id.as_str())
    );
    assert_eq!(
        last_fixed_sample_browser_row_scroll(context.into_command()),
        Some((selected_index, 0))
    );
    let window_start = state.library.folder_browser.file_view_start();
    assert!(
        window_start > 0 && window_start <= selected_index,
        "list window should be prepared around the selected sample immediately, start={}",
        window_start
    );
}

#[test]
fn map_audition_selection_is_revealed_when_returning_to_list_mode() {
    let source_root = tempfile::tempdir().expect("source root");
    let first = source_root.path().join("a.wav");
    let second = source_root.path().join("b.wav");
    let third = source_root.path().join("c.wav");
    write_test_wav_i16(&first, &[0, 100, -100]);
    write_test_wav_i16(&second, &[0, 120, -120]);
    write_test_wav_i16(&third, &[0, 140, -140]);
    let third_id = third.display().to_string();
    let mut state = crate::native_app::test_support::state::NativeAppStateFixture::default()
        .with_folder_browser(
            crate::native_app::test_support::state::FolderBrowserState::from_sample_sources(&[
                wavecrate::sample_sources::SampleSource::new(source_root.path().to_path_buf()),
            ]),
        )
        .build();
    state.ui.chrome.sample_browser_display = crate::native_app::app::SampleBrowserDisplayMode::Map;

    state.apply_message(
        crate::native_app::test_support::state::GuiMessage::BeginStarmapAuditionDrag {
            path: Some(third_id.clone()),
            position: Point::new(10.0, 10.0),
            modifiers: PointerModifiers::default(),
        },
        &mut radiant::prelude::UiUpdateContext::default(),
    );
    let mut context = radiant::prelude::UiUpdateContext::default();
    state.apply_message(
        crate::native_app::test_support::state::GuiMessage::ToggleSampleBrowserMapView,
        &mut context,
    );

    assert_eq!(
        state.ui.chrome.sample_browser_display,
        crate::native_app::app::SampleBrowserDisplayMode::List
    );
    assert_eq!(
        state.library.folder_browser.selected_file_id(),
        Some(third_id.as_str())
    );
    assert_eq!(
        last_fixed_sample_browser_row_scroll(context.into_command()),
        Some((2, 0)),
        "list mode should reveal the node selected by starmap audition"
    );
}

#[test]
fn copying_sample_selected_from_map_flashes_map_node_and_waveform() {
    let source_root = tempfile::tempdir().expect("source root");
    let first = source_root.path().join("a.wav");
    let second = source_root.path().join("b.wav");
    write_test_wav_i16(&first, &[0, 100, -100]);
    write_test_wav_i16(&second, &[0, 120, -120]);
    let first_id = first.display().to_string();
    let second_id = second.display().to_string();
    let mut state = crate::native_app::test_support::state::NativeAppStateFixture::default()
        .with_folder_browser(
            crate::native_app::test_support::state::FolderBrowserState::from_sample_sources(&[
                wavecrate::sample_sources::SampleSource::new(source_root.path().to_path_buf()),
            ]),
        )
        .build();
    state.library.folder_browser.select_file(first_id.clone());
    state.ui.chrome.sample_browser_display = crate::native_app::app::SampleBrowserDisplayMode::Map;
    prepare_sample_browser_view(&mut state);
    let layout_request = state
        .library
        .folder_browser
        .take_starmap_layout_load_request(&state.metadata.tags_by_file)
        .expect("starmap layout request");
    state
        .library
        .folder_browser
        .apply_starmap_layout_load_result(wavecrate::sample_sources::StarmapLayoutLoadResult {
            signature: layout_request.signature,
            result: Ok(HashMap::from([
                (
                    first_id,
                    wavecrate::sample_sources::StarmapLayoutPoint {
                        x: 0.35,
                        y: 0.50,
                        cluster_id: None,
                    },
                ),
                (
                    second_id.clone(),
                    wavecrate::sample_sources::StarmapLayoutPoint {
                        x: 0.65,
                        y: 0.50,
                        cluster_id: None,
                    },
                ),
            ])),
        });
    prepare_sample_browser_view(&mut state);

    state.apply_message(
        crate::native_app::test_support::state::GuiMessage::BeginStarmapAuditionDrag {
            path: Some(second_id.clone()),
            position: Point::new(10.0, 10.0),
            modifiers: PointerModifiers::default(),
        },
        &mut radiant::prelude::UiUpdateContext::default(),
    );
    assert_eq!(
        state.library.folder_browser.selected_file_paths(),
        [std::path::PathBuf::from(&second_id)]
    );
    state.copy_selected_files(&mut radiant::prelude::UiUpdateContext::default());
    assert!(state.library.folder_browser.copy_flash_active());

    assert_eq!(
        state.library.folder_browser.selected_file_id(),
        Some(second_id.as_str()),
        "copy feedback must not disturb map-driven selection"
    );
    assert!(
        state.waveform.current.copy_flash_frames() > 0,
        "copying the map-selected sample should flash the waveform"
    );
    let items = state.library.folder_browser.starmap_projection(
        crate::native_app::sample_library::folder_browser::starmap::StarmapProjection {
            tags_by_file: &state.metadata.tags_by_file,
            instant_audition_sample_paths: &state.waveform.cache.instant_audition_sample_paths,
            preview_audition_sample_paths: state.waveform.cache.preview_audition_sample_paths(),
        },
    );
    let copied = items
        .iter()
        .find(|item| item.file_id == second_id)
        .unwrap_or_else(|| {
            panic!(
                "map-selected sample should remain projected: {:?}",
                items
                    .iter()
                    .map(|item| item.file_id.as_str())
                    .collect::<Vec<_>>()
            )
        });
    assert!(copied.selected, "copied map node should remain selected");
    assert!(
        copied.copy_flash,
        "copying the map-selected sample should flash the selected map node"
    );
}

#[test]
fn starmap_drag_sweep_retargets_to_latest_hit_without_backlog() {
    let source_root = tempfile::tempdir().expect("source root");
    let first = source_root.path().join("a.wav");
    let second = source_root.path().join("b.wav");
    let third = source_root.path().join("c.wav");
    write_test_wav_i16(&first, &[0, 100, -100]);
    write_test_wav_i16(&second, &[0, 120, -120]);
    write_test_wav_i16(&third, &[0, 140, -140]);
    let first_id = first.display().to_string();
    let second_id = second.display().to_string();
    let third_id = third.display().to_string();
    let mut state = crate::native_app::test_support::state::NativeAppStateFixture::default()
        .with_folder_browser(
            crate::native_app::test_support::state::FolderBrowserState::from_sample_sources(&[
                wavecrate::sample_sources::SampleSource::new(source_root.path().to_path_buf()),
            ]),
        )
        .build();
    let mut context = radiant::prelude::UiUpdateContext::default();

    state.apply_message(
        crate::native_app::test_support::state::GuiMessage::BeginStarmapAuditionDrag {
            path: Some(first_id.clone()),
            position: Point::new(10.0, 10.0),
            modifiers: PointerModifiers::default(),
        },
        &mut context,
    );
    state.apply_message(
        crate::native_app::test_support::state::GuiMessage::UpdateStarmapAuditionDrag {
            paths: vec![second_id.clone(), third_id.clone()],
            position: Point::new(90.0, 10.0),
            modifiers: PointerModifiers::default(),
        },
        &mut context,
    );

    assert_eq!(
        state.library.folder_browser.selected_file_id(),
        Some(third_id.as_str()),
        "the latest crossed drag hit should become the immediate playback target"
    );
    assert_eq!(
        state
            .ui
            .chrome
            .starmap_audition_queue
            .active_file_id
            .as_deref(),
        Some(third_id.as_str())
    );
    assert_eq!(
        state
            .ui
            .chrome
            .starmap_audition_queue
            .queued_file_ids
            .iter()
            .cloned()
            .collect::<Vec<_>>(),
        Vec::<String>::new(),
        "drag sweeps should not create a delayed playback backlog"
    );
}

#[test]
fn starmap_drag_hot_sweep_starts_latest_without_ready_tail() {
    let source_root = tempfile::tempdir().expect("source root");
    let first = source_root.path().join("a.wav");
    let second = source_root.path().join("b.wav");
    let third = source_root.path().join("c.wav");
    write_test_wav_i16(&first, &[0, 100, -100]);
    write_test_wav_i16(&second, &[0, 120, -120]);
    write_test_wav_i16(&third, &[0, 140, -140]);
    let first_id = first.display().to_string();
    let second_id = second.display().to_string();
    let third_id = third.display().to_string();
    let mut state = crate::native_app::test_support::state::NativeAppStateFixture::default()
        .with_folder_browser(
            crate::native_app::test_support::state::FolderBrowserState::from_sample_sources(&[
                wavecrate::sample_sources::SampleSource::new(source_root.path().to_path_buf()),
            ]),
        )
        .build();
    state.waveform.cache.store_preview_audition_clip(
        crate::native_app::waveform::PreviewAuditionClip {
            path: PathBuf::from(&second_id),
            source_len: 128,
            source_modified: Some(std::time::SystemTime::UNIX_EPOCH),
            samples: Arc::from([0.25_f32, -0.25, 0.0, 0.125]),
            sample_rate: 44_100,
            channels: 1,
            frames: 4,
            normalized_gain: 1.0,
        },
    );
    let mut context = radiant::prelude::UiUpdateContext::default();

    state.apply_message(
        crate::native_app::test_support::state::GuiMessage::BeginStarmapAuditionDrag {
            path: Some(first_id),
            position: Point::new(10.0, 10.0),
            modifiers: PointerModifiers::default(),
        },
        &mut context,
    );
    state.apply_message(
        crate::native_app::test_support::state::GuiMessage::UpdateStarmapAuditionDrag {
            paths: vec![second_id.clone(), third_id.clone()],
            position: Point::new(90.0, 10.0),
            modifiers: PointerModifiers::default(),
        },
        &mut context,
    );

    assert_eq!(
        state.library.folder_browser.selected_file_id(),
        Some(third_id.as_str()),
        "hot intermediate drag hits must not delay the latest pointer target"
    );
    assert_eq!(
        state
            .ui
            .chrome
            .starmap_audition_queue
            .active_file_id
            .as_deref(),
        Some(third_id.as_str())
    );
    assert_eq!(
        state
            .ui
            .chrome
            .starmap_audition_queue
            .queued_file_ids
            .iter()
            .cloned()
            .collect::<Vec<_>>(),
        Vec::<String>::new(),
        "hot intermediate hits should not leave a ready-tail backlog"
    );
}

#[test]
fn starmap_drag_ready_descriptor_skips_foreground_sample_load_validation() {
    let source_root = tempfile::tempdir().expect("source root");
    let sample = source_root.path().join("large-ready.wav");
    write_sparse_test_wav_i16(&sample, 1, 700);
    let sample_id = sample.display().to_string();
    let cache_file = crate::native_app::waveform::PersistedPlaybackCacheFile::new(
        sample.with_extension("f32"),
        700,
    )
    .expect("playback cache file");
    let descriptor = crate::native_app::waveform::PersistedPlaybackDescriptor::new(
        sample.clone(),
        cache_file,
        48_000,
        1,
        700,
    )
    .expect("persisted playback descriptor");
    let mut state = crate::native_app::test_support::state::NativeAppStateFixture::default()
        .with_folder_browser(
            crate::native_app::test_support::state::FolderBrowserState::from_sample_sources(&[
                wavecrate::sample_sources::SampleSource::new(source_root.path().to_path_buf()),
            ]),
        )
        .build();
    state
        .waveform
        .cache
        .mark_sample_playback_descriptor_ready(descriptor);
    let mut context = radiant::prelude::UiUpdateContext::default();

    state.apply_message(
        crate::native_app::test_support::state::GuiMessage::BeginStarmapAuditionDrag {
            path: Some(sample_id.clone()),
            position: Point::new(10.0, 10.0),
            modifiers: PointerModifiers::default(),
        },
        &mut context,
    );
    let command = context.into_command();

    assert_eq!(
        state.library.folder_browser.selected_file_id(),
        Some(sample_id.as_str())
    );
    assert_eq!(
        state
            .ui
            .chrome
            .starmap_audition_queue
            .active_file_id
            .as_deref(),
        Some(sample_id.as_str())
    );
    assert_eq!(
        command.business_task_priority("gui-sample-load-validate"),
        None,
        "playback-ready starmap drag targets should not enter the foreground sample-load path"
    );
}

#[test]
fn starmap_drag_cold_wav_queues_preview_audition_not_sample_load_validation() {
    let source_root = tempfile::tempdir().expect("source root");
    let sample = source_root.path().join("large-cold.wav");
    write_sparse_test_wav_i16(&sample, 1, 700);
    let sample_id = sample.display().to_string();
    let mut state = crate::native_app::test_support::state::NativeAppStateFixture::default()
        .with_folder_browser(
            crate::native_app::test_support::state::FolderBrowserState::from_sample_sources(&[
                wavecrate::sample_sources::SampleSource::new(source_root.path().to_path_buf()),
            ]),
        )
        .build();
    let mut context = radiant::prelude::UiUpdateContext::default();

    state.apply_message(
        crate::native_app::test_support::state::GuiMessage::BeginStarmapAuditionDrag {
            path: Some(sample_id.clone()),
            position: Point::new(10.0, 10.0),
            modifiers: PointerModifiers::default(),
        },
        &mut context,
    );
    let command = context.into_command();

    assert_eq!(
        state.library.folder_browser.selected_file_id(),
        Some(sample_id.as_str())
    );
    assert_eq!(
        command.business_task_priority("gui-sample-load-validate"),
        None,
        "cold starmap drag audition should not need the normal foreground load path"
    );
    assert_eq!(
        command.business_task_priority("gui-preview-audition-decode"),
        Some(radiant::prelude::TaskPriority::Interactive),
        "cold starmap drag WAV targets should decode only a tiny preview head"
    );
}

#[test]
fn starmap_fast_audition_cancels_replaced_foreground_load() {
    let source_root = tempfile::tempdir().expect("source root");
    let first = source_root.path().join("first.wav");
    let second = source_root.path().join("second.wav");
    write_sparse_test_wav_i16(&first, 1, 700);
    write_sparse_test_wav_i16(&second, 1, 700);
    let first_id = first.display().to_string();
    let second_id = second.display().to_string();
    let mut state = crate::native_app::test_support::state::NativeAppStateFixture::default()
        .with_folder_browser(
            crate::native_app::test_support::state::FolderBrowserState::from_sample_sources(&[
                wavecrate::sample_sources::SampleSource::new(source_root.path().to_path_buf()),
            ]),
        )
        .build();
    let mut context = radiant::prelude::UiUpdateContext::default();
    state.library.folder_browser.select_file(first_id.clone());
    state.load_navigation_sample(first_id.clone(), &mut context);
    assert!(
        state.active_sample_load_task().is_some(),
        "initial cold navigation should have a foreground sample load to cancel"
    );
    assert_eq!(
        state.waveform.load.selection.selected_path.as_deref(),
        Some(first_id.as_str())
    );
    assert!(
        state.background.preview_audition_task.active().is_some(),
        "initial cold navigation should have a stale preview decode to cancel"
    );

    state.library.folder_browser.select_file(second_id.clone());
    state.ui.chrome.starmap_audition_drag =
        Some(crate::native_app::app::StarmapAuditionDragState {
            last_hit_file_id: Some(second_id.clone()),
            last_position: Point::new(10.0, 10.0),
            modifiers: PointerModifiers::default(),
        });
    state.cancel_replaced_starmap_foreground_load_for_fast_audition(
        second_id.as_str(),
        "starmap_drag",
    );

    assert_eq!(
        state.active_sample_load_task(),
        None,
        "starmap fast audition should cancel the replaced full-load worker"
    );
    assert_eq!(
        state.background.preview_audition_task.active(),
        None,
        "starmap fast audition should cancel stale preview-decode work"
    );
    assert_eq!(
        state.waveform.load.selection.selected_path.as_deref(),
        None,
        "replaced foreground load selection must not be able to complete later"
    );
}

#[test]
fn starmap_mode_frame_warms_preview_audition_heads() {
    let source_root = tempfile::tempdir().expect("source root");
    let sample = source_root.path().join("large-cold.wav");
    write_sparse_test_wav_i16(&sample, 1, 700);
    let mut state = crate::native_app::test_support::state::NativeAppStateFixture::default()
        .with_folder_browser(
            crate::native_app::test_support::state::FolderBrowserState::from_sample_sources(&[
                wavecrate::sample_sources::SampleSource::new(source_root.path().to_path_buf()),
            ]),
        )
        .build();
    state.ui.chrome.sample_browser_display = crate::native_app::app::SampleBrowserDisplayMode::Map;
    crate::native_app::test_support::sample_browser::complete_starmap_layout(
        &mut state,
        HashMap::from([(
            sample.display().to_string(),
            wavecrate::sample_sources::StarmapLayoutPoint {
                x: 0.50,
                y: 0.50,
                cluster_id: None,
            },
        )]),
    );
    prepare_sample_browser_view(&mut state);
    let sample_id = sample.display().to_string();
    assert!(
        crate::native_app::waveform::should_use_file_backed_wav_decode(&sample),
        "fixture should exercise the large/long WAV path instead of the ordinary in-memory cache path"
    );
    let item = state
        .library
        .folder_browser
        .cached_starmap_projection()
        .and_then(|items| items.iter().find(|item| item.file_id == sample_id).cloned())
        .expect("long wav should remain present in starmap projection");
    assert!(
        !item.instant_audition_ready,
        "long WAVs should not be treated as full-cache instant-ready before cache warm"
    );
    assert!(
        item.preview_audition_candidate,
        "long WAVs should still use the tiny preview-head audition path"
    );
    let mut context = radiant::prelude::UiUpdateContext::default();

    state.apply_message(
        crate::native_app::test_support::state::GuiMessage::Frame,
        &mut context,
    );
    let command = context.into_command();

    assert_eq!(
        command.business_task_priority("gui-preview-audition-warm"),
        Some(radiant::prelude::TaskPriority::Background),
        "starmap mode should warm tiny preview heads before drag playback needs them"
    );
    assert!(
        state
            .background
            .preview_audition_warm_task
            .active()
            .is_some(),
        "preview audition warm should be tracked as cancellable background work"
    );
    assert!(
        state
            .waveform
            .cache
            .preview_audition_scheduled_paths()
            .contains(&sample_id),
        "starmap preview warm should schedule long WAVs instead of filtering them out"
    );
}

#[test]
fn starmap_drag_begin_cancels_active_preview_audition_warm() {
    let source_root = tempfile::tempdir().expect("source root");
    let sample = source_root.path().join("large-cold.wav");
    write_sparse_test_wav_i16(&sample, 1, 700);
    let sample_id = sample.display().to_string();
    let mut state = crate::native_app::test_support::state::NativeAppStateFixture::default()
        .with_folder_browser(
            crate::native_app::test_support::state::FolderBrowserState::from_sample_sources(&[
                wavecrate::sample_sources::SampleSource::new(source_root.path().to_path_buf()),
            ]),
        )
        .build();
    state.ui.chrome.sample_browser_display = crate::native_app::app::SampleBrowserDisplayMode::Map;
    crate::native_app::test_support::sample_browser::complete_starmap_layout(
        &mut state,
        HashMap::from([(
            sample_id.clone(),
            wavecrate::sample_sources::StarmapLayoutPoint {
                x: 0.50,
                y: 0.50,
                cluster_id: None,
            },
        )]),
    );
    prepare_sample_browser_view(&mut state);
    let mut context = radiant::prelude::UiUpdateContext::default();
    state.apply_message(
        crate::native_app::test_support::state::GuiMessage::Frame,
        &mut context,
    );
    assert!(
        state
            .background
            .preview_audition_warm_task
            .active()
            .is_some(),
        "test setup should start preview warming"
    );
    assert!(
        !state
            .waveform
            .cache
            .preview_audition_scheduled_paths()
            .is_empty(),
        "test setup should mark preview-warm paths scheduled"
    );

    state.apply_message(
        crate::native_app::test_support::state::GuiMessage::BeginStarmapAuditionDrag {
            path: Some(sample_id),
            position: Point::new(10.0, 10.0),
            modifiers: PointerModifiers::default(),
        },
        &mut context,
    );

    assert_eq!(
        state.background.preview_audition_warm_task.active(),
        None,
        "drag playback should immediately cancel background preview warming"
    );
    assert!(
        state
            .waveform
            .cache
            .preview_audition_scheduled_paths()
            .is_empty(),
        "cancelled preview warming must release scheduled paths instead of sparkling forever"
    );
}

#[test]
fn list_mode_frame_warms_preview_audition_heads_near_selected_sample() {
    let source_root = tempfile::tempdir().expect("source root");
    let mut paths = Vec::new();
    for index in 0..36 {
        let path = source_root.path().join(format!("{index:02}.wav"));
        fs::write(&path, []).expect("write sample");
        paths.push(path.display().to_string());
    }
    let selected = paths[30].clone();
    let first_visible = paths[0].clone();
    let next_selected_neighbor = paths[31].clone();
    let mut state = crate::native_app::test_support::state::NativeAppStateFixture::default()
        .with_folder_browser(
            crate::native_app::test_support::state::FolderBrowserState::from_sample_sources(&[
                wavecrate::sample_sources::SampleSource::new(source_root.path().to_path_buf()),
            ]),
        )
        .build();
    state.ui.chrome.sample_browser_display = crate::native_app::app::SampleBrowserDisplayMode::List;
    state.library.folder_browser.select_file(selected.clone());
    prepare_sample_browser_view(&mut state);
    let mut context = radiant::prelude::UiUpdateContext::default();

    state.apply_message(
        crate::native_app::test_support::state::GuiMessage::Frame,
        &mut context,
    );
    let command = context.into_command();
    let scheduled = state.waveform.cache.preview_audition_scheduled_paths();

    assert_eq!(
        command.business_task_priority("gui-preview-audition-warm"),
        Some(radiant::prelude::TaskPriority::Background),
        "list mode should warm tiny preview heads for rapid navigation"
    );
    assert!(
        scheduled.contains(&selected),
        "list preview warming should prioritize the selected sample"
    );
    assert!(
        scheduled.contains(&next_selected_neighbor),
        "list preview warming should include the next keyboard-navigation target"
    );
    assert!(
        !scheduled.contains(&first_visible),
        "list preview warming should not spend the first batch on far-away visible rows"
    );
}

#[test]
fn starmap_mode_frame_does_not_duplicate_scheduled_preview_audition_heads() {
    let source_root = tempfile::tempdir().expect("source root");
    let sample = source_root.path().join("large-cold.wav");
    write_sparse_test_wav_i16(&sample, 1, 700);
    let sample_id = sample.display().to_string();
    let mut state = crate::native_app::test_support::state::NativeAppStateFixture::default()
        .with_folder_browser(
            crate::native_app::test_support::state::FolderBrowserState::from_sample_sources(&[
                wavecrate::sample_sources::SampleSource::new(source_root.path().to_path_buf()),
            ]),
        )
        .build();
    state.ui.chrome.sample_browser_display = crate::native_app::app::SampleBrowserDisplayMode::Map;
    prepare_sample_browser_view(&mut state);
    state
        .waveform
        .cache
        .mark_preview_audition_warm_scheduled(std::slice::from_ref(&sample_id));
    let mut context = radiant::prelude::UiUpdateContext::default();

    state.apply_message(
        crate::native_app::test_support::state::GuiMessage::Frame,
        &mut context,
    );
    let command = context.into_command();

    assert_eq!(
        command.business_task_priority("gui-preview-audition-warm"),
        None,
        "preview warming should not rediscover a path already queued by a prior frame"
    );
}

#[test]
fn starmap_mode_frame_does_not_warm_preview_audition_heads_while_playback_active() {
    let source_root = tempfile::tempdir().expect("source root");
    let sample = source_root.path().join("large-cold.wav");
    write_sparse_test_wav_i16(&sample, 1, 700);
    let sample_id = sample.display().to_string();
    let mut state = crate::native_app::test_support::state::NativeAppStateFixture::default()
        .with_folder_browser(
            crate::native_app::test_support::state::FolderBrowserState::from_sample_sources(&[
                wavecrate::sample_sources::SampleSource::new(source_root.path().to_path_buf()),
            ]),
        )
        .build();
    state.ui.chrome.sample_browser_display = crate::native_app::app::SampleBrowserDisplayMode::Map;
    crate::native_app::test_support::state::seed_sample_playback_session(
        &mut state,
        sample_id,
        "preview_samples",
    );
    prepare_sample_browser_view(&mut state);
    let mut context = radiant::prelude::UiUpdateContext::default();

    state.apply_message(
        crate::native_app::test_support::state::GuiMessage::Frame,
        &mut context,
    );
    let command = context.into_command();

    assert_eq!(
        command.business_task_priority("gui-preview-audition-warm"),
        None,
        "preview warming should yield while playback is active"
    );
    assert_eq!(
        state.background.preview_audition_warm_task.active(),
        None,
        "preview warming should not leave a tracked background task while playback is active"
    );
}

#[test]
fn starmap_drag_finish_cancels_cold_preview_audition_decode() {
    let source_root = tempfile::tempdir().expect("source root");
    let sample = source_root.path().join("large-cold.wav");
    write_sparse_test_wav_i16(&sample, 1, 700);
    let sample_id = sample.display().to_string();
    let mut state = crate::native_app::test_support::state::NativeAppStateFixture::default()
        .with_folder_browser(
            crate::native_app::test_support::state::FolderBrowserState::from_sample_sources(&[
                wavecrate::sample_sources::SampleSource::new(source_root.path().to_path_buf()),
            ]),
        )
        .build();
    let mut context = radiant::prelude::UiUpdateContext::default();

    state.apply_message(
        crate::native_app::test_support::state::GuiMessage::BeginStarmapAuditionDrag {
            path: Some(sample_id),
            position: Point::new(10.0, 10.0),
            modifiers: PointerModifiers::default(),
        },
        &mut context,
    );
    assert!(
        state.background.preview_audition_task.active().is_some(),
        "cold drag targets should start preview decode while the drag is active"
    );
    state.apply_message(
        crate::native_app::test_support::state::GuiMessage::UpdateStarmapAuditionDrag {
            paths: vec![sample.display().to_string()],
            position: Point::new(12.0, 10.0),
            modifiers: PointerModifiers::default(),
        },
        &mut context,
    );

    state.apply_message(
        crate::native_app::test_support::state::GuiMessage::FinishStarmapAuditionDrag,
        &mut context,
    );

    assert_eq!(state.background.preview_audition_task.active(), None);
    assert_eq!(state.ui.chrome.starmap_audition_queue, Default::default());
}

#[test]
fn starmap_click_finish_preserves_cold_preview_audition_decode() {
    let source_root = tempfile::tempdir().expect("source root");
    let sample = source_root.path().join("large-cold-click.wav");
    write_sparse_test_wav_i16(&sample, 1, 700);
    let sample_id = sample.display().to_string();
    let mut state = crate::native_app::test_support::state::NativeAppStateFixture::default()
        .with_folder_browser(
            crate::native_app::test_support::state::FolderBrowserState::from_sample_sources(&[
                wavecrate::sample_sources::SampleSource::new(source_root.path().to_path_buf()),
            ]),
        )
        .build();
    let mut context = radiant::prelude::UiUpdateContext::default();

    state.apply_message(
        crate::native_app::test_support::state::GuiMessage::BeginStarmapAuditionDrag {
            path: Some(sample_id),
            position: Point::new(10.0, 10.0),
            modifiers: PointerModifiers::default(),
        },
        &mut context,
    );
    assert!(
        state.background.preview_audition_task.active().is_some(),
        "cold click targets should start preview decode on press"
    );

    state.apply_message(
        crate::native_app::test_support::state::GuiMessage::FinishStarmapAuditionDrag,
        &mut context,
    );

    assert!(
        state.background.preview_audition_task.active().is_some(),
        "click release should not cancel the pending preview decode before it can start playback"
    );
    assert_eq!(state.ui.chrome.starmap_audition_queue, Default::default());
}

#[test]
fn starmap_audition_promotion_only_loads_latest_stable_target() {
    let source_root = tempfile::tempdir().expect("source root");
    let first = source_root.path().join("first.wav");
    let second = source_root.path().join("second.wav");
    write_sparse_test_wav_i16(&first, 1, 700);
    write_sparse_test_wav_i16(&second, 1, 700);
    let first_id = first.display().to_string();
    let second_id = second.display().to_string();
    let mut state = crate::native_app::test_support::state::NativeAppStateFixture::default()
        .with_folder_browser(
            crate::native_app::test_support::state::FolderBrowserState::from_sample_sources(&[
                wavecrate::sample_sources::SampleSource::new(source_root.path().to_path_buf()),
            ]),
        )
        .build();
    state.library.folder_browser.select_file(second_id.clone());
    let mut context = radiant::prelude::UiUpdateContext::default();

    state.schedule_starmap_audition_promotion(first_id, &mut context);
    state.schedule_starmap_audition_promotion(second_id.clone(), &mut context);
    let delayed = run_after_commands(&mut state, context.into_command());
    assert_eq!(delayed.len(), 2);

    let mut stale_context = radiant::prelude::UiUpdateContext::default();
    state.apply_message(delayed[0].clone(), &mut stale_context);
    assert_eq!(
        stale_context
            .into_command()
            .business_task_priority("gui-sample-load-validate"),
        None,
        "stale starmap promotion tickets must not start full sample loads"
    );

    let mut latest_context = radiant::prelude::UiUpdateContext::default();
    state.apply_message(delayed[1].clone(), &mut latest_context);
    assert_eq!(
        latest_context
            .into_command()
            .business_task_priority("gui-sample-load-validate"),
        Some(radiant::prelude::TaskPriority::Interactive),
        "latest stable starmap target should promote to the normal full load path"
    );
}

#[test]
fn starmap_drag_duplicate_active_hit_preserves_pending_promotion() {
    let source_root = tempfile::tempdir().expect("source root");
    let sample = source_root.path().join("same-node.wav");
    write_sparse_test_wav_i16(&sample, 1, 700);
    let sample_id = sample.display().to_string();
    let mut state = crate::native_app::test_support::state::NativeAppStateFixture::default()
        .with_folder_browser(
            crate::native_app::test_support::state::FolderBrowserState::from_sample_sources(&[
                wavecrate::sample_sources::SampleSource::new(source_root.path().to_path_buf()),
            ]),
        )
        .build();
    state.library.folder_browser.select_file(sample_id.clone());
    state.ui.chrome.starmap_audition_drag =
        Some(crate::native_app::app::StarmapAuditionDragState {
            last_hit_file_id: Some(sample_id.clone()),
            last_position: Point::new(10.0, 10.0),
            modifiers: PointerModifiers::default(),
        });
    state.ui.chrome.starmap_audition_queue.active_file_id = Some(sample_id.clone());
    let mut schedule_context = radiant::prelude::UiUpdateContext::default();

    state.schedule_starmap_audition_promotion(sample_id.clone(), &mut schedule_context);
    let delayed = run_after_commands(&mut state, schedule_context.into_command());
    assert_eq!(delayed.len(), 1);

    state.apply_message(
        crate::native_app::test_support::state::GuiMessage::UpdateStarmapAuditionDrag {
            paths: vec![sample_id],
            position: Point::new(12.0, 10.0),
            modifiers: PointerModifiers::default(),
        },
        &mut radiant::prelude::UiUpdateContext::default(),
    );

    let mut promote_context = radiant::prelude::UiUpdateContext::default();
    state.apply_message(delayed[0].clone(), &mut promote_context);
    assert_eq!(
        promote_context
            .into_command()
            .business_task_priority("gui-sample-load-validate"),
        Some(radiant::prelude::TaskPriority::Interactive),
        "same-node drag updates should not cancel the pending full-quality promotion"
    );
}

#[test]
fn starmap_drag_retriggers_sample_after_sweeping_away_and_back() {
    let source_root = tempfile::tempdir().expect("source root");
    let first = source_root.path().join("a.wav");
    let second = source_root.path().join("b.wav");
    write_test_wav_i16(&first, &[0, 100, -100]);
    write_test_wav_i16(&second, &[0, 120, -120]);
    let first_id = first.display().to_string();
    let second_id = second.display().to_string();
    let mut state = crate::native_app::test_support::state::NativeAppStateFixture::default()
        .with_folder_browser(
            crate::native_app::test_support::state::FolderBrowserState::from_sample_sources(&[
                wavecrate::sample_sources::SampleSource::new(source_root.path().to_path_buf()),
            ]),
        )
        .build();
    let mut context = radiant::prelude::UiUpdateContext::default();

    state.apply_message(
        crate::native_app::test_support::state::GuiMessage::BeginStarmapAuditionDrag {
            path: Some(first_id.clone()),
            position: Point::new(10.0, 10.0),
            modifiers: PointerModifiers::default(),
        },
        &mut context,
    );
    state.apply_message(
        crate::native_app::test_support::state::GuiMessage::UpdateStarmapAuditionDrag {
            paths: vec![second_id.clone()],
            position: Point::new(90.0, 10.0),
            modifiers: PointerModifiers::default(),
        },
        &mut context,
    );
    state.apply_message(
        crate::native_app::test_support::state::GuiMessage::UpdateStarmapAuditionDrag {
            paths: vec![first_id.clone()],
            position: Point::new(92.0, 10.0),
            modifiers: PointerModifiers::default(),
        },
        &mut context,
    );

    assert_eq!(
        state
            .ui
            .chrome
            .starmap_audition_queue
            .active_file_id
            .as_deref(),
        Some(first_id.as_str())
    );
    assert_eq!(
        state.library.folder_browser.selected_file_id(),
        Some(first_id.as_str()),
        "returning to a node should make it the immediate playback target again"
    );
    assert_eq!(
        state
            .ui
            .chrome
            .starmap_audition_queue
            .queued_file_ids
            .iter()
            .cloned()
            .collect::<Vec<_>>(),
        Vec::<String>::new(),
        "dragging away from a sample and back should not leave a delayed replay queued"
    );
}

#[test]
fn starmap_drag_finish_clears_active_drag_audition_state() {
    let source_root = tempfile::tempdir().expect("source root");
    let first = source_root.path().join("a.wav");
    let second = source_root.path().join("b.wav");
    let third = source_root.path().join("c.wav");
    write_test_wav_i16(&first, &[0, 100, -100]);
    write_test_wav_i16(&second, &[0, 120, -120]);
    write_test_wav_i16(&third, &[0, 140, -140]);
    let first_id = first.display().to_string();
    let second_id = second.display().to_string();
    let third_id = third.display().to_string();
    let mut state = crate::native_app::test_support::state::NativeAppStateFixture::default()
        .with_folder_browser(
            crate::native_app::test_support::state::FolderBrowserState::from_sample_sources(&[
                wavecrate::sample_sources::SampleSource::new(source_root.path().to_path_buf()),
            ]),
        )
        .build();
    let mut context = radiant::prelude::UiUpdateContext::default();

    state.apply_message(
        crate::native_app::test_support::state::GuiMessage::BeginStarmapAuditionDrag {
            path: Some(first_id.clone()),
            position: Point::new(10.0, 10.0),
            modifiers: PointerModifiers::default(),
        },
        &mut context,
    );
    state.apply_message(
        crate::native_app::test_support::state::GuiMessage::UpdateStarmapAuditionDrag {
            paths: vec![second_id.clone(), third_id.clone()],
            position: Point::new(90.0, 10.0),
            modifiers: PointerModifiers::default(),
        },
        &mut context,
    );

    assert_eq!(
        state
            .ui
            .chrome
            .starmap_audition_queue
            .active_file_id
            .as_deref(),
        Some(third_id.as_str())
    );
    assert_eq!(
        state
            .ui
            .chrome
            .starmap_audition_queue
            .queued_file_ids
            .iter()
            .cloned()
            .collect::<Vec<_>>(),
        Vec::<String>::new(),
        "drag updates should not leave swept follow-up targets queued behind the current pointer"
    );

    state.apply_message(
        crate::native_app::test_support::state::GuiMessage::FinishStarmapAuditionDrag,
        &mut context,
    );

    assert_eq!(state.ui.chrome.starmap_audition_drag, None);
    assert_eq!(
        state.ui.chrome.starmap_audition_queue,
        Default::default(),
        "releasing the drag should not leave active or queued audition state behind"
    );
}

#[test]
fn starmap_drag_finish_after_motion_keeps_drag_playback_state() {
    let source_root = tempfile::tempdir().expect("source root");
    let first = source_root.path().join("a.wav");
    let second = source_root.path().join("b.wav");
    write_test_wav_i16(&first, &[0, 100, -100]);
    write_test_wav_i16(&second, &[0, 120, -120]);
    let first_id = first.display().to_string();
    let second_id = second.display().to_string();
    let mut state = crate::native_app::test_support::state::NativeAppStateFixture::default()
        .with_folder_browser(
            crate::native_app::test_support::state::FolderBrowserState::from_sample_sources(&[
                wavecrate::sample_sources::SampleSource::new(source_root.path().to_path_buf()),
            ]),
        )
        .build();
    let mut context = radiant::prelude::UiUpdateContext::default();

    state.apply_message(
        crate::native_app::test_support::state::GuiMessage::BeginStarmapAuditionDrag {
            path: Some(first_id),
            position: Point::new(10.0, 10.0),
            modifiers: PointerModifiers::default(),
        },
        &mut context,
    );
    state.apply_message(
        crate::native_app::test_support::state::GuiMessage::UpdateStarmapAuditionDrag {
            paths: vec![second_id.clone()],
            position: Point::new(90.0, 10.0),
            modifiers: PointerModifiers::default(),
        },
        &mut context,
    );
    crate::native_app::test_support::state::seed_sample_playback_session(
        &mut state,
        second_id,
        "preview_samples",
    );
    state.audio.current_playback_span = Some((0.0, 1.0));
    state.audio.playback_progress = wavecrate::audio::PlaybackRuntimeProgress {
        active: true,
        elapsed: Some(std::time::Duration::from_millis(90)),
        looping: false,
        progress: Some(0.25),
        error: None,
    };

    state.apply_message(
        crate::native_app::test_support::state::GuiMessage::FinishStarmapAuditionDrag,
        &mut context,
    );

    assert!(state.audio.sample_playback_session.is_some());
    assert_eq!(state.audio.current_playback_span, Some((0.0, 1.0)));
    assert_eq!(
        state.audio.playback_progress,
        wavecrate::audio::PlaybackRuntimeProgress {
            active: true,
            elapsed: Some(std::time::Duration::from_millis(90)),
            looping: false,
            progress: Some(0.25),
            error: None,
        },
        "drag release after motion should keep active playback running"
    );
    assert_ne!(
        state
            .audio
            .sample_playback_session
            .as_ref()
            .map(|session| session.request.origin),
        Some("starmap_drag"),
        "release should not leave kept playback tagged as an active starmap drag"
    );
    assert_eq!(state.ui.chrome.starmap_audition_queue, Default::default());
}

#[test]
fn starmap_drag_release_preserves_loaded_last_played_sample_not_latest_hit() {
    let source_root = tempfile::tempdir().expect("source root");
    let first = source_root.path().join("a.wav");
    let second = source_root.path().join("b.wav");
    let third = source_root.path().join("c.wav");
    write_test_wav_i16(&first, &[0, 100, -100]);
    write_test_wav_i16(&second, &[0, 120, -120, 60]);
    write_test_wav_i16(&third, &[0, 140, -140]);
    let first_id = first.display().to_string();
    let second_id = second.display().to_string();
    let third_id = third.display().to_string();
    let mut state = crate::native_app::test_support::state::NativeAppStateFixture::default()
        .with_folder_browser(
            crate::native_app::test_support::state::FolderBrowserState::from_sample_sources(&[
                wavecrate::sample_sources::SampleSource::new(source_root.path().to_path_buf()),
            ]),
        )
        .build();
    state.waveform.current =
        crate::native_app::test_support::state::WaveformState::load_path(second.clone())
            .expect("second sample loads");
    state.apply_message(
        crate::native_app::test_support::state::GuiMessage::BeginStarmapAuditionDrag {
            path: Some(first_id),
            position: Point::new(10.0, 10.0),
            modifiers: PointerModifiers::default(),
        },
        &mut radiant::prelude::UiUpdateContext::default(),
    );
    state.apply_message(
        crate::native_app::test_support::state::GuiMessage::UpdateStarmapAuditionDrag {
            paths: vec![third_id.clone()],
            position: Point::new(90.0, 10.0),
            modifiers: PointerModifiers::default(),
        },
        &mut radiant::prelude::UiUpdateContext::default(),
    );
    state.ui.chrome.starmap_audition_queue.last_played_file_id = Some(second_id.clone());
    crate::native_app::test_support::state::seed_sample_playback_session(
        &mut state,
        second_id,
        "preview_samples",
    );
    state.audio.current_playback_span = Some((0.0, 1.0));
    state.audio.playback_progress = wavecrate::audio::PlaybackRuntimeProgress {
        active: true,
        elapsed: Some(std::time::Duration::from_millis(90)),
        looping: false,
        progress: Some(0.25),
        error: None,
    };
    assert!(
        !state.waveform.current.has_loaded_sample(),
        "test setup should reproduce the blank waveform preview-loading state"
    );
    assert_eq!(
        state
            .ui
            .chrome
            .starmap_audition_queue
            .active_file_id
            .as_deref(),
        Some(third_id.as_str()),
        "test setup keeps the latest drag hit different from the audible sample"
    );
    let stale_settled_ticket = state.background.settled_sample_promotion_task.begin();

    let mut release_context = radiant::prelude::UiUpdateContext::default();
    state.apply_message(
        crate::native_app::test_support::state::GuiMessage::FinishStarmapAuditionDrag,
        &mut release_context,
    );
    let (delayed, perform_count) =
        collect_after_commands_and_perform_count(&mut state, release_context.into_command());

    assert_eq!(state.waveform.current.path(), second.as_path());
    assert!(state.waveform.current.has_loaded_sample());
    assert_eq!(
        state.library.folder_browser.selected_file_id(),
        Some(second.to_string_lossy().as_ref()),
        "release should keep keyboard playback aligned with the restored loaded sample"
    );
    assert!(
        !state.waveform.instant_preview_active(),
        "release should restore the authoritative waveform display"
    );
    assert!(
        state.waveform.current.is_playing(),
        "release should keep the loaded starmap sample playing"
    );
    assert_eq!(state.audio.current_playback_span, Some((0.0, 1.0)));
    assert!(
        state.audio.sample_playback_session.is_some(),
        "release should preserve the matching playback session"
    );
    assert_ne!(
        state
            .audio
            .sample_playback_session
            .as_ref()
            .map(|session| session.request.origin),
        Some("starmap_drag"),
        "release should not leave kept playback tagged as an active starmap drag"
    );
    assert_ne!(
        state.waveform.current.path(),
        third.as_path(),
        "release should not retarget to the latest dense-node hit"
    );
    assert_eq!(
        delayed,
        Vec::new(),
        "release should not schedule a delayed load for the last played sample"
    );
    assert_eq!(
        perform_count, 0,
        "release should not run a foreground sample load"
    );
    assert!(
        state
            .background
            .settled_sample_promotion_task
            .active()
            .is_none(),
        "release should cancel settled promotions that could retarget after mouse-up"
    );
    state.apply_message(
        crate::native_app::test_support::state::GuiMessage::SettledSamplePromotion {
            ticket: stale_settled_ticket,
            path: third_id.clone(),
            scheduled_at: std::time::Instant::now(),
        },
        &mut radiant::prelude::UiUpdateContext::default(),
    );
    assert_eq!(
        state.waveform.current.path(),
        second.as_path(),
        "stale settled promotion after release must not load the latest hit"
    );
    assert_eq!(state.ui.chrome.starmap_audition_queue, Default::default());
}

#[test]
fn starmap_drag_release_replaces_mismatched_audio_target_with_restored_sample() {
    let source_root = tempfile::tempdir().expect("source root");
    let first = source_root.path().join("a.wav");
    let second = source_root.path().join("b.wav");
    let third = source_root.path().join("c.wav");
    write_test_wav_i16(&first, &[0, 100, -100]);
    write_test_wav_i16(&second, &[0, 120, -120, 60]);
    write_test_wav_i16(&third, &[0, 140, -140]);
    let first_id = first.display().to_string();
    let second_id = second.display().to_string();
    let third_id = third.display().to_string();
    let mut state = crate::native_app::test_support::state::NativeAppStateFixture::default()
        .with_folder_browser(
            crate::native_app::test_support::state::FolderBrowserState::from_sample_sources(&[
                wavecrate::sample_sources::SampleSource::new(source_root.path().to_path_buf()),
            ]),
        )
        .build();
    state.waveform.current =
        crate::native_app::test_support::state::WaveformState::load_path(second.clone())
            .expect("second sample loads");
    state.apply_message(
        crate::native_app::test_support::state::GuiMessage::BeginStarmapAuditionDrag {
            path: Some(first_id),
            position: Point::new(10.0, 10.0),
            modifiers: PointerModifiers::default(),
        },
        &mut radiant::prelude::UiUpdateContext::default(),
    );
    state.apply_message(
        crate::native_app::test_support::state::GuiMessage::UpdateStarmapAuditionDrag {
            paths: vec![third_id.clone()],
            position: Point::new(90.0, 10.0),
            modifiers: PointerModifiers::default(),
        },
        &mut radiant::prelude::UiUpdateContext::default(),
    );
    state.ui.chrome.starmap_audition_queue.last_played_file_id = Some(second_id.clone());
    crate::native_app::test_support::state::seed_sample_playback_session(
        &mut state,
        second_id.clone(),
        "preview_samples",
    );
    state.ui.chrome.starmap_audition_queue.last_played_session =
        state.audio.sample_playback_session.clone();
    crate::native_app::test_support::state::seed_sample_playback_session(
        &mut state,
        third_id.clone(),
        "preview_samples",
    );
    state.audio.current_playback_span = Some((0.0, 1.0));
    state.audio.playback_progress = wavecrate::audio::PlaybackRuntimeProgress {
        active: true,
        elapsed: Some(std::time::Duration::from_millis(90)),
        looping: false,
        progress: Some(0.25),
        error: None,
    };

    state.apply_message(
        crate::native_app::test_support::state::GuiMessage::FinishStarmapAuditionDrag,
        &mut radiant::prelude::UiUpdateContext::default(),
    );

    assert_eq!(state.waveform.current.path(), second.as_path());
    assert!(state.waveform.current.has_loaded_sample());
    assert!(
        state.waveform.current.is_playing(),
        "release should keep the restored loaded sample visually playing"
    );
    assert_ne!(
        state.audio.active_sample_playback_path(),
        Some(third_id.as_str()),
        "release must not leave a stale dense-node hit as the active audio target"
    );
    assert_eq!(
        state.audio.active_sample_playback_path(),
        Some(second_id.as_str()),
        "release should restore the previous active session instead of restarting playback"
    );
    assert_eq!(
        state.library.folder_browser.selected_file_id(),
        Some(second.to_string_lossy().as_ref()),
        "keyboard playback should remain aligned with the restored sample"
    );
    assert_eq!(state.ui.chrome.starmap_audition_queue, Default::default());
}

#[test]
fn starmap_drag_replacement_stops_previous_drag_playback_tail() {
    let source_root = tempfile::tempdir().expect("source root");
    let first = source_root.path().join("a.wav");
    let second = source_root.path().join("b.wav");
    write_test_wav_i16(&first, &[0, 100, -100]);
    write_sparse_test_wav_i16(&second, 1, 700);
    let first_id = first.display().to_string();
    let second_id = second.display().to_string();
    let mut state = crate::native_app::test_support::state::NativeAppStateFixture::default()
        .with_folder_browser(
            crate::native_app::test_support::state::FolderBrowserState::from_sample_sources(&[
                wavecrate::sample_sources::SampleSource::new(source_root.path().to_path_buf()),
            ]),
        )
        .build();
    let mut context = radiant::prelude::UiUpdateContext::default();

    state.apply_message(
        crate::native_app::test_support::state::GuiMessage::BeginStarmapAuditionDrag {
            path: Some(first_id.clone()),
            position: Point::new(10.0, 10.0),
            modifiers: PointerModifiers::default(),
        },
        &mut context,
    );
    crate::native_app::test_support::state::seed_sample_playback_session(
        &mut state,
        first_id,
        "preview_samples",
    );
    state.audio.current_playback_span = Some((0.0, 1.0));
    state.audio.playback_progress = wavecrate::audio::PlaybackRuntimeProgress {
        active: true,
        elapsed: Some(std::time::Duration::from_millis(90)),
        looping: false,
        progress: Some(0.25),
        error: None,
    };

    state.apply_message(
        crate::native_app::test_support::state::GuiMessage::UpdateStarmapAuditionDrag {
            paths: vec![second_id.clone()],
            position: Point::new(90.0, 10.0),
            modifiers: PointerModifiers::default(),
        },
        &mut context,
    );

    assert_eq!(
        state
            .ui
            .chrome
            .starmap_audition_queue
            .active_file_id
            .as_deref(),
        Some(second_id.as_str())
    );
    assert_eq!(
        state.audio.sample_playback_session, None,
        "replacing an active drag target should stop the previous preview immediately"
    );
    assert_eq!(state.audio.current_playback_span, None);
    assert_eq!(
        state.audio.playback_progress,
        wavecrate::audio::PlaybackRuntimeProgress::default(),
        "drag replacement should not leave the previous target visually playing while the next preview decodes"
    );
    assert!(
        state.background.preview_audition_task.active().is_some(),
        "the next cold target should still begin the shared preview-decode path"
    );
}

#[test]
fn starmap_click_finish_preserves_started_audition_playback_state() {
    let source_root = tempfile::tempdir().expect("source root");
    let sample = source_root.path().join("click.wav");
    write_test_wav_i16(&sample, &[0, 100, -100]);
    let sample_id = sample.display().to_string();
    let mut state = crate::native_app::test_support::state::NativeAppStateFixture::default()
        .with_folder_browser(
            crate::native_app::test_support::state::FolderBrowserState::from_sample_sources(&[
                wavecrate::sample_sources::SampleSource::new(source_root.path().to_path_buf()),
            ]),
        )
        .build();
    let mut context = radiant::prelude::UiUpdateContext::default();

    state.apply_message(
        crate::native_app::test_support::state::GuiMessage::BeginStarmapAuditionDrag {
            path: Some(sample_id.clone()),
            position: Point::new(10.0, 10.0),
            modifiers: PointerModifiers::default(),
        },
        &mut context,
    );
    crate::native_app::test_support::state::seed_sample_playback_session(
        &mut state,
        sample_id.clone(),
        "preview_samples",
    );
    state.audio.current_playback_span = Some((0.0, 1.0));
    state.audio.playback_progress = wavecrate::audio::PlaybackRuntimeProgress {
        active: true,
        elapsed: Some(std::time::Duration::from_millis(90)),
        looping: false,
        progress: Some(0.25),
        error: None,
    };

    state.apply_message(
        crate::native_app::test_support::state::GuiMessage::FinishStarmapAuditionDrag,
        &mut context,
    );

    assert_eq!(
        state.audio.active_sample_playback_path(),
        Some(sample_id.as_str())
    );
    assert_eq!(state.audio.current_playback_span, Some((0.0, 1.0)));
    assert!(
        state.audio.playback_progress.active,
        "click audition should keep playing after the release; only real drag sweeps stop on release"
    );
    assert_eq!(state.ui.chrome.starmap_audition_queue, Default::default());
}

#[test]
fn starmap_drag_finish_cancels_pending_promotion() {
    let source_root = tempfile::tempdir().expect("source root");
    let sample = source_root.path().join("late-promotion.wav");
    write_sparse_test_wav_i16(&sample, 1, 700);
    let sample_id = sample.display().to_string();
    let mut state = crate::native_app::test_support::state::NativeAppStateFixture::default()
        .with_folder_browser(
            crate::native_app::test_support::state::FolderBrowserState::from_sample_sources(&[
                wavecrate::sample_sources::SampleSource::new(source_root.path().to_path_buf()),
            ]),
        )
        .build();
    let mut context = radiant::prelude::UiUpdateContext::default();

    state.apply_message(
        crate::native_app::test_support::state::GuiMessage::BeginStarmapAuditionDrag {
            path: Some(sample_id.clone()),
            position: Point::new(10.0, 10.0),
            modifiers: PointerModifiers::default(),
        },
        &mut context,
    );
    let mut schedule_context = radiant::prelude::UiUpdateContext::default();
    state.schedule_starmap_audition_promotion(sample_id.clone(), &mut schedule_context);
    let delayed = run_after_commands(&mut state, schedule_context.into_command());
    assert_eq!(delayed.len(), 1);
    state.apply_message(
        crate::native_app::test_support::state::GuiMessage::UpdateStarmapAuditionDrag {
            paths: vec![sample_id],
            position: Point::new(12.0, 10.0),
            modifiers: PointerModifiers::default(),
        },
        &mut context,
    );

    state.apply_message(
        crate::native_app::test_support::state::GuiMessage::FinishStarmapAuditionDrag,
        &mut context,
    );

    let mut stale_context = radiant::prelude::UiUpdateContext::default();
    state.apply_message(delayed[0].clone(), &mut stale_context);

    assert_eq!(
        stale_context
            .into_command()
            .business_task_priority("gui-sample-load-validate"),
        None,
        "promotion tickets scheduled before drag finish must not restart full sample loading"
    );
}

#[test]
fn starmap_drag_update_after_finish_is_ignored() {
    let source_root = tempfile::tempdir().expect("source root");
    let first = source_root.path().join("a.wav");
    let second = source_root.path().join("b.wav");
    write_test_wav_i16(&first, &[0, 100, -100]);
    write_test_wav_i16(&second, &[0, 120, -120]);
    let first_id = first.display().to_string();
    let second_id = second.display().to_string();
    let mut state = crate::native_app::test_support::state::NativeAppStateFixture::default()
        .with_folder_browser(
            crate::native_app::test_support::state::FolderBrowserState::from_sample_sources(&[
                wavecrate::sample_sources::SampleSource::new(source_root.path().to_path_buf()),
            ]),
        )
        .build();
    let mut context = radiant::prelude::UiUpdateContext::default();

    state.apply_message(
        crate::native_app::test_support::state::GuiMessage::BeginStarmapAuditionDrag {
            path: Some(first_id.clone()),
            position: Point::new(10.0, 10.0),
            modifiers: PointerModifiers::default(),
        },
        &mut context,
    );
    state.apply_message(
        crate::native_app::test_support::state::GuiMessage::FinishStarmapAuditionDrag,
        &mut context,
    );
    state.apply_message(
        crate::native_app::test_support::state::GuiMessage::UpdateStarmapAuditionDrag {
            paths: vec![second_id],
            position: Point::new(90.0, 10.0),
            modifiers: PointerModifiers::default(),
        },
        &mut context,
    );

    assert_eq!(
        state.library.folder_browser.selected_file_id(),
        Some(first_id.as_str()),
        "late pointer updates after release should not restart drag-play audition"
    );
    assert_eq!(state.ui.chrome.starmap_audition_queue, Default::default());
}

#[test]
fn starmap_drag_audition_ignores_multi_select_modifiers() {
    let source_root = tempfile::tempdir().expect("source root");
    let first = source_root.path().join("a.wav");
    let second = source_root.path().join("b.wav");
    write_test_wav_i16(&first, &[0, 100, -100]);
    write_test_wav_i16(&second, &[0, 120, -120]);
    let first_id = first.display().to_string();
    let second_id = second.display().to_string();
    let mut state = crate::native_app::test_support::state::NativeAppStateFixture::default()
        .with_folder_browser(
            crate::native_app::test_support::state::FolderBrowserState::from_sample_sources(&[
                wavecrate::sample_sources::SampleSource::new(source_root.path().to_path_buf()),
            ]),
        )
        .build();
    let shift = PointerModifiers {
        shift: true,
        ..PointerModifiers::default()
    };
    state.library.folder_browser.select_file(first_id.clone());

    state.apply_message(
        crate::native_app::test_support::state::GuiMessage::BeginStarmapAuditionDrag {
            path: Some(second_id.clone()),
            position: Point::new(90.0, 10.0),
            modifiers: shift,
        },
        &mut radiant::prelude::UiUpdateContext::default(),
    );

    assert_eq!(
        state.library.folder_browser.selected_file_id(),
        Some(second_id.as_str())
    );
    assert_eq!(
        state.library.folder_browser.selected_audio_file_count(),
        1,
        "map audition should replace the focused sample instead of range-selecting swept nodes"
    );
}

#[test]
fn starmap_audition_hit_preserves_current_viewport() {
    let source_root = tempfile::tempdir().expect("source root");
    let first = source_root.path().join("a.wav");
    let second = source_root.path().join("b.wav");
    write_test_wav_i16(&first, &[0, 100, -100]);
    write_test_wav_i16(&second, &[0, 120, -120]);
    let second_id = second.display().to_string();
    let mut state = crate::native_app::test_support::state::NativeAppStateFixture::default()
        .with_folder_browser(
            crate::native_app::test_support::state::FolderBrowserState::from_sample_sources(&[
                wavecrate::sample_sources::SampleSource::new(source_root.path().to_path_buf()),
            ]),
        )
        .build();
    state.ui.chrome.sample_browser_display = crate::native_app::app::SampleBrowserDisplayMode::Map;
    state.ui.chrome.starmap_viewport.zoom = 4.0;
    state.ui.chrome.starmap_viewport.center_x = 0.1;
    state.ui.chrome.starmap_viewport.center_y = 0.9;
    let initial_viewport = state.ui.chrome.starmap_viewport;

    state.apply_message(
        crate::native_app::test_support::state::GuiMessage::BeginStarmapAuditionDrag {
            path: Some(second_id.clone()),
            position: Point::new(10.0, 10.0),
            modifiers: PointerModifiers::default(),
        },
        &mut radiant::prelude::UiUpdateContext::default(),
    );

    assert_eq!(
        state.library.folder_browser.selected_file_id(),
        Some(second_id.as_str())
    );
    assert_eq!(
        state.ui.chrome.starmap_viewport, initial_viewport,
        "map audition should select/play without panning the viewport",
    );
}

#[test]
fn starmap_drag_update_selects_next_hit_immediately() {
    let source_root = tempfile::tempdir().expect("source root");
    let first = source_root.path().join("a.wav");
    let second = source_root.path().join("b.wav");
    write_test_wav_i16(&first, &[0, 100, -100]);
    write_test_wav_i16(&second, &[0, 120, -120]);
    let first_id = first.display().to_string();
    let second_id = second.display().to_string();
    let mut state = crate::native_app::test_support::state::NativeAppStateFixture::default()
        .with_folder_browser(
            crate::native_app::test_support::state::FolderBrowserState::from_sample_sources(&[
                wavecrate::sample_sources::SampleSource::new(source_root.path().to_path_buf()),
            ]),
        )
        .build();
    state.ui.chrome.sample_browser_display = crate::native_app::app::SampleBrowserDisplayMode::Map;
    state.ui.chrome.starmap_viewport.zoom = 4.0;
    state.ui.chrome.starmap_viewport.center_x = 0.12;
    state.ui.chrome.starmap_viewport.center_y = 0.88;
    let initial_viewport = state.ui.chrome.starmap_viewport;
    let mut context = radiant::prelude::UiUpdateContext::default();
    state.apply_message(
        crate::native_app::test_support::state::GuiMessage::BeginStarmapAuditionDrag {
            path: Some(first_id.clone()),
            position: Point::new(10.0, 10.0),
            modifiers: PointerModifiers::default(),
        },
        &mut context,
    );
    state.apply_message(
        crate::native_app::test_support::state::GuiMessage::UpdateStarmapAuditionDrag {
            paths: vec![second_id.clone()],
            position: Point::new(90.0, 10.0),
            modifiers: PointerModifiers::default(),
        },
        &mut context,
    );

    assert_eq!(
        state.library.folder_browser.selected_file_id(),
        Some(second_id.as_str())
    );
    assert_eq!(
        state.ui.chrome.starmap_viewport, initial_viewport,
        "queued drag-play audition should not pan to the selected node"
    );
    assert_eq!(
        state
            .ui
            .chrome
            .starmap_audition_queue
            .active_file_id
            .as_deref(),
        Some(second_id.as_str())
    );
    assert!(
        state
            .ui
            .chrome
            .starmap_audition_queue
            .queued_file_ids
            .is_empty()
    );
    let command = context.into_command();
    assert!(
        command.requests_paint_only(),
        "live starmap drag updates should repaint the active node overlay without waiting for release"
    );
}

#[test]
fn starmap_drag_replacement_ignores_stale_advance_ticket() {
    let source_root = tempfile::tempdir().expect("source root");
    let first = source_root.path().join("a.wav");
    let second = source_root.path().join("b.wav");
    write_test_wav_i16(&first, &[0, 100, -100]);
    write_test_wav_i16(&second, &[0, 120, -120]);
    let first_id = first.display().to_string();
    let second_id = second.display().to_string();
    let mut state = crate::native_app::test_support::state::NativeAppStateFixture::default()
        .with_folder_browser(
            crate::native_app::test_support::state::FolderBrowserState::from_sample_sources(&[
                wavecrate::sample_sources::SampleSource::new(source_root.path().to_path_buf()),
            ]),
        )
        .build();
    let mut context = radiant::prelude::UiUpdateContext::default();

    state.apply_message(
        crate::native_app::test_support::state::GuiMessage::BeginStarmapAuditionDrag {
            path: Some(first_id),
            position: Point::new(10.0, 10.0),
            modifiers: PointerModifiers::default(),
        },
        &mut context,
    );
    let stale_advance = state.background.starmap_audition_advance_task.begin();
    state.apply_message(
        crate::native_app::test_support::state::GuiMessage::UpdateStarmapAuditionDrag {
            paths: vec![second_id.clone()],
            position: Point::new(90.0, 10.0),
            modifiers: PointerModifiers::default(),
        },
        &mut context,
    );
    state.apply_message(
        crate::native_app::test_support::state::GuiMessage::AdvanceStarmapAudition {
            ticket: stale_advance,
        },
        &mut context,
    );

    assert_eq!(
        state.library.folder_browser.selected_file_id(),
        Some(second_id.as_str()),
        "stale delayed advances must not undo the latest drag target"
    );
    assert_eq!(
        state
            .ui
            .chrome
            .starmap_audition_queue
            .active_file_id
            .as_deref(),
        Some(second_id.as_str())
    );
}

#[test]
fn starmap_drag_latest_hit_advances_without_zero_delay_message() {
    let source_root = tempfile::tempdir().expect("source root");
    let first = source_root.path().join("a.wav");
    let second = source_root.path().join("b.wav");
    write_test_wav_i16(&first, &[0, 100, -100]);
    write_test_wav_i16(&second, &[0, 120, -120]);
    let first_id = first.display().to_string();
    let second_id = second.display().to_string();
    let mut state = crate::native_app::test_support::state::NativeAppStateFixture::default()
        .with_folder_browser(
            crate::native_app::test_support::state::FolderBrowserState::from_sample_sources(&[
                wavecrate::sample_sources::SampleSource::new(source_root.path().to_path_buf()),
            ]),
        )
        .build();
    state.ui.chrome.starmap_audition_drag =
        Some(crate::native_app::app::StarmapAuditionDragState {
            last_hit_file_id: Some(first_id.clone()),
            last_position: Point::new(10.0, 10.0),
            modifiers: PointerModifiers::default(),
        });
    state.ui.chrome.starmap_audition_queue.active_file_id = Some(first_id);
    state
        .ui
        .chrome
        .starmap_audition_queue
        .queued_file_ids
        .push_back(second_id.clone());
    let mut context = radiant::prelude::UiUpdateContext::default();

    state.advance_starmap_drag_audition_latest_immediately(&mut context);
    let delayed = run_after_commands(&mut state, context.into_command());

    assert_eq!(
        state.library.folder_browser.selected_file_id(),
        Some(second_id.as_str()),
        "latest drag target progression should start the next target in the same update"
    );
    assert_eq!(
        state
            .ui
            .chrome
            .starmap_audition_queue
            .active_file_id
            .as_deref(),
        Some(second_id.as_str())
    );
    assert!(
        state
            .ui
            .chrome
            .starmap_audition_queue
            .queued_file_ids
            .is_empty()
    );
    assert!(
        delayed.iter().all(|message| !matches!(
            message,
            crate::native_app::test_support::state::GuiMessage::AdvanceStarmapAudition { .. }
        )),
        "latest drag target progression should not depend on a zero-delay advance message"
    );
}

#[test]
fn starmap_preview_decode_error_advances_to_queued_latest_hit() {
    let source_root = tempfile::tempdir().expect("source root");
    let first = source_root.path().join("a.wav");
    let second = source_root.path().join("b.wav");
    write_sparse_test_wav_i16(&first, 1, 700);
    write_sparse_test_wav_i16(&second, 1, 700);
    let first_id = first.display().to_string();
    let second_id = second.display().to_string();
    let mut state = crate::native_app::test_support::state::NativeAppStateFixture::default()
        .with_folder_browser(
            crate::native_app::test_support::state::FolderBrowserState::from_sample_sources(&[
                wavecrate::sample_sources::SampleSource::new(source_root.path().to_path_buf()),
            ]),
        )
        .build();
    state.library.folder_browser.select_file(first_id.clone());
    state.ui.chrome.starmap_audition_drag =
        Some(crate::native_app::app::StarmapAuditionDragState {
            last_hit_file_id: Some(first_id.clone()),
            last_position: Point::new(10.0, 10.0),
            modifiers: PointerModifiers::default(),
        });
    state.ui.chrome.starmap_audition_queue.active_file_id = Some(first_id.clone());
    state
        .ui
        .chrome
        .starmap_audition_queue
        .queued_file_ids
        .push_back(second_id.clone());
    let failed_preview_ticket = state.background.preview_audition_task.begin();
    let mut context = radiant::prelude::UiUpdateContext::default();

    state.apply_message(
        crate::native_app::test_support::state::GuiMessage::PreviewAuditionDecoded {
            completion: radiant::prelude::TaskCompletion {
                ticket: failed_preview_ticket,
                output: crate::native_app::app::PreviewAuditionResult {
                    path: first_id.clone(),
                    clip: Err(String::from("synthetic preview decode failed")),
                },
            },
            started_at: std::time::Instant::now(),
        },
        &mut context,
    );
    let command = context.into_command();

    assert_eq!(
        state.library.folder_browser.selected_file_id(),
        Some(second_id.as_str()),
        "a failed preview head must not strand a newer starmap drag target behind it"
    );
    assert_eq!(
        state
            .ui
            .chrome
            .starmap_audition_queue
            .active_file_id
            .as_deref(),
        Some(second_id.as_str())
    );
    assert!(
        !state
            .waveform
            .cache
            .preview_audition_decode_needed(std::path::Path::new(&first_id)),
        "known failed preview heads should not be retried on the next hover"
    );
    assert!(
        command.requests_paint_only(),
        "preview failure recovery should repaint the live active node immediately"
    );
}

#[test]
fn starmap_audition_queue_clears_after_last_hit_finishes() {
    let source_root = tempfile::tempdir().expect("source root");
    let first = source_root.path().join("a.wav");
    write_test_wav_i16(&first, &[0, 100, -100]);
    let first_id = first.display().to_string();
    let mut state = crate::native_app::test_support::state::NativeAppStateFixture::default()
        .with_folder_browser(
            crate::native_app::test_support::state::FolderBrowserState::from_sample_sources(&[
                wavecrate::sample_sources::SampleSource::new(source_root.path().to_path_buf()),
            ]),
        )
        .build();
    state.apply_message(
        crate::native_app::test_support::state::GuiMessage::BeginStarmapAuditionDrag {
            path: Some(first_id),
            position: Point::new(10.0, 10.0),
            modifiers: PointerModifiers::default(),
        },
        &mut radiant::prelude::UiUpdateContext::default(),
    );
    state.apply_message(
        crate::native_app::test_support::state::GuiMessage::FinishStarmapAuditionDrag,
        &mut radiant::prelude::UiUpdateContext::default(),
    );

    state.schedule_next_starmap_audition_hit(&mut radiant::prelude::UiUpdateContext::default());

    assert_eq!(state.ui.chrome.starmap_audition_drag, None);
    assert_eq!(state.ui.chrome.starmap_audition_queue, Default::default());
}

#[test]
fn rapid_last_played_records_only_latest_delayed_persist() {
    let source_root = tempfile::tempdir().expect("source root");
    let first_path = source_root.path().join("first.wav");
    let second_path = source_root.path().join("second.wav");
    fs::write(&first_path, []).expect("write first sample");
    fs::write(&second_path, []).expect("write second sample");
    let first_path_string = first_path.display().to_string();
    let second_path_string = second_path.display().to_string();

    let mut state = crate::native_app::test_support::state::NativeAppStateFixture::default()
        .with_folder_browser(
            crate::native_app::test_support::state::FolderBrowserState::from_sample_sources(&[
                wavecrate::sample_sources::SampleSource::new(source_root.path().to_path_buf()),
            ]),
        )
        .build();
    let mut context = radiant::prelude::UiUpdateContext::default();

    state
        .library
        .folder_browser
        .select_file(first_path_string.clone());
    state.record_sample_last_played(first_path_string, &mut context);
    state
        .library
        .folder_browser
        .select_file(second_path_string.clone());
    state.record_sample_last_played(second_path_string.clone(), &mut context);

    let delayed = run_after_commands(&mut state, context.into_command());
    assert_eq!(delayed.len(), 2);
    let mut stale_context = radiant::prelude::UiUpdateContext::default();
    state.apply_message(delayed[0].clone(), &mut stale_context);
    assert!(
        matches!(stale_context.into_command(), Command::None),
        "stale delayed last-played writes should not schedule disk work"
    );

    let mut latest_context = radiant::prelude::UiUpdateContext::default();
    state.apply_message(delayed[1].clone(), &mut latest_context);
    let message =
        run_first_perform(latest_context.into_command()).expect("latest last played persist");

    assert!(matches!(
        message,
        crate::native_app::test_support::state::GuiMessage::LastPlayedPersisted(result)
            if result.file_id == second_path_string
    ));
}

#[test]
fn active_playback_defers_last_played_disk_persist_until_idle() {
    let source_root = tempfile::tempdir().expect("source root");
    let sample_path = source_root.path().join("sample.wav");
    fs::write(&sample_path, [1_u8, 2, 3, 4]).expect("write sample");
    let sample_path_string = sample_path.display().to_string();

    let mut state = crate::native_app::test_support::state::NativeAppStateFixture::default()
        .with_folder_browser(
            crate::native_app::test_support::state::FolderBrowserState::from_sample_sources(&[
                wavecrate::sample_sources::SampleSource::new(source_root.path().to_path_buf()),
            ]),
        )
        .build();
    let mut context = radiant::prelude::UiUpdateContext::default();

    state
        .library
        .folder_browser
        .select_file(sample_path_string.clone());
    state.record_sample_last_played(sample_path_string.clone(), &mut context);
    state.waveform.current.start_playback(0.25);

    let delayed =
        run_first_after(&mut state, context.into_command()).expect("last played delayed command");
    let mut active_context = radiant::prelude::UiUpdateContext::default();
    state.apply_message(delayed, &mut active_context);

    let (retry_messages, perform_count) =
        collect_after_commands_and_perform_count(&mut state, active_context.into_command());
    assert_eq!(
        perform_count, 0,
        "active playback should defer last-played persistence before DB work starts"
    );
    assert_eq!(
        retry_messages.len(),
        0,
        "active playback should not schedule recurring last-played retry messages"
    );

    state.waveform.current.stop_playback();
    let mut idle_context = radiant::prelude::UiUpdateContext::default();
    state.advance_frame(&mut idle_context);
    let message = run_first_perform(idle_context.into_command())
        .expect("idle last played should persist to disk");

    assert!(matches!(
        message,
        crate::native_app::test_support::state::GuiMessage::LastPlayedPersisted(result)
            if result.file_id == sample_path_string
    ));
}

#[test]
fn stop_playback_flushes_deferred_last_played_after_clearing_runtime_state() {
    let source_root = tempfile::tempdir().expect("source root");
    let sample_path = source_root.path().join("sample.wav");
    fs::write(&sample_path, [1_u8, 2, 3, 4]).expect("write sample");
    let sample_path_string = sample_path.display().to_string();

    let mut state = crate::native_app::test_support::state::NativeAppStateFixture::default()
        .with_folder_browser(
            crate::native_app::test_support::state::FolderBrowserState::from_sample_sources(&[
                wavecrate::sample_sources::SampleSource::new(source_root.path().to_path_buf()),
            ]),
        )
        .build();
    let mut context = radiant::prelude::UiUpdateContext::default();

    state
        .library
        .folder_browser
        .select_file(sample_path_string.clone());
    state.record_sample_last_played(sample_path_string.clone(), &mut context);
    crate::native_app::test_support::state::seed_sample_playback_session(
        &mut state,
        sample_path_string.clone(),
        "audio_file",
    );
    state.audio.playback_progress = wavecrate::audio::PlaybackRuntimeProgress {
        active: true,
        elapsed: Some(std::time::Duration::from_millis(90)),
        looping: false,
        progress: Some(0.25),
        error: None,
    };

    let delayed =
        run_first_after(&mut state, context.into_command()).expect("last played delayed command");
    let mut active_context = radiant::prelude::UiUpdateContext::default();
    state.apply_message(delayed, &mut active_context);
    assert!(
        matches!(active_context.into_command(), Command::None),
        "active runtime state should defer last-played persistence"
    );
    assert!(
        state.audio.pending_last_played_persist.is_some(),
        "deferred last-played request should remain pending while playback is active"
    );

    let mut stop_context = radiant::prelude::UiUpdateContext::default();
    state.apply_message(
        crate::native_app::test_support::state::GuiMessage::StopPlayback,
        &mut stop_context,
    );

    assert!(state.audio.sample_playback_session.is_none());
    assert!(!state.audio.playback_progress.active);
    assert!(
        state.audio.pending_last_played_persist.is_none(),
        "stopping playback should flush deferred last-played persistence immediately"
    );
    let message = run_first_perform(stop_context.into_command())
        .expect("stop should queue idle last-played persist");

    assert!(matches!(
        message,
        crate::native_app::test_support::state::GuiMessage::LastPlayedPersisted(result)
            if result.file_id == sample_path_string
    ));
}

fn run_first_after(
    state: &mut crate::native_app::test_support::state::NativeAppState,
    command: Command<crate::native_app::test_support::state::GuiMessage>,
) -> Option<crate::native_app::test_support::state::GuiMessage> {
    run_after_commands(state, command).into_iter().next()
}

fn run_after_commands(
    state: &mut crate::native_app::test_support::state::NativeAppState,
    command: Command<crate::native_app::test_support::state::GuiMessage>,
) -> Vec<crate::native_app::test_support::state::GuiMessage> {
    let timer_count = count_timer_commands(&command);
    let messages = state.take_scheduled_timer_messages();
    assert_eq!(
        messages.len(),
        timer_count,
        "every queued timer should have a test-visible mapped message"
    );
    messages
}

fn collect_after_commands_and_perform_count(
    state: &mut crate::native_app::test_support::state::NativeAppState,
    command: Command<crate::native_app::test_support::state::GuiMessage>,
) -> (
    Vec<crate::native_app::test_support::state::GuiMessage>,
    usize,
) {
    let timer_count = count_timer_commands(&command);
    let delayed = state.take_scheduled_timer_messages();
    assert_eq!(
        delayed.len(),
        timer_count,
        "every queued timer should have a test-visible mapped message"
    );
    (delayed, count_perform_commands(&command))
}

fn count_timer_commands(
    command: &Command<crate::native_app::test_support::state::GuiMessage>,
) -> usize {
    match command {
        Command::Timer(..) => 1,
        Command::Batch(commands) => commands.iter().map(count_timer_commands).sum(),
        _ => 0,
    }
}

fn count_perform_commands(
    command: &Command<crate::native_app::test_support::state::GuiMessage>,
) -> usize {
    match command {
        Command::Perform { .. } => 1,
        Command::Batch(commands) => commands.iter().map(count_perform_commands).sum(),
        _ => 0,
    }
}

fn run_first_perform(
    command: Command<crate::native_app::test_support::state::GuiMessage>,
) -> Option<crate::native_app::test_support::state::GuiMessage> {
    match command {
        Command::Perform { work, .. } => Some(work()),
        Command::Batch(commands) => commands.into_iter().find_map(run_first_perform),
        _ => None,
    }
}

fn assert_starmap_viewport_reveals(
    viewport: crate::native_app::app::StarmapViewport,
    position: (f32, f32),
    message: &str,
) {
    let half_span = 0.5 / viewport.zoom.max(1.0);
    let min_x = viewport.center_x - half_span;
    let max_x = viewport.center_x + half_span;
    let min_y = viewport.center_y - half_span;
    let max_y = viewport.center_y + half_span;
    assert!(
        position.0 >= min_x - 0.001
            && position.0 <= max_x + 0.001
            && position.1 >= min_y - 0.001
            && position.1 <= max_y + 0.001,
        "{message}: position {:?} outside x=[{min_x:.3}, {max_x:.3}] y=[{min_y:.3}, {max_y:.3}]",
        position
    );
}
