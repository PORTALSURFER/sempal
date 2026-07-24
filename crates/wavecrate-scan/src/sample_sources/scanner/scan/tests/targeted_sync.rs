use super::*;
use crate::sample_sources::scanner::scan_fs::force_directory_identity;
use crate::sample_sources::scanner::scan_fs::force_directory_read_failure;
use crate::sample_sources::scanner::{DirectoryRepeatKind, SourceTreeDiagnostic};
use crate::sample_sources::scanner::{sync_paths, sync_paths_with_progress};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};

#[test]
fn targeted_sync_updates_only_requested_file() {
    let dir = tempdir().unwrap();
    std::fs::write(dir.path().join("one.wav"), b"one").unwrap();
    std::fs::write(dir.path().join("two.wav"), b"two").unwrap();
    let db = SourceDatabase::open_for_scan(dir.path()).unwrap();
    scan_once(&db).unwrap();

    std::fs::write(dir.path().join("one.wav"), b"changed").unwrap();
    let stats = sync_paths(&db, &[PathBuf::from("one.wav")]).unwrap();

    assert_eq!(stats.total_files, 1);
    assert_eq!(stats.updated, 1);
    assert_eq!(stats.content_changed, 1);
    assert_eq!(db.list_files().unwrap().len(), 2);
    assert_eq!(stats.committed_delta.changed.len(), 1);
    assert!(stats.committed_delta.revision > 0);
}

#[test]
fn targeted_sync_does_not_reconcile_wildcard_sibling_subtrees() {
    let dir = tempdir().unwrap();
    let target = dir.path().join("drum_kits%_!");
    let sibling = dir.path().join("drumXkitsX_Y!");
    std::fs::create_dir_all(&target).unwrap();
    std::fs::create_dir_all(&sibling).unwrap();
    std::fs::write(target.join("removed.wav"), b"removed").unwrap();
    std::fs::write(sibling.join("unrelated.wav"), b"unrelated").unwrap();

    let db = SourceDatabase::open_for_scan(dir.path()).unwrap();
    scan_once(&db).unwrap();
    std::fs::remove_file(target.join("removed.wav")).unwrap();

    sync_paths(&db, &[PathBuf::from("drum_kits%_!")]).unwrap();

    let rows = db.list_files().unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(
        rows[0].relative_path,
        Path::new("drumXkitsX_Y!/unrelated.wav")
    );
    assert!(!rows[0].missing);
}

#[cfg(unix)]
#[test]
fn targeted_sync_rejects_a_root_swap_before_commit() {
    let parent = tempdir().unwrap();
    let root = parent.path().join("source");
    std::fs::create_dir(&root).unwrap();
    let path = root.join("one.wav");
    std::fs::write(&path, b"old").unwrap();
    let db = SourceDatabase::open_for_scan(&root).unwrap();
    scan_once(&db).unwrap();
    let before = db.entry_for_path(Path::new("one.wav")).unwrap().unwrap();

    let mut swapped = false;
    let result = sync_paths_with_progress(&db, &[PathBuf::from("one.wav")], None, &mut |_, _| {
        if !swapped {
            swapped = true;
            let old_root = parent.path().join("old-source");
            std::fs::rename(&root, &old_root).unwrap();
            std::fs::create_dir(&root).unwrap();
            std::fs::write(root.join("one.wav"), b"new").unwrap();
        }
    });

    assert!(matches!(result, Err(ScanError::StaleRootGeneration { .. })));
    let after = db.entry_for_path(Path::new("one.wav")).unwrap().unwrap();
    assert_eq!(after.file_size, before.file_size);
    assert_eq!(after.modified_ns, before.modified_ns);
    assert_eq!(after.content_hash, before.content_hash);
}

#[test]
fn targeted_sync_detects_same_size_edit_with_restored_mtime() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("same.wav");
    std::fs::write(&path, b"one").unwrap();
    let original_modified = std::fs::metadata(&path).unwrap().modified().unwrap();
    let db = SourceDatabase::open_for_scan(dir.path()).unwrap();
    scan_once(&db).unwrap();
    let original_hash = db
        .entry_for_path(Path::new("same.wav"))
        .unwrap()
        .unwrap()
        .content_hash
        .unwrap();

    std::fs::write(&path, b"two").unwrap();
    let file = std::fs::OpenOptions::new().write(true).open(&path).unwrap();
    file.set_times(std::fs::FileTimes::new().set_modified(original_modified))
        .unwrap();
    let stats = sync_paths(&db, &[PathBuf::from("same.wav")]).unwrap();
    let current_hash = db
        .entry_for_path(Path::new("same.wav"))
        .unwrap()
        .unwrap()
        .content_hash
        .unwrap();

    assert_ne!(current_hash, original_hash);
    assert_eq!(stats.content_changed, 1);
    assert_eq!(stats.committed_delta.changed.len(), 1);
    assert!(stats.committed_delta.created.is_empty());
}

