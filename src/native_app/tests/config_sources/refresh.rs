use super::*;
use crate::native_app::app::SourceSelectionRequest;
use std::path::Path;

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
    let selection = state
        .library
        .begin_select_source(first_source_id.clone(), task_id);
    assert!(matches!(selection, SourceSelectionRequest::Settled));

    assert_eq!(
        state.library.folder_browser.selected_source_id(),
        first_source_id
    );
    assert!(state.library.folder_browser.selected_source_loaded());
    assert!(
        state.background.folder_tree_refresh_task.active().is_none(),
        "library source selection should not queue UI work directly"
    );
    assert!(
        state.library.folder_progress().is_none(),
        "cached source selection must not queue a foreground scan"
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
    let sync_finished = crate::native_app::tests::run_worker_message_for_tests(
        context.into_command(),
        "gui-source-db-sync",
    )
    .expect("db sync command");
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

#[test]
fn source_filesystem_change_patches_new_folder_after_targeted_commit() {
    let source_root = tempfile::tempdir().expect("source root");
    let existing = source_root.path().join("existing");
    fs::create_dir_all(&existing).expect("create existing folder");
    write_test_wav_i16(&existing.join("keep.wav"), &[0, 512, -512]);
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

    let created = source_root.path().join("created-folder");
    let nested = created.join("nested");
    fs::create_dir_all(&nested).expect("create nested folder");
    write_test_wav_i16(&nested.join("new.wav"), &[0, 1024, -1024]);
    let mut context = ui::UiUpdateContext::default();
    state.apply_message(
        crate::native_app::test_support::state::GuiMessage::SourceFilesystemChanged {
            source_id: source_id.clone(),
            paths: vec![PathBuf::from("created-folder")],
            overflowed: false,
            source_root_available: true,
            journal_checkpoint_event_id: None,
        },
        &mut context,
    );

    let sync_finished = crate::native_app::tests::run_worker_message_for_tests(
        context.into_command(),
        "gui-source-db-sync",
    )
    .expect("targeted source sync command");
    state.apply_message(sync_finished, &mut ui::UiUpdateContext::default());
    assert!(
        state
            .library
            .folder_progress()
            .is_none(),
        "a committed directory watcher event should not queue a source-wide folder scan"
    );
    assert!(state
        .library
        .folder_browser
        .folder_path(&created.to_string_lossy())
        .is_some());
    assert!(state
        .library
        .folder_browser
        .folder_path(&nested.to_string_lossy())
        .is_some());
    let db = wavecrate::sample_sources::SourceDatabase::open_for_test_fixture_source_write(
        source_root.path(),
    )
    .expect("db");
    assert!(db
        .list_files()
        .expect("synced rows")
        .iter()
        .any(|entry| entry.relative_path == Path::new("created-folder/nested/new.wav")));
}

#[test]
fn source_filesystem_change_renames_nested_folder_from_coalesced_descendant_events() {
    let source_root = tempfile::tempdir().expect("source root");
    let old_folder = source_root.path().join("old-folder");
    let old_nested = old_folder.join("nested");
    fs::create_dir_all(&old_nested).expect("create old nested folder");
    write_test_wav_i16(&old_nested.join("kick.wav"), &[0, 512, -512]);

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

    let new_folder = source_root.path().join("new-folder");
    fs::rename(&old_folder, &new_folder).expect("rename folder");
    let mut context = ui::UiUpdateContext::default();
    state.apply_message(
        crate::native_app::test_support::state::GuiMessage::SourceFilesystemChanged {
            source_id: source_id.clone(),
            paths: vec![
                PathBuf::from("old-folder/nested/kick.wav"),
                PathBuf::from("new-folder/nested/kick.wav"),
            ],
            overflowed: false,
            source_root_available: true,
            journal_checkpoint_event_id: None,
        },
        &mut context,
    );
    let sync_finished = crate::native_app::tests::run_worker_message_for_tests(
        context.into_command(),
        "gui-source-db-sync",
    )
    .expect("targeted source sync command");
    state.apply_message(sync_finished, &mut ui::UiUpdateContext::default());

    assert!(state.library.folder_progress().is_none());
    assert!(
        state
            .library
            .folder_browser
            .folder_path(&old_folder.to_string_lossy())
            .is_none()
    );
    assert!(
        state
            .library
            .folder_browser
            .folder_path(&new_folder.to_string_lossy())
            .is_some()
    );
    assert!(
        state
            .library
            .folder_browser
            .folder_path(&new_folder.join("nested").to_string_lossy())
            .is_some()
    );
    let db = wavecrate::sample_sources::SourceDatabase::open_for_test_fixture_source_write(
        source_root.path(),
    )
    .expect("db");
    let rows = db.list_files().expect("synced rows");
    assert_eq!(rows.len(), 1);
    assert_eq!(
        rows[0].relative_path,
        Path::new("new-folder/nested/kick.wav")
    );
}

#[test]
fn source_filesystem_change_reparents_nested_folder_between_existing_parents() {
    let source_root = tempfile::tempdir().expect("source root");
    let old_parent = source_root.path().join("old-parent");
    let old_folder = old_parent.join("old-folder");
    let old_nested = old_folder.join("nested");
    let new_parent = source_root.path().join("new-parent");
    fs::create_dir_all(&old_nested).expect("create old nested folder");
    fs::create_dir_all(&new_parent).expect("create new parent folder");
    write_test_wav_i16(&old_nested.join("kick.wav"), &[0, 512, -512]);

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

    let new_folder = new_parent.join("new-folder");
    fs::rename(&old_folder, &new_folder).expect("reparent and rename folder");
    let mut context = ui::UiUpdateContext::default();
    state.apply_message(
        crate::native_app::test_support::state::GuiMessage::SourceFilesystemChanged {
            source_id: source_id.clone(),
            paths: vec![
                PathBuf::from("old-parent/old-folder/nested/kick.wav"),
                PathBuf::from("new-parent/new-folder/nested/kick.wav"),
            ],
            overflowed: false,
            source_root_available: true,
            journal_checkpoint_event_id: None,
        },
        &mut context,
    );
    let sync_finished = crate::native_app::tests::run_worker_message_for_tests(
        context.into_command(),
        "gui-source-db-sync",
    )
    .expect("targeted source sync command");
    state.apply_message(sync_finished, &mut ui::UiUpdateContext::default());

    assert!(state.library.folder_progress().is_none());
    assert!(
        state
            .library
            .folder_browser
            .folder_path(&old_parent.to_string_lossy())
            .is_some()
    );
    assert!(
        state
            .library
            .folder_browser
            .folder_path(&old_folder.to_string_lossy())
            .is_none()
    );
    assert!(
        state
            .library
            .folder_browser
            .folder_path(&new_parent.to_string_lossy())
            .is_some()
    );
    assert!(
        state
            .library
            .folder_browser
            .folder_path(&new_folder.to_string_lossy())
            .is_some()
    );
    assert!(
        state
            .library
            .folder_browser
            .folder_path(&new_folder.join("nested").to_string_lossy())
            .is_some()
    );
    let db = wavecrate::sample_sources::SourceDatabase::open_for_test_fixture_source_write(
        source_root.path(),
    )
    .expect("db");
    let rows = db.list_files().expect("synced rows");
    assert_eq!(rows.len(), 1);
    assert_eq!(
        rows[0].relative_path,
        Path::new("new-parent/new-folder/nested/kick.wav")
    );
}

#[test]
fn source_filesystem_change_removes_nested_folder_from_descendant_event() {
    let source_root = tempfile::tempdir().expect("source root");
    let removed_folder = source_root.path().join("removed-folder");
    let removed_nested = removed_folder.join("nested");
    fs::create_dir_all(&removed_nested).expect("create removed nested folder");
    write_test_wav_i16(&removed_nested.join("kick.wav"), &[0, 512, -512]);
    write_test_wav_i16(&source_root.path().join("keep.wav"), &[0, 1024, -1024]);

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

    fs::remove_dir_all(&removed_folder).expect("remove nested folder");
    let mut context = ui::UiUpdateContext::default();
    state.apply_message(
        crate::native_app::test_support::state::GuiMessage::SourceFilesystemChanged {
            source_id,
            paths: vec![PathBuf::from("removed-folder/nested/kick.wav")],
            overflowed: false,
            source_root_available: true,
            journal_checkpoint_event_id: None,
        },
        &mut context,
    );
    let sync_finished = crate::native_app::tests::run_worker_message_for_tests(
        context.into_command(),
        "gui-source-db-sync",
    )
    .expect("targeted source sync command");
    state.apply_message(sync_finished, &mut ui::UiUpdateContext::default());

    assert!(state.library.folder_progress().is_none());
    assert!(
        state
            .library
            .folder_browser
            .folder_path(&removed_folder.to_string_lossy())
            .is_none()
    );
    let db = wavecrate::sample_sources::SourceDatabase::open_for_test_fixture_source_write(
        source_root.path(),
    )
    .expect("db");
    let rows = db.list_files().expect("synced rows");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].relative_path, Path::new("keep.wav"));
    assert!(state
        .library
        .folder_browser
        .selected_audio_files()
        .iter()
        .any(|file| file.name == "keep.wav"));
}

