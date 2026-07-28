use super::{
    gui_state_for_span_tests, native_app_state_with_temp_sample, reduce_gui_message_for_tests,
    run_command_for_tests, write_test_wav_i16,
};
use crate::native_app::app::WaveformPlaySelectionSnapshot;
use crate::native_app::sample_library::folder_browser::model::BrowserCurationScope;
use crate::native_app::test_support::state::{
    FolderBrowserMessage, FolderBrowserState, GuiMessage, WaveformState, view,
};
use radiant::{
    gui::types::{Point, Vector2},
    prelude::{self as ui, IntoView},
    runtime::Command,
    widgets::{DragHandleMessage, PointerModifiers},
};
use std::{fs, path::Path, sync::Arc};
use wavecrate::sample_sources::{Rating, SourceDatabase};
use wavecrate::selection::SelectionRange;

fn last_fixed_sample_browser_row_scroll(command: &Command<GuiMessage>) -> Option<(usize, i32)> {
    match command {
        Command::Batch(commands) => commands
            .iter()
            .filter_map(last_fixed_sample_browser_row_scroll)
            .last(),
        Command::ScrollFixedRowIntoView {
            node_id,
            row_index,
            direction,
            ..
        } if *node_id == crate::native_app::sample_library::sample_list::SAMPLE_BROWSER_LIST_ID => {
            Some((*row_index, *direction))
        }
        _ => None,
    }
}

fn read_test_wav_i16(path: &std::path::Path) -> Vec<i16> {
    let mut reader = hound::WavReader::open(path).expect("open wav");
    reader
        .samples::<i16>()
        .collect::<Result<Vec<_>, _>>()
        .expect("read samples")
}

fn assert_sample_rating(
    state: &crate::native_app::test_support::state::NativeAppState,
    path: &Path,
    rating: Rating,
    locked: bool,
) {
    let file_id = path.display().to_string();
    let row = state
        .library
        .folder_browser
        .loaded_source_audio_files()
        .into_iter()
        .find(|file| file.id == file_id)
        .expect("rated file should be loaded");
    assert_eq!(row.rating, rating);
    assert_eq!(row.rating_locked, locked);

    let (source_root, source_database_root, relative_path) = state
        .library
        .folder_browser
        .source_database_relative_file_path(path)
        .expect("rated file should belong to a source");
    let db =
        SourceDatabase::open_for_ui_read_with_database_root(source_root, &source_database_root)
            .expect("source database should open");
    let persisted = db
        .list_files()
        .expect("source database files should list")
        .into_iter()
        .find(|entry| entry.relative_path == relative_path)
        .expect("rated file should be persisted");
    assert_eq!(persisted.tag, rating);
    assert_eq!(persisted.locked, locked);
}

fn assert_sample_not_keep_rated(
    state: &crate::native_app::test_support::state::NativeAppState,
    path: &Path,
) {
    let file_id = path.display().to_string();
    let row = state
        .library
        .folder_browser
        .loaded_source_audio_files()
        .into_iter()
        .find(|file| file.id == file_id)
        .expect("source file should be loaded");
    assert_eq!(row.rating, Rating::NEUTRAL);
    assert!(!row.rating_locked);

    let Some((source_root, source_database_root, relative_path)) = state
        .library
        .folder_browser
        .source_database_relative_file_path(path)
    else {
        return;
    };
    let Ok(db) =
        SourceDatabase::open_for_ui_read_with_database_root(source_root, &source_database_root)
    else {
        return;
    };
    if let Some(persisted) = db
        .list_files()
        .expect("source database files should list")
        .into_iter()
        .find(|entry| entry.relative_path == relative_path)
    {
        assert_eq!(persisted.tag, Rating::NEUTRAL);
        assert!(!persisted.locked);
    }
}

fn run_command_collecting_followups(
    state: &mut crate::native_app::test_support::state::NativeAppState,
    command: Command<GuiMessage>,
) -> Vec<Command<GuiMessage>> {
    crate::native_app::tests::run_command_collecting_followups_for_tests(state, command)
}

fn assert_short_edge_faded_drag_extraction(path: &std::path::Path) {
    let samples = read_test_wav_i16(path);
    assert_eq!(samples.len(), 4);
    assert_eq!(
        samples,
        vec![0, 200, 300, 0],
        "drag extraction should preserve the selected interior and fade hard cut edges"
    );
}

#[test]
fn file_move_conflict_dialog_renders_resolution_choices() {
    let mut state = gui_state_for_span_tests();
    let source_root = tempfile::tempdir().expect("source root");
    let drums = source_root.path().join("drums");
    let loops = source_root.path().join("loops");
    fs::create_dir_all(&drums).expect("create drums folder");
    fs::create_dir_all(&loops).expect("create loops folder");
    let source = drums.join("kick.wav");
    let destination = loops.join("kick.wav");
    fs::write(&source, b"source").expect("write source");
    fs::write(&destination, b"destination").expect("write destination");
    state.library.folder_browser =
        FolderBrowserState::from_sample_sources(&[wavecrate::sample_sources::SampleSource::new(
            source_root.path().to_path_buf(),
        )]);
    state
        .library
        .folder_browser
        .apply_message(FolderBrowserMessage::ActivateFolder(
            drums.display().to_string(),
            Default::default(),
        ));
    state
        .library
        .folder_browser
        .select_file(source.display().to_string());
    state
        .library
        .folder_browser
        .begin_file_drag(source.display().to_string(), Point::new(4.0, 8.0));
    let mut context = radiant::prelude::UiUpdateContext::default();
    state.drop_browser_drag_on_folder(loops.display().to_string(), &mut context);
    run_command_for_tests(&mut state, context.into_command());

    let frame = view(&mut state).view_frame_at_size_with_default_theme(Vector2::new(900.0, 620.0));

    assert!(frame.paint_plan.contains_text("File Move Conflict"));
    assert!(frame.paint_plan.contains_text("Conflict 1 of 1"));
    assert!(frame.paint_plan.contains_text("kick.wav"));
    assert!(
        frame
            .paint_plan
            .contains_text("Apply to all remaining conflicts")
    );
    assert!(frame.paint_plan.contains_text("Overwrite"));
    assert!(frame.paint_plan.contains_text("Rename"));
    assert!(frame.paint_plan.contains_text("Skip"));
}

#[test]
fn successful_selected_file_copy_adds_keep_rating() {
    let mut state = gui_state_for_span_tests();
    let source_root = tempfile::tempdir().expect("source root");
    let drums = source_root.path().join("drums");
    fs::create_dir_all(&drums).expect("create drums folder");
    let kick = drums.join("kick.wav");
    write_test_wav_i16(&kick, &[0, 256, -256, 512]);
    state.library.folder_browser =
        FolderBrowserState::from_sample_sources(&[wavecrate::sample_sources::SampleSource::new(
            source_root.path().to_path_buf(),
        )]);
    state
        .library
        .folder_browser
        .apply_message(FolderBrowserMessage::ActivateFolder(
            drums.display().to_string(),
            Default::default(),
        ));
    state
        .library
        .folder_browser
        .select_file(kick.display().to_string());

    let mut context = ui::UiUpdateContext::default();
    state.finish_copy_selected_files(
        vec![kick.clone()],
        1,
        std::time::Instant::now(),
        Ok(()),
        &mut context,
    );

    assert_sample_rating(&state, &kick, Rating::KEEP_1, false);
}

#[test]
fn successful_selected_file_copy_increments_existing_keep_rating() {
    let mut state = gui_state_for_span_tests();
    let source_root = tempfile::tempdir().expect("source root");
    let drums = source_root.path().join("drums");
    fs::create_dir_all(&drums).expect("create drums folder");
    let kick = drums.join("kick.wav");
    write_test_wav_i16(&kick, &[0, 256, -256, 512]);
    state.library.folder_browser =
        FolderBrowserState::from_sample_sources(&[wavecrate::sample_sources::SampleSource::new(
            source_root.path().to_path_buf(),
        )]);
    state
        .library
        .folder_browser
        .apply_message(FolderBrowserMessage::ActivateFolder(
            drums.display().to_string(),
            Default::default(),
        ));
    state
        .library
        .folder_browser
        .select_file(kick.display().to_string());
    assert!(
        state
            .library
            .folder_browser
            .set_file_rating_state(&kick, Rating::KEEP_1, false)
    );

    let mut context = ui::UiUpdateContext::default();
    state.finish_copy_selected_files(
        vec![kick.clone()],
        1,
        std::time::Instant::now(),
        Ok(()),
        &mut context,
    );

    assert_sample_rating(&state, &kick, Rating::new(2), false);
}

#[test]
fn accepted_external_file_drag_adds_keep_rating() {
    let mut state = gui_state_for_span_tests();
    let source_root = tempfile::tempdir().expect("source root");
    let drums = source_root.path().join("drums");
    fs::create_dir_all(&drums).expect("create drums folder");
    let kick = drums.join("kick.wav");
    write_test_wav_i16(&kick, &[0, 256, -256, 512]);
    state.library.folder_browser =
        FolderBrowserState::from_sample_sources(&[wavecrate::sample_sources::SampleSource::new(
            source_root.path().to_path_buf(),
        )]);
    state
        .library
        .folder_browser
        .apply_message(FolderBrowserMessage::ActivateFolder(
            drums.display().to_string(),
            Default::default(),
        ));
    state
        .library
        .folder_browser
        .select_file(kick.display().to_string());
    state
        .library
        .folder_browser
        .begin_file_drag(kick.display().to_string(), Point::new(4.0, 8.0));
    let mut context = radiant::prelude::UiUpdateContext::default();
    state.arm_browser_drag(&mut context);

    state.external_drag_completed(
        Ok(radiant::runtime::ExternalDragOutcome {
            effect: radiant::runtime::ExternalDragEffect::Copy,
        }),
        &mut context,
    );

    assert_sample_rating(&state, &kick, Rating::KEEP_1, false);
}

#[test]
fn waveform_selection_drag_start_prepares_extraction_for_external_handoff() {
    let (mut state, _source_root, selected_file) = native_app_state_with_temp_sample("drag.wav");
    let source = std::path::PathBuf::from(&selected_file);
    write_test_wav_i16(&source, &[0, 256, -256, 512]);
    state.waveform.current =
        crate::native_app::test_support::state::WaveformState::load_path(source.clone())
            .expect("load waveform");
    state.waveform.current.set_play_selection_range(0.25, 0.75);
    let extraction = source.with_file_name("drag_extraction.wav");
    let mut context = ui::UiUpdateContext::default();

    assert!(state.drag_waveform_play_selection(
        DragHandleMessage::started(Point::new(20.0, 12.0)),
        &mut context,
    ));
    assert!(
        !extraction.is_file(),
        "drag start should queue extraction instead of writing on the UI thread"
    );
    run_command_for_tests(&mut state, context.into_command());
    assert!(
        extraction.is_file(),
        "running the extraction completion must create the file before native drag-out"
    );

    let mut end_context = ui::UiUpdateContext::default();
    assert!(state.drag_waveform_play_selection(
        DragHandleMessage::ended(Point::new(26.0, 12.0)),
        &mut end_context,
    ));

    assert!(
        extraction.is_file(),
        "cancelling the drag keeps the durable extraction that was offered externally"
    );
    assert!(!state.library.folder_browser.drag_active());
}

#[test]
fn accepted_waveform_selection_external_drag_does_not_add_whole_file_keep_rating() {
    let (mut state, _source_root, selected_file) =
        native_app_state_with_temp_sample("drag-partial-rating.wav");
    let source = std::path::PathBuf::from(&selected_file);
    write_test_wav_i16(&source, &[0, 256, -256, 512]);
    state.waveform.current =
        crate::native_app::test_support::state::WaveformState::load_path(source.clone())
            .expect("load waveform");
    state.waveform.current.set_play_selection_range(0.25, 0.75);
    let extraction = source.with_file_name("drag-partial-rating_extraction.wav");
    let mut drag_context = ui::UiUpdateContext::default();

    assert!(state.drag_waveform_play_selection(
        DragHandleMessage::started(Point::new(20.0, 12.0)),
        &mut drag_context,
    ));
    let followups = run_command_collecting_followups(&mut state, drag_context.into_command());
    for command in followups {
        run_command_for_tests(&mut state, command);
    }
    assert_sample_rating(&state, &extraction, Rating::KEEP_1, false);

    let mut completion_context = ui::UiUpdateContext::default();
    state.external_drag_completed(
        Ok(radiant::runtime::ExternalDragOutcome {
            effect: radiant::runtime::ExternalDragEffect::Copy,
        }),
        &mut completion_context,
    );

    assert_sample_rating(&state, &extraction, Rating::KEEP_1, false);
    assert_sample_not_keep_rated(&state, &source);
}

