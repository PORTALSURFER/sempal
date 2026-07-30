use super::*;
use crate::native_app::app::{BrowserProjectionDelta, PreparedFolderProjection};
use crate::native_app::sample_library::folder_browser::model::file_entry_with_snapshot_metadata;
use crate::native_app::sample_library::folder_browser::scan_types::{
    FolderScanDiscovery, FolderScanItem, MetadataHydrationStatus,
};
use std::sync::atomic::{AtomicBool, Ordering};

#[test]
fn committed_discovery_emission_stops_at_the_cancellation_boundary() {
    let root = temp_source_root("wavecrate-gui-source-cancelled-discovery");
    for index in 0..96 {
        fs::write(root.join(format!("sample-{index:03}.wav")), [0_u8; 8]).expect("write sample");
    }
    let mut browser = FolderBrowserState::load_default();
    let request = browser
        .begin_add_source_path(root.clone(), 89)
        .expect("new source should request scan");
    let cancel = AtomicBool::new(false);
    let mut discovery_events = Vec::new();

    let result = scan_source_with_progress_cancellable(
        request,
        |_| {},
        |event| {
            discovery_events.push(event);
            cancel.store(true, Ordering::Release);
        },
        &cancel,
        &wavecrate_scan::sample_sources::scanner::UncoordinatedScanWriter,
    );

    assert!(result.cancelled);
    assert_eq!(
        discovery_events.len(),
        1,
        "a cancellation raised by the first committed discovery must fence every later event"
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn switching_away_from_pending_source_does_not_cache_its_placeholder() {
    let first_root = temp_source_root("wavecrate-pending-source-first");
    let second_root = temp_source_root("wavecrate-pending-source-second");
    let mut browser = FolderBrowserState::load_default();
    let first_request = browser
        .begin_add_source_path(first_root.clone(), 41)
        .expect("first source scan");

    browser
        .begin_add_source_path(second_root.clone(), 42)
        .expect("second source scan");

    let first = browser
        .source
        .sources
        .iter()
        .find(|source| source.id == first_request.source_id)
        .expect("first source");
    assert_eq!(first.root_folder, None);

    assert!(
        browser
            .begin_select_source(first_request.source_id.clone(), 43)
            .is_none()
    );
    assert_eq!(browser.selected_source_id(), first_request.source_id);
    assert!(!browser.selected_source_loaded());

    let _ = fs::remove_dir_all(first_root);
    let _ = fs::remove_dir_all(second_root);
}

#[test]
fn source_scan_installs_finished_tree_after_placeholder_selection() {
    let root = temp_source_root("wavecrate-gui-source-scan");
    fs::create_dir_all(root.join("drums")).expect("create nested folder");
    fs::write(root.join("drums").join("kick.wav"), [0_u8; 8]).expect("write wav");
    let mut browser = FolderBrowserState::load_default();
    let request = browser
        .begin_add_source_path(root.clone(), 42)
        .expect("new source should request scan");

    assert_eq!(browser.root_path(), root.as_path());
    assert!(browser.selected_audio_files().is_empty());

    let mut progress_events = Vec::new();
    let mut discovery_events = Vec::new();
    let result = scan_source_with_progress(
        request,
        |progress| progress_events.push(progress),
        |event| discovery_events.push(event),
    );
    assert_eq!(result.source_db_error, None);
    let db = SourceDatabase::open_for_test_fixture_source_write(&root).expect("source db");
    let entries = db.list_files().expect("source db files");
    assert!(
        entries
            .iter()
            .any(|entry| entry.relative_path == std::path::Path::new("drums/kick.wav"))
    );
    assert!(
        db.get_metadata(wavecrate::sample_sources::db::META_LAST_SCAN_COMPLETED_AT)
            .expect("scan metadata")
            .is_some()
    );
    assert!(browser.apply_scan_finished(result));

    browser.begin_select_source(root.to_string_lossy().to_string(), 43);
    browser.activate_folder(path_id(&root.join("drums")));
    assert_eq!(
        browser
            .selected_audio_files()
            .iter()
            .map(|file| file.name.as_str())
            .collect::<Vec<_>>(),
        vec!["kick.wav"]
    );
    assert!(progress_events.iter().any(|progress| {
        matches!(
            progress.lifecycle,
            super::super::scan::FolderScanLifecycle::Scanning
        ) && progress.total == 0
    }));
    assert!(!discovery_events.is_empty());
    let _ = fs::remove_dir_all(root);
}

#[test]
fn source_scan_hydration_does_not_mutate_ratings() {
    let root = temp_source_root("wavecrate-gui-rating-decay");
    fs::write(root.join("unlocked.wav"), [0_u8; 8]).expect("write wav");
    fs::write(root.join("locked.wav"), [0_u8; 8]).expect("write wav");
    let mut browser = FolderBrowserState::load_default();
    let initial_request = browser
        .begin_add_source_path(root.clone(), 42)
        .expect("new source should request scan");
    assert!(browser.apply_scan_finished(scan_source_with_progress(
        initial_request,
        |_| {},
        |_| {}
    )));

    let stale_curated_at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock")
        .as_secs()
        .saturating_sub(8 * 7 * 24 * 60 * 60 + 1) as i64;
    let unlocked_relative = std::path::Path::new("unlocked.wav");
    let locked_relative = std::path::Path::new("locked.wav");
    let db = SourceDatabase::open_for_test_fixture_source_write(&root).expect("source db");
    db.set_tag(unlocked_relative, Rating::KEEP_3)
        .expect("seed unlocked rating");
    db.set_last_curated_at(unlocked_relative, stale_curated_at)
        .expect("seed unlocked curation time");
    db.set_tag(locked_relative, Rating::KEEP_3)
        .expect("seed locked rating");
    db.set_locked(locked_relative, true)
        .expect("seed locked flag");
    db.set_last_curated_at(locked_relative, stale_curated_at)
        .expect("seed locked curation time");

    let mut decay_request = browser
        .begin_selected_source_scan(43)
        .expect("selected source refresh should queue");
    decay_request.rating_decay_weeks = 4;
    let result = scan_source_with_progress(decay_request, |_| {}, |_| {});
    let unlocked_file = result
        .folder
        .all_files()
        .into_iter()
        .find(|file| file.name == "unlocked.wav")
        .expect("unlocked file");
    let locked_file = result
        .folder
        .all_files()
        .into_iter()
        .find(|file| file.name == "locked.wav")
        .expect("locked file");

    assert_eq!(unlocked_file.rating, Rating::KEEP_3);
    assert_eq!(locked_file.rating, Rating::KEEP_3);
    let rows = db.list_files().expect("decayed source db files");
    assert_eq!(
        rows.iter()
            .find(|entry| entry.relative_path == unlocked_relative)
            .expect("unlocked row")
            .tag,
        Rating::KEEP_3
    );
    assert_eq!(
        rows.iter()
            .find(|entry| entry.relative_path == locked_relative)
            .expect("locked row")
            .tag,
        Rating::KEEP_3
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn metadata_hydration_failure_preserves_last_good_browser_metadata() {
    let root = temp_source_root("wavecrate-gui-metadata-hydration-failure");
    fs::write(root.join("sample.wav"), [0_u8; 8]).expect("write wav");
    let mut browser = FolderBrowserState::load_default();
    let initial_request = browser
        .begin_add_source_path(root.clone(), 42)
        .expect("new source should request scan");
    assert!(browser.apply_scan_finished(scan_source_with_progress(
        initial_request,
        |_| {},
        |_| {}
    )));
    let db = SourceDatabase::open_for_test_fixture_source_write(&root).expect("source db");
    db.set_tag(std::path::Path::new("sample.wav"), Rating::KEEP_3)
        .expect("seed rating");
    let refresh = browser
        .begin_selected_source_scan(43)
        .expect("metadata refresh");
    assert!(browser.apply_scan_finished(scan_source_with_progress(refresh, |_| {}, |_| {})));

    let failed_refresh = browser
        .begin_selected_source_scan(44)
        .expect("failed metadata refresh");
    let mut failed = scan_source_with_progress(failed_refresh, |_| {}, |_| {});
    failed.metadata_hydration = MetadataHydrationStatus::Failed {
        error: String::from("database unavailable"),
    };
    failed.folder.files.clear();
    assert!(browser.apply_scan_finished(failed));

    assert_eq!(browser.selected_audio_files().len(), 1);
    assert_eq!(browser.selected_audio_files()[0].rating, Rating::KEEP_3);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn committed_projection_delta_applies_only_at_the_next_revision() {
    let root = temp_source_root("wavecrate-gui-committed-projection-delta");
    let old = root.join("old.wav");
    let new = root.join("nested/new.wav");
    fs::write(&old, [0_u8; 8]).expect("write original");
    let mut browser = FolderBrowserState::load_default();
    let request = browser
        .begin_add_source_path(root.clone(), 51)
        .expect("initial scan");
    let source_id = request.source_id.clone();
    assert!(browser.apply_scan_finished(scan_source_with_progress(request, |_| {}, |_| {})));
    let revision = browser
        .source
        .sources
        .iter()
        .find(|source| source.id == source_id)
        .and_then(|source| source.projection_revision)
        .expect("projection revision");
    let new_file =
        file_entry_with_snapshot_metadata(&new, 12, Rating::KEEP_1, false, Vec::new(), None, None);

    assert!(browser.apply_committed_projection_delta(
        &source_id,
        BrowserProjectionDelta {
            manifest_revision: revision + 1,
            snapshot_revision: revision + 1,
            folders: vec![root.join("nested")],
            removed_file_ids: vec![path_id(&old)],
            upserted_files: vec![new_file],
        },
    ));
    assert!(browser.tree.folders[0].find_file(&path_id(&old)).is_none());
    assert_eq!(
        browser.tree.folders[0]
            .find_file(&path_id(&new))
            .expect("incremental file")
            .rating,
        Rating::KEEP_1
    );

    assert!(!browser.apply_committed_projection_delta(
        &source_id,
        BrowserProjectionDelta {
            manifest_revision: revision + 3,
            snapshot_revision: revision + 3,
            folders: Vec::new(),
            removed_file_ids: vec![path_id(&new)],
            upserted_files: Vec::new(),
        },
    ));
    assert!(browser.tree.folders[0].find_file(&path_id(&new)).is_some());
    let _ = fs::remove_dir_all(root);
}

#[test]
fn equal_revision_applies_structural_projection_but_stale_revision_remains_fenced() {
    let root = temp_source_root("wavecrate-gui-equal-revision-structural-projection");
    let empty = root.join("empty");
    fs::create_dir_all(&empty).expect("create empty folder");
    let mut browser = FolderBrowserState::load_default();
    let request = browser
        .begin_add_source_path(root.clone(), 52)
        .expect("initial scan");
    let source_id = request.source_id.clone();
    assert!(browser.apply_scan_finished(scan_source_with_progress(request, |_| {}, |_| {})));
    let revision = browser
        .source
        .sources
        .iter()
        .find(|source| source.id == source_id)
        .and_then(|source| source.projection_revision)
        .expect("projection revision");
    assert!(revision > 0);

    assert!(browser.apply_committed_projection(
        &source_id,
        Some(BrowserProjectionDelta {
            manifest_revision: revision,
            snapshot_revision: revision,
            folders: Vec::new(),
            removed_file_ids: Vec::new(),
            upserted_files: Vec::new(),
        }),
        vec![PreparedFolderProjection {
            relative_path: std::path::PathBuf::from("empty"),
            snapshot_revision: revision,
            folder: None,
        }],
    ));
    assert!(browser.find_folder(&path_id(&empty)).is_none());
    assert_eq!(
        browser
            .source
            .sources
            .iter()
            .find(|source| source.id == source_id)
            .and_then(|source| source.projection_revision),
        Some(revision)
    );

    assert!(!browser.apply_committed_projection_delta(
        &source_id,
        BrowserProjectionDelta {
            manifest_revision: revision.saturating_sub(1),
            snapshot_revision: revision.saturating_sub(1),
            folders: Vec::new(),
            removed_file_ids: Vec::new(),
            upserted_files: Vec::new(),
        },
    ));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn source_scan_publishes_restored_rating_after_large_rename() {
    let root = temp_source_root("wavecrate-gui-large-rename");
    let old_path = root.join("old.wav");
    let new_path = root.join("new.wav");
    fs::write(&old_path, vec![7_u8; 9 * 1024 * 1024]).expect("write large wav");
    let mut browser = FolderBrowserState::load_default();
    let initial_request = browser
        .begin_add_source_path(root.clone(), 42)
        .expect("new source should request scan");
    assert!(browser.apply_scan_finished(scan_source_with_progress(
        initial_request,
        |_| {},
        |_| {}
    )));

    let db = SourceDatabase::open_for_test_fixture_source_write(&root).expect("source db");
    db.set_tag(std::path::Path::new("old.wav"), Rating::KEEP_1)
        .expect("rate original file");
    fs::rename(&old_path, &new_path).expect("rename large wav");

    let request = browser
        .begin_selected_source_scan(43)
        .expect("selected source refresh should queue");
    let result = scan_source_with_progress(request, |_| {}, |_| {});
    let renamed = result
        .folder
        .all_files()
        .into_iter()
        .find(|file| file.name == "new.wav")
        .expect("renamed file");

    assert_eq!(result.source_db_error, None);
    assert_eq!(renamed.rating, Rating::KEEP_1);
    assert_eq!(
        db.entry_for_path(std::path::Path::new("new.wav"))
            .unwrap()
            .unwrap()
            .tag,
        Rating::KEEP_1
    );
    assert!(db.list_pending_renames().unwrap().is_empty());
    let _ = fs::remove_dir_all(root);
}

#[test]
fn non_wav_audio_looking_files_are_visible_but_not_supported_audio() {
    let root = temp_source_root("wavecrate-gui-unsupported-audio");
    let drums = root.join("drums");
    fs::create_dir_all(&drums).expect("create drums folder");
    for name in ["kick.wav", "loop.aif", "loop.aiff", "loop.flac", "loop.mp3"] {
        fs::write(drums.join(name), [0_u8; 8]).expect("write audio-looking file");
    }

    let mut browser = FolderBrowserState::from_root(root.clone());
    browser.activate_folder(path_id(&drums));

    assert_eq!(
        browser
            .selected_audio_files()
            .iter()
            .map(|file| file.name.as_str())
            .collect::<Vec<_>>(),
        vec!["kick.wav"]
    );

    let unsupported = browser
        .selected_files()
        .iter()
        .filter(|file| file.kind == "Unsupported audio")
        .map(|file| file.name.as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        unsupported,
        vec!["loop.aif", "loop.aiff", "loop.flac", "loop.mp3"]
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn apple_double_sidecars_are_hidden_from_source_browser() {
    let root = temp_source_root("wavecrate-gui-appledouble-sidecars");
    let drums = root.join("drums");
    fs::create_dir_all(&drums).expect("create drums folder");
    fs::write(drums.join("kick.wav"), [0_u8; 8]).expect("write wav");
    fs::write(drums.join("._kick.wav"), [0_u8; 8]).expect("write sidecar");

    let mut browser = FolderBrowserState::from_root(root.clone());
    browser.activate_folder(path_id(&drums));

    assert_eq!(
        browser
            .selected_files()
            .iter()
            .map(|file| file.name.as_str())
            .collect::<Vec<_>>(),
        vec!["kick.wav"]
    );
    assert_eq!(
        browser
            .selected_audio_files()
            .iter()
            .map(|file| file.name.as_str())
            .collect::<Vec<_>>(),
        vec!["kick.wav"]
    );
    let _ = fs::remove_dir_all(root);
}
#[test]
fn source_scan_discoveries_populate_selected_tree_before_finish() {
    let root = temp_source_root("wavecrate-gui-source-streaming");
    let drums = root.join("drums");
    fs::create_dir_all(&drums).expect("create nested folder");
    fs::write(drums.join("kick.wav"), [0_u8; 8]).expect("write wav");
    let mut browser = FolderBrowserState::load_default();
    let request = browser
        .begin_add_source_path(root.clone(), 77)
        .expect("new source should request scan");

    let mut progress_events = Vec::new();
    let mut discovery_events = Vec::new();
    let result = scan_source_with_progress(
        request,
        |progress| progress_events.push(progress),
        |event| discovery_events.push(event),
    );

    for event in discovery_events {
        browser.apply_scan_discovered(event);
    }
    browser.activate_folder(path_id(&drums));
    assert_eq!(
        browser
            .selected_audio_files()
            .iter()
            .map(|file| file.name.as_str())
            .collect::<Vec<_>>(),
        vec!["kick.wav"]
    );

    assert!(browser.apply_scan_finished(result));
    assert!(progress_events.iter().all(|progress| progress.total == 0));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn selected_source_refresh_preserves_clicked_nested_folder() {
    let root = temp_source_root("wavecrate-gui-source-refresh-preserve-selection");
    let drums = root.join("drums");
    let kicks = drums.join("kicks");
    let empty = drums.join("empty");
    fs::create_dir_all(&kicks).expect("create nested folder");
    fs::create_dir_all(&empty).expect("create empty sibling");
    fs::write(kicks.join("kick.wav"), [0_u8; 8]).expect("write wav");
    let mut browser = FolderBrowserState::from_root(root.clone());
    let drums_id = path_id(&drums);
    let kicks_id = path_id(&kicks);
    let empty_id = path_id(&empty);

    let request = browser
        .begin_selected_source_scan(101)
        .expect("selected source refresh should queue");
    browser.activate_folder(drums_id.clone());
    browser.activate_folder(kicks_id.clone());
    let result = scan_source_with_progress(request, |_| {}, |_| {});

    assert!(browser.apply_scan_finished(result));
    let visible = browser.visible_folders();

    assert_eq!(browser.selected_folder_path(), Some(kicks.clone()));
    assert!(
        visible
            .iter()
            .any(|folder| folder.id == kicks_id && folder.selected),
        "refresh finish should keep the clicked folder visible and selected"
    );
    assert!(
        browser.tree.expanded_folders.contains(&drums_id),
        "refresh finish should keep selected-folder ancestors expanded"
    );
    assert!(
        visible.iter().all(|folder| folder.id != empty_id),
        "empty sibling folders should be hidden by default"
    );

    browser.apply_message(FolderBrowserMessage::ToggleEmptyFolderVisibility);
    assert!(
        browser
            .visible_folders()
            .iter()
            .any(|folder| folder.id == empty_id && !folder.has_children),
        "show-empty toggle should reveal empty siblings without bogus disclosure state"
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn source_scan_loads_deep_folders_before_click_verification() {
    let root = temp_source_root("wavecrate-gui-source-scan-deep-folders");
    let deep = root
        .join("level-1")
        .join("level-2")
        .join("level-3")
        .join("level-4")
        .join("level-5");
    fs::create_dir_all(&deep).expect("create deep folder");

    let browser = FolderBrowserState::from_root(root.clone());

    assert!(
        browser.find_folder(&path_id(&deep)).is_some(),
        "source scans should represent deep on-disk folders before a click-time refresh runs"
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn source_scan_loads_all_sibling_folders_before_click_verification() {
    let root = temp_source_root("wavecrate-gui-source-scan-many-siblings");
    for index in 0..96 {
        fs::create_dir_all(root.join(format!("folder-{index:03}"))).expect("create sibling");
    }
    let late_sibling = root.join("folder-095");

    let browser = FolderBrowserState::from_root(root.clone());

    assert!(
        browser.find_folder(&path_id(&late_sibling)).is_some(),
        "source scans should not drop on-disk sibling folders beyond an arbitrary projection cap"
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn batched_scan_discoveries_clone_selected_tree_once_per_batch() {
    let root = temp_source_root("wavecrate-gui-source-batch");
    let drums = root.join("drums");
    fs::create_dir_all(&drums).expect("create nested folder");
    fs::write(drums.join("kick.wav"), [0_u8; 8]).expect("write wav");
    fs::write(drums.join("snare.wav"), [0_u8; 8]).expect("write wav");
    let mut browser = FolderBrowserState::load_default();
    let request = browser
        .begin_add_source_path(root.clone(), 88)
        .expect("new source should request scan");

    let mut discovery_events = Vec::new();
    let result = scan_source_with_progress(request, |_| {}, |event| discovery_events.push(event));
    assert!(matches!(
        discovery_events.first().map(|event| &event.item),
        Some(FolderScanItem::ResetFolder)
    ));
    let mut batches = Vec::<Vec<FolderScanDiscovery>>::new();
    for event in discovery_events {
        if batches
            .last()
            .and_then(|batch| batch.first())
            .is_some_and(|first| first.committed_revision != event.committed_revision)
        {
            batches.push(Vec::new());
        }
        if batches.is_empty() {
            batches.push(Vec::new());
        }
        batches.last_mut().expect("batch").push(event);
    }
    for (sequence, events) in batches.into_iter().enumerate() {
        assert!(
            browser.apply_scan_discovered_batch(FolderScanDiscoveryBatch {
                task_id: 88,
                source_id: path_id(&root),
                committed_revision: events.first().and_then(|event| event.committed_revision),
                lifecycle_generation: None,
                sequence: sequence as u64,
                events,
            })
        );
    }
    browser.activate_folder(path_id(&drums));
    assert_eq!(browser.selected_audio_files().len(), 2);

    assert!(browser.apply_scan_finished(result));
    let _ = fs::remove_dir_all(root);
}
