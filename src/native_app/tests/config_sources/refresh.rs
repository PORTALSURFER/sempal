use super::*;
use crate::native_app::app::SourceSelectionRequest;

#[test]
fn context_source_refresh_queues_scan_without_clearing_loaded_tree() {
    let source_root = tempfile::tempdir().expect("source root");
    let drums = source_root.path().join("drums");
    fs::create_dir_all(&drums).expect("create drums");
    fs::write(drums.join("kick.wav"), [0_u8; 8]).expect("write sample");
    let mut state = gui_state_for_span_tests();
    let request = state
        .library
        .folder_browser
        .begin_add_source_path(source_root.path().to_path_buf(), 100)
        .expect("new source requests scan");
    let source_id = request.source_id.clone();
    let result = crate::native_app::sample_library::folder_browser::scan::scan_source_with_progress(
        request,
        |_| {},
        |_| {},
    );
    state.finish_folder_scan(result, &mut ui::UiUpdateContext::default());
    state.ui.browser_interaction.context_menu = Some(
        crate::native_app::test_support::context_menu::BrowserContextMenu {
            kind: crate::native_app::test_support::context_menu::BrowserContextTargetKind::Source,
            path: source_root.path().to_path_buf(),
            source_id: Some(source_id.clone()),
            source_role: wavecrate::sample_sources::SourceRole::Normal,
            source_removable: true,
            folder_locked: false,
            folder_lock_inherited: false,
            metadata_tag: None,
            collection: None,
            sample_missing: false,
            sample_keep_locked: false,
            anchor: Point::new(12.0, 24.0),
            title: String::from("source root"),
        },
    );
    let visible_before = state.library.folder_browser.selected_audio_files().len();
    let mut context = ui::UiUpdateContext::default();

    state.refresh_context_source(&mut context);

    assert_eq!(state.ui.browser_interaction.context_menu, None);
    let task_id = state
        .library
        .folder_progress()
        .expect("refresh should show scan progress")
        .task_id;
    assert!(
        state
            .library
            .folder_browser
            .scan_is_active(&source_id, task_id),
        "refresh should queue the next background scan task"
    );
    assert_eq!(
        state.library.folder_browser.selected_audio_files().len(),
        visible_before,
        "refresh should keep the current cached tree visible while the scan runs"
    );
    assert!(state.ui.status.sample.contains("Queued source scan"));
}

#[test]
fn enabling_empty_folders_queues_tree_refresh_for_disk_only_folders() {
    let source_root = tempfile::tempdir().expect("source root");
    write_test_wav_i16(&source_root.path().join("kick.wav"), &[0, 512, -512]);
    let folder_browser = crate::native_app::test_support::state::FolderBrowserState::from_root(
        source_root.path().to_path_buf(),
    );
    let mut state = crate::native_app::test_support::state::NativeAppStateFixture::default()
        .with_folder_browser(folder_browser)
        .with_sample_status("Ready")
        .build();
    let disk_only_empty = source_root.path().join("new-empty-folder");
    fs::create_dir_all(&disk_only_empty).expect("create empty folder after source load");
    assert!(
        state
            .library
            .folder_browser
            .folder_path(&disk_only_empty.to_string_lossy())
            .is_none(),
        "test setup should leave the empty folder outside the loaded tree"
    );

    let mut context = ui::UiUpdateContext::default();
    state.apply_folder_browser_message(
        crate::native_app::sample_library::folder_browser::commands::FolderBrowserMessage::ToggleEmptyFolderVisibility,
        &mut context,
    );

    assert!(
        state
            .library
            .folder_browser
            .empty_folder_visibility_enabled()
    );
    assert!(
        state.background.folder_tree_refresh_task.active().is_some(),
        "show-empty toggle should immediately queue a selected-source tree refresh"
    );
}

#[test]
fn selecting_missing_source_reports_missing_status_without_scan() {
    let temp = tempfile::tempdir().expect("tempdir");
    let missing_root = temp.path().join("missing-source");
    let source = wavecrate::sample_sources::SampleSource::new(missing_root.clone());
    let source_id = source.id.as_str().to_string();
    let folder_browser =
        crate::native_app::test_support::state::FolderBrowserState::from_sample_sources_deferred(
            &[source],
        );
    let mut state = crate::native_app::test_support::state::NativeAppStateFixture::default()
        .with_folder_browser(folder_browser)
        .with_sample_status("Ready")
        .build();
    let mut context = ui::UiUpdateContext::default();

    state.select_source(source_id.clone(), &mut context);

    assert_eq!(state.library.folder_browser.selected_source_id(), source_id);
    assert!(state.library.folder_browser.source_is_missing(&source_id));
    assert!(state.library.folder_progress().is_none());
    assert_eq!(
        state.ui.status.sample,
        format!("Source missing: {}", missing_root.display())
    );
}

