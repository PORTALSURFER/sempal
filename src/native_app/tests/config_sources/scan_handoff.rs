use super::*;
use std::path::Path;

#[test]
fn adding_source_after_startup_registers_it_before_scan_admission_and_finish() {
    let source_root = tempfile::tempdir().expect("source root");
    let mut state = gui_state_for_span_tests();
    let request = state
        .library
        .folder_browser
        .begin_add_source_path(source_root.path().to_path_buf(), 101)
        .expect("new source requests scan");
    let source_id = request.source_id.clone();
    let request_for_worker = request.clone();
    let mut context = ui::UiUpdateContext::default();

    state.launch_folder_scan(request, &mut context);

    assert_eq!(
        state.background.source_lifecycle_generations[&source_id],
        state.background.source_processing.lifecycle_generations()[&source_id],
        "scan registration must synchronize the UI epoch before background work starts"
    );

    let permit = state
        .background
        .source_processing
        .budget_handle()
        .acquire_scan(&source_id)
        .expect("newly added source must be admitted before its first scan");
    let result = crate::native_app::sample_library::folder_browser::scan::scan_source_with_progress(
        request_for_worker,
        |_| {},
        |_| {},
    );
    drop(permit);
    state.finish_folder_scan(result, &mut context);

    assert!(state.library.folder_browser.selected_source_loaded());
    assert!(state.library.folder_progress().is_none());
}

#[test]
fn foreground_scan_hands_exact_committed_identities_to_readiness() {
    let source_root = tempfile::tempdir().expect("source root");
    let sample_path = source_root.path().join("foreground.wav");
    fs::write(&sample_path, [0_u8; 8]).expect("write sample");
    let mut state = gui_state_for_span_tests();
    let request = state
        .library
        .folder_browser
        .begin_add_source_path(source_root.path().to_path_buf(), 104)
        .expect("new source requests scan");
    let source_id = request.source_id.clone();
    let mut context = ui::UiUpdateContext::default();
    state.launch_folder_scan(request.clone(), &mut context);
    let result = crate::native_app::sample_library::folder_browser::scan::scan_source_with_progress(
        request,
        |_| {},
        |_| {},
    );
    let committed_delta = result
        .committed_delta
        .as_ref()
        .expect("successful full scan must carry its committed delta");
    let created = committed_delta
        .created
        .iter()
        .find(|entry| entry.relative_path == Path::new("foreground.wav"))
        .expect("created sample must be present in the committed delta");
    let created_identity = created.identity.clone();

    state.finish_folder_scan(result, &mut context);

    assert!(
        state
            .background
            .source_processing
            .pending_source_delta_contains_identity_for_tests(&source_id, &created_identity),
        "foreground scan must publish the exact committed identity instead of generic discovery"
    );
}

#[test]
fn foreground_scan_terminal_release_admits_coalesced_watcher_paths() {
    let source_root = tempfile::tempdir().expect("source root");
    let sample_path = source_root.path().join("followup.wav");
    fs::write(&sample_path, [0_u8; 8]).expect("write sample");
    let mut state = gui_state_for_span_tests();
    let request = state
        .library
        .folder_browser
        .begin_add_source_path(source_root.path().to_path_buf(), 105)
        .expect("new source requests scan");
    let source_id = request.source_id.clone();
    let mut context = ui::UiUpdateContext::default();
    state.launch_folder_scan(request.clone(), &mut context);
    state.refresh_source_after_filesystem_change(
        source_id.clone(),
        vec![sample_path],
        false,
        true,
        None,
        &mut context,
    );
    assert!(
        !state
            .library
            .targeted_source_sync_active_for_tests(&source_id),
        "watcher paths must wait while the foreground scan owns the source"
    );

    let result = crate::native_app::sample_library::folder_browser::scan::scan_source_with_progress(
        request,
        |_| {},
        |_| {},
    );
    state.finish_folder_scan(result, &mut context);

    assert!(
        state
            .library
            .targeted_source_sync_active_for_tests(&source_id),
        "terminal scan release must immediately admit the coalesced targeted follow-up"
    );
}