#[test]
fn loaded_waveform_sample_drag_moves_file_to_hovered_folder() {
    let mut state = gui_state_for_span_tests();
    let source_root = tempfile::tempdir().expect("source root");
    let drums = source_root.path().join("drums");
    let loops = source_root.path().join("loops");
    fs::create_dir_all(&drums).expect("create drums folder");
    fs::create_dir_all(&loops).expect("create loops folder");
    let drums_id = drums.display().to_string();
    let loops_id = loops.display().to_string();
    let source = drums.join("loaded-drag.wav");
    write_test_wav_i16(&source, &[0, 256, -256, 512]);
    state.library.folder_browser =
        FolderBrowserState::from_sample_sources(&[wavecrate::sample_sources::SampleSource::new(
            source_root.path().to_path_buf(),
        )]);
    state
        .library
        .folder_browser
        .apply_message(FolderBrowserMessage::ActivateFolder(
            drums_id.clone(),
            Default::default(),
        ));
    state.waveform.current =
        crate::native_app::test_support::state::WaveformState::load_path(source.clone())
            .expect("load waveform");
    let mut context = ui::UiUpdateContext::default();

    assert!(state.drag_loaded_waveform_sample(
        DragHandleMessage::started(Point::new(20.0, 12.0)),
        &mut context,
    ));
    state
        .library
        .folder_browser
        .apply_message(FolderBrowserMessage::HoverDropTarget(
            loops_id,
            Point::new(40.0, 12.0),
        ));
    assert!(state.drag_loaded_waveform_sample(
        DragHandleMessage::ended(Point::new(40.0, 12.0)),
        &mut context,
    ));
    run_command_for_tests(&mut state, context.into_command());

    let moved = loops.join("loaded-drag.wav");
    assert!(!source.exists());
    assert!(moved.is_file());
    assert_eq!(state.waveform.current.path(), moved);
    assert_eq!(
        state.library.folder_browser.selected_folder_id(),
        Some(drums_id.as_str())
    );
}

#[test]
fn loaded_waveform_sample_drag_reports_missing_file_from_move_worker() {
    let mut state = gui_state_for_span_tests();
    let source_root = tempfile::tempdir().expect("source root");
    let drums = source_root.path().join("drums");
    let loops = source_root.path().join("loops");
    fs::create_dir_all(&drums).expect("create drums folder");
    fs::create_dir_all(&loops).expect("create loops folder");
    let source = drums.join("missing-loaded-drag.wav");
    write_test_wav_i16(&source, &[0, 256, -256, 512]);
    state.library.folder_browser =
        FolderBrowserState::from_sample_sources(&[wavecrate::sample_sources::SampleSource::new(
            source_root.path().to_path_buf(),
        )]);
    state
        .library
        .folder_browser
        .apply_message(FolderBrowserMessage::ActivateFolder(
            drums.display().to_string(),
            Default::default(),
        ));
    state.waveform.current =
        crate::native_app::test_support::state::WaveformState::load_path(source.clone())
            .expect("load waveform");
    fs::remove_file(&source).expect("remove loaded sample before drag");
    let mut context = ui::UiUpdateContext::default();

    assert!(state.drag_loaded_waveform_sample(
        DragHandleMessage::started(Point::new(20.0, 12.0)),
        &mut context,
    ));
    state
        .library
        .folder_browser
        .apply_message(FolderBrowserMessage::HoverDropTarget(
            loops.display().to_string(),
            Point::new(40.0, 12.0),
        ));
    assert!(state.drag_loaded_waveform_sample(
        DragHandleMessage::ended(Point::new(40.0, 12.0)),
        &mut context,
    ));
    run_command_for_tests(&mut state, context.into_command());

    assert!(!state.library.folder_browser.drag_active());
    assert!(
        state
            .ui
            .status
            .sample
            .contains("File move failed: missing-loaded-drag.wav is missing"),
        "{}",
        state.ui.status.sample
    );
}

#[test]
fn waveform_selection_drag_uses_prepared_extraction_for_sample_list_drop() {
    let (mut state, _source_root, selected_file) =
        native_app_state_with_temp_sample("sample-list-drop.wav");
    let source = std::path::PathBuf::from(&selected_file);
    write_test_wav_i16(&source, &[0, 100, 200, 300, 400, 500]);
    state.waveform.current =
        crate::native_app::test_support::state::WaveformState::load_path(source.clone())
            .expect("load waveform");
    state.waveform.current.set_play_selection_range(0.25, 0.75);
    let extraction = source.with_file_name("sample-list-drop_extraction.wav");
    let mut drag_context = ui::UiUpdateContext::default();

    assert!(state.drag_waveform_play_selection(
        DragHandleMessage::started(Point::new(20.0, 12.0)),
        &mut drag_context,
    ));
    run_command_for_tests(&mut state, drag_context.into_command());
    assert!(extraction.is_file());

    let mut drop_context = ui::UiUpdateContext::default();
    state.drop_waveform_play_selection_on_sample_list(&mut drop_context);

    assert!(extraction.is_file());
    assert_short_edge_faded_drag_extraction(&extraction);
    assert_eq!(
        state.ui.status.sample,
        "Extracted sample-list-drop_extraction.wav"
    );
    assert!(!state.library.folder_browser.drag_active());
}

#[test]
fn waveform_selection_drag_extracts_into_dropped_folder() {
    let mut state = gui_state_for_span_tests();
    let source_root = tempfile::tempdir().expect("source root");
    let drums = source_root.path().join("drums");
    let loops = source_root.path().join("loops");
    fs::create_dir_all(&drums).expect("create drums folder");
    fs::create_dir_all(&loops).expect("create loops folder");
    let source = drums.join("folder-drop.wav");
    write_test_wav_i16(&source, &[0, 100, 200, 300, 400, 500]);
    state.library.folder_browser =
        FolderBrowserState::from_sample_sources(&[wavecrate::sample_sources::SampleSource::new(
            source_root.path().to_path_buf(),
        )]);
    state
        .library
        .folder_browser
        .apply_message(FolderBrowserMessage::ActivateFolder(
            drums.display().to_string(),
            Default::default(),
        ));
    state
        .library
        .folder_browser
        .select_file(source.display().to_string());
    state.waveform.current =
        crate::native_app::test_support::state::WaveformState::load_path(source.clone())
            .expect("load waveform");
    state.waveform.current.set_play_selection_range(0.25, 0.75);
    let default_extraction = drums.join("folder-drop_extraction.wav");
    let dropped_extraction = loops.join("folder-drop_extraction.wav");
    let mut drag_context = ui::UiUpdateContext::default();

    assert!(state.drag_waveform_play_selection(
        DragHandleMessage::started(Point::new(20.0, 12.0)),
        &mut drag_context,
    ));
    run_command_for_tests(&mut state, drag_context.into_command());
    assert!(default_extraction.exists());
    assert!(!dropped_extraction.exists());

    let mut drop_context = ui::UiUpdateContext::default();
    state.drop_browser_drag_on_folder(loops.display().to_string(), &mut drop_context);
    run_command_for_tests(&mut state, drop_context.into_command());

    assert!(!default_extraction.exists());
    assert!(dropped_extraction.is_file());
    assert_short_edge_faded_drag_extraction(&dropped_extraction);
    assert_eq!(
        state.ui.status.sample,
        "Moved sample folder-drop_extraction.wav"
    );
    assert!(!state.library.folder_browser.drag_active());
}

#[test]
fn moving_selected_file_loads_next_visible_sample() {
    let mut state = gui_state_for_span_tests();
    let source_root = tempfile::tempdir().expect("source root");
    let drums = source_root.path().join("drums");
    let loops = source_root.path().join("loops");
    fs::create_dir_all(&drums).expect("create drums folder");
    fs::create_dir_all(&loops).expect("create loops folder");
    let first = drums.join("a-kick.wav");
    let second = drums.join("b-snare.wav");
    write_test_wav_i16(&first, &[0, 256, -256, 512]);
    write_test_wav_i16(&second, &[0, 1024, -2048, 4096, -1024, 512]);
    let first_id = first.display().to_string();
    let second_id = second.display().to_string();

    state.library.folder_browser =
        FolderBrowserState::from_sample_sources(&[wavecrate::sample_sources::SampleSource::new(
            source_root.path().to_path_buf(),
        )]);
    state
        .library
        .folder_browser
        .apply_message(FolderBrowserMessage::ActivateFolder(
            drums.display().to_string(),
            Default::default(),
        ));
    state.library.folder_browser.select_file(first_id.clone());
    state
        .library
        .folder_browser
        .begin_file_drag(first_id, Point::new(4.0, 8.0));
    let request = match state
        .library
        .folder_browser
        .drop_drag_on_folder(&loops.display().to_string())
        .expect("drop should be accepted")
    {
        crate::native_app::sample_library::folder_browser::commands::FolderMoveDropInput::Request(
            request,
        ) => request,
        other => panic!("expected file move request, got {other:?}"),
    };
    let completion =
        crate::native_app::sample_library::folder_browser::commands::execute_folder_move_request(
            request,
        );
    let mut context = radiant::prelude::UiUpdateContext::default();

    state.finish_folder_move(std::time::Instant::now(), completion, &mut context);
    run_command_for_tests(&mut state, context.into_command());

    assert_eq!(
        state.library.folder_browser.selected_file_id(),
        Some(second_id.as_str())
    );
    assert_eq!(state.waveform.load.label.as_deref(), Some("b-snare.wav"));
    assert!(
        state.active_sample_load_task().is_some(),
        "moving the selected file should queue autoplay loading for the replacement selection"
    );
}

#[test]
fn moving_folder_registers_undo_redo_transaction() {
    let mut state = gui_state_for_span_tests();
    let source_root = tempfile::tempdir().expect("source root");
    let drums = source_root.path().join("drums");
    let kicks = drums.join("kicks");
    let loops = source_root.path().join("loops");
    fs::create_dir_all(&kicks).expect("create kicks folder");
    fs::create_dir_all(&loops).expect("create loops folder");
    let kick = kicks.join("kick.wav");
    write_test_wav_i16(&kick, &[0, 256, -256, 512]);
    state.waveform.current = WaveformState::load_path(kick.clone()).expect("load kick");
    let stale_play_selection = WaveformPlaySelectionSnapshot {
        path: kick.clone(),
        play_mark_ratio: Some(0.25),
        play_selection: Some(SelectionRange::new(0.25, 0.5)),
        marked_play_ranges: Vec::new(),
    };
    let stale_undo = stale_play_selection.clone();
    let stale_redo = stale_play_selection;
    state.register_transaction_action(
        "Change play mark selection",
        move |transaction| transaction.restore_play_selection(stale_undo.clone()),
        move |transaction| transaction.restore_play_selection(stale_redo.clone()),
    );
    assert_eq!(state.transactions.history.list_items().len(), 1);
    state.library.folder_browser =
        FolderBrowserState::from_sample_sources(&[wavecrate::sample_sources::SampleSource::new(
            source_root.path().to_path_buf(),
        )]);
    state
        .library
        .folder_browser
        .apply_message(FolderBrowserMessage::ActivateFolder(
            drums.display().to_string(),
            Default::default(),
        ));
    state.library.folder_browser.expand_selected_folder();
    state
        .library
        .folder_browser
        .apply_message(FolderBrowserMessage::DragFolder(
            kicks.display().to_string(),
            DragHandleMessage::started(Point::new(4.0, 8.0)),
        ));
    let request = match state
        .library
        .folder_browser
        .drop_drag_on_folder(&loops.display().to_string())
        .expect("drop should be accepted")
    {
        crate::native_app::sample_library::folder_browser::commands::FolderMoveDropInput::Request(
            request,
        ) => request,
        other => panic!("expected folder move request, got {other:?}"),
    };
    let completion =
        crate::native_app::sample_library::folder_browser::commands::execute_folder_move_request(
            request,
        );
    let mut context = radiant::prelude::UiUpdateContext::default();

    state.finish_folder_move(std::time::Instant::now(), completion, &mut context);
    run_command_for_tests(&mut state, context.into_command());

    let moved_kicks = loops.join("kicks");
    assert!(!kicks.exists());
    assert!(moved_kicks.join("kick.wav").is_file());
    assert_eq!(state.transactions.history.list_items().len(), 1);
    assert_eq!(
        state.transactions.history.list_items()[0].label,
        "Move folder"
    );

    let command = reduce_gui_message_for_tests(&mut state, GuiMessage::UndoTransaction);
    run_command_for_tests(&mut state, command);
    assert_eq!(state.ui.status.sample, "Undid Move folder");
    assert!(kicks.join("kick.wav").is_file());
    assert!(!moved_kicks.exists());

    let command = reduce_gui_message_for_tests(&mut state, GuiMessage::RedoTransaction);
    run_command_for_tests(&mut state, command);
    assert_eq!(state.ui.status.sample, "Redid Move folder");
    assert!(!kicks.exists());
    assert!(moved_kicks.join("kick.wav").is_file());
}

