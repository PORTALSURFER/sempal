use super::*;
use crate::native_app::sample_library::folder_browser::scan_types::{
    FolderScanItem, RatingHydrationStatus,
};
use crate::native_app::sample_library::folder_browser::{
    refreshed_file_entries_for_paths, scan::verify_direct_folder,
};
use wavecrate::sample_sources::{Rating, SourceDatabase};
#[test]
fn selected_folder_audio_projection_refreshes_after_file_update() {
    let root = temp_source_root("wavecrate-gui-folder-projection-refresh");
    let drums = root.join("drums");
    fs::create_dir_all(&drums).expect("create drums folder");
    let kick = drums.join("kick.wav");
    let snare = drums.join("snare.wav");
    fs::write(&kick, [0_u8; 8]).expect("write kick");

    let mut browser = FolderBrowserState::from_root(root.clone());
    browser.activate_folder(path_id(&drums));
    assert_eq!(
        browser.selected_audio_projection_cache_len_for_tests(),
        0,
        "source load should leave projection cache cold until the selected folder is queried"
    );
    assert_eq!(
        browser
            .selected_audio_files()
            .iter()
            .map(|file| file.name.as_str())
            .collect::<Vec<_>>(),
        vec!["kick.wav"]
    );

    fs::write(&snare, [0_u8; 8]).expect("write snare");
    assert!(browser.refresh_file_path(&snare));
    assert_eq!(
        browser.selected_audio_projection_cache_len_for_tests(),
        0,
        "content revision changes should invalidate stale projection cache entries"
    );

    assert_eq!(
        browser
            .selected_audio_files()
            .iter()
            .map(|file| file.name.as_str())
            .collect::<Vec<_>>(),
        vec!["kick.wav", "snare.wav"]
    );
    assert_eq!(
        browser.selected_audio_projection_cache_len_for_tests(),
        1,
        "projection cache should rebuild explicitly after invalidation"
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn selected_folder_audio_projection_refreshes_multiple_files_as_one_batch() {
    let root = temp_source_root("wavecrate-gui-folder-projection-refresh-batch");
    let drums = root.join("drums");
    let kick = drums.join("kick.wav");
    let snare = drums.join("snare.wav");
    fs::create_dir_all(&drums).expect("create drums folder");
    fs::write(&kick, [0_u8; 8]).expect("write kick");
    fs::write(&snare, [0_u8; 8]).expect("write snare");

    let mut browser = FolderBrowserState::from_root(root.clone());
    browser.activate_folder(path_id(&drums));
    let _ = browser.selected_audio_files();
    assert!(
        browser.selected_audio_projection_cache_len_for_tests() > 0,
        "test should start with a warm selected-folder projection"
    );

    fs::write(&kick, [1_u8; 16]).expect("rewrite kick");
    fs::write(&snare, [2_u8; 32]).expect("rewrite snare");
    assert!(browser.refresh_file_paths(&[kick.clone(), snare.clone()]));

    assert_eq!(
        browser.selected_audio_projection_cache_len_for_tests(),
        0,
        "batched refresh should invalidate the projection cache once"
    );
    let sizes = browser
        .selected_audio_files()
        .iter()
        .map(|file| (file.name.clone(), file.size_bytes))
        .collect::<Vec<_>>();
    assert_eq!(
        sizes,
        vec![
            (String::from("kick.wav"), 16),
            (String::from("snare.wav"), 32)
        ]
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn selected_folder_refresh_applies_precomputed_file_entries_without_disk_reread() {
    let root = temp_source_root("wavecrate-gui-folder-precomputed-refresh");
    let drums = root.join("drums");
    let kick = drums.join("kick.wav");
    fs::create_dir_all(&drums).expect("create drums folder");
    fs::write(&kick, [0_u8; 8]).expect("write initial kick");

    let mut browser = FolderBrowserState::from_root(root.clone());
    browser.activate_folder(path_id(&drums));

    fs::write(&kick, [1_u8; 16]).expect("write worker-visible kick");
    let entries = refreshed_file_entries_for_paths(std::slice::from_ref(&kick), &root, &root);
    fs::write(&kick, [2_u8; 32]).expect("write later kick");

    let source_id = browser.selected_source_id().to_string();
    assert!(browser.refresh_file_entries(&source_id, &entries));

    let refreshed = browser
        .selected_audio_files()
        .into_iter()
        .find(|file| file.name == "kick.wav")
        .expect("refreshed kick row");
    assert_eq!(
        refreshed.size_bytes, 16,
        "UI-side refresh should merge the worker's precomputed row without rereading the file"
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn targeted_filesystem_refresh_prunes_deleted_cached_file() {
    let root = temp_source_root("wavecrate-gui-targeted-refresh-prune-file");
    let drums = root.join("drums");
    let stale_sample = drums.join("stale.wav");
    fs::create_dir_all(&drums).expect("create drums folder");
    fs::write(&stale_sample, [0_u8; 8]).expect("write stale sample");

    let mut browser = FolderBrowserState::from_root(root.clone());
    browser.activate_folder(path_id(&drums));
    fs::remove_file(&stale_sample).expect("remove stale sample");

    let source_id = browser.selected_source_id().to_string();
    assert!(browser.refresh_filesystem_paths(
        &source_id,
        &[std::path::PathBuf::from("drums").join("stale.wav")]
    ));

    assert!(
        browser.selected_audio_files().is_empty(),
        "targeted refresh should remove deleted samples from the cached folder listing"
    );
    let _ = fs::remove_dir_all(root);
}
#[test]
fn direct_folder_verify_patches_visible_folder_without_dropping_nested_cache() {
    let root = temp_source_root("wavecrate-gui-direct-folder-verify");
    let drums = root.join("drums");
    let kicks = drums.join("kicks");
    fs::create_dir_all(&kicks).expect("create nested folder");
    fs::write(kicks.join("deep.wav"), [0_u8; 8]).expect("write nested sample");

    let mut browser = FolderBrowserState::from_root(root.clone());
    assert!(browser.find_folder(&path_id(&kicks)).is_some());
    fs::write(root.join("new-root.wav"), [1_u8; 16]).expect("write new root sample");

    let request = browser
        .selected_folder_verify_request()
        .expect("selected root should be verifiable");
    let result = verify_direct_folder(request);

    assert!(browser.apply_direct_folder_verify_result(result));
    assert!(
        browser.find_folder(&path_id(&kicks)).is_some(),
        "direct verification should preserve cached nested child folders"
    );
    assert_eq!(
        browser
            .selected_audio_files()
            .iter()
            .map(|file| file.name.as_str())
            .collect::<Vec<_>>(),
        vec!["new-root.wav"]
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn selected_file_refresh_preserves_source_database_rating_metadata() {
    let root = temp_source_root("wavecrate-gui-refresh-preserves-rating");
    let drums = root.join("drums");
    let snare = drums.join("snare.wav");
    fs::create_dir_all(&drums).expect("create drums folder");
    fs::write(&snare, [0_u8; 8]).expect("write snare");

    let db = SourceDatabase::open_for_test_fixture_source_write(&root).expect("open source db");
    db.upsert_file(std::path::Path::new("drums/snare.wav"), 8, 1)
        .expect("upsert snare");
    db.set_tag(std::path::Path::new("drums/snare.wav"), Rating::new(2))
        .expect("set rating");
    db.set_locked(std::path::Path::new("drums/snare.wav"), true)
        .expect("set locked");

    let mut browser = FolderBrowserState::from_root(root.clone());
    browser.activate_folder(path_id(&drums));
    assert!(browser.refresh_file_path(&snare));

    let refreshed = browser
        .selected_audio_files()
        .into_iter()
        .find(|file| file.name == "snare.wav")
        .expect("refreshed snare row");
    assert_eq!(refreshed.rating, Rating::new(2));
    assert!(refreshed.rating_locked);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn direct_folder_verify_keeps_unchanged_selected_folder_visible() {
    let root = temp_source_root("wavecrate-gui-direct-folder-verify-unchanged");
    let drums = root.join("drums");
    let kicks = drums.join("kicks");
    fs::create_dir_all(&kicks).expect("create nested folder");
    fs::write(kicks.join("deep.wav"), [0_u8; 8]).expect("write nested sample");

    let mut browser = FolderBrowserState::from_root(root.clone());
    browser.activate_folder(path_id(&drums));
    let request = browser
        .selected_folder_verify_request()
        .expect("selected child folder should be verifiable");
    let result = verify_direct_folder(request);

    assert!(
        !browser.apply_direct_folder_verify_result(result),
        "unchanged folder verification should not mutate the tree"
    );
    assert!(
        browser.find_folder(&path_id(&drums)).is_some(),
        "unchanged selected folder should remain visible after click-triggered verification"
    );
    assert!(
        browser.find_folder(&path_id(&kicks)).is_some(),
        "unchanged selected folder should keep its cached child folders"
    );
    assert_eq!(browser.selected_folder_path(), Some(drums.clone()));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn direct_folder_verify_prunes_deleted_visible_file() {
    let root = temp_source_root("wavecrate-gui-direct-folder-verify-prune-file");
    let drums = root.join("drums");
    let stale_sample = drums.join("stale.wav");
    let keep_sample = drums.join("keep.wav");
    fs::create_dir_all(&drums).expect("create drums folder");
    fs::write(&stale_sample, [0_u8; 8]).expect("write stale sample");
    fs::write(&keep_sample, [1_u8; 8]).expect("write keep sample");

    let mut browser = FolderBrowserState::from_root(root.clone());
    browser.activate_folder(path_id(&drums));
    browser.select_file(path_id(&stale_sample));
    let request = browser
        .selected_folder_verify_request()
        .expect("selected folder should be verifiable");
    fs::remove_file(&stale_sample).expect("remove stale sample");

    let result = verify_direct_folder(request);
    assert!(browser.apply_direct_folder_verify_result(result));

    assert_eq!(
        browser
            .selected_audio_files()
            .iter()
            .map(|file| file.name.as_str())
            .collect::<Vec<_>>(),
        vec!["keep.wav"]
    );
    assert_eq!(
        browser.selected_file_paths(),
        Vec::<std::path::PathBuf>::new(),
        "selection should drop a file pruned by folder verification"
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn direct_folder_verify_prunes_deleted_selected_folder() {
    let root = temp_source_root("wavecrate-gui-direct-folder-verify-prune-folder");
    let drums = root.join("drums");
    fs::create_dir_all(&drums).expect("create drums folder");
    fs::write(drums.join("stale.wav"), [0_u8; 8]).expect("write stale sample");

    let mut browser = FolderBrowserState::from_root(root.clone());
    browser.activate_folder(path_id(&drums));
    let request = browser
        .selected_folder_verify_request()
        .expect("selected folder should be verifiable");
    fs::remove_dir_all(&drums).expect("remove selected folder");

    let result = verify_direct_folder(request);
    assert!(browser.apply_direct_folder_verify_result(result));

    assert!(browser.find_folder(&path_id(&drums)).is_none());
    assert_eq!(browser.selected_folder_path(), Some(root.clone()));
    assert!(browser.selected_audio_files().is_empty());
    let _ = fs::remove_dir_all(root);
}

#[test]
fn selected_source_refresh_prunes_deleted_cached_folders_on_finish() {
    let root = temp_source_root("wavecrate-gui-source-refresh-prune");
    let stale = root.join("stale");
    fs::create_dir_all(&stale).expect("create stale folder");
    fs::write(stale.join("old.wav"), [0_u8; 8]).expect("write old sample");

    let mut browser = FolderBrowserState::from_root(root.clone());
    assert!(browser.find_folder(&path_id(&stale)).is_some());
    fs::remove_dir_all(&stale).expect("remove stale folder");

    let request = browser
        .begin_selected_source_scan(91)
        .expect("selected source refresh should queue");
    let result = scan_source_with_progress(request, |_| {}, |_| {});
    assert!(browser.apply_scan_finished(result));

    assert!(browser.find_folder(&path_id(&stale)).is_none());
    let _ = fs::remove_dir_all(root);
}

#[test]
fn folder_tree_refresh_prunes_deleted_folders_and_preserves_files() {
    let root = temp_source_root("wavecrate-gui-folder-tree-refresh-prune");
    let stale = root.join("stale");
    let keep = root.join("keep");
    let keep_file = keep.join("keep.wav");
    fs::create_dir_all(&stale).expect("create stale folder");
    fs::create_dir_all(&keep).expect("create keep folder");
    fs::write(&keep_file, [0_u8; 8]).expect("write keep sample");

    let mut browser = FolderBrowserState::from_root(root.clone());
    assert!(browser.find_folder(&path_id(&stale)).is_some());
    assert!(browser.find_folder(&path_id(&keep)).is_some());
    fs::remove_dir_all(&stale).expect("remove stale folder");

    let result = refresh_folder_tree_only(FolderTreeRefreshRequest {
        source_id: browser.selected_source_id().to_string(),
        label: String::from("Assets"),
        root: root.clone(),
        database_root: root.clone(),
    });
    assert!(browser.apply_folder_tree_refresh_result(result));

    assert!(browser.find_folder(&path_id(&stale)).is_none());
    browser.activate_folder(path_id(&keep));
    assert_eq!(
        browser
            .selected_audio_files()
            .iter()
            .map(|file| file.name.as_str())
            .collect::<Vec<_>>(),
        vec!["keep.wav"],
        "folder-only refresh should preserve cached file rows for folders that still exist"
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn folder_tree_refresh_hydrates_cached_file_rating_and_lock_from_source_db() {
    let root = temp_source_root("wavecrate-gui-folder-tree-refresh-rating-hydration");
    let kick = root.join("kick.wav");
    fs::write(&kick, [0_u8; 8]).expect("write kick");
    let mut browser = FolderBrowserState::from_root(root.clone());
    let database = SourceDatabase::open_for_source_write(&root).expect("open source db");
    database
        .upsert_file(std::path::Path::new("kick.wav"), 8, 0)
        .expect("index kick");
    database
        .set_tag(std::path::Path::new("kick.wav"), Rating::KEEP_3)
        .expect("persist kick rating");
    database
        .set_locked(std::path::Path::new("kick.wav"), true)
        .expect("persist kick lock");

    let result = refresh_folder_tree_only(FolderTreeRefreshRequest {
        source_id: browser.selected_source_id().to_string(),
        label: String::from("Assets"),
        root: root.clone(),
        database_root: root.clone(),
    });
    assert!(browser.apply_folder_tree_refresh_result(result));

    let refreshed = browser
        .selected_audio_files()
        .into_iter()
        .find(|file| file.id == path_id(&kick))
        .expect("hydrated kick row");
    assert_eq!(refreshed.rating, Rating::KEEP_3);
    assert!(refreshed.rating_locked);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn folder_tree_refresh_missing_db_fails_hydration_without_creating_schema() {
    let root = temp_source_root("wavecrate-gui-folder-tree-refresh-missing-db");
    let kick = root.join("kick.wav");
    fs::write(&kick, [0_u8; 8]).expect("write kick");
    let mut browser = FolderBrowserState::from_root(root.clone());
    assert!(browser.set_file_rating_state(&kick, Rating::KEEP_3, true));
    let db_path = root.join(wavecrate::sample_sources::DB_FILE_NAME);
    assert!(
        !db_path.exists(),
        "test setup should have no source database"
    );

    let result = refresh_folder_tree_only(FolderTreeRefreshRequest {
        source_id: browser.selected_source_id().to_string(),
        label: String::from("Assets"),
        root: root.clone(),
        database_root: root.clone(),
    });
    assert!(matches!(
        result.rating_hydration,
        RatingHydrationStatus::Failed { .. }
    ));
    assert!(
        !db_path.exists(),
        "background hydration must not create a database"
    );
    assert!(!browser.apply_folder_tree_refresh_result(result));
    let cached = browser
        .selected_audio_files()
        .into_iter()
        .find(|file| file.id == path_id(&kick))
        .expect("cached kick row");
    assert_eq!(cached.rating, Rating::KEEP_3);
    assert!(cached.rating_locked);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn folder_tree_refresh_adds_new_empty_folder() {
    let root = temp_source_root("wavecrate-gui-folder-tree-refresh-add-empty");
    let added = root.join("added-empty");

    let mut browser = FolderBrowserState::from_root(root.clone());
    assert!(browser.find_folder(&path_id(&added)).is_none());
    fs::create_dir_all(&added).expect("create added folder");

    let result = refresh_folder_tree_only(FolderTreeRefreshRequest {
        source_id: browser.selected_source_id().to_string(),
        label: String::from("Assets"),
        root: root.clone(),
        database_root: root.clone(),
    });
    assert!(browser.apply_folder_tree_refresh_result(result));

    let folder = browser
        .find_folder(&path_id(&added))
        .expect("new empty folder should be visible");
    assert!(
        !folder.contains_audio(),
        "new folder should be marked empty by the existing contains-audio styling contract"
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn folder_tree_refresh_honors_excluded_hidden_directory_policy() {
    let root = temp_source_root("wavecrate-gui-folder-tree-refresh-hidden-policy");
    let hidden = root.join(".hidden");
    fs::create_dir_all(&hidden).expect("create hidden folder");
    fs::write(hidden.join("kick.wav"), [0_u8; 8]).expect("write hidden sample");
    SourceDatabase::open_for_source_write(&root)
        .expect("open source db")
        .set_source_traversal_policy(
            wavecrate::sample_sources::SourceTraversalPolicy::exclude_hidden_directories(),
        )
        .expect("set hidden directory policy");

    let result = refresh_folder_tree_only(FolderTreeRefreshRequest {
        source_id: String::from("source"),
        label: String::from("Assets"),
        root: root.clone(),
        database_root: root.clone(),
    });

    assert!(result.folder.find(&path_id(&hidden)).is_none());
    let _ = fs::remove_dir_all(root);
}

#[test]
fn folder_tree_refresh_reconciles_deleted_selected_folder() {
    let root = temp_source_root("wavecrate-gui-folder-tree-refresh-selected");
    let stale = root.join("stale");
    fs::create_dir_all(&stale).expect("create stale folder");

    let mut browser = FolderBrowserState::from_root(root.clone());
    browser.activate_folder(path_id(&stale));
    fs::remove_dir_all(&stale).expect("remove stale folder");

    let result = refresh_folder_tree_only(FolderTreeRefreshRequest {
        source_id: browser.selected_source_id().to_string(),
        label: String::from("Assets"),
        root: root.clone(),
        database_root: root.clone(),
    });
    assert!(browser.apply_folder_tree_refresh_result(result));

    assert!(browser.find_folder(&path_id(&stale)).is_none());
    assert_eq!(browser.selected_folder_path(), Some(root.clone()));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn folder_tree_refresh_ignores_stale_source_result() {
    let first_root = temp_source_root("wavecrate-gui-folder-tree-refresh-first");
    let second_root = temp_source_root("wavecrate-gui-folder-tree-refresh-second");
    let stale_first = first_root.join("stale-first");
    let second_child = second_root.join("second-child");
    fs::create_dir_all(&stale_first).expect("create first child");
    fs::create_dir_all(&second_child).expect("create second child");
    let first_source = wavecrate::sample_sources::SampleSource::new_with_id(
        wavecrate::sample_sources::SourceId::from_string("first"),
        first_root.clone(),
    );
    let second_source = wavecrate::sample_sources::SampleSource::new_with_id(
        wavecrate::sample_sources::SourceId::from_string("second"),
        second_root.clone(),
    );
    let mut browser =
        FolderBrowserState::from_sample_sources(&[first_source.clone(), second_source.clone()]);
    let second_scan = browser
        .begin_source_scan(String::from("second"), 42)
        .expect("second source scan can queue");
    let second_result = scan_source_with_progress(second_scan, |_| {}, |_| {});
    assert!(browser.apply_scan_finished(second_result));
    assert!(browser.select_source_without_scan(String::from("second")));
    fs::remove_dir_all(&stale_first).expect("remove first child");

    let stale_result = refresh_folder_tree_only(FolderTreeRefreshRequest {
        source_id: String::from("first"),
        label: String::from("First"),
        root: first_root.clone(),
        database_root: first_root.clone(),
    });
    assert!(!browser.apply_folder_tree_refresh_result(stale_result));

    assert_eq!(browser.selected_source_id(), "second");
    assert!(browser.find_folder(&path_id(&second_child)).is_some());
    let _ = fs::remove_dir_all(first_root);
    let _ = fs::remove_dir_all(second_root);
}

#[test]
fn selected_source_refresh_prunes_deleted_cached_files_on_finish() {
    let root = temp_source_root("wavecrate-gui-source-refresh-prune-file");
    let drums = root.join("drums");
    let stale_sample = drums.join("stale.wav");
    fs::create_dir_all(&drums).expect("create drums folder");
    fs::write(&stale_sample, [0_u8; 8]).expect("write stale sample");

    let mut browser = FolderBrowserState::from_root(root.clone());
    browser.activate_folder(path_id(&drums));
    assert_eq!(
        browser
            .selected_audio_files()
            .iter()
            .map(|file| file.name.as_str())
            .collect::<Vec<_>>(),
        vec!["stale.wav"]
    );
    fs::remove_file(&stale_sample).expect("remove stale sample");

    let request = browser
        .begin_selected_source_scan(94)
        .expect("selected source refresh should queue");
    let result = scan_source_with_progress(request, |_| {}, |_| {});
    assert!(browser.apply_scan_finished(result));

    browser.activate_folder(path_id(&drums));
    assert!(
        browser.selected_audio_files().is_empty(),
        "refresh should remove deleted samples from the selected folder listing"
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn reselecting_loaded_source_reconciles_disk_without_clearing_cached_tree() {
    let root = temp_source_root("wavecrate-gui-source-reselect-reconcile");
    let drums = root.join("drums");
    let stale_sample = drums.join("stale.wav");
    let fresh_sample = drums.join("fresh.wav");
    fs::create_dir_all(&drums).expect("create drums folder");
    fs::write(&stale_sample, [0_u8; 8]).expect("write stale sample");

    let mut browser = FolderBrowserState::from_root(root.clone());
    browser.activate_folder(path_id(&drums));
    let source_id = browser.selected_source_id().to_string();
    fs::remove_file(&stale_sample).expect("remove stale sample");
    fs::write(&fresh_sample, [0_u8; 8]).expect("write fresh sample");

    let request = browser
        .begin_select_source(source_id.clone(), 95)
        .expect("reselecting a loaded source should queue reconciliation");
    assert_eq!(browser.selected_folder_path(), Some(drums.clone()));
    assert_eq!(
        browser
            .selected_audio_files()
            .iter()
            .map(|file| file.name.as_str())
            .collect::<Vec<_>>(),
        vec!["stale.wav"],
        "the cached listing should remain visible while reconciliation runs"
    );
    assert!(
        browser.begin_select_source(source_id.clone(), 96).is_none(),
        "repeated selection must not queue a duplicate scan"
    );

    let mut discoveries = Vec::new();
    let result = scan_source_with_progress(request, |_| {}, |event| discoveries.push(event));
    assert!(matches!(
        discoveries.first().map(|event| &event.item),
        Some(FolderScanItem::ResetFolder)
    ));
    assert!(
        !browser.apply_scan_discovered_batch(FolderScanDiscoveryBatch {
            task_id: 95,
            source_id: source_id.clone(),
            committed_revision: discoveries
                .first()
                .and_then(|event| event.committed_revision),
            lifecycle_generation: None,
            sequence: 0,
            events: discoveries,
        }),
        "background discoveries must not mutate a fully loaded selected tree"
    );
    assert_eq!(
        browser
            .selected_audio_files()
            .iter()
            .map(|file| file.name.as_str())
            .collect::<Vec<_>>(),
        vec!["stale.wav"],
        "the cached listing should remain visible after discovery batches arrive"
    );
    assert!(browser.apply_scan_finished(result));
    assert_eq!(
        browser.selected_folder_path(),
        Some(drums.clone()),
        "reconciliation should preserve a selected folder that still exists"
    );
    assert_eq!(
        browser
            .selected_audio_files()
            .iter()
            .map(|file| file.name.as_str())
            .collect::<Vec<_>>(),
        vec!["fresh.wav"]
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn completed_scan_discovery_preserves_cached_root_until_finish() {
    let root = temp_source_root("wavecrate-gui-source-discovery-prune-root");
    let keep = root.join("keep");
    let stale = root.join("stale");
    fs::create_dir_all(&keep).expect("create keep folder");
    fs::create_dir_all(&stale).expect("create stale folder");

    let mut browser = FolderBrowserState::from_root(root.clone());
    assert!(browser.find_folder(&path_id(&stale)).is_some());
    fs::remove_dir_all(&stale).expect("remove stale folder");

    let request = browser
        .begin_selected_source_scan(92)
        .expect("selected source refresh should queue");
    let mut discovery_events = Vec::new();
    let result = scan_source_with_progress(request, |_| {}, |event| discovery_events.push(event));

    for event in discovery_events {
        browser.apply_scan_discovered(event);
    }
    assert!(browser.find_folder(&path_id(&keep)).is_some());
    assert!(
        browser.find_folder(&path_id(&stale)).is_some(),
        "root discovery should preserve cached children until the scan finishes"
    );

    assert!(browser.apply_scan_finished(result));
    assert!(browser.find_folder(&path_id(&stale)).is_none());
    let _ = fs::remove_dir_all(root);
}
#[test]
fn completed_scan_discovery_preserves_cached_nested_child_until_finish() {
    let root = temp_source_root("wavecrate-gui-source-discovery-prune-nested");
    let parent = root.join("drums");
    let keep = parent.join("keep");
    let stale = parent.join("stale");
    fs::create_dir_all(&keep).expect("create keep folder");
    fs::create_dir_all(&stale).expect("create stale folder");

    let mut browser = FolderBrowserState::from_root(root.clone());
    assert!(browser.find_folder(&path_id(&stale)).is_some());
    fs::remove_dir_all(&stale).expect("remove stale folder");

    let request = browser
        .begin_selected_source_scan(93)
        .expect("selected source refresh should queue");
    let mut discovery_events = Vec::new();
    let result = scan_source_with_progress(request, |_| {}, |event| discovery_events.push(event));

    for event in discovery_events {
        browser.apply_scan_discovered(event);
    }
    assert!(browser.find_folder(&path_id(&keep)).is_some());
    assert!(
        browser.find_folder(&path_id(&stale)).is_some(),
        "nested-folder discovery should preserve cached children until the scan finishes"
    );

    assert!(browser.apply_scan_finished(result));
    assert!(browser.find_folder(&path_id(&stale)).is_none());
    let _ = fs::remove_dir_all(root);
}