#[test]
fn targeted_sync_exactly_hashes_an_existing_large_file_edit() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("large.wav");
    std::fs::write(&path, vec![1_u8; 9 * 1024 * 1024]).unwrap();
    let original_modified = std::fs::metadata(&path).unwrap().modified().unwrap();
    let db = SourceDatabase::open_for_scan(dir.path()).unwrap();
    scan_once(&db).unwrap();
    complete_pending_deep_hash_for_path(&db, Path::new("large.wav"), None).unwrap();
    let original_hash = db
        .entry_for_path(Path::new("large.wav"))
        .unwrap()
        .unwrap()
        .content_hash
        .unwrap();

    std::fs::write(&path, vec![2_u8; 9 * 1024 * 1024]).unwrap();
    let file = std::fs::OpenOptions::new().write(true).open(&path).unwrap();
    file.set_times(std::fs::FileTimes::new().set_modified(original_modified))
        .unwrap();
    let stats = sync_paths(&db, &[PathBuf::from("large.wav")]).unwrap();
    let current_hash = db
        .entry_for_path(Path::new("large.wav"))
        .unwrap()
        .unwrap()
        .content_hash
        .unwrap();

    assert_ne!(current_hash, original_hash);
    assert_eq!(stats.committed_delta.changed.len(), 1);
    assert_eq!(stats.hashes_pending, 0);
}

#[test]
fn targeted_sync_hides_confirmed_missing_file() {
    let dir = tempdir().unwrap();
    std::fs::write(dir.path().join("one.wav"), b"one").unwrap();
    std::fs::write(dir.path().join("two.wav"), b"two").unwrap();
    let db = SourceDatabase::open_for_scan(dir.path()).unwrap();
    scan_once(&db).unwrap();

    std::fs::remove_file(dir.path().join("one.wav")).unwrap();
    let stats = sync_paths(&db, &[PathBuf::from("one.wav")]).unwrap();

    assert_eq!(stats.total_files, 0);
    assert_eq!(stats.missing, 1);
    let rows = db.list_files().unwrap();
    assert_eq!(rows.len(), 1);
    assert!(db.list_source_index_entries().unwrap().is_empty());
    assert_eq!(rows[0].relative_path, Path::new("two.wav"));
    assert_eq!(stats.committed_delta.deleted.len(), 1);
}

#[test]
fn targeted_sync_prunes_removed_folder_prefix() {
    let dir = tempdir().unwrap();
    let drums = dir.path().join("drums");
    std::fs::create_dir_all(&drums).unwrap();
    std::fs::write(drums.join("one.wav"), b"one").unwrap();
    std::fs::write(drums.join("two.wav"), b"two").unwrap();
    std::fs::write(dir.path().join("keep.wav"), b"keep").unwrap();
    let db = SourceDatabase::open_for_scan(dir.path()).unwrap();
    scan_once(&db).unwrap();

    std::fs::remove_dir_all(&drums).unwrap();
    let stats = sync_paths(&db, &[PathBuf::from("drums")]).unwrap();

    assert_eq!(stats.total_files, 0);
    assert_eq!(stats.missing, 2);
    let rows = db.list_files().unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].relative_path, Path::new("keep.wav"));
}