#[test]
fn moving_scrolled_sample_materializes_source_rows_and_loads_next_sample() {
    let mut state = gui_state_for_span_tests();
    let source_root = tempfile::tempdir().expect("source root");
    let drums = source_root.path().join("drums");
    let loops = source_root.path().join("loops");
    fs::create_dir_all(&drums).expect("create drums folder");
    fs::create_dir_all(&loops).expect("create loops folder");
    let samples = (0..72)
        .map(|index| drums.join(format!("sample_{index:03}.wav")))
        .collect::<Vec<_>>();
    for sample in &samples {
        write_test_wav_i16(sample, &[0, 256, -256, 512]);
    }
    state.library.folder_browser =
        FolderBrowserState::from_sample_sources(&[wavecrate::sample_sources::SampleSource::new(
            source_root.path().to_path_buf(),
        )]);
    state
        .library
        .folder_browser
        .apply_message(FolderBrowserMessage::ActivateFolder(
            drums.display().to_string(),
            Default::default(),
        ));
    state
        .library
        .folder_browser
        .apply_file_view_window_change(ui::VirtualListWindowChange {
            offset_y: 50.0
                * crate::native_app::test_support::sample_browser::SAMPLE_BROWSER_ROW_HEIGHT,
            row_height: crate::native_app::test_support::sample_browser::SAMPLE_BROWSER_ROW_HEIGHT,
            window: ui::VirtualListWindow {
                total_items: 72,
                viewport_start: 50,
                viewport_end: 68,
                window_start: 46,
                window_end: 72,
            },
        });
    let moved_id = samples[60].display().to_string();
    state.library.folder_browser.select_file(moved_id.clone());
    state
        .library
        .folder_browser
        .begin_file_drag(moved_id, Point::new(4.0, 8.0));
    let request = match state
        .library
        .folder_browser
        .drop_drag_on_folder(&loops.display().to_string())
        .expect("drop should be accepted")
    {
        crate::native_app::sample_library::folder_browser::commands::FolderMoveDropInput::Request(
            request,
        ) => request,
        other => panic!("expected file move request, got {other:?}"),
    };
    let completion =
        crate::native_app::sample_library::folder_browser::commands::execute_folder_move_request(
            request,
        );
    let mut context = radiant::prelude::UiUpdateContext::default();

    state.finish_folder_move(std::time::Instant::now(), completion, &mut context);
    run_command_for_tests(&mut state, context.into_command());

    let drums_id = drums.display().to_string();
    let replacement = samples[61].display().to_string();
    assert_eq!(
        state.library.folder_browser.selected_folder_id(),
        Some(drums_id.as_str())
    );
    assert_eq!(
        state.library.folder_browser.selected_file_id(),
        Some(replacement.as_str())
    );
    assert_eq!(state.waveform.load.label.as_deref(), Some("sample_061.wav"));
    assert!(
        state.active_sample_load_task().is_some(),
        "moving a scrolled selected file should queue autoplay loading for the replacement"
    );

    crate::native_app::test_support::sample_browser::prepare_sample_browser_view(&mut state);
    let projection =
        crate::native_app::test_support::sample_browser::sample_browser_window_projection(
            &state, 64,
        );

    assert_eq!(projection.total_count, 71);
    assert_eq!(projection.visible_rows, projection.window_len);
    assert!(
        projection
            .first_stems
            .iter()
            .any(|stem| stem == "sample_061"),
        "remaining rows should include the next sample after the moved file: {:?}",
        projection.first_stems
    );
}

#[test]
fn cut_paste_selected_files_moves_audio_into_selected_folder() {
    let mut state = gui_state_for_span_tests();
    let source_root = tempfile::tempdir().expect("source root");
    let drums = source_root.path().join("drums");
    let loops = source_root.path().join("loops");
    fs::create_dir_all(&drums).expect("create drums folder");
    fs::create_dir_all(&loops).expect("create loops folder");
    let kick = drums.join("kick.wav");
    let snare = drums.join("snare.wav");
    let hat = drums.join("hat.wav");
    for file in [&kick, &snare, &hat] {
        write_test_wav_i16(file, &[0, 256, -256, 512]);
    }
    let db = SourceDatabase::open_for_test_fixture_source_write(source_root.path())
        .expect("open source db");
    let kick_relative = Path::new("drums/kick.wav");
    let snare_relative = Path::new("drums/snare.wav");
    db.upsert_file(kick_relative, 8, 1)
        .expect("register kick metadata row");
    db.upsert_file(snare_relative, 8, 1)
        .expect("register snare metadata row");
    let mut batch = db.write_batch().expect("open metadata batch");
    batch
        .set_tag(kick_relative, Rating::new(2))
        .expect("set kick rating");
    batch
        .set_locked(kick_relative, true)
        .expect("lock kick rating");
    batch
        .set_looped(kick_relative, true)
        .expect("set kick loop marker");
    batch
        .assign_tag_to_path(kick_relative, "Analog Kick")
        .expect("tag kick");
    batch
        .assign_tag_to_path(kick_relative, "loop")
        .expect("tag kick playback type");
    batch
        .assign_tag_to_path(snare_relative, "Snappy Snare")
        .expect("tag snare");
    batch
        .assign_tag_to_path(snare_relative, "one-shot")
        .expect("tag snare playback type");
    batch.commit().expect("commit metadata");

    state.library.folder_browser =
        FolderBrowserState::from_sample_sources(&[wavecrate::sample_sources::SampleSource::new(
            source_root.path().to_path_buf(),
        )]);
    let source_id = state
        .library
        .folder_browser
        .selected_source_id()
        .to_string();
    state.refresh_persisted_metadata_tags_for_source(&source_id);
    state
        .library
        .folder_browser
        .apply_message(FolderBrowserMessage::ActivateFolder(
            drums.display().to_string(),
            Default::default(),
        ));
    state
        .library
        .folder_browser
        .select_file(kick.display().to_string());
    state.library.folder_browser.select_file_with_modifiers(
        snare.display().to_string(),
        PointerModifiers {
            command: true,
            ..Default::default()
        },
    );

    state.cut_selected_files();

    assert_eq!(state.ui.status.sample, "Cut 2 selected files");
    assert_eq!(
        state
            .ui
            .browser_interaction
            .cut_file_clipboard
            .as_ref()
            .map(|clipboard| clipboard.len()),
        Some(2)
    );

    state
        .library
        .folder_browser
        .apply_message(FolderBrowserMessage::ActivateFolder(
            loops.display().to_string(),
            Default::default(),
        ));
    let mut context = ui::UiUpdateContext::default();

    state.paste_cut_files(&mut context);
    assert!(
        state.ui.browser_interaction.cut_file_clipboard.is_some(),
        "cut buffer should stay available until the queued paste succeeds"
    );
    assert!(
        state
            .ui
            .browser_interaction
            .cut_file_paste_task_id
            .is_some(),
        "pasting should track the queued move task"
    );
    run_command_for_tests(&mut state, context.into_command());

    let moved_kick = loops.join("kick.wav");
    let moved_snare = loops.join("snare.wav");
    let moved_kick_id = moved_kick.to_string_lossy().to_string();
    let moved_snare_id = moved_snare.to_string_lossy().to_string();
    assert!(!kick.exists());
    assert!(!snare.exists());
    assert!(hat.is_file());
    assert!(moved_kick.is_file());
    assert!(moved_snare.is_file());
    assert!(
        !state
            .metadata
            .tags_by_file
            .contains_key(kick.to_string_lossy().as_ref())
    );
    assert!(
        !state
            .metadata
            .tags_by_file
            .contains_key(snare.to_string_lossy().as_ref())
    );
    assert_eq!(
        state.metadata.tags_by_file.get(&moved_kick_id),
        Some(&vec![String::from("Analog Kick"), String::from("loop")])
    );
    assert_eq!(
        state.metadata.tags_by_file.get(&moved_snare_id),
        Some(&vec![
            String::from("one-shot"),
            String::from("Snappy Snare")
        ])
    );
    assert_eq!(
        db.tag_for_path(kick_relative)
            .expect("read old kick rating"),
        None
    );
    assert_eq!(
        db.tag_for_path(Path::new("loops/kick.wav"))
            .expect("read moved kick rating"),
        Some(Rating::new(2))
    );
    assert_eq!(
        db.looped_for_path(Path::new("loops/kick.wav"))
            .expect("read moved kick loop marker"),
        Some(true)
    );
    assert!(
        db.locked_for_path(Path::new("loops/kick.wav"))
            .expect("read moved kick lock")
            .unwrap_or(false)
    );
    assert_eq!(
        db.tag_labels_for_path(Path::new("loops/kick.wav"))
            .expect("read moved kick tags"),
        vec![String::from("Analog Kick"), String::from("loop")]
    );
    assert_eq!(
        db.tag_labels_for_path(Path::new("loops/snare.wav"))
            .expect("read moved snare tags"),
        vec![String::from("one-shot"), String::from("Snappy Snare")]
    );
    assert!(state.ui.browser_interaction.cut_file_clipboard.is_none());
    assert_eq!(
        state.library.folder_browser.selected_file_paths(),
        vec![moved_kick.clone(), moved_snare.clone()]
    );
    assert_eq!(
        state
            .library
            .folder_browser
            .selected_audio_files()
            .iter()
            .map(|file| file.id.clone())
            .collect::<Vec<_>>(),
        vec![
            moved_kick.display().to_string(),
            moved_snare.display().to_string()
        ]
    );
}

#[test]
fn protected_cut_paste_to_primary_records_harvest_copy_derivation() {
    let config_base = tempfile::tempdir().expect("config base");
    let _base_guard = wavecrate::app_dirs::ConfigBaseGuard::set(config_base.path().to_path_buf());
    let mut state = gui_state_for_span_tests();
    let protected_root = tempfile::tempdir().expect("protected source root");
    let primary_root = tempfile::tempdir().expect("primary source root");
    let drums = protected_root.path().join("drums");
    let inbox = primary_root.path().join("_Wavecrate Inbox");
    fs::create_dir_all(&drums).expect("create protected folder");
    fs::create_dir_all(&inbox).expect("create primary inbox");
    let kick = drums.join("kick.wav");
    fs::write(&kick, [0_u8; 8]).expect("write kick");

    let protected_source = wavecrate::sample_sources::SampleSource::new_with_id(
        wavecrate::sample_sources::SourceId::from_string("protected-harvest-copy-source"),
        protected_root.path().to_path_buf(),
    )
    .protected();
    let primary_source = wavecrate::sample_sources::SampleSource::new_with_id(
        wavecrate::sample_sources::SourceId::from_string("primary-harvest-copy-source"),
        primary_root.path().to_path_buf(),
    )
    .primary();
    let protected_db_root = protected_source
        .database_root()
        .expect("protected metadata root");
    let protected_db = SourceDatabase::open_for_user_metadata_write_with_database_root(
        &protected_source.root,
        &protected_db_root,
    )
    .expect("open protected db");
    let source_relative = Path::new("drums/kick.wav");
    let mut batch = protected_db.write_batch().expect("open protected batch");
    batch
        .upsert_file_with_hash(source_relative, 8, 1, "harvest-copy-hash")
        .expect("upsert source row");
    batch
        .set_tag(source_relative, Rating::new(3))
        .expect("set source rating");
    batch.commit().expect("commit source metadata");

    state.library.folder_browser = FolderBrowserState::from_sample_sources(&[
        protected_source.clone(),
        primary_source.clone(),
    ]);
    state
        .library
        .folder_browser
        .apply_message(FolderBrowserMessage::ActivateFolder(
            drums.display().to_string(),
            Default::default(),
        ));
    state
        .library
        .folder_browser
        .select_file(kick.display().to_string());
    state.cut_selected_files();

    let scan_request = state
        .library
        .folder_browser
        .begin_select_source(primary_source.id.as_str().to_string(), 41)
        .expect("primary source should need loading");
    let scan_result =
        crate::native_app::sample_library::folder_browser::scan::scan_source_with_progress(
            scan_request,
            |_| {},
            |_| {},
        );
    assert!(
        state
            .library
            .folder_browser
            .apply_scan_finished(scan_result)
    );
    state
        .library
        .folder_browser
        .apply_message(FolderBrowserMessage::ActivateFolder(
            inbox.display().to_string(),
            Default::default(),
        ));
    let mut context = ui::UiUpdateContext::default();

    state.paste_cut_files(&mut context);
    run_command_for_tests(&mut state, context.into_command());

    let copied = inbox.join("kick.wav");
    assert!(kick.is_file(), "protected source file should remain");
    assert!(
        copied.is_file(),
        "copy should be written into primary source"
    );
    let parent_key = wavecrate::sample_sources::HarvestFileKey::new(
        protected_source.id.clone(),
        Path::new("drums/kick.wav").to_path_buf(),
    );
    let parent = wavecrate::sample_sources::library::harvest_file(&parent_key)
        .expect("load harvest parent")
        .expect("harvest parent");
    assert_eq!(
        parent.state,
        wavecrate::sample_sources::HarvestState::Touched
    );
    let edges = wavecrate::sample_sources::library::harvest_derivations_for_parent(&parent_key)
        .expect("load harvest derivations");
    assert_eq!(edges.len(), 1);
    assert_eq!(
        edges[0].operation,
        wavecrate::sample_sources::HarvestDerivationOperation::CopyToPrimary
    );
    assert_eq!(edges[0].child.key.source_id, primary_source.id);
    assert_eq!(
        edges[0].child.key.relative_path,
        Path::new("_Wavecrate Inbox/kick.wav").to_path_buf()
    );
    assert_eq!(edges[0].inherited_metadata.rating, Some(3));
}