#[test]
fn selecting_loaded_cached_source_keeps_tree_visible_and_reconciles_in_background() {
    let first_root = tempfile::tempdir().expect("first source root");
    let second_root = tempfile::tempdir().expect("second source root");
    write_test_wav_i16(&first_root.path().join("first.wav"), &[0, 512, -512]);
    write_test_wav_i16(&second_root.path().join("second.wav"), &[0, 1024, -1024]);
    let mut state = gui_state_for_span_tests();
    let first_request = state
        .library
        .folder_browser
        .begin_add_source_path(first_root.path().to_path_buf(), 100)
        .expect("first source requests scan");
    let first_source_id = first_request.source_id.clone();
    let first_result =
        crate::native_app::sample_library::folder_browser::scan::scan_source_with_progress(
            first_request,
            |_| {},
            |_| {},
        );
    state.finish_folder_scan(first_result, &mut ui::UiUpdateContext::default());
    let second_request = state
        .library
        .folder_browser
        .begin_add_source_path(second_root.path().to_path_buf(), 101)
        .expect("second source requests scan");
    let second_source_id = second_request.source_id.clone();
    let second_result =
        crate::native_app::sample_library::folder_browser::scan::scan_source_with_progress(
            second_request,
            |_| {},
            |_| {},
        );
    state.finish_folder_scan(second_result, &mut ui::UiUpdateContext::default());
    assert_eq!(
        state.library.folder_browser.selected_source_id(),
        second_source_id
    );
    let task_id = state.next_folder_task_id();
    let SourceSelectionRequest::Queued(request) = state
        .library
        .begin_select_source(first_source_id.clone(), task_id)
    else {
        panic!("selecting a cached source should queue reconciliation");
    };

    assert_eq!(
        state.library.folder_browser.selected_source_id(),
        first_source_id
    );
    assert!(state.library.folder_browser.selected_source_loaded());
    assert!(
        state.background.folder_tree_refresh_task.active().is_none(),
        "source switching should reconcile through the source scan worker"
    );
    state.library.start_folder_scan(&request);
    let progress = state
        .library
        .folder_progress()
        .expect("selecting a loaded cached source should start a background scan");
    assert!(
        state
            .library
            .folder_browser
            .scan_is_active(&first_source_id, progress.task_id),
        "the selected source should own the queued reconciliation scan"
    );
}

#[test]
fn source_scan_records_discovered_audio_as_new_harvest_files() {
    let config_base = tempfile::tempdir().expect("config base");
    let _base_guard = wavecrate::app_dirs::ConfigBaseGuard::set(config_base.path().to_path_buf());
    let source_root = tempfile::tempdir().expect("source root");
    let nested = source_root.path().join("drums");
    fs::create_dir_all(&nested).expect("create nested folder");
    let sample = nested.join("harvest-new.wav");
    write_test_wav_i16(&sample, &[0, 1024, -1024, 0]);
    let mut state = gui_state_for_span_tests();
    let request = state
        .library
        .folder_browser
        .begin_add_source_path(source_root.path().to_path_buf(), 100)
        .expect("new source requests scan");
    let result = crate::native_app::sample_library::folder_browser::scan::scan_source_with_progress(
        request,
        |_| {},
        |_| {},
    );

    state.finish_folder_scan(result, &mut ui::UiUpdateContext::default());

    let (source, relative_path) = state
        .library
        .folder_browser
        .sample_source_for_file_path(&sample)
        .expect("sample should belong to scanned source");
    let harvest_key = wavecrate::sample_sources::HarvestFileKey::new(source.id, relative_path);
    let harvest_record = wavecrate::sample_sources::library::harvest_file(&harvest_key)
        .expect("load harvest file")
        .expect("scanned audio should have a harvest row");
    assert_eq!(
        harvest_record.state,
        wavecrate::sample_sources::HarvestState::New
    );
    assert!(harvest_record.discovered_at > 0);
    assert_eq!(
        harvest_record.file_size,
        Some(fs::metadata(&sample).unwrap().len())
    );
    assert!(harvest_record.seen_at.is_none());
    assert!(harvest_record.touched_at.is_none());
}