#[test]
fn targeted_sync_reconciles_hidden_directory_policy_changes() {
    let dir = tempdir().unwrap();
    let hidden = dir.path().join(".hidden");
    std::fs::create_dir_all(&hidden).unwrap();
    std::fs::write(hidden.join("kick.wav"), b"kick").unwrap();
    let db = SourceDatabase::open_for_scan(dir.path()).unwrap();

    assert_eq!(scan_once(&db).unwrap().added, 1);
    db.set_source_traversal_policy(
        wavecrate_library::sample_sources::SourceTraversalPolicy::exclude_hidden_directories(),
    )
    .unwrap();
    assert_eq!(
        sync_paths(&db, &[PathBuf::from(".hidden")])
            .unwrap()
            .missing,
        1
    );
    assert!(db.list_files().unwrap().is_empty());

    db.set_source_traversal_policy(
        wavecrate_library::sample_sources::SourceTraversalPolicy::include_hidden_directories(),
    )
    .unwrap();
    assert_eq!(
        sync_paths(&db, &[PathBuf::from(".hidden")]).unwrap().added,
        1
    );
    assert_eq!(db.list_files().unwrap().len(), 1);

    db.set_source_traversal_policy(
        wavecrate_library::sample_sources::SourceTraversalPolicy::exclude_hidden_directories(),
    )
    .unwrap();
    assert_eq!(
        sync_paths(&db, &[PathBuf::from(".hidden")])
            .unwrap()
            .missing,
        1
    );
    assert!(db.list_files().unwrap().is_empty());
}

#[test]
fn targeted_sync_preserves_an_unreadable_directory_until_recovery() {
    let dir = tempdir().unwrap();
    let protected = dir.path().join("protected");
    std::fs::create_dir(&protected).unwrap();
    std::fs::write(protected.join("kick.wav"), b"kick").unwrap();
    let db = SourceDatabase::open_for_scan(dir.path()).unwrap();
    scan_once(&db).unwrap();
    db.set_tag(Path::new("protected/kick.wav"), Rating::KEEP_1)
        .unwrap();

    std::fs::remove_file(protected.join("kick.wav")).unwrap();
    let failure = force_directory_read_failure(&protected);
    let result = sync_paths(&db, &[PathBuf::from("protected")]);
    let ScanError::Incomplete { committed, error } = result.unwrap_err() else {
        panic!("targeted unreadable directory must be retryable");
    };
    assert!(error.contains("retry required"));
    assert!(committed.committed_delta.deleted.is_empty());
    assert_eq!(
        db.entry_for_path(Path::new("protected/kick.wav"))
            .unwrap()
            .unwrap()
            .tag,
        Rating::KEEP_1
    );

    drop(failure);
    let recovered = sync_paths(&db, &[PathBuf::from("protected")]).unwrap();
    assert_eq!(recovered.missing, 1);
    assert!(
        db.entry_for_path(Path::new("protected/kick.wav"))
            .unwrap()
            .is_none()
    );
}

#[test]
fn targeted_sync_normalizes_dot_prefixed_uncertainty_boundaries() {
    let dir = tempdir().unwrap();
    let protected = dir.path().join("protected");
    std::fs::create_dir(&protected).unwrap();
    std::fs::write(protected.join("kick.wav"), b"kick").unwrap();
    let db = SourceDatabase::open_for_scan(dir.path()).unwrap();
    scan_once(&db).unwrap();

    std::fs::remove_file(protected.join("kick.wav")).unwrap();
    let failure = force_directory_read_failure(&protected);
    let target = PathBuf::from("./protected");
    let result = sync_paths(&db, &[target.clone()]);
    let ScanError::Incomplete { committed, .. } = result.unwrap_err() else {
        panic!("dot-prefixed uncertain target must be retryable");
    };
    assert!(committed.committed_delta.deleted.is_empty());
    assert!(
        db.entry_for_path(Path::new("protected/kick.wav"))
            .unwrap()
            .is_some()
    );

    drop(failure);
    assert_eq!(sync_paths(&db, &[target]).unwrap().missing, 1);
}

#[test]
fn targeted_sync_adds_new_file_inside_requested_folder() {
    let dir = tempdir().unwrap();
    let drums = dir.path().join("drums");
    std::fs::create_dir_all(&drums).unwrap();
    let db = SourceDatabase::open_for_scan(dir.path()).unwrap();
    scan_once(&db).unwrap();

    std::fs::write(drums.join("kick.wav"), b"kick").unwrap();
    let stats = sync_paths(&db, &[PathBuf::from("drums")]).unwrap();

    assert_eq!(stats.total_files, 1);
    assert_eq!(stats.added, 1);
    let rows = db.list_files().unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].relative_path, Path::new("drums/kick.wav"));
    assert_eq!(stats.committed_delta.created.len(), 1);
}