#[test]
fn protected_cut_paste_to_writable_source_records_harvest_copy_derivation() {
    let config_base = tempfile::tempdir().expect("config base");
    let _base_guard = wavecrate::app_dirs::ConfigBaseGuard::set(config_base.path().to_path_buf());
    let mut state = gui_state_for_span_tests();
    let protected_root = tempfile::tempdir().expect("protected source root");
    let target_root = tempfile::tempdir().expect("target source root");
    let drums = protected_root.path().join("drums");
    let inbox = target_root.path().join("inbox");
    fs::create_dir_all(&drums).expect("create protected folder");
    fs::create_dir_all(&inbox).expect("create target inbox");
    let kick = drums.join("kick.wav");
    fs::write(&kick, [0_u8; 8]).expect("write kick");

    let protected_source = wavecrate::sample_sources::SampleSource::new_with_id(
        wavecrate::sample_sources::SourceId::from_string("protected-harvest-copy-any-source"),
        protected_root.path().to_path_buf(),
    )
    .protected();
    let target_source = wavecrate::sample_sources::SampleSource::new_with_id(
        wavecrate::sample_sources::SourceId::from_string("target-harvest-copy-any-source"),
        target_root.path().to_path_buf(),
    );
    let protected_db_root = protected_source
        .database_root()
        .expect("protected metadata root");
    let protected_db = SourceDatabase::open_for_user_metadata_write_with_database_root(
        &protected_source.root,
        &protected_db_root,
    )
    .expect("open protected db");
    let source_relative = Path::new("drums/kick.wav");
    let mut batch = protected_db.write_batch().expect("open protected batch");
    batch
        .upsert_file_with_hash(source_relative, 8, 1, "harvest-copy-any-hash")
        .expect("upsert source row");
    batch
        .set_tag(source_relative, Rating::new(2))
        .expect("set source rating");
    batch.commit().expect("commit source metadata");

    state.library.folder_browser =
        FolderBrowserState::from_sample_sources(&[protected_source.clone(), target_source.clone()]);
    state
        .library
        .folder_browser
        .apply_message(FolderBrowserMessage::ActivateFolder(
            drums.display().to_string(),
            Default::default(),
        ));
    state
        .library
        .folder_browser
        .select_file(kick.display().to_string());
    state.cut_selected_files();

    let scan_request = state
        .library
        .folder_browser
        .begin_select_source(target_source.id.as_str().to_string(), 41)
        .expect("target source should need loading");
    let scan_result =
        crate::native_app::sample_library::folder_browser::scan::scan_source_with_progress(
            scan_request,
            |_| {},
            |_| {},
        );
    assert!(
        state
            .library
            .folder_browser
            .apply_scan_finished(scan_result)
    );
    state
        .library
        .folder_browser
        .apply_message(FolderBrowserMessage::ActivateFolder(
            inbox.display().to_string(),
            Default::default(),
        ));
    let mut context = ui::UiUpdateContext::default();

    state.paste_cut_files(&mut context);
    run_command_for_tests(&mut state, context.into_command());

    let copied = inbox.join("kick.wav");
    assert!(kick.is_file(), "protected source file should remain");
    assert!(
        copied.is_file(),
        "copy should be written into target source"
    );
    let parent_key = wavecrate::sample_sources::HarvestFileKey::new(
        protected_source.id.clone(),
        Path::new("drums/kick.wav").to_path_buf(),
    );
    let parent = wavecrate::sample_sources::library::harvest_file(&parent_key)
        .expect("load harvest parent")
        .expect("harvest parent");
    assert_eq!(
        parent.state,
        wavecrate::sample_sources::HarvestState::Touched
    );
    let edges = wavecrate::sample_sources::library::harvest_derivations_for_parent(&parent_key)
        .expect("load harvest derivations");
    assert_eq!(edges.len(), 1);
    assert_eq!(
        edges[0].operation,
        wavecrate::sample_sources::HarvestDerivationOperation::Copy
    );
    assert_eq!(edges[0].child.key.source_id, target_source.id);
    assert_eq!(
        edges[0].child.key.relative_path,
        Path::new("inbox/kick.wav").to_path_buf()
    );
    assert_eq!(edges[0].inherited_metadata.rating, Some(2));
}

#[test]
fn moving_harvest_origin_remaps_derivation_graph_to_new_path() {
    let config_base = tempfile::tempdir().expect("config base");
    let _base_guard = wavecrate::app_dirs::ConfigBaseGuard::set(config_base.path().to_path_buf());
    let mut state = gui_state_for_span_tests();
    let source_root = tempfile::tempdir().expect("source root");
    let drums = source_root.path().join("drums");
    let loops = source_root.path().join("loops");
    fs::create_dir_all(&drums).expect("create drums folder");
    fs::create_dir_all(&loops).expect("create loops folder");
    let kick = drums.join("kick.wav");
    let chop = drums.join("kick_chop.wav");
    fs::write(&kick, [0_u8; 8]).expect("write kick");
    fs::write(&chop, [1_u8; 8]).expect("write chop");

    let source = wavecrate::sample_sources::SampleSource::new_with_id(
        wavecrate::sample_sources::SourceId::from_string("harvest-file-move-source"),
        source_root.path().to_path_buf(),
    );
    let old_parent_key = wavecrate::sample_sources::HarvestFileKey::new(
        source.id.clone(),
        Path::new("drums/kick.wav").to_path_buf(),
    );
    let child_key = wavecrate::sample_sources::HarvestFileKey::new(
        source.id.clone(),
        Path::new("drums/kick_chop.wav").to_path_buf(),
    );
    wavecrate::sample_sources::library::record_harvest_derivation(
        &wavecrate::sample_sources::NewHarvestDerivation {
            parent: wavecrate::sample_sources::HarvestFileIdentity::new(old_parent_key.clone()),
            child: wavecrate::sample_sources::HarvestFileIdentity::new(child_key.clone()),
            operation: wavecrate::sample_sources::HarvestDerivationOperation::Extract,
            source_range: None,
            output_duration_seconds: None,
            destination_folder: Some(Path::new("drums").to_path_buf()),
            inherited_metadata: wavecrate::sample_sources::HarvestMetadataSnapshot::default(),
            tool_version: String::from("test"),
        },
    )
    .expect("record harvest edge");

    state.library.folder_browser = FolderBrowserState::from_sample_sources(&[source.clone()]);
    state
        .library
        .folder_browser
        .apply_message(FolderBrowserMessage::ActivateFolder(
            drums.display().to_string(),
            Default::default(),
        ));
    state
        .library
        .folder_browser
        .select_file(kick.display().to_string());
    state
        .library
        .folder_browser
        .begin_file_drag(kick.display().to_string(), Point::new(4.0, 8.0));
    let request = match state
        .library
        .folder_browser
        .drop_drag_on_folder(&loops.display().to_string())
        .expect("drop should be accepted")
    {
        crate::native_app::sample_library::folder_browser::commands::FolderMoveDropInput::Request(
            request,
        ) => request,
        other => panic!("expected file move request, got {other:?}"),
    };
    let completion =
        crate::native_app::sample_library::folder_browser::commands::execute_folder_move_request(
            request,
        );
    let mut context = ui::UiUpdateContext::default();

    state.finish_folder_move(std::time::Instant::now(), completion, &mut context);
    run_command_for_tests(&mut state, context.into_command());

    let new_parent_key = wavecrate::sample_sources::HarvestFileKey::new(
        source.id,
        Path::new("loops/kick.wav").to_path_buf(),
    );
    assert!(
        wavecrate::sample_sources::library::harvest_derivations_for_parent(&old_parent_key)
            .expect("load old parent edges")
            .is_empty()
    );
    let edges = wavecrate::sample_sources::library::harvest_derivations_for_parent(&new_parent_key)
        .expect("load new parent edges");
    assert_eq!(edges.len(), 1);
    assert_eq!(edges[0].parent.key, new_parent_key);
    assert_eq!(edges[0].child.key, child_key);
}

#[test]
fn moving_harvest_folder_remaps_derivation_graph_prefix() {
    let config_base = tempfile::tempdir().expect("config base");
    let _base_guard = wavecrate::app_dirs::ConfigBaseGuard::set(config_base.path().to_path_buf());
    let mut state = gui_state_for_span_tests();
    let source_root = tempfile::tempdir().expect("source root");
    let kicks = source_root.path().join("drums").join("kicks");
    let loops = source_root.path().join("loops");
    fs::create_dir_all(&kicks).expect("create kicks folder");
    fs::create_dir_all(&loops).expect("create loops folder");
    let kick = kicks.join("kick.wav");
    let chop = kicks.join("kick_chop.wav");
    fs::write(&kick, [0_u8; 8]).expect("write kick");
    fs::write(&chop, [1_u8; 8]).expect("write chop");

    let source = wavecrate::sample_sources::SampleSource::new_with_id(
        wavecrate::sample_sources::SourceId::from_string("harvest-folder-move-source"),
        source_root.path().to_path_buf(),
    );
    let old_parent_key = wavecrate::sample_sources::HarvestFileKey::new(
        source.id.clone(),
        Path::new("drums/kicks/kick.wav").to_path_buf(),
    );
    let old_child_key = wavecrate::sample_sources::HarvestFileKey::new(
        source.id.clone(),
        Path::new("drums/kicks/kick_chop.wav").to_path_buf(),
    );
    wavecrate::sample_sources::library::record_harvest_derivation(
        &wavecrate::sample_sources::NewHarvestDerivation {
            parent: wavecrate::sample_sources::HarvestFileIdentity::new(old_parent_key.clone()),
            child: wavecrate::sample_sources::HarvestFileIdentity::new(old_child_key.clone()),
            operation: wavecrate::sample_sources::HarvestDerivationOperation::Extract,
            source_range: None,
            output_duration_seconds: None,
            destination_folder: Some(Path::new("drums/kicks").to_path_buf()),
            inherited_metadata: wavecrate::sample_sources::HarvestMetadataSnapshot::default(),
            tool_version: String::from("test"),
        },
    )
    .expect("record harvest edge");

    state.library.folder_browser = FolderBrowserState::from_sample_sources(&[source.clone()]);
    state
        .library
        .folder_browser
        .apply_message(FolderBrowserMessage::ActivateFolder(
            kicks.display().to_string(),
            Default::default(),
        ));
    state
        .library
        .folder_browser
        .apply_message(FolderBrowserMessage::DragFolder(
            kicks.display().to_string(),
            DragHandleMessage::started(Point::new(0.0, 0.0)),
        ));
    let request = match state
        .library
        .folder_browser
        .drop_drag_on_folder(&loops.display().to_string())
        .expect("drop should be accepted")
    {
        crate::native_app::sample_library::folder_browser::commands::FolderMoveDropInput::Request(
            request,
        ) => request,
        other => panic!("expected folder move request, got {other:?}"),
    };
    let completion =
        crate::native_app::sample_library::folder_browser::commands::execute_folder_move_request(
            request,
        );
    let mut context = ui::UiUpdateContext::default();

    state.finish_folder_move(std::time::Instant::now(), completion, &mut context);
    run_command_for_tests(&mut state, context.into_command());

    let new_parent_key = wavecrate::sample_sources::HarvestFileKey::new(
        source.id.clone(),
        Path::new("loops/kicks/kick.wav").to_path_buf(),
    );
    let new_child_key = wavecrate::sample_sources::HarvestFileKey::new(
        source.id,
        Path::new("loops/kicks/kick_chop.wav").to_path_buf(),
    );
    assert!(
        wavecrate::sample_sources::library::harvest_derivations_for_parent(&old_parent_key)
            .expect("load old parent edges")
            .is_empty()
    );
    assert!(
        wavecrate::sample_sources::library::harvest_parents_for_child(&old_child_key)
            .expect("load old child parents")
            .is_empty()
    );
    let edges = wavecrate::sample_sources::library::harvest_derivations_for_parent(&new_parent_key)
        .expect("load new parent edges");
    assert_eq!(edges.len(), 1);
    assert_eq!(edges[0].parent.key, new_parent_key);
    assert_eq!(edges[0].child.key, new_child_key);
}

#[test]
fn folder_move_remaps_nested_metadata_tags_in_live_cache() {
    let mut state = gui_state_for_span_tests();
    let source_root = tempfile::tempdir().expect("source root");
    let kicks = source_root.path().join("drums").join("kicks");
    let loops = source_root.path().join("loops");
    fs::create_dir_all(&kicks).expect("create kicks folder");
    fs::create_dir_all(&loops).expect("create loops folder");
    let kick = kicks.join("kick.wav");
    write_test_wav_i16(&kick, &[0, 256, -256, 512]);

    let db = SourceDatabase::open_for_test_fixture_source_write(source_root.path())
        .expect("open source db");
    let kick_relative = Path::new("drums/kicks/kick.wav");
    db.upsert_file(kick_relative, 8, 1)
        .expect("register kick metadata row");
    let mut batch = db.write_batch().expect("open metadata batch");
    batch
        .assign_tag_to_path(kick_relative, "loop")
        .expect("tag kick playback type");
    batch
        .assign_tag_to_path(kick_relative, "Rubber Kick")
        .expect("tag kick");
    batch.commit().expect("commit metadata");

    state.library.folder_browser =
        FolderBrowserState::from_sample_sources(&[wavecrate::sample_sources::SampleSource::new(
            source_root.path().to_path_buf(),
        )]);
    let source_id = state
        .library
        .folder_browser
        .selected_source_id()
        .to_string();
    state.refresh_persisted_metadata_tags_for_source(&source_id);
    state
        .library
        .folder_browser
        .apply_message(FolderBrowserMessage::ActivateFolder(
            kicks.display().to_string(),
            Default::default(),
        ));
    state
        .library
        .folder_browser
        .apply_message(FolderBrowserMessage::DragFolder(
            kicks.display().to_string(),
            DragHandleMessage::started(Point::new(0.0, 0.0)),
        ));
    let request = match state
        .library
        .folder_browser
        .drop_drag_on_folder(&loops.display().to_string())
        .expect("drop should be accepted")
    {
        crate::native_app::sample_library::folder_browser::commands::FolderMoveDropInput::Request(
            request,
        ) => request,
        other => panic!("expected folder move request, got {other:?}"),
    };
    let completion =
        crate::native_app::sample_library::folder_browser::commands::execute_folder_move_request(
            request,
        );
    let mut context = ui::UiUpdateContext::default();

    state.finish_folder_move(std::time::Instant::now(), completion, &mut context);
    run_command_for_tests(&mut state, context.into_command());

    let moved_kick = loops.join("kicks").join("kick.wav");
    let moved_kick_id = moved_kick.to_string_lossy().to_string();
    assert!(
        !state
            .metadata
            .tags_by_file
            .contains_key(kick.to_string_lossy().as_ref())
    );
    assert_eq!(
        state.metadata.tags_by_file.get(&moved_kick_id),
        Some(&vec![String::from("loop"), String::from("Rubber Kick")])
    );
    assert_eq!(
        db.tag_labels_for_path(Path::new("loops/kicks/kick.wav"))
            .expect("read moved kick tags"),
        vec![String::from("loop"), String::from("Rubber Kick")]
    );
}