#[test]
fn mismatched_folder_scan_registration_cannot_adopt_existing_source_generation() {
    let requested_root = tempfile::tempdir().expect("requested source root");
    let authoritative_root = tempfile::tempdir().expect("authoritative source root");
    let mut state = gui_state_for_span_tests();
    let request = state
        .library
        .folder_browser
        .begin_add_source_path(requested_root.path().to_path_buf(), 102)
        .expect("new source requests scan");
    let source_id = request.source_id.clone();
    let authoritative = wavecrate::sample_sources::SampleSource::new_with_id(
        wavecrate::sample_sources::SourceId::from_string(source_id.clone()),
        authoritative_root.path().to_path_buf(),
    );
    let authoritative_generation = state
        .background
        .source_processing
        .register_source_for_scan(authoritative)
        .expect("seed authoritative descriptor");
    let mut context = ui::UiUpdateContext::default();

    state.launch_folder_scan(request, &mut context);

    assert!(
        !state
            .background
            .source_lifecycle_generations
            .contains_key(&source_id),
        "failed descriptor registration must not publish an expected UI generation"
    );
    assert_eq!(
        state.background.source_processing.lifecycle_generations()[&source_id],
        authoritative_generation
    );
}

#[test]
fn targeted_sync_cannot_pair_updated_storage_with_previous_source_generation() {
    let source_root = tempfile::tempdir().expect("source root");
    let mut state = gui_state_for_span_tests();
    let request = state
        .library
        .folder_browser
        .begin_add_source_path(source_root.path().to_path_buf(), 103)
        .expect("new source requests scan");
    let source_id = request.source_id;
    let original_source = state
        .library
        .folder_browser
        .configured_sample_sources()
        .into_iter()
        .find(|source| source.id.as_str() == source_id)
        .expect("configured source descriptor");
    let original_role = original_source.role;
    let original_metadata_storage = original_source.metadata_storage;
    let original_generation = state
        .background
        .source_processing
        .register_source_for_scan(original_source)
        .expect("seed original descriptor");
    state
        .background
        .source_lifecycle_generations
        .insert(source_id.clone(), original_generation);

    state
        .library
        .folder_browser
        .set_source_protected(&source_id, true)
        .expect("change source storage role");
    let updated_source = state
        .library
        .folder_browser
        .configured_sample_sources()
        .into_iter()
        .find(|source| source.id.as_str() == source_id)
        .expect("updated source descriptor");
    assert_ne!(
        updated_source.role, original_role,
        "the source role must be part of targeted-sync admission"
    );
    assert_ne!(
        updated_source.metadata_storage, original_metadata_storage,
        "the metadata location policy must be part of targeted-sync admission"
    );

    let error = state
        .admit_source_filesystem_sync(&source_id)
        .expect_err("descriptor mismatch must reject targeted sync");

    assert!(error.contains("different descriptor"));
    assert_eq!(
        state.background.source_processing.lifecycle_generations()[&source_id],
        original_generation,
        "rejected admission must not change the supervisor generation"
    );
    assert_eq!(
        state.background.source_lifecycle_generations[&source_id], original_generation,
        "rejected admission must not publish a different UI generation"
    );
}

#[test]
fn source_filesystem_change_during_scan_is_refreshed_after_scan_finishes() {
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
    let mut context = ui::UiUpdateContext::default();
    state.refresh_source_after_filesystem_change(
        source_id.clone(),
        Vec::new(),
        true,
        true,
        None,
        &mut context,
    );

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
    assert!(
        state
            .library
            .pending_source_refresh_contains_for_tests(&source_id)
    );

    let active_task = state
        .library
        .folder_progress()
        .expect("first refresh should be active")
        .task_id;
    assert!(
        state
            .library
            .folder_browser
            .scan_is_active(&source_id, active_task),
        "first scan should still own the active task"
    );
    let finished =
        crate::native_app::sample_library::folder_browser::scan::scan_source_with_progress(
            crate::native_app::sample_library::folder_browser::scan::FolderScanRequest {
                task_id: active_task,
                source_id: source_id.clone(),
                label: String::from("source"),
                root: source_root.path().to_path_buf(),
                database_root: source_root.path().to_path_buf(),
                rating_decay_weeks: crate::native_app::sample_library::folder_browser::scan::FolderScanRequest::default_rating_decay_weeks(),
            },
            |_| {},
            |_| {},
        );
    state.finish_folder_scan(finished, &mut ui::UiUpdateContext::default());
    state.maybe_run_pending_source_refresh(&mut context);

    let next_task = state
        .library
        .folder_progress()
        .expect("pending refresh should start after active scan")
        .task_id;
    assert_ne!(next_task, active_task);
    assert!(
        state
            .library
            .folder_browser
            .scan_is_active(&source_id, next_task)
    );
}