#[test]
fn source_filesystem_change_queues_refresh_without_clearing_loaded_tree() {
    let source_root = tempfile::tempdir().expect("source root");
    let drums = source_root.path().join("drums");
    fs::create_dir_all(&drums).expect("create drums");
    fs::write(drums.join("kick.wav"), [0_u8; 8]).expect("write sample");
    let mut state = gui_state_for_span_tests();
    let request = state
        .library
        .folder_browser
        .begin_add_source_path(source_root.path().to_path_buf(), 100)
        .expect("new source requests scan");
    let source_id = request.source_id.clone();
    let result = crate::native_app::sample_library::folder_browser::scan::scan_source_with_progress(
        request,
        |_| {},
        |_| {},
    );
    state.finish_folder_scan(result, &mut ui::UiUpdateContext::default());
    let visible_before = state.library.folder_browser.selected_audio_files().len();
    let mut context = ui::UiUpdateContext::default();

    state.apply_message(
        crate::native_app::test_support::state::GuiMessage::SourceFilesystemChanged {
            source_id: source_id.clone(),
            paths: Vec::new(),
            overflowed: true,
            source_root_available: true,
            journal_checkpoint_event_id: None,
        },
        &mut context,
    );

    let task_id = state
        .library
        .folder_progress()
        .expect("filesystem change should show scan progress")
        .task_id;
    assert!(
        state
            .library
            .folder_browser
            .scan_is_active(&source_id, task_id)
    );
    assert_eq!(
        state.library.folder_browser.selected_audio_files().len(),
        visible_before,
        "live sync should keep the current cached tree visible while the scan runs"
    );
}

#[test]
fn source_filesystem_change_syncs_removed_file_to_source_database() {
    let source_root = tempfile::tempdir().expect("source root");
    fs::write(source_root.path().join("stale.wav"), [0_u8; 8]).expect("write stale sample");
    fs::write(source_root.path().join("keep.wav"), [1_u8; 8]).expect("write keep sample");
    let mut state = gui_state_for_span_tests();
    let request = state
        .library
        .folder_browser
        .begin_add_source_path(source_root.path().to_path_buf(), 100)
        .expect("new source requests scan");
    let source_id = request.source_id.clone();
    let result = crate::native_app::sample_library::folder_browser::scan::scan_source_with_progress(
        request,
        |_| {},
        |_| {},
    );
    state.finish_folder_scan(result, &mut ui::UiUpdateContext::default());
    let db = wavecrate::sample_sources::SourceDatabase::open_for_test_fixture_source_write(
        source_root.path(),
    )
    .expect("db");
    assert_eq!(db.list_files().expect("seeded rows").len(), 2);
    fs::remove_file(source_root.path().join("stale.wav")).expect("remove stale sample");
    let mut context = ui::UiUpdateContext::default();

    state.apply_message(
        crate::native_app::test_support::state::GuiMessage::SourceFilesystemChanged {
            source_id: source_id.clone(),
            paths: vec![PathBuf::from("stale.wav")],
            overflowed: false,
            source_root_available: true,
            journal_checkpoint_event_id: None,
        },
        &mut context,
    );

    assert_eq!(
        state
            .library
            .folder_browser
            .selected_audio_files()
            .into_iter()
            .map(|file| file.name.clone())
            .collect::<Vec<_>>(),
        vec!["keep.wav", "stale.wav"],
        "watcher hints must not patch the visible tree before the source transaction commits"
    );
    let sync_finished =
        run_named_perform(context.into_command(), "gui-source-db-sync").expect("db sync command");
    let mut post_commit = ui::UiUpdateContext::default();
    state.apply_message(sync_finished, &mut post_commit);

    let rows = db.list_files().expect("synced rows");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].relative_path, std::path::Path::new("keep.wav"));
    let refresh_task = state
        .library
        .folder_progress()
        .expect("post-commit projection refresh should run in the background")
        .task_id;
    assert_eq!(
        state
            .library
            .folder_browser
            .selected_audio_files()
            .into_iter()
            .map(|file| file.name.clone())
            .collect::<Vec<_>>(),
        vec!["keep.wav", "stale.wav"],
        "the UI thread must retain owned projection data while background refresh runs"
    );
    let refreshed =
        crate::native_app::sample_library::folder_browser::scan::scan_source_with_progress(
            crate::native_app::sample_library::folder_browser::scan::FolderScanRequest {
                task_id: refresh_task,
                source_id,
                label: String::from("source"),
                root: source_root.path().to_path_buf(),
                database_root: source_root.path().to_path_buf(),
                rating_decay_weeks: crate::native_app::sample_library::folder_browser::scan::FolderScanRequest::default_rating_decay_weeks(),
            },
            |_| {},
            |_| {},
        );
    state.finish_folder_scan(refreshed, &mut ui::UiUpdateContext::default());
    assert_eq!(
        state
            .library
            .folder_browser
            .selected_audio_files()
            .into_iter()
            .map(|file| file.name.clone())
            .collect::<Vec<_>>(),
        vec!["keep.wav"],
        "the browser projection should refresh only from committed background state"
    );
}

fn run_named_perform(
    command: Command<crate::native_app::test_support::state::GuiMessage>,
    target_name: &'static str,
) -> Option<crate::native_app::test_support::state::GuiMessage> {
    match command {
        Command::Perform { name, work, .. } if name == target_name => Some(work()),
        Command::Batch(commands) => commands
            .into_iter()
            .find_map(|command| run_named_perform(command, target_name)),
        _ => None,
    }
}