#[test]
fn cut_paste_moved_cached_file_reloads_from_new_path() {
    let mut state = gui_state_for_span_tests();
    let source_root = tempfile::tempdir().expect("source root");
    let drums = source_root.path().join("drums");
    let loops = source_root.path().join("loops");
    fs::create_dir_all(&drums).expect("create drums folder");
    fs::create_dir_all(&loops).expect("create loops folder");
    let kick = drums.join("kick.wav");
    write_test_wav_i16(&kick, &[0, 256, -256, 512, -1024, 1024, 0, 128]);
    state.library.folder_browser =
        FolderBrowserState::from_sample_sources(&[wavecrate::sample_sources::SampleSource::new(
            source_root.path().to_path_buf(),
        )]);
    state
        .library
        .folder_browser
        .apply_message(FolderBrowserMessage::ActivateFolder(
            drums.display().to_string(),
            Default::default(),
        ));
    state
        .library
        .folder_browser
        .select_file(kick.display().to_string());
    state.waveform.current =
        crate::native_app::test_support::state::WaveformState::load_path(kick.clone())
            .expect("sample loads");
    let loaded = state.waveform.current.clone();
    state.remember_waveform(&loaded);

    state.cut_selected_files();
    state
        .library
        .folder_browser
        .apply_message(FolderBrowserMessage::ActivateFolder(
            loops.display().to_string(),
            Default::default(),
        ));
    let mut context = ui::UiUpdateContext::default();
    state.paste_cut_files(&mut context);
    run_command_for_tests(&mut state, context.into_command());

    let moved_kick = loops.join("kick.wav");
    assert!(moved_kick.is_file());
    assert!(state.waveform.cache.entries.contains_key(&moved_kick));
    assert!(!state.waveform.cache.entries.contains_key(&kick));
    let cached_state =
        crate::native_app::test_support::state::WaveformState::from_cached_file(Arc::clone(
            &state
                .waveform
                .cache
                .entries
                .get(&moved_kick)
                .expect("moved cache entry")
                .file,
        ));
    assert_eq!(cached_state.path(), moved_kick);

    state.waveform.current = crate::native_app::test_support::state::WaveformState::load_default()
        .expect("clear current waveform");
    let mut context = ui::UiUpdateContext::default();
    state.load_validated_sample_without_autoplay(
        moved_kick.display().to_string(),
        &mut context,
        std::time::Instant::now(),
    );

    assert_eq!(state.waveform.current.path(), moved_kick);
    assert!(
        state.waveform.current.has_loaded_sample(),
        "moved cached files should reload from the remapped cache entry"
    );
}

#[test]
fn trashing_selected_block_materializes_remaining_rows_and_loads_next_sample() {
    let mut state = gui_state_for_span_tests();
    let source_root = tempfile::tempdir().expect("source root");
    let trash_root = tempfile::tempdir().expect("trash root");
    let samples = (0..72)
        .map(|index| source_root.path().join(format!("sample_{index:03}.wav")))
        .collect::<Vec<_>>();
    for sample in &samples {
        write_test_wav_i16(sample, &[0, 256, -256, 512]);
    }
    state.library.folder_browser =
        FolderBrowserState::from_sample_sources(&[wavecrate::sample_sources::SampleSource::new(
            source_root.path().to_path_buf(),
        )]);
    state
        .library
        .folder_browser
        .apply_file_view_window_change(ui::VirtualListWindowChange {
            offset_y: 50.0
                * crate::native_app::test_support::sample_browser::SAMPLE_BROWSER_ROW_HEIGHT,
            row_height: crate::native_app::test_support::sample_browser::SAMPLE_BROWSER_ROW_HEIGHT,
            window: ui::VirtualListWindow {
                total_items: 72,
                viewport_start: 50,
                viewport_end: 68,
                window_start: 46,
                window_end: 72,
            },
        });
    state
        .library
        .folder_browser
        .select_file(samples[34].display().to_string());

    let trashed = samples[34..68].to_vec();
    for path in &trashed {
        fs::remove_file(path).expect("trash source removal");
    }
    let outcomes = trashed
        .iter()
        .map(|path| {
            crate::native_app::sample_library::trash_actions::movement::TrashMoveOutcome {
                source: path.clone(),
                result: crate::native_app::sample_library::trash_actions::movement::TrashMoveResult::Moved {
                    destination: trash_root
                        .path()
                        .join(path.file_name().expect("sample file name")),
                },
            }
        })
        .collect::<Vec<_>>();
    let mut context = radiant::prelude::UiUpdateContext::default();

    state.finish_trash_move(
        crate::native_app::app::TrashMoveTarget::Files(trashed),
        "browser.delete_selected_files",
        std::time::Instant::now(),
        outcomes,
        &mut context,
    );
    run_command_for_tests(&mut state, context.into_command());

    let replacement = samples[68].display().to_string();
    assert_eq!(
        state.library.folder_browser.selected_file_id(),
        Some(replacement.as_str())
    );
    assert_eq!(state.waveform.load.label.as_deref(), Some("sample_068.wav"));
    assert!(
        state.active_sample_load_task().is_some(),
        "trashing the selected block should queue autoplay loading for the replacement selection"
    );
    assert_eq!(
        state.library.folder_browser.file_view_start(),
        20,
        "trashing a scrolled bottom block should clamp the file viewport before any manual scroll"
    );

    crate::native_app::test_support::sample_browser::prepare_sample_browser_view(&mut state);
    let projection =
        crate::native_app::test_support::sample_browser::sample_browser_window_projection(
            &state, 64,
        );

    assert_eq!(projection.total_count, 38);
    assert_eq!(projection.visible_rows, projection.window_len);
    assert!(
        projection
            .first_stems
            .iter()
            .any(|stem| stem == "sample_068"),
        "remaining rows should include the next sample after the trashed block: {:?}",
        projection.first_stems
    );
}

#[test]
fn partial_batch_trash_reconciles_moved_rows_and_keeps_failed_rows() {
    use crate::native_app::sample_library::trash_actions::movement::{
        TrashMoveOutcome, TrashMoveResult,
    };

    let mut state = gui_state_for_span_tests();
    let source_root = tempfile::tempdir().expect("source root");
    let moved = source_root.path().join("a-moved.wav");
    let failed = source_root.path().join("b-failed.wav");
    let untouched = source_root.path().join("c-untouched.wav");
    for sample in [&moved, &failed, &untouched] {
        write_test_wav_i16(sample, &[0, 256, -256, 512]);
    }
    state.library.folder_browser =
        FolderBrowserState::from_sample_sources(&[wavecrate::sample_sources::SampleSource::new(
            source_root.path().to_path_buf(),
        )]);
    state
        .library
        .folder_browser
        .select_file(moved.display().to_string());
    fs::remove_file(&moved).expect("simulate completed first trash move");
    let mut context = radiant::prelude::UiUpdateContext::default();

    state.finish_trash_move(
        crate::native_app::app::TrashMoveTarget::Files(vec![moved.clone(), failed.clone()]),
        "browser.delete_selected_files",
        std::time::Instant::now(),
        vec![
            TrashMoveOutcome {
                source: moved.clone(),
                result: TrashMoveResult::Moved {
                    destination: source_root.path().join("trash").join("a-moved.wav"),
                },
            },
            TrashMoveOutcome {
                source: failed.clone(),
                result: TrashMoveResult::Failed {
                    error: String::from("injected second-item failure"),
                },
            },
        ],
        &mut context,
    );
    run_command_for_tests(&mut state, context.into_command());

    let visible = state.library.folder_browser.selected_audio_files();
    assert!(!visible.iter().any(|file| file.name == "a-moved.wav"));
    assert!(visible.iter().any(|file| file.name == "b-failed.wav"));
    assert!(visible.iter().any(|file| file.name == "c-untouched.wav"));
    assert_eq!(
        state.library.folder_browser.selected_file_id(),
        Some(failed.display().to_string().as_str())
    );
    assert!(
        state
            .ui
            .status
            .sample
            .contains("Moved 1 file to trash; 1 failed")
    );
    assert!(
        state
            .ui
            .status
            .sample
            .contains("injected second-item failure")
    );
}

#[test]
fn activating_folder_queues_selected_folder_verify() {
    let mut state = gui_state_for_span_tests();
    let source_root = tempfile::tempdir().expect("source root");
    let drums = source_root.path().join("drums");
    fs::create_dir_all(&drums).expect("create drums folder");
    fs::write(drums.join("kick.wav"), b"sample").expect("write sample");
    state.library.folder_browser =
        FolderBrowserState::from_sample_sources(&[wavecrate::sample_sources::SampleSource::new(
            source_root.path().to_path_buf(),
        )]);
    let mut context = radiant::prelude::UiUpdateContext::default();

    state.apply_folder_browser_message(
        FolderBrowserMessage::ActivateFolder(drums.display().to_string(), Default::default()),
        &mut context,
    );

    assert!(
        state.background.folder_verify_task.active().is_some(),
        "activating a folder should schedule direct verification to reconcile stale rows"
    );
}

#[test]
fn activating_folder_replaces_pending_selected_folder_verify() {
    let mut state = gui_state_for_span_tests();
    let source_root = tempfile::tempdir().expect("source root");
    let drums = source_root.path().join("drums");
    let loops = source_root.path().join("loops");
    fs::create_dir_all(&drums).expect("create drums folder");
    fs::create_dir_all(&loops).expect("create loops folder");
    state.library.folder_browser =
        FolderBrowserState::from_sample_sources(&[wavecrate::sample_sources::SampleSource::new(
            source_root.path().to_path_buf(),
        )]);
    let mut context = radiant::prelude::UiUpdateContext::default();

    state.apply_folder_browser_message(
        FolderBrowserMessage::ActivateFolder(drums.display().to_string(), Default::default()),
        &mut context,
    );
    let first_ticket = state
        .background
        .folder_verify_task
        .active()
        .expect("first activation should queue verify");
    state.apply_folder_browser_message(
        FolderBrowserMessage::ActivateFolder(loops.display().to_string(), Default::default()),
        &mut context,
    );

    assert_ne!(
        state.background.folder_verify_task.active(),
        Some(first_ticket),
        "new folder activation should supersede an older pending verification"
    );
}

#[test]
fn context_new_folder_creates_child_and_starts_inline_rename() {
    let mut state = gui_state_for_span_tests();
    let source_root = tempfile::tempdir().expect("source root");
    let parent = source_root.path().join("drums");
    fs::create_dir_all(&parent).expect("create drums folder");
    state.library.folder_browser =
        FolderBrowserState::from_sample_sources(&[wavecrate::sample_sources::SampleSource::new(
            source_root.path().to_path_buf(),
        )]);
    let parent_id = parent.display().to_string();
    state.open_folder_context_menu(parent_id.clone(), Point::new(40.0, 120.0));

    let mut context = radiant::prelude::UiUpdateContext::default();
    state.apply_message(GuiMessage::CreateFolderAtContextTarget, &mut context);
    run_command_for_tests(&mut state, context.into_command());

    let created = parent.join("New Folder");
    let created_id = created.display().to_string();
    assert!(created.is_dir());
    assert_eq!(
        state.library.folder_browser.selected_folder_id(),
        Some(created_id.as_str())
    );
    assert_eq!(
        state
            .library
            .folder_browser
            .folder_expansion_for_tests(&parent_id),
        Some(true)
    );
    assert!(
        state
            .library
            .folder_browser
            .visible_folders()
            .into_iter()
            .any(|folder| {
                folder.id == created_id
                    && folder.selected
                    && folder.rename_draft.as_deref() == Some("New Folder")
                    && folder.rename_input_id.is_some()
            })
    );
    assert!(state.ui.status.sample.contains("Created folder New Folder"));
}

#[test]
fn context_new_folder_creates_root_child_from_source_context() {
    let mut state = gui_state_for_span_tests();
    let source_root = tempfile::tempdir().expect("source root");
    let source = wavecrate::sample_sources::SampleSource::new(source_root.path().to_path_buf());
    let source_id = source.id.as_str().to_string();
    state.library.folder_browser = FolderBrowserState::from_sample_sources(&[source]);
    state.open_source_context_menu(source_id, Point::new(40.0, 120.0));

    let mut context = radiant::prelude::UiUpdateContext::default();
    state.apply_message(GuiMessage::CreateFolderAtContextTarget, &mut context);
    run_command_for_tests(&mut state, context.into_command());

    let created = source_root.path().join("New Folder");
    let created_id = created.display().to_string();
    assert!(created.is_dir());
    assert_eq!(
        state.library.folder_browser.selected_folder_id(),
        Some(created_id.as_str())
    );
    assert!(
        state
            .library
            .folder_browser
            .visible_folders()
            .into_iter()
            .any(|folder| folder.id == created_id
                && folder.rename_draft.as_deref() == Some("New Folder"))
    );
}

#[test]
fn context_new_folder_uses_collision_safe_name() {
    let mut state = gui_state_for_span_tests();
    let source_root = tempfile::tempdir().expect("source root");
    let parent = source_root.path().join("drums");
    fs::create_dir_all(parent.join("New Folder")).expect("create first collision");
    fs::create_dir_all(parent.join("New Folder 2")).expect("create second collision");
    state.library.folder_browser =
        FolderBrowserState::from_sample_sources(&[wavecrate::sample_sources::SampleSource::new(
            source_root.path().to_path_buf(),
        )]);
    state.open_folder_context_menu(parent.display().to_string(), Point::new(40.0, 120.0));

    let mut context = radiant::prelude::UiUpdateContext::default();
    state.apply_message(GuiMessage::CreateFolderAtContextTarget, &mut context);
    run_command_for_tests(&mut state, context.into_command());

    let created = parent.join("New Folder 3");
    let created_id = created.display().to_string();
    assert!(created.is_dir());
    assert_eq!(
        state.library.folder_browser.selected_folder_id(),
        Some(created_id.as_str())
    );
    assert!(
        state
            .ui
            .status
            .sample
            .contains("Created folder New Folder 3")
    );
}