#[test]
fn source_filesystem_change_removes_unsupported_only_nested_folder_from_descendant_event() {
    let source_root = tempfile::tempdir().expect("source root");
    let removed_folder = source_root.path().join("removed-folder");
    let removed_nested = removed_folder.join("nested");
    fs::create_dir_all(&removed_nested).expect("create removed nested folder");
    fs::write(removed_nested.join("notes.txt"), b"not an indexed sample")
        .expect("write unsupported file");

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
    assert!(state
        .library
        .folder_browser
        .folder_path(&removed_folder.to_string_lossy())
        .is_some());
    let db = wavecrate::sample_sources::SourceDatabase::open_for_test_fixture_source_write(
        source_root.path(),
    )
    .expect("db");
    assert!(db.list_files().expect("initial rows").is_empty());

    fs::remove_dir_all(&removed_folder).expect("remove nested folder");
    let mut context = ui::UiUpdateContext::default();
    state.apply_message(
        crate::native_app::test_support::state::GuiMessage::SourceFilesystemChanged {
            source_id,
            paths: vec![PathBuf::from("removed-folder/nested/notes.txt")],
            overflowed: false,
            source_root_available: true,
            journal_checkpoint_event_id: None,
        },
        &mut context,
    );
    let sync_finished = crate::native_app::tests::run_worker_message_for_tests(
        context.into_command(),
        "gui-source-db-sync",
    )
    .expect("targeted source sync command");
    state.apply_message(sync_finished, &mut ui::UiUpdateContext::default());

    assert!(state.library.folder_progress().is_none());
    assert!(state
        .library
        .folder_browser
        .folder_path(&removed_folder.to_string_lossy())
        .is_none());
    assert!(db.list_files().expect("synced rows").is_empty());
}