#[test]
fn targeted_sync_does_not_claim_unrelated_missing_rename_source() {
    let dir = tempdir().unwrap();
    let unrelated = dir.path().join("unrelated.wav");
    std::fs::write(&unrelated, b"same").unwrap();
    let db = SourceDatabase::open_for_scan(dir.path()).unwrap();
    scan_once(&db).unwrap();
    db.set_tag(Path::new("unrelated.wav"), Rating::KEEP_1)
        .unwrap();

    std::fs::remove_file(&unrelated).unwrap();
    std::fs::write(dir.path().join("requested.wav"), b"same").unwrap();
    let stats = sync_paths(&db, &[PathBuf::from("requested.wav")]).unwrap();

    assert_eq!(stats.renames_reconciled, 0);
    assert_eq!(stats.added, 1);
    assert!(
        db.entry_for_path(Path::new("unrelated.wav"))
            .unwrap()
            .is_some()
    );
    assert_eq!(
        db.entry_for_path(Path::new("requested.wav"))
            .unwrap()
            .unwrap()
            .tag,
        Rating::NEUTRAL
    );
}

#[test]
fn targeted_sync_ignores_appledouble_sidecars() {
    let dir = tempdir().unwrap();
    let drums = dir.path().join("drums");
    std::fs::create_dir_all(&drums).unwrap();
    let db = SourceDatabase::open_for_scan(dir.path()).unwrap();
    scan_once(&db).unwrap();

    std::fs::write(drums.join("kick.wav"), b"kick").unwrap();
    std::fs::write(drums.join("._kick.wav"), b"sidecar").unwrap();
    let stats = sync_paths(&db, &[PathBuf::from("drums")]).unwrap();

    assert_eq!(stats.total_files, 1);
    assert_eq!(stats.added, 1);
    let rows = db.list_files().unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].relative_path, Path::new("drums/kick.wav"));
    assert!(db.list_source_index_entries().unwrap().is_empty());
}

#[test]
fn targeted_sync_cancels_after_a_committed_batch_and_resumes_safely() {
    let dir = tempdir().unwrap();
    let drums = dir.path().join("drums");
    std::fs::create_dir_all(&drums).unwrap();
    for index in 0..70 {
        std::fs::write(drums.join(format!("sample-{index:03}.wav")), b"x").unwrap();
    }
    let db = SourceDatabase::open_for_scan(dir.path()).unwrap();
    let cancel = AtomicBool::new(false);
    let targets = [PathBuf::from("drums")];

    let result = sync_paths_with_progress(&db, &targets, Some(&cancel), &mut |count, _| {
        if count == 65 {
            cancel.store(true, Ordering::Relaxed);
        }
    });

    let ScanError::Incomplete { committed, error } = result.unwrap_err() else {
        panic!("targeted cancellation must return the committed checkpoint outcome");
    };
    let partial = *committed;
    assert_eq!(partial.committed_delta.created.len(), 64);
    assert!(partial.committed_delta.revision > 0);
    assert_eq!(error, "Scan canceled");
    assert_eq!(db.count_files().unwrap(), 64);

    cancel.store(false, Ordering::Relaxed);
    let resumed = sync_paths_with_progress(&db, &targets, Some(&cancel), &mut |_, _| {})
        .expect("targeted sync must resume from the committed checkpoint");
    assert_eq!(resumed.total_files, 70);
    assert_eq!(db.count_files().unwrap(), 70);
}

#[test]
fn targeted_sync_reconciles_each_directory_identity_once() {
    let source = tempdir().unwrap();
    let parent = source.path().join("parent");
    let first = parent.join("first");
    let second = parent.join("second");
    std::fs::create_dir_all(&first).unwrap();
    std::fs::create_dir_all(&second).unwrap();
    std::fs::write(first.join("first.wav"), b"first").unwrap();
    std::fs::write(second.join("second.wav"), b"second").unwrap();

    let _first_identity = force_directory_identity(&first, Some("repeated-target"));
    let _second_identity = force_directory_identity(&second, Some("repeated-target"));
    let db = SourceDatabase::open_for_scan(source.path()).unwrap();
    let ScanError::Incomplete { committed, .. } =
        sync_paths(&db, &[PathBuf::from("parent")]).unwrap_err()
    else {
        panic!("an injected repeated subtree must return the committed partial result");
    };
    let stats = *committed;

    assert_eq!(stats.total_files, 1);
    assert!(
        stats
            .traversal_diagnostics
            .iter()
            .any(|diagnostic| matches!(
                diagnostic,
                SourceTreeDiagnostic::RepeatedDirectory {
                    kind: DirectoryRepeatKind::RepeatedTarget,
                    ..
                }
            ))
    );
}