#[test]
fn context_new_folder_missing_parent_reports_error_without_tree_corruption() {
    let mut state = gui_state_for_span_tests();
    let source_root = tempfile::tempdir().expect("source root");
    let parent = source_root.path().join("drums");
    fs::create_dir_all(&parent).expect("create drums folder");
    state.library.folder_browser =
        FolderBrowserState::from_sample_sources(&[wavecrate::sample_sources::SampleSource::new(
            source_root.path().to_path_buf(),
        )]);
    let parent_id = parent.display().to_string();
    state.open_folder_context_menu(parent_id.clone(), Point::new(40.0, 120.0));
    fs::remove_dir_all(&parent).expect("remove context target");

    let mut context = radiant::prelude::UiUpdateContext::default();
    state.apply_message(GuiMessage::CreateFolderAtContextTarget, &mut context);
    run_command_for_tests(&mut state, context.into_command());

    assert!(state.ui.status.sample.contains("parent folder"));
    assert!(state.ui.status.sample.contains("unavailable"));
    assert!(
        !state
            .library
            .folder_browser
            .visible_folders()
            .into_iter()
            .any(|folder| folder.name == "New Folder" && folder.id.starts_with(&parent_id))
    );
}

#[test]
fn context_delete_folder_requests_confirmation() {
    let mut state = gui_state_for_span_tests();
    let source_root = tempfile::tempdir().expect("source root");
    let folder = source_root.path().join("drums");
    fs::create_dir_all(&folder).expect("create drums folder");
    state.library.folder_browser =
        FolderBrowserState::from_sample_sources(&[wavecrate::sample_sources::SampleSource::new(
            source_root.path().to_path_buf(),
        )]);
    state.open_folder_context_menu(folder.display().to_string(), Point::new(40.0, 120.0));

    let mut context = radiant::prelude::UiUpdateContext::default();
    state.apply_message(GuiMessage::RequestDeleteContextFolder, &mut context);

    let pending = state
        .ui
        .browser_interaction
        .pending_folder_delete
        .as_ref()
        .expect("folder delete should wait for confirmation");
    assert_eq!(pending.path, folder);
    assert!(state.ui.browser_interaction.context_menu.is_none());

    let frame = view(&mut state).view_frame_at_size_with_default_theme(Vector2::new(900.0, 620.0));
    assert!(frame.paint_plan.contains_text("Delete Folder"));
    assert!(
        frame
            .paint_plan
            .contains_text("Move folder contents to the configured trash folder?")
    );
    assert!(folder.is_dir());
}

#[test]
fn context_delete_folder_confirmation_moves_folder_to_trash() {
    let mut state = gui_state_for_span_tests();
    let source_root = tempfile::tempdir().expect("source root");
    let trash_root = tempfile::tempdir().expect("trash root");
    let folder = source_root.path().join("drums");
    let sibling = source_root.path().join("loops");
    fs::create_dir_all(folder.join("nested")).expect("create nested folder");
    fs::create_dir_all(&sibling).expect("create sibling folder");
    fs::write(folder.join("nested").join("kick.wav"), []).expect("write nested sample");
    fs::write(sibling.join("loop.wav"), []).expect("write sibling sample");
    state.ui.settings.persisted.trash_folder = Some(trash_root.path().to_path_buf());
    state.library.folder_browser =
        FolderBrowserState::from_sample_sources(&[wavecrate::sample_sources::SampleSource::new(
            source_root.path().to_path_buf(),
        )]);
    let folder_id = folder.display().to_string();
    state
        .library
        .folder_browser
        .apply_message(FolderBrowserMessage::ActivateFolder(
            folder_id.clone(),
            Default::default(),
        ));
    state.open_folder_context_menu(folder_id.clone(), Point::new(40.0, 120.0));

    let mut context = radiant::prelude::UiUpdateContext::default();
    state.apply_message(GuiMessage::RequestDeleteContextFolder, &mut context);
    state.apply_message(GuiMessage::ConfirmContextFolderDelete, &mut context);
    run_command_for_tests(&mut state, context.into_command());

    assert!(!folder.exists());
    assert!(
        trash_root
            .path()
            .join("drums")
            .join("nested")
            .join("kick.wav")
            .exists()
    );
    assert!(sibling.exists());
    assert!(
        state
            .library
            .folder_browser
            .visible_folders()
            .into_iter()
            .all(|visible| visible.id != folder_id)
    );
    let source_root_id = source_root.path().display().to_string();
    assert_eq!(
        state.library.folder_browser.selected_folder_id(),
        Some(source_root_id.as_str())
    );
    assert!(state.ui.status.sample.contains("Moved drums to trash"));
}

#[test]
fn context_delete_folder_missing_target_reconciles_tree_without_trash_move() {
    let mut state = gui_state_for_span_tests();
    let source_root = tempfile::tempdir().expect("source root");
    let folder = source_root.path().join("drums");
    fs::create_dir_all(&folder).expect("create drums folder");
    state.library.folder_browser =
        FolderBrowserState::from_sample_sources(&[wavecrate::sample_sources::SampleSource::new(
            source_root.path().to_path_buf(),
        )]);
    let folder_id = folder.display().to_string();
    state.open_folder_context_menu(folder_id.clone(), Point::new(40.0, 120.0));

    let mut context = radiant::prelude::UiUpdateContext::default();
    state.apply_message(GuiMessage::RequestDeleteContextFolder, &mut context);
    fs::remove_dir_all(&folder).expect("remove folder before confirmation");
    state.apply_message(GuiMessage::ConfirmContextFolderDelete, &mut context);
    run_command_for_tests(&mut state, context.into_command());

    assert!(
        state
            .library
            .folder_browser
            .visible_folders()
            .into_iter()
            .all(|visible| visible.id != folder_id)
    );
    assert!(state.ui.status.sample.contains("no longer exists"));
    assert!(
        state
            .ui
            .status
            .sample
            .contains("removed it from the browser")
    );
}

#[test]
fn delete_selected_file_moves_it_to_configured_trash_folder() {
    let mut state = gui_state_for_span_tests();
    let source_root = tempfile::tempdir().expect("source root");
    let trash_root = tempfile::tempdir().expect("trash root");
    let keep = source_root.path().join("keep.wav");
    let delete = source_root.path().join("delete.wav");
    fs::write(&keep, []).expect("write keep wav");
    fs::write(&delete, []).expect("write delete wav");
    state.ui.settings.persisted.trash_folder = Some(trash_root.path().to_path_buf());
    state.library.folder_browser =
        FolderBrowserState::from_sample_sources(&[wavecrate::sample_sources::SampleSource::new(
            source_root.path().to_path_buf(),
        )]);
    state
        .library
        .folder_browser
        .select_file(delete.display().to_string());

    let mut context = radiant::prelude::UiUpdateContext::default();
    state.delete_selected_item(&mut context);
    run_command_for_tests(&mut state, context.into_command());

    assert!(!delete.exists());
    assert!(trash_root.path().join("delete.wav").exists());
    assert!(keep.exists());
    let keep_id = keep.display().to_string();
    assert_eq!(
        state.library.folder_browser.selected_file_id(),
        Some(keep_id.as_str())
    );
    assert!(
        state
            .library
            .folder_browser
            .selected_audio_files()
            .iter()
            .any(|file| file.name == "keep.wav")
    );
    assert!(
        !state
            .library
            .folder_browser
            .selected_audio_files()
            .iter()
            .any(|file| file.name == "delete.wav")
    );
    assert!(state.ui.status.sample.contains("Moved 1 file to trash"));
}

#[test]
fn context_trash_selected_sample_moves_full_selected_set() {
    let mut state = gui_state_for_span_tests();
    let source_root = tempfile::tempdir().expect("source root");
    let trash_root = tempfile::tempdir().expect("trash root");
    let keep = source_root.path().join("keep.wav");
    let kick = source_root.path().join("kick.wav");
    let snare = source_root.path().join("snare.wav");
    for file in [&keep, &kick, &snare] {
        fs::write(file, []).expect("write sample");
    }
    state.ui.settings.persisted.trash_folder = Some(trash_root.path().to_path_buf());
    state.library.folder_browser =
        FolderBrowserState::from_sample_sources(&[wavecrate::sample_sources::SampleSource::new(
            source_root.path().to_path_buf(),
        )]);
    state
        .library
        .folder_browser
        .select_file(kick.display().to_string());
    state.library.folder_browser.select_file_with_modifiers(
        snare.display().to_string(),
        PointerModifiers {
            command: true,
            ..PointerModifiers::default()
        },
    );

    state.open_sample_context_menu(kick.display().to_string(), Point::new(40.0, 120.0));
    assert_eq!(
        state.library.folder_browser.selected_file_paths(),
        vec![kick.clone(), snare.clone()]
    );

    let mut context = radiant::prelude::UiUpdateContext::default();
    state.apply_message(GuiMessage::MoveContextTargetToTrash, &mut context);
    run_command_for_tests(&mut state, context.into_command());

    assert!(!kick.exists());
    assert!(!snare.exists());
    assert!(keep.exists());
    assert!(trash_root.path().join("kick.wav").exists());
    assert!(trash_root.path().join("snare.wav").exists());
    assert_eq!(state.ui.browser_interaction.context_menu, None);
    let keep_id = keep.display().to_string();
    assert_eq!(
        state.library.folder_browser.selected_file_id(),
        Some(keep_id.as_str())
    );
    assert!(state.ui.status.sample.contains("Moved 2 files to trash"));
}

#[test]
fn context_menu_unlock_drops_locked_keep_sample_to_keep_three() {
    let (mut state, _source_root, selected_file) = native_app_state_with_temp_sample("locked.wav");
    let sample_path = Path::new(&selected_file).to_path_buf();
    assert!(
        state
            .library
            .folder_browser
            .set_file_rating_state(&sample_path, Rating::KEEP_3, true),
        "fixture should mark the loaded sample as locked keep"
    );

    state.open_sample_context_menu(selected_file.clone(), Point::new(40.0, 120.0));
    assert!(
        state
            .ui
            .browser_interaction
            .context_menu
            .as_ref()
            .is_some_and(|menu| menu.sample_keep_locked),
        "context menu should carry the locked-keep flag"
    );

    let mut context = radiant::prelude::UiUpdateContext::default();
    state.apply_message(GuiMessage::UnlockContextSample, &mut context);

    assert_eq!(state.ui.browser_interaction.context_menu, None);
    assert_sample_rating(&state, &sample_path, Rating::KEEP_3, false);
    assert!(state.ui.status.sample.contains("Unlocked locked.wav"));
}

#[test]
fn third_negative_rating_does_not_auto_trash_selected_file() {
    let mut state = gui_state_for_span_tests();
    let source_root = tempfile::tempdir().expect("source root");
    let trash_root = tempfile::tempdir().expect("trash root");
    let sample = source_root.path().join("third.wav");
    fs::write(&sample, []).expect("write sample");
    state.ui.settings.persisted.trash_folder = Some(trash_root.path().to_path_buf());
    state.library.folder_browser =
        FolderBrowserState::from_sample_sources(&[wavecrate::sample_sources::SampleSource::new(
            source_root.path().to_path_buf(),
        )]);
    state
        .library
        .folder_browser
        .select_file(sample.display().to_string());
    assert!(
        state
            .library
            .folder_browser
            .set_file_rating_state(&sample, Rating::new(-2), false)
    );

    let mut context = radiant::prelude::UiUpdateContext::default();
    state.adjust_selected_rating(-1, &mut context);

    assert!(sample.exists());
    assert!(!trash_root.path().join("third.wav").exists());
    let selected = state.library.folder_browser.selected_audio_files();
    assert_eq!(selected.len(), 1);
    assert_eq!(selected[0].rating, Rating::TRASH_3);
    assert!(state.ui.status.sample.contains("Rated 1 sample"));
}

#[test]
fn rating_advance_uses_pre_rating_sorted_order_when_rating_sort_changes() {
    let mut state = gui_state_for_span_tests();
    let source_root = tempfile::tempdir().expect("source root");
    let current = source_root.path().join("a-current.wav");
    let next = source_root.path().join("b-next.wav");
    write_test_wav_i16(&current, &[0, 256, -256, 512]);
    write_test_wav_i16(&next, &[0, 1024, -2048, 4096]);
    let current_id = current.display().to_string();
    let next_id = next.display().to_string();
    state.ui.settings.persisted.controls.advance_after_rating = true;
    state.library.folder_browser =
        FolderBrowserState::from_sample_sources(&[wavecrate::sample_sources::SampleSource::new(
            source_root.path().to_path_buf(),
        )]);
    state
        .library
        .folder_browser
        .apply_message(FolderBrowserMessage::SortFileColumn(String::from("rating")));
    state.library.folder_browser.select_file(current_id.clone());

    let mut context = radiant::prelude::UiUpdateContext::default();
    state.adjust_selected_rating(1, &mut context);
    run_command_for_tests(&mut state, context.into_command());

    assert_eq!(
        state.library.folder_browser.selected_file_id(),
        Some(next_id.as_str())
    );
    assert_eq!(state.waveform.load.label.as_deref(), Some("b-next.wav"));
    let rows = state.library.folder_browser.selected_audio_files();
    assert_eq!(
        rows.iter().map(|file| file.id.as_str()).collect::<Vec<_>>(),
        vec![next_id.as_str(), current_id.as_str()]
    );
    assert_eq!(rows[1].rating, Rating::KEEP_1);
}