#[test]
fn source_filesystem_change_removes_empty_folder_without_manifest_file_delta() {
    let source_root = tempfile::tempdir().expect("source root");
    let empty_folder = source_root.path().join("empty-folder");
    fs::create_dir_all(&empty_folder).expect("create empty folder");

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
    let manifest_revision = state
        .library
        .folder_browser
        .source_projection_revision(&source_id)
        .expect("initial manifest revision");
    assert!(manifest_revision > 0);
    assert!(state
        .library
        .folder_browser
        .folder_path(&empty_folder.to_string_lossy())
        .is_some());
    let db = wavecrate::sample_sources::SourceDatabase::open_for_test_fixture_source_write(
        source_root.path(),
    )
    .expect("db");
    let rows_before = db.list_files().expect("initial rows");

    fs::remove_dir(&empty_folder).expect("remove empty folder");
    let mut context = ui::UiUpdateContext::default();
    state.apply_message(
        crate::native_app::test_support::state::GuiMessage::SourceFilesystemChanged {
            source_id: source_id.clone(),
            paths: vec![PathBuf::from("empty-folder")],
            overflowed: false,
            source_root_available: true,
            journal_checkpoint_event_id: None,
        },
        &mut context,
    );
    let sync_finished = crate::native_app::tests::run_worker_message_for_tests(
        context.into_command(),
        "gui-source-db-sync",
    )
    .expect("targeted source sync command");
    let committed_delta = match &sync_finished {
        crate::native_app::test_support::state::GuiMessage::SourceFilesystemSyncFinished(result) => {
            result
                .result
                .as_ref()
                .expect("targeted source sync result")
                .committed_delta
                .clone()
        }
        message => panic!("expected source sync completion, got {message:?}"),
    };
    assert!(committed_delta.is_empty());
    state.apply_message(sync_finished, &mut ui::UiUpdateContext::default());

    assert!(state.library.folder_progress().is_none());
    assert!(state
        .library
        .folder_browser
        .folder_path(&empty_folder.to_string_lossy())
        .is_none());
    assert_eq!(
        state
            .library
            .folder_browser
            .source_projection_revision(&source_id),
        Some(manifest_revision + 1)
    );
    let rows_after = db.list_files().expect("synced rows");
    assert_eq!(rows_after.len(), rows_before.len());
    assert_eq!(
        rows_after
            .iter()
            .map(|row| row.relative_path.clone())
            .collect::<Vec<_>>(),
        rows_before
            .iter()
            .map(|row| row.relative_path.clone())
            .collect::<Vec<_>>()
    );

    assert!(!state
        .library
        .folder_browser
        .apply_committed_projection_delta(
            &source_id,
            crate::native_app::app::BrowserProjectionDelta {
                manifest_revision: manifest_revision.saturating_sub(1),
                snapshot_revision: manifest_revision.saturating_sub(1),
                folders: Vec::new(),
                removed_file_ids: Vec::new(),
                upserted_files: Vec::new(),
            },
        ));
}