#[test]
fn targeted_sync_processes_sibling_files_sharing_a_parent_target() {
    let source = tempdir().unwrap();
    let nested = source.path().join("nested");
    std::fs::create_dir(&nested).unwrap();
    std::fs::write(nested.join("first.wav"), b"first").unwrap();
    std::fs::write(nested.join("second.wav"), b"second").unwrap();
    let db = SourceDatabase::open_for_scan(source.path()).unwrap();

    let stats = sync_paths(
        &db,
        &[
            PathBuf::from("nested/first.wav"),
            PathBuf::from("nested/second.wav"),
        ],
    )
    .unwrap();

    assert_eq!(stats.total_files, 2);
    assert_eq!(db.count_files().unwrap(), 2);
    assert!(stats.traversal_diagnostics.is_empty());
}

#[test]
fn targeted_sync_rejects_a_file_below_a_repeated_ancestor() {
    let source = tempdir().unwrap();
    let alias = source.path().join("alias");
    std::fs::create_dir(&alias).unwrap();
    let target = alias.join("keep.wav");
    std::fs::write(&target, b"keep").unwrap();
    let db = SourceDatabase::open_for_scan(source.path()).unwrap();
    scan_once(&db).unwrap();

    let _root_identity = force_directory_identity(source.path(), Some("root-identity"));
    let _alias_identity = force_directory_identity(&alias, Some("root-identity"));
    let ScanError::Incomplete { .. } =
        sync_paths(&db, &[PathBuf::from("alias/keep.wav")]).unwrap_err()
    else {
        panic!("a repeated targeted ancestor must return an incomplete scan");
    };

    assert_eq!(db.count_files().unwrap(), 1);
}

#[cfg(unix)]
#[test]
fn targeted_sync_skips_directory_symlink_targets() {
    use std::os::unix::fs as unix_fs;

    let source = tempdir().unwrap();
    let outside = tempdir().unwrap();
    std::fs::create_dir_all(source.path().join("nested")).unwrap();
    std::fs::write(source.path().join("nested/keep.wav"), b"keep").unwrap();
    std::fs::write(outside.path().join("outside.wav"), b"outside").unwrap();
    unix_fs::symlink(outside.path(), source.path().join("outside-link")).unwrap();
    unix_fs::symlink(source.path(), source.path().join("ancestor-loop")).unwrap();

    let db = SourceDatabase::open_for_scan(source.path()).unwrap();
    scan_once(&db).unwrap();
    let stats = sync_paths(
        &db,
        &[
            PathBuf::from("outside-link"),
            PathBuf::from("outside-link/outside.wav"),
            PathBuf::from("ancestor-loop"),
            PathBuf::from("ancestor-loop/nested/keep.wav"),
        ],
    )
    .unwrap();

    assert_eq!(stats.total_files, 0);
    let rows = db.list_files().unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].relative_path, Path::new("nested/keep.wav"));
}

#[cfg(unix)]
#[test]
fn targeted_sync_retires_a_file_replaced_by_a_symlink() {
    use std::os::unix::fs as unix_fs;

    let source = tempdir().unwrap();
    let outside = tempdir().unwrap();
    let tracked = source.path().join("tracked.wav");
    std::fs::write(&tracked, b"tracked").unwrap();
    std::fs::write(outside.path().join("outside.wav"), b"outside").unwrap();

    let db = SourceDatabase::open_for_scan(source.path()).unwrap();
    scan_once(&db).unwrap();
    std::fs::remove_file(&tracked).unwrap();
    unix_fs::symlink(outside.path().join("outside.wav"), &tracked).unwrap();

    let stats = sync_paths(&db, &[PathBuf::from("tracked.wav")]).unwrap();

    assert_eq!(stats.total_files, 0);
    assert_eq!(stats.missing, 1);
    assert!(
        db.entry_for_path(Path::new("tracked.wav"))
            .unwrap()
            .is_none()
    );
    assert_eq!(stats.committed_delta.deleted.len(), 1);
}