#[test]
fn rating_hotkey_adjustment_does_not_advance_when_auto_advance_is_enabled() {
    let mut state = gui_state_for_span_tests();
    let source_root = tempfile::tempdir().expect("source root");
    let current = source_root.path().join("a-current.wav");
    let next = source_root.path().join("b-next.wav");
    write_test_wav_i16(&current, &[0, 256, -256, 512]);
    write_test_wav_i16(&next, &[0, 1024, -2048, 4096]);
    let current_id = current.display().to_string();
    state.ui.settings.persisted.controls.advance_after_rating = true;
    state.library.folder_browser =
        FolderBrowserState::from_sample_sources(&[wavecrate::sample_sources::SampleSource::new(
            source_root.path().to_path_buf(),
        )]);
    state.library.folder_browser.select_file(current_id.clone());

    let mut context = radiant::prelude::UiUpdateContext::default();
    state.adjust_selected_rating_without_advance(1, &mut context);
    run_command_for_tests(&mut state, context.into_command());

    assert_eq!(
        state.library.folder_browser.selected_file_id(),
        Some(current_id.as_str())
    );
    assert_eq!(
        state.library.folder_browser.selected_file_paths(),
        vec![current.clone()]
    );
    assert_eq!(state.waveform.load.label.as_deref(), None);
    let rows = state.library.folder_browser.selected_audio_files();
    let current_row = rows
        .iter()
        .find(|file| file.id == current_id)
        .expect("current row remains visible");
    assert_eq!(current_row.rating, Rating::KEEP_1);
}

#[test]
fn rating_hotkey_updates_visible_row_when_loaded_source_tree_is_stale() {
    let mut state = gui_state_for_span_tests();
    let source_root = tempfile::tempdir().expect("source root");
    let drums = source_root.path().join("drums");
    fs::create_dir_all(&drums).expect("create drums folder");
    let current = drums.join("a-current.wav");
    write_test_wav_i16(&current, &[0, 256, -256, 512]);
    let drums_id = drums.display().to_string();
    let current_id = current.display().to_string();
    state.library.folder_browser =
        FolderBrowserState::from_sample_sources(&[wavecrate::sample_sources::SampleSource::new(
            source_root.path().to_path_buf(),
        )]);
    state
        .library
        .folder_browser
        .apply_message(FolderBrowserMessage::ActivateFolder(
            drums_id.clone(),
            Default::default(),
        ));
    state.library.folder_browser.select_file(current_id.clone());
    assert!(
        state
            .library
            .folder_browser
            .remove_loaded_source_folder_for_tests(&drums_id),
        "test setup should leave the visible tree with a selected row while making the loaded source tree unable to refresh the sample"
    );

    let mut context = radiant::prelude::UiUpdateContext::default();
    state.adjust_selected_rating_without_advance(1, &mut context);
    run_command_for_tests(&mut state, context.into_command());

    assert!(state.ui.status.sample.contains("Rated 1 sample"));
    let current_row = state
        .library
        .folder_browser
        .selected_audio_files()
        .into_iter()
        .find(|file| file.id == current_id)
        .expect("current row remains visible");
    assert_eq!(current_row.rating, Rating::KEEP_1);
}

#[test]
fn rating_advance_moves_to_next_recursive_root_sample() {
    let mut state = gui_state_for_span_tests();
    let source_root = tempfile::tempdir().expect("source root");
    let drums = source_root.path().join("drums");
    fs::create_dir_all(&drums).expect("create drums folder");
    let current = drums.join("a-current.wav");
    let next = drums.join("b-next.wav");
    write_test_wav_i16(&current, &[0, 256, -256, 512]);
    write_test_wav_i16(&next, &[0, 1024, -2048, 4096]);
    let current_id = current.display().to_string();
    let next_id = next.display().to_string();
    state.ui.settings.persisted.controls.advance_after_rating = true;
    state.library.folder_browser =
        FolderBrowserState::from_sample_sources(&[wavecrate::sample_sources::SampleSource::new(
            source_root.path().to_path_buf(),
        )]);
    state
        .library
        .folder_browser
        .apply_message(FolderBrowserMessage::ToggleFolderSubtreeListing);
    state.library.folder_browser.select_file(current_id.clone());

    let mut context = radiant::prelude::UiUpdateContext::default();
    state.adjust_selected_rating(1, &mut context);
    run_command_for_tests(&mut state, context.into_command());

    assert_eq!(
        state.library.folder_browser.selected_file_id(),
        Some(next_id.as_str())
    );
    assert_eq!(state.waveform.load.label.as_deref(), Some("b-next.wav"));
    let rows = state.library.folder_browser.selected_audio_files();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].id, current_id);
    assert_eq!(rows[0].rating, Rating::KEEP_1);
}

#[test]
fn rating_advance_uses_pre_rating_recursive_unrated_filter_order() {
    let mut state = gui_state_for_span_tests();
    let source_root = tempfile::tempdir().expect("source root");
    let drums = source_root.path().join("drums");
    fs::create_dir_all(&drums).expect("create drums folder");
    let current = drums.join("a-current.wav");
    let next = drums.join("b-next.wav");
    write_test_wav_i16(&current, &[0, 256, -256, 512]);
    write_test_wav_i16(&next, &[0, 1024, -2048, 4096]);
    let next_id = next.display().to_string();
    state.ui.settings.persisted.controls.advance_after_rating = true;
    state.library.folder_browser =
        FolderBrowserState::from_sample_sources(&[wavecrate::sample_sources::SampleSource::new(
            source_root.path().to_path_buf(),
        )]);
    state
        .library
        .folder_browser
        .apply_message(FolderBrowserMessage::ToggleFolderSubtreeListing);
    state
        .library
        .folder_browser
        .apply_message(FolderBrowserMessage::ToggleRatingFilter(0, true));
    state
        .library
        .folder_browser
        .select_file(current.display().to_string());

    let mut context = radiant::prelude::UiUpdateContext::default();
    state.adjust_selected_rating(1, &mut context);
    run_command_for_tests(&mut state, context.into_command());

    assert_eq!(
        state.library.folder_browser.selected_file_id(),
        Some(next_id.as_str())
    );
    assert_eq!(
        state
            .library
            .folder_browser
            .selected_audio_files()
            .iter()
            .map(|file| file.id.as_str())
            .collect::<Vec<_>>(),
        vec![next_id.as_str()]
    );
}

#[test]
fn rating_advance_wraps_to_first_remaining_visible_sample() {
    let mut state = gui_state_for_span_tests();
    let source_root = tempfile::tempdir().expect("source root");
    let drums = source_root.path().join("drums");
    fs::create_dir_all(&drums).expect("create drums folder");
    let first = drums.join("a-first.wav");
    let last = drums.join("b-last.wav");
    write_test_wav_i16(&first, &[0, 256, -256, 512]);
    write_test_wav_i16(&last, &[0, 1024, -2048, 4096]);
    let first_id = first.display().to_string();
    state.ui.settings.persisted.controls.advance_after_rating = true;
    state.library.folder_browser =
        FolderBrowserState::from_sample_sources(&[wavecrate::sample_sources::SampleSource::new(
            source_root.path().to_path_buf(),
        )]);
    state
        .library
        .folder_browser
        .apply_message(FolderBrowserMessage::ActivateFolder(
            drums.display().to_string(),
            Default::default(),
        ));
    state
        .library
        .folder_browser
        .apply_message(FolderBrowserMessage::ToggleRatingFilter(0, true));
    state
        .library
        .folder_browser
        .select_file(last.display().to_string());

    let mut context = radiant::prelude::UiUpdateContext::default();
    state.adjust_selected_rating(1, &mut context);
    let command = context.into_command();

    assert_eq!(
        state.library.folder_browser.selected_file_id(),
        Some(first_id.as_str())
    );
    assert_eq!(
        state
            .library
            .folder_browser
            .selected_audio_files()
            .iter()
            .map(|file| file.id.as_str())
            .collect::<Vec<_>>(),
        vec![first_id.as_str()]
    );
    assert_eq!(
        last_fixed_sample_browser_row_scroll(&command),
        Some((0, -1))
    );
    run_command_for_tests(&mut state, command);
    assert_eq!(state.waveform.load.label.as_deref(), Some("a-first.wav"));
}

#[test]
fn rating_adjustment_applies_to_history_revealed_rating_filtered_curation_sample() {
    let mut state = gui_state_for_span_tests();
    let source_root = tempfile::tempdir().expect("source root");
    let drums = source_root.path().join("drums");
    let loops = drums.join("loops");
    fs::create_dir_all(&loops).expect("create loops folder");
    let visible = loops.join("visible-k1.wav");
    let hidden = loops.join("history-k2.wav");
    write_test_wav_i16(&visible, &[0, 256, -256, 512]);
    write_test_wav_i16(&hidden, &[0, 512, -512, 1024]);
    let hidden_id = hidden.display().to_string();
    state.ui.settings.persisted.controls.advance_after_rating = false;
    state.library.folder_browser =
        FolderBrowserState::from_sample_sources(&[wavecrate::sample_sources::SampleSource::new(
            source_root.path().to_path_buf(),
        )]);
    assert!(
        state
            .library
            .folder_browser
            .set_file_rating_state(&visible, Rating::KEEP_1, false)
    );
    assert!(
        state
            .library
            .folder_browser
            .set_file_rating_state(&hidden, Rating::new(2), false)
    );
    let stale_curated_at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
        - 60 * 60 * 24 * 90;
    assert!(
        state
            .library
            .folder_browser
            .set_file_last_curated_at(&visible, stale_curated_at)
    );
    state
        .library
        .folder_browser
        .apply_message(FolderBrowserMessage::ActivateFolder(
            drums.display().to_string(),
            Default::default(),
        ));
    state
        .library
        .folder_browser
        .apply_message(FolderBrowserMessage::ToggleFolderSubtreeListing);
    state
        .library
        .folder_browser
        .apply_message(FolderBrowserMessage::ToggleRatingFilter(1, true));
    state
        .library
        .folder_browser
        .apply_message(FolderBrowserMessage::SetCurationScope(
            BrowserCurationScope::All,
            true,
        ));
    let tags_by_file = std::collections::HashMap::new();
    assert!(
        state
            .library
            .folder_browser
            .focus_file_across_sources_matching_tags(&hidden, &tags_by_file)
    );
    assert_eq!(
        state.library.folder_browser.selected_file_id(),
        Some(hidden_id.as_str())
    );

    let mut context = radiant::prelude::UiUpdateContext::default();
    state.adjust_selected_rating(-1, &mut context);

    assert_eq!(
        state.library.folder_browser.selected_file_paths(),
        vec![hidden.clone()]
    );
    let hidden_row = state
        .library
        .folder_browser
        .selected_audio_files_matching_tags(&state.metadata.tags_by_file)
        .into_iter()
        .find(|file| file.id == hidden_id)
        .expect("revealed hidden row should remain visible after re-rating");
    assert_eq!(hidden_row.rating, Rating::KEEP_1);
    assert!(state.ui.status.sample.contains("Rated 1 sample"));
}

#[test]
fn rating_filter_hiding_last_recursive_sample_clears_selection() {
    let mut state = gui_state_for_span_tests();
    let source_root = tempfile::tempdir().expect("source root");
    let drums = source_root.path().join("drums");
    fs::create_dir_all(&drums).expect("create drums folder");
    let current = drums.join("only.wav");
    write_test_wav_i16(&current, &[0, 256, -256, 512]);
    state.ui.settings.persisted.controls.advance_after_rating = true;
    state.library.folder_browser =
        FolderBrowserState::from_sample_sources(&[wavecrate::sample_sources::SampleSource::new(
            source_root.path().to_path_buf(),
        )]);
    state
        .library
        .folder_browser
        .apply_message(FolderBrowserMessage::ToggleFolderSubtreeListing);
    state
        .library
        .folder_browser
        .apply_message(FolderBrowserMessage::ToggleRatingFilter(0, true));
    state
        .library
        .folder_browser
        .select_file(current.display().to_string());

    let mut context = radiant::prelude::UiUpdateContext::default();
    state.adjust_selected_rating(1, &mut context);

    assert_eq!(state.library.folder_browser.selected_file_id(), None);
    assert!(
        state
            .library
            .folder_browser
            .selected_audio_files()
            .is_empty()
    );
}

#[test]
fn enabling_curation_filter_focuses_first_visible_sample_immediately() {
    let mut state = gui_state_for_span_tests();
    let source_root = tempfile::tempdir().expect("source root");
    let hidden = source_root.path().join("a-hidden-locked.wav");
    let visible = source_root.path().join("b-visible-curation.wav");
    let visible_id = visible.display().to_string();
    for file in [&hidden, &visible] {
        write_test_wav_i16(file, &[0, 256, -256, 512]);
    }
    state.library.folder_browser =
        FolderBrowserState::from_sample_sources(&[wavecrate::sample_sources::SampleSource::new(
            source_root.path().to_path_buf(),
        )]);
    assert!(
        state
            .library
            .folder_browser
            .set_file_rating_state(&hidden, Rating::KEEP_3, true)
    );
    state
        .library
        .folder_browser
        .select_file(hidden.display().to_string());

    let mut context = radiant::prelude::UiUpdateContext::default();
    state.apply_folder_browser_message(
        FolderBrowserMessage::SetCurationScope(BrowserCurationScope::All, true),
        &mut context,
    );
    let command = context.into_command();

    assert_eq!(
        state.library.folder_browser.selected_file_id(),
        Some(visible_id.as_str())
    );
    assert_eq!(last_fixed_sample_browser_row_scroll(&command), Some((0, 0)));
    run_command_for_tests(&mut state, command);
    assert_eq!(
        state.waveform.load.label.as_deref(),
        Some("b-visible-curation.wav")
    );
}