#[cfg(unix)]
#[test]
fn source_filesystem_change_removes_folder_replaced_by_symlink() {
    use std::os::unix::fs as unix_fs;

    let source_root = tempfile::tempdir().expect("source root");
    let replaced = source_root.path().join("replaced");
    fs::create_dir_all(&replaced).expect("create replaced folder");
    write_test_wav_i16(&replaced.join("old.wav"), &[0, 512, -512]);
    let outside = tempfile::tempdir().expect("outside root");
    write_test_wav_i16(&outside.path().join("outside.wav"), &[0, 1024, -1024]);

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
    assert!(state
        .library
        .folder_browser
        .folder_path(&replaced.to_string_lossy())
        .is_some());

    fs::remove_dir_all(&replaced).expect("remove replaced folder");
    unix_fs::symlink(outside.path(), &replaced).expect("replace folder with symlink");
    let mut context = ui::UiUpdateContext::default();
    state.apply_message(
        crate::native_app::test_support::state::GuiMessage::SourceFilesystemChanged {
            source_id,
            paths: vec![PathBuf::from("replaced")],
            overflowed: false,
            source_root_available: true,
            journal_checkpoint_event_id: None,
        },
        &mut context,
    );
    let sync_finished = crate::native_app::tests::run_worker_message_for_tests(
        context.into_command(),
        "gui-source-db-sync",
    )
    .expect("targeted source sync command");
    state.apply_message(sync_finished, &mut ui::UiUpdateContext::default());

    assert!(state
        .library
        .folder_browser
        .folder_path(&replaced.to_string_lossy())
        .is_none());
    assert!(state.library.folder_progress().is_none());
}