#[test]
fn rating_advance_skips_hidden_multi_selected_target_and_reveals_final_focus() {
    let mut state = gui_state_for_span_tests();
    let source_root = tempfile::tempdir().expect("source root");
    let drums = source_root.path().join("drums");
    fs::create_dir_all(&drums).expect("create drums folder");
    let current = drums.join("a-current.wav");
    let hidden_next = drums.join("b-hidden-next.wav");
    let visible_next = drums.join("c-visible-next.wav");
    let visible_next_id = visible_next.display().to_string();
    for file in [&current, &hidden_next, &visible_next] {
        write_test_wav_i16(file, &[0, 256, -256, 512]);
    }
    state.ui.settings.persisted.controls.advance_after_rating = true;
    state.library.folder_browser =
        FolderBrowserState::from_sample_sources(&[wavecrate::sample_sources::SampleSource::new(
            source_root.path().to_path_buf(),
        )]);
    state
        .library
        .folder_browser
        .apply_message(FolderBrowserMessage::ActivateFolder(
            drums.display().to_string(),
            Default::default(),
        ));
    state
        .library
        .folder_browser
        .apply_message(FolderBrowserMessage::ToggleRatingFilter(0, true));
    state
        .library
        .folder_browser
        .select_file(current.display().to_string());
    state.library.folder_browser.select_file_with_modifiers(
        hidden_next.display().to_string(),
        PointerModifiers {
            command: true,
            ..Default::default()
        },
    );
    state
        .library
        .folder_browser
        .focus_file_preserving_selection_matching_tags(
            current.display().to_string(),
            &state.metadata.tags_by_file,
        );

    let mut context = radiant::prelude::UiUpdateContext::default();
    state.adjust_selected_rating(1, &mut context);
    let command = context.into_command();

    assert_eq!(
        state.library.folder_browser.selected_file_id(),
        Some(visible_next_id.as_str())
    );
    assert_eq!(
        state
            .library
            .folder_browser
            .selected_audio_files_matching_tags(&state.metadata.tags_by_file)
            .iter()
            .map(|file| file.id.as_str())
            .collect::<Vec<_>>(),
        vec![visible_next_id.as_str()]
    );
    assert_eq!(last_fixed_sample_browser_row_scroll(&command), Some((0, 1)));
    run_command_for_tests(&mut state, command);
    assert_eq!(
        state.waveform.load.label.as_deref(),
        Some("c-visible-next.wav")
    );
}

#[test]
fn rating_adjustment_survives_selected_file_rename() {
    let (mut state, source_root, selected_file) = native_app_state_with_temp_sample("kick.wav");
    state.ui.settings.persisted.controls.advance_after_rating = false;
    let mut context = radiant::prelude::UiUpdateContext::default();

    state.adjust_selected_rating(1, &mut context);
    state
        .library
        .folder_browser
        .begin_rename_selected()
        .expect("rename can start")
        .expect("rename input id");
    super::submit_folder_browser_rename_for_tests(&mut state, "snare");

    let renamed = source_root.path().join("snare.wav");
    let rows = state.library.folder_browser.selected_audio_files();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id, renamed.display().to_string());
    assert_eq!(rows[0].rating, Rating::KEEP_1);
    assert!(!std::path::Path::new(&selected_file).exists());
    assert!(renamed.exists());
    let db = wavecrate::sample_sources::SourceDatabase::open_for_test_fixture_source_write(
        source_root.path(),
    )
    .expect("db");
    assert_eq!(
        db.tag_for_path(std::path::Path::new("snare.wav"))
            .expect("rating"),
        Some(Rating::KEEP_1)
    );
}

#[test]
fn rating_adjustment_survives_selected_file_move() {
    let mut state = gui_state_for_span_tests();
    let source_root = tempfile::tempdir().expect("source root");
    let drums = source_root.path().join("drums");
    let loops = source_root.path().join("loops");
    fs::create_dir_all(&drums).expect("create drums folder");
    fs::create_dir_all(&loops).expect("create loops folder");
    let kick = drums.join("kick.wav");
    fs::write(&kick, []).expect("write kick");
    state.ui.settings.persisted.controls.advance_after_rating = false;
    state.library.folder_browser =
        FolderBrowserState::from_sample_sources(&[wavecrate::sample_sources::SampleSource::new(
            source_root.path().to_path_buf(),
        )]);
    state
        .library
        .folder_browser
        .apply_message(FolderBrowserMessage::ActivateFolder(
            drums.display().to_string(),
            Default::default(),
        ));
    state
        .library
        .folder_browser
        .select_file(kick.display().to_string());

    let mut context = radiant::prelude::UiUpdateContext::default();
    state.adjust_selected_rating(1, &mut context);
    state
        .library
        .folder_browser
        .begin_file_drag(kick.display().to_string(), Point::new(4.0, 8.0));
    state.drop_browser_drag_on_folder(loops.display().to_string(), &mut context);
    run_command_for_tests(&mut state, context.into_command());

    let moved = loops.join("kick.wav");
    state
        .library
        .folder_browser
        .apply_message(FolderBrowserMessage::ActivateFolder(
            loops.display().to_string(),
            Default::default(),
        ));
    let rows = state.library.folder_browser.selected_audio_files();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id, moved.display().to_string());
    assert_eq!(rows[0].rating, Rating::KEEP_1);
    assert!(!kick.exists());
    assert!(moved.exists());
    let db = wavecrate::sample_sources::SourceDatabase::open_for_test_fixture_source_write(
        source_root.path(),
    )
    .expect("db");
    assert_eq!(
        db.tag_for_path(std::path::Path::new("loops/kick.wav"))
            .expect("rating"),
        Some(Rating::KEEP_1)
    );
}

#[test]
fn rating_adjustment_survives_selected_file_move_and_source_refresh() {
    let mut state = gui_state_for_span_tests();
    let source_root = tempfile::tempdir().expect("source root");
    let drums = source_root.path().join("drums");
    let loops = source_root.path().join("loops");
    fs::create_dir_all(&drums).expect("create drums folder");
    fs::create_dir_all(&loops).expect("create loops folder");
    let kick = drums.join("kick.wav");
    fs::write(&kick, []).expect("write kick");
    state.ui.settings.persisted.controls.advance_after_rating = false;
    let source = wavecrate::sample_sources::SampleSource::new(source_root.path().to_path_buf());
    let source_id = source.id.clone();
    state.library.folder_browser = FolderBrowserState::from_sample_sources(&[source]);
    state
        .library
        .folder_browser
        .apply_message(FolderBrowserMessage::ActivateFolder(
            drums.display().to_string(),
            Default::default(),
        ));
    state
        .library
        .folder_browser
        .select_file(kick.display().to_string());

    let mut context = radiant::prelude::UiUpdateContext::default();
    state.adjust_selected_rating(1, &mut context);
    state
        .library
        .folder_browser
        .begin_file_drag(kick.display().to_string(), Point::new(4.0, 8.0));
    state.drop_browser_drag_on_folder(loops.display().to_string(), &mut context);
    run_command_for_tests(&mut state, context.into_command());
    state.library.folder_browser.refresh_filesystem_paths(
        source_id.as_str(),
        &[std::path::PathBuf::from("loops").join("kick.wav")],
    );

    let moved = loops.join("kick.wav");
    state
        .library
        .folder_browser
        .apply_message(FolderBrowserMessage::ActivateFolder(
            loops.display().to_string(),
            Default::default(),
        ));
    let rows = state.library.folder_browser.selected_audio_files();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id, moved.display().to_string());
    assert_eq!(rows[0].rating, Rating::KEEP_1);
    let db = wavecrate::sample_sources::SourceDatabase::open_for_test_fixture_source_write(
        source_root.path(),
    )
    .expect("db");
    assert_eq!(
        db.tag_for_path(std::path::Path::new("loops/kick.wav"))
            .expect("rating"),
        Some(Rating::KEEP_1)
    );
}

#[test]
fn fourth_negative_rating_moves_selected_file_to_trash() {
    let mut state = gui_state_for_span_tests();
    let source_root = tempfile::tempdir().expect("source root");
    let trash_root = tempfile::tempdir().expect("trash root");
    let keep = source_root.path().join("keep.wav");
    let sample = source_root.path().join("fourth.wav");
    fs::write(&keep, []).expect("write keep wav");
    fs::write(&sample, []).expect("write sample");
    state.ui.settings.persisted.trash_folder = Some(trash_root.path().to_path_buf());
    state.library.folder_browser =
        FolderBrowserState::from_sample_sources(&[wavecrate::sample_sources::SampleSource::new(
            source_root.path().to_path_buf(),
        )]);
    state
        .library
        .folder_browser
        .select_file(sample.display().to_string());
    assert!(
        state
            .library
            .folder_browser
            .set_file_rating_state(&sample, Rating::TRASH_3, false)
    );

    let mut context = radiant::prelude::UiUpdateContext::default();
    state.adjust_selected_rating(-1, &mut context);
    assert!(
        state.ui.status.sample.contains("fourth negative rating"),
        "{}",
        state.ui.status.sample
    );
    run_command_for_tests(&mut state, context.into_command());

    assert!(!sample.exists());
    assert!(trash_root.path().join("fourth.wav").exists());
    assert!(keep.exists());
    let keep_id = keep.display().to_string();
    assert_eq!(
        state.library.folder_browser.selected_file_id(),
        Some(keep_id.as_str())
    );
    assert!(
        !state
            .library
            .folder_browser
            .selected_audio_files()
            .iter()
            .any(|file| file.name == "fourth.wav")
    );
    assert!(
        state
            .ui
            .status
            .sample
            .contains("Moved 1 file to trash after fourth negative rating")
    );
}

#[test]
fn fourth_negative_rating_keeps_file_available_when_trash_move_fails() {
    let mut state = gui_state_for_span_tests();
    let source_root = tempfile::tempdir().expect("source root");
    let sample = source_root.path().join("blocked.wav");
    fs::write(&sample, []).expect("write sample");
    state.library.folder_browser =
        FolderBrowserState::from_sample_sources(&[wavecrate::sample_sources::SampleSource::new(
            source_root.path().to_path_buf(),
        )]);
    state
        .library
        .folder_browser
        .select_file(sample.display().to_string());
    assert!(
        state
            .library
            .folder_browser
            .set_file_rating_state(&sample, Rating::TRASH_3, false)
    );

    let mut context = radiant::prelude::UiUpdateContext::default();
    state.adjust_selected_rating(-1, &mut context);
    run_command_for_tests(&mut state, context.into_command());

    assert!(sample.exists());
    let expected_selected = sample.display().to_string();
    assert_eq!(
        state.library.folder_browser.selected_file_id(),
        Some(expected_selected.as_str())
    );
    assert_eq!(
        state.library.folder_browser.selected_audio_files()[0].rating,
        Rating::TRASH_3
    );
    assert!(
        state
            .ui
            .status
            .sample
            .contains("Set a trash folder in Settings > General"),
        "{}",
        state.ui.status.sample
    );
}

#[test]
fn delete_selected_folder_moves_it_to_configured_trash_folder() {
    let mut state = gui_state_for_span_tests();
    let source_root = tempfile::tempdir().expect("source root");
    let trash_root = tempfile::tempdir().expect("trash root");
    let drums = source_root.path().join("drums");
    let loops = source_root.path().join("loops");
    fs::create_dir_all(&drums).expect("create drums folder");
    fs::create_dir_all(&loops).expect("create loops folder");
    fs::write(drums.join("kick.wav"), []).expect("write kick wav");
    fs::write(loops.join("loop.wav"), []).expect("write loop wav");
    state.ui.settings.persisted.trash_folder = Some(trash_root.path().to_path_buf());
    state.library.folder_browser =
        FolderBrowserState::from_sample_sources(&[wavecrate::sample_sources::SampleSource::new(
            source_root.path().to_path_buf(),
        )]);
    state
        .library
        .folder_browser
        .apply_message(FolderBrowserMessage::ActivateFolder(
            drums.display().to_string(),
            Default::default(),
        ));

    let mut context = radiant::prelude::UiUpdateContext::default();
    state.delete_selected_item(&mut context);
    run_command_for_tests(&mut state, context.into_command());

    assert!(!drums.exists());
    assert!(trash_root.path().join("drums").join("kick.wav").exists());
    assert!(loops.exists());
    assert_eq!(state.library.folder_browser.selected_file_id(), None);
    state
        .library
        .folder_browser
        .apply_message(FolderBrowserMessage::ActivateFolder(
            loops.display().to_string(),
            Default::default(),
        ));
    assert!(
        state
            .library
            .folder_browser
            .selected_audio_files()
            .iter()
            .any(|file| file.name == "loop.wav")
    );
    assert!(state.ui.status.sample.contains("Moved drums to trash"));
}

#[test]
fn delete_selected_file_requires_configured_trash_folder() {
    let (mut state, _source_root, selected_file) = native_app_state_with_temp_sample("blocked.wav");

    let mut context = radiant::prelude::UiUpdateContext::default();
    state.delete_selected_item(&mut context);
    run_command_for_tests(&mut state, context.into_command());

    assert!(std::path::Path::new(&selected_file).exists());
    assert_eq!(
        state.library.folder_browser.selected_file_id(),
        Some(selected_file.as_str())
    );
    assert!(
        state
            .ui
            .status
            .sample
            .contains("Set a trash folder in Settings > General"),
        "{}",
        state.ui.status.sample
    );
}
