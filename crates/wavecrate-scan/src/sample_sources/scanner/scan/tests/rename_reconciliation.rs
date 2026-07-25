use super::*;
use crate::sample_sources::db::{SampleCollection, SampleSoundType};
use crate::sync_paths;
use std::collections::HashSet;
use std::path::PathBuf;

fn rename_candidates(paths: &[&str]) -> HashSet<PathBuf> {
    paths.iter().map(PathBuf::from).collect()
}

#[test]
fn scan_detects_rename_and_preserves_tag() {
    let dir = tempdir().unwrap();
    let first_path = dir.path().join("one.wav");
    let second_path = dir.path().join("two.wav");
    std::fs::write(&first_path, b"one").unwrap();

    let db = SourceDatabase::open_for_scan(dir.path()).unwrap();
    scan_once(&db).unwrap();
    db.set_tag(Path::new("one.wav"), Rating::KEEP_1).unwrap();
    db.set_sound_type(Path::new("one.wav"), Some(SampleSoundType::Kick))
        .unwrap();
    db.set_user_tag(Path::new("one.wav"), Some("Vintage FX"))
        .unwrap();
    db.assign_tag_to_path(Path::new("one.wav"), "Analog Kick")
        .unwrap();
    let historical_curation = 1_650_000_123;
    let expected_collections = [
        SampleCollection::new(1).unwrap(),
        SampleCollection::new(4).unwrap(),
    ];
    let mut batch = db.write_batch().unwrap();
    for collection in expected_collections {
        batch
            .add_collection(Path::new("one.wav"), collection)
            .unwrap();
    }
    batch
        .set_last_curated_at(Path::new("one.wav"), historical_curation)
        .unwrap();
    batch.commit().unwrap();
    insert_analysis_artifacts(dir.path(), "source::one.wav", "one.wav");

    std::fs::rename(&first_path, &second_path).unwrap();
    let stats = scan_once(&db).unwrap();

    assert_eq!(stats.missing, 0);
    assert_eq!(stats.added, 0);
    assert_eq!(stats.content_changed, 0);
    assert_eq!(stats.updated, 1);

    let rows = db.list_files().unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].relative_path, Path::new("two.wav"));
    assert_eq!(rows[0].tag, Rating::KEEP_1);
    assert_eq!(rows[0].sound_type, Some(SampleSoundType::Kick));
    assert_eq!(rows[0].user_tag.as_deref(), Some("Vintage FX"));
    assert_eq!(rows[0].normal_tags, vec!["Analog Kick"]);
    assert_eq!(rows[0].last_curated_at, Some(historical_curation));
    assert_eq!(
        db.collections_for_path(Path::new("two.wav")).unwrap(),
        expected_collections
    );
    assert!(!rows[0].missing);
    assert_eq!(
        sample_id_count(dir.path(), "features", "source::one.wav"),
        0
    );
    assert_eq!(
        sample_id_count(dir.path(), "features", "source::two.wav"),
        1
    );
    assert_eq!(
        analysis_job_relative_path(dir.path(), "source::two.wav"),
        "two.wav"
    );
}

#[test]
fn scan_detected_rename_preserves_unset_curation_timestamp() {
    let dir = tempdir().unwrap();
    let old_path = dir.path().join("old.wav");
    let new_path = dir.path().join("new.wav");
    std::fs::write(&old_path, b"same sample").unwrap();

    let db = SourceDatabase::open_for_scan(dir.path()).unwrap();
    scan_once(&db).unwrap();
    db.set_tag(Path::new("old.wav"), Rating::KEEP_1).unwrap();
    db.clear_last_curated_at(Path::new("old.wav")).unwrap();

    std::fs::rename(old_path, new_path).unwrap();
    let stats = scan_once(&db).unwrap();

    assert_eq!(stats.renames_reconciled, 1);
    assert_eq!(
        db.entry_for_path(Path::new("new.wav"))
            .unwrap()
            .unwrap()
            .last_curated_at,
        None
    );
}

#[test]
fn hard_rescan_preserves_genuine_rename_metadata_and_analysis_identity() {
    let dir = tempdir().unwrap();
    let old = dir.path().join("old.wav");
    let renamed = dir.path().join("renamed.wav");
    std::fs::write(&old, b"same sample").unwrap();
    let db = SourceDatabase::open_for_scan(dir.path()).unwrap();
    hard_rescan(&db).unwrap();
    db.set_tag(Path::new("old.wav"), Rating::KEEP_1).unwrap();
    db.set_user_tag(Path::new("old.wav"), Some("Hard rename"))
        .unwrap();
    insert_analysis_artifacts(dir.path(), "source::old.wav", "old.wav");

    std::fs::rename(&old, &renamed).unwrap();
    let stats = hard_rescan(&db).unwrap();

    assert_eq!(stats.renames_reconciled, 1);
    let entry = db
        .entry_for_path(Path::new("renamed.wav"))
        .unwrap()
        .unwrap();
    assert_eq!(entry.tag, Rating::KEEP_1);
    assert_eq!(entry.user_tag.as_deref(), Some("Hard rename"));
    assert!(db.list_pending_renames().unwrap().is_empty());
    assert_eq!(
        sample_id_count(dir.path(), "features", "source::old.wav"),
        0
    );
    assert_eq!(
        sample_id_count(dir.path(), "features", "source::renamed.wav"),
        1
    );
    assert_eq!(
        analysis_job_relative_path(dir.path(), "source::renamed.wav"),
        "renamed.wav"
    );
}

#[cfg(unix)]
#[test]
fn hard_rescan_reconciles_identity_move_when_old_path_is_an_external_link() {
    use std::os::unix::fs::symlink;

    let dir = tempdir().unwrap();
    let outside = tempdir().unwrap();
    let old = dir.path().join("old.wav");
    let renamed = dir.path().join("renamed.wav");
    let outside_file = outside.path().join("outside.wav");
    let outside_payload = b"outside target must remain untouched";
    std::fs::write(&old, b"same sample").unwrap();
    std::fs::write(&outside_file, outside_payload).unwrap();
    let db = SourceDatabase::open_for_scan(dir.path()).unwrap();
    hard_rescan(&db).unwrap();
    db.set_tag(Path::new("old.wav"), Rating::KEEP_1).unwrap();
    db.set_user_tag(Path::new("old.wav"), Some("External-link move"))
        .unwrap();
    insert_analysis_artifacts(dir.path(), "source::old.wav", "old.wav");

    std::fs::rename(&old, &renamed).unwrap();
    symlink(&outside_file, &old).unwrap();
    let stats = hard_rescan(&db).unwrap();

    assert_eq!(stats.renames_reconciled, 1);
    let entry = db
        .entry_for_path(Path::new("renamed.wav"))
        .unwrap()
        .unwrap();
    assert_eq!(entry.tag, Rating::KEEP_1);
    assert_eq!(entry.user_tag.as_deref(), Some("External-link move"));
    assert!(db.entry_for_path(Path::new("old.wav")).unwrap().is_none());
    assert!(db.list_pending_renames().unwrap().is_empty());
    assert_eq!(
        sample_id_count(dir.path(), "features", "source::old.wav"),
        0
    );
    assert_eq!(
        sample_id_count(dir.path(), "features", "source::renamed.wav"),
        1
    );
    assert_eq!(
        analysis_job_relative_path(dir.path(), "source::renamed.wav"),
        "renamed.wav"
    );
    assert_eq!(std::fs::read(&outside_file).unwrap(), outside_payload);
    assert_eq!(std::fs::read_link(&old).unwrap(), outside_file);
}

#[cfg(unix)]
#[test]
fn deferred_hash_copy_delete_recovers_when_old_path_is_an_external_link() {
    use std::os::unix::fs::symlink;

    let dir = tempdir().unwrap();
    let outside = tempdir().unwrap();
    let old = dir.path().join("old.wav");
    let copied = dir.path().join("copied.wav");
    let outside_file = outside.path().join("outside.wav");
    let outside_payload = b"outside hash target must remain untouched";
    let payload = vec![7_u8; 9 * 1024 * 1024];
    std::fs::write(&old, &payload).unwrap();
    std::fs::write(&outside_file, outside_payload).unwrap();

    let db = SourceDatabase::open_for_scan(dir.path()).unwrap();
    hard_rescan(&db).unwrap();
    db.set_tag(Path::new("old.wav"), Rating::KEEP_1).unwrap();
    db.set_user_tag(Path::new("old.wav"), Some("External-link copy-delete"))
        .unwrap();
    insert_analysis_artifacts(dir.path(), "source::old.wav", "old.wav");

    std::fs::copy(&old, &copied).unwrap();
    std::fs::remove_file(&old).unwrap();
    symlink(&outside_file, &old).unwrap();
    let removed = sync_paths(
        &db,
        &[PathBuf::from("copied.wav"), PathBuf::from("old.wav")],
    )
    .unwrap();
    let stats = complete_deferred_hashes(&db, removed).unwrap();

    assert_eq!(stats.renames_reconciled, 1);
    let entry = db.entry_for_path(Path::new("copied.wav")).unwrap().unwrap();
    assert_eq!(entry.tag, Rating::KEEP_1);
    assert_eq!(entry.user_tag.as_deref(), Some("External-link copy-delete"));
    assert!(db.entry_for_path(Path::new("old.wav")).unwrap().is_none());
    assert!(db.list_pending_renames().unwrap().is_empty());
    assert!(db.list_pending_rename_destinations().unwrap().is_empty());
    assert_eq!(
        sample_id_count(dir.path(), "features", "source::old.wav"),
        0
    );
    assert_eq!(
        sample_id_count(dir.path(), "features", "source::copied.wav"),
        1
    );
    assert_eq!(
        analysis_job_relative_path(dir.path(), "source::copied.wav"),
        "copied.wav"
    );
    assert_eq!(std::fs::read(&outside_file).unwrap(), outside_payload);
    assert_eq!(std::fs::read_link(&old).unwrap(), outside_file);
}

#[test]
fn hard_rescan_retains_metadata_for_ambiguous_same_content_destinations() {
    let dir = tempdir().unwrap();
    let old = dir.path().join("old.wav");
    let first = dir.path().join("first-copy.wav");
    let second = dir.path().join("second-copy.wav");
    std::fs::write(&old, b"same sample").unwrap();
    let db = SourceDatabase::open_for_scan(dir.path()).unwrap();
    hard_rescan(&db).unwrap();
    db.set_tag(Path::new("old.wav"), Rating::KEEP_1).unwrap();
    db.set_user_tag(Path::new("old.wav"), Some("Ambiguous hard rename"))
        .unwrap();
    insert_analysis_artifacts(dir.path(), "source::old.wav", "old.wav");

    std::fs::copy(&old, &first).unwrap();
    std::fs::copy(&old, &second).unwrap();
    std::fs::remove_file(&old).unwrap();
    let stats = hard_rescan(&db).unwrap();

    assert_eq!(stats.renames_reconciled, 0);
    for path in ["first-copy.wav", "second-copy.wav"] {
        let entry = db.entry_for_path(Path::new(path)).unwrap().unwrap();
        assert_eq!(entry.tag, Rating::NEUTRAL);
        assert_eq!(entry.user_tag, None);
        assert_eq!(
            sample_id_count(dir.path(), "features", &format!("source::{path}")),
            0
        );
    }
    assert!(db.list_pending_renames().unwrap().iter().any(|entry| {
        entry.relative_path == Path::new("old.wav")
            && entry.metadata.tag == Rating::KEEP_1
            && entry.metadata.user_tag.as_deref() == Some("Ambiguous hard rename")
    }));
    assert_eq!(
        sample_id_count(dir.path(), "features", "source::old.wav"),
        1
    );

    std::fs::remove_file(&first).unwrap();
    let resolved = hard_rescan(&db).unwrap();

    assert_eq!(resolved.renames_reconciled, 1);
    let survivor = db
        .entry_for_path(Path::new("second-copy.wav"))
        .unwrap()
        .unwrap();
    assert_eq!(survivor.tag, Rating::KEEP_1);
    assert_eq!(survivor.user_tag.as_deref(), Some("Ambiguous hard rename"));
    assert!(db.list_pending_renames().unwrap().is_empty());
    assert!(db.list_pending_rename_destinations().unwrap().is_empty());
    assert_eq!(
        sample_id_count(dir.path(), "features", "source::old.wav"),
        0
    );
    assert_eq!(
        sample_id_count(dir.path(), "features", "source::second-copy.wav"),
        1
    );
}

#[test]
fn hard_rescan_retains_destinations_when_pending_source_is_only_prior_state() {
    let dir = tempdir().unwrap();
    let old = dir.path().join("old.wav");
    let first = dir.path().join("first-copy.wav");
    let second = dir.path().join("second-copy.wav");
    let payload = b"same sample";
    std::fs::write(&old, payload).unwrap();
    let db = SourceDatabase::open_for_scan(dir.path()).unwrap();
    hard_rescan(&db).unwrap();
    db.set_tag(Path::new("old.wav"), Rating::KEEP_1).unwrap();
    db.set_user_tag(Path::new("old.wav"), Some("Empty manifest recovery"))
        .unwrap();
    insert_analysis_artifacts(dir.path(), "source::old.wav", "old.wav");

    std::fs::remove_file(&old).unwrap();
    sync_paths(&db, &[PathBuf::from("old.wav")]).unwrap();
    assert!(db.list_files().unwrap().is_empty());
    assert_eq!(db.list_pending_renames().unwrap().len(), 1);

    std::fs::write(&first, payload).unwrap();
    std::fs::write(&second, payload).unwrap();
    let ambiguous = hard_rescan(&db).unwrap();
    assert_eq!(ambiguous.renames_reconciled, 0);
    assert_eq!(db.list_pending_rename_destinations().unwrap().len(), 2);
    drop(db);

    let db = SourceDatabase::open_for_scan(dir.path()).unwrap();
    std::fs::remove_file(&first).unwrap();
    let resolved = hard_rescan(&db).unwrap();

    assert_eq!(resolved.renames_reconciled, 1);
    let survivor = db
        .entry_for_path(Path::new("second-copy.wav"))
        .unwrap()
        .unwrap();
    assert_eq!(survivor.tag, Rating::KEEP_1);
    assert_eq!(
        survivor.user_tag.as_deref(),
        Some("Empty manifest recovery")
    );
    assert!(db.list_pending_renames().unwrap().is_empty());
    assert!(db.list_pending_rename_destinations().unwrap().is_empty());
    assert_eq!(
        sample_id_count(dir.path(), "features", "source::old.wav"),
        0
    );
    assert_eq!(
        sample_id_count(dir.path(), "features", "source::second-copy.wav"),
        1
    );
}

#[test]
fn hard_rescan_converges_two_pending_sources_with_one_matching_destination() {
    let dir = tempdir().unwrap();
    let first_old = dir.path().join("first-old.wav");
    let second_old = dir.path().join("second-old.wav");
    let destination = dir.path().join("destination.wav");
    let payload = b"ambiguous source identity";
    std::fs::write(&first_old, payload).unwrap();
    std::fs::write(&second_old, payload).unwrap();
    let db = SourceDatabase::open_for_scan(dir.path()).unwrap();
    hard_rescan(&db).unwrap();
    db.set_tag(Path::new("first-old.wav"), Rating::KEEP_1)
        .unwrap();
    db.set_tag(Path::new("second-old.wav"), Rating::KEEP_3)
        .unwrap();

    std::fs::remove_file(&first_old).unwrap();
    std::fs::remove_file(&second_old).unwrap();
    sync_paths(
        &db,
        &[
            PathBuf::from("first-old.wav"),
            PathBuf::from("second-old.wav"),
        ],
    )
    .unwrap();
    std::fs::write(&destination, payload).unwrap();
    let added = sync_paths(&db, &[PathBuf::from("destination.wav")]).unwrap();
    let unresolved = complete_deferred_hashes(&db, added).unwrap();
    assert_eq!(unresolved.renames_reconciled, 0);
    assert_eq!(db.list_pending_renames().unwrap().len(), 2);

    let converged = hard_rescan(&db).unwrap();

    assert_eq!(converged.pending_renames_pruned, 2);
    assert!(db.list_pending_renames().unwrap().is_empty());
    assert!(db.list_pending_rename_destinations().unwrap().is_empty());
    assert_eq!(
        db.entry_for_path(Path::new("destination.wav"))
            .unwrap()
            .unwrap()
            .tag,
        Rating::NEUTRAL
    );
}

#[test]
fn hard_rescan_ignores_stale_unresolved_destination_when_pruning() {
    let dir = tempdir().unwrap();
    let old = dir.path().join("old.wav");
    std::fs::write(&old, b"deleted sample").unwrap();
    let db = SourceDatabase::open_for_scan(dir.path()).unwrap();
    hard_rescan(&db).unwrap();
    db.set_tag(Path::new("old.wav"), Rating::KEEP_1).unwrap();

    std::fs::remove_file(&old).unwrap();
    sync_paths(&db, &[PathBuf::from("old.wav")]).unwrap();
    let mut batch = db.write_batch().unwrap();
    batch
        .stage_pending_rename_destination(Path::new("stale.wav"), u64::MAX / 2)
        .unwrap();
    batch.commit().unwrap();

    let converged = hard_rescan(&db).unwrap();

    assert_eq!(converged.pending_renames_pruned, 1);
    assert!(db.list_pending_renames().unwrap().is_empty());
    assert!(db.list_pending_rename_destinations().unwrap().is_empty());
}

#[test]
fn rename_apply_refreshes_metadata_changed_during_discovery() {
    let dir = tempdir().unwrap();
    let first_path = dir.path().join("one.wav");
    let second_path = dir.path().join("two.wav");
    std::fs::write(&first_path, b"one").unwrap();
    let db = SourceDatabase::open_for_scan(dir.path()).unwrap();
    scan_once(&db).unwrap();
    std::fs::rename(&first_path, &second_path).unwrap();
    let mut edited = false;

    let stats = scan_with_progress(&db, ScanMode::Quick, None, &mut |_, _| {
        if !edited {
            db.set_tag(Path::new("one.wav"), Rating::KEEP_1).unwrap();
            db.set_user_tag(Path::new("one.wav"), Some("Edited during scan"))
                .unwrap();
            edited = true;
        }
    })
    .unwrap();

    assert!(edited);
    assert_eq!(stats.renames_reconciled, 1);
    let row = db.entry_for_path(Path::new("two.wav")).unwrap().unwrap();
    assert_eq!(row.tag, Rating::KEEP_1);
    assert_eq!(row.user_tag.as_deref(), Some("Edited during scan"));
}

#[test]
fn pending_rename_staging_refreshes_metadata_changed_during_discovery() {
    let dir = tempdir().unwrap();
    let removed_path = dir.path().join("removed.wav");
    std::fs::write(&removed_path, b"removed").unwrap();
    std::fs::write(dir.path().join("live.wav"), b"live").unwrap();
    let db = SourceDatabase::open_for_scan(dir.path()).unwrap();
    scan_once(&db).unwrap();
    std::fs::remove_file(&removed_path).unwrap();
    let mut edited = false;

    scan_with_progress(&db, ScanMode::Quick, None, &mut |_, _| {
        if !edited {
            db.set_tag(Path::new("removed.wav"), Rating::KEEP_1)
                .unwrap();
            edited = true;
        }
    })
    .unwrap();

    let pending = db.list_pending_renames().unwrap();
    let removed = pending
        .iter()
        .find(|entry| entry.relative_path == Path::new("removed.wav"))
        .expect("removed path must be staged");
    assert_eq!(removed.metadata.tag, Rating::KEEP_1);
}

#[test]
fn quick_scan_defers_hash_for_large_file() {
    let dir = tempdir().unwrap();
    let file_path = dir.path().join("large.wav");
    std::fs::write(&file_path, vec![0u8; 9 * 1024 * 1024]).unwrap();

    let db = SourceDatabase::open_for_scan(dir.path()).unwrap();
    let stats = scan_once(&db).unwrap();
    assert_eq!(stats.hashes_pending, 1);
    assert_eq!(stats.hashes_computed, 0);

    let rows = db.list_files().unwrap();
    assert_eq!(rows.len(), 1);
    assert!(rows[0].content_hash.is_none());

    let completed = complete_deferred_hashes(&db, stats).unwrap();
    assert_eq!(completed.hashes_pending, 0);
    assert!(
        db.entry_for_path(Path::new("large.wav"))
            .unwrap()
            .unwrap()
            .content_hash
            .is_some(),
        "scan consumers should receive the result only after the initial large-file hash is durable"
    );
}

#[test]
fn owned_deferred_hash_publishes_a_new_committed_content_generation() {
    let dir = tempdir().unwrap();
    let file_path = dir.path().join("large.wav");
    std::fs::write(&file_path, vec![0u8; 9 * 1024 * 1024]).unwrap();
    let db = SourceDatabase::open_for_scan(dir.path()).unwrap();
    let initial = scan_once(&db).unwrap();

    let completed = complete_pending_deep_hash_for_path(&db, Path::new("large.wav"), None)
        .expect("owned deferred hash completion");

    assert!(completed.committed_delta.revision > initial.committed_delta.revision);
    assert_eq!(completed.committed_delta.changed.len(), 1);
    assert_eq!(
        completed.committed_delta.changed[0].relative_path,
        Path::new("large.wav")
    );
}

#[test]
fn canceled_deferred_hashing_reports_cancellation_after_quick_scan_commit() {
    let dir = tempdir().unwrap();
    let file_path = dir.path().join("large.wav");
    std::fs::write(&file_path, vec![0u8; 9 * 1024 * 1024]).unwrap();
    let db = SourceDatabase::open_for_scan(dir.path()).unwrap();
    let stats = scan_once(&db).unwrap();
    let cancel = std::sync::atomic::AtomicBool::new(true);

    let result = complete_deferred_hashes_with_cancel(&db, stats, Some(&cancel));

    assert!(matches!(result, Err(ScanError::Canceled)));
    assert!(db.entry_for_path(Path::new("large.wav")).unwrap().is_some());
}

#[test]
fn canceled_rename_reconciliation_can_be_safely_requeued() {
    let dir = tempdir().unwrap();
    let old_path = dir.path().join("old.wav");
    let new_path = dir.path().join("new.wav");
    std::fs::write(&old_path, vec![3_u8; 9 * 1024 * 1024]).unwrap();
    let db = SourceDatabase::open_for_scan(dir.path()).unwrap();
    hard_rescan(&db).unwrap();
    db.set_tag(Path::new("old.wav"), Rating::KEEP_1).unwrap();
    std::fs::rename(&old_path, &new_path).unwrap();
    let stats = scan_once(&db).unwrap();
    let cancel = std::sync::atomic::AtomicBool::new(true);

    let cancelled =
        complete_deferred_rename_candidates_with_cancel(&db, stats.clone(), Some(&cancel));

    assert!(matches!(cancelled, Err(ScanError::Canceled)));
    assert_eq!(
        db.entry_for_path(Path::new("new.wav"))
            .unwrap()
            .unwrap()
            .tag,
        Rating::NEUTRAL
    );

    cancel.store(false, std::sync::atomic::Ordering::Release);
    let completed =
        complete_deferred_rename_candidates_with_cancel(&db, stats, Some(&cancel)).unwrap();
    assert_eq!(completed.renames_reconciled, 1);
    assert_eq!(
        db.entry_for_path(Path::new("new.wav"))
            .unwrap()
            .unwrap()
            .tag,
        Rating::KEEP_1
    );
}

#[test]
fn candidate_completion_keeps_cold_large_import_hashing_deferred() {
    let dir = tempdir().unwrap();
    std::fs::write(dir.path().join("large.wav"), vec![0_u8; 9 * 1024 * 1024]).unwrap();
    let db = SourceDatabase::open_for_scan(dir.path()).unwrap();
    let stats = scan_once(&db).unwrap();
    assert_eq!(stats.hashes_pending, 1);

    let completed = complete_deferred_rename_candidates(&db, stats).unwrap();

    assert_eq!(completed.hashes_pending, 1);
    assert_eq!(completed.hashes_computed, 0);
    assert!(
        db.entry_for_path(Path::new("large.wav"))
            .unwrap()
            .unwrap()
            .content_hash
            .is_none()
    );
}

#[test]
fn large_rename_defers_identity_until_deep_hash_and_survives_restart() {
    let dir = tempdir().unwrap();
    let first_path = dir.path().join("one.wav");
    let second_path = dir.path().join("two.wav");
    std::fs::write(&first_path, vec![0u8; 9 * 1024 * 1024]).unwrap();

    let db = SourceDatabase::open_for_scan(dir.path()).unwrap();
    hard_rescan(&db).unwrap();
    db.set_tag(Path::new("one.wav"), Rating::KEEP_1).unwrap();
    db.set_sound_type(Path::new("one.wav"), Some(SampleSoundType::Texture))
        .unwrap();
    db.set_user_tag(Path::new("one.wav"), Some("Night Pad"))
        .unwrap();
    db.assign_tag_to_path(Path::new("one.wav"), "Night Pad")
        .unwrap();
    let historical_curation = 1_640_000_456;
    let expected_collections = [
        SampleCollection::new(0).unwrap(),
        SampleCollection::new(5).unwrap(),
    ];
    let mut batch = db.write_batch().unwrap();
    for collection in expected_collections {
        batch
            .add_collection(Path::new("one.wav"), collection)
            .unwrap();
    }
    batch
        .set_last_curated_at(Path::new("one.wav"), historical_curation)
        .unwrap();
    batch.commit().unwrap();
    insert_analysis_artifacts(dir.path(), "source::one.wav", "one.wav");

    std::fs::rename(&first_path, &second_path).unwrap();
    let stats = scan_once(&db).unwrap();
    assert_eq!(stats.renames_reconciled, 0);
    assert_eq!(stats.added, 1);
    assert_eq!(stats.missing, 1);
    assert_eq!(stats.hashes_pending, 1);

    let rows = db.list_files().unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].relative_path, Path::new("two.wav"));
    assert_eq!(rows[0].tag, Rating::NEUTRAL);
    assert_eq!(rows[0].sound_type, None);
    assert_eq!(rows[0].user_tag, None);
    assert!(rows[0].normal_tags.is_empty());
    assert!(!rows[0].missing);
    assert!(rows[0].content_hash.is_none());
    assert_eq!(
        sample_id_count(dir.path(), "features", "source::one.wav"),
        1
    );
    assert_eq!(
        sample_id_count(dir.path(), "features", "source::two.wav"),
        0
    );
    drop(db);

    let db = SourceDatabase::open_for_scan(dir.path()).unwrap();
    let deep_stats = crate::sample_sources::scanner::scan_hash::deep_hash_scan(
        &db,
        None,
        &rename_candidates(&["two.wav"]),
        crate::sample_sources::scanner::scan_hash::DeferredHashScope::AllUnhashed,
        None,
        None,
    )
    .unwrap();
    assert_eq!(deep_stats.hashes_computed, 1);
    assert_eq!(deep_stats.renames_reconciled, 1);

    let rows = db.list_files().unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].relative_path, Path::new("two.wav"));
    assert_eq!(rows[0].tag, Rating::KEEP_1);
    assert_eq!(rows[0].sound_type, Some(SampleSoundType::Texture));
    assert_eq!(rows[0].user_tag.as_deref(), Some("Night Pad"));
    assert_eq!(rows[0].normal_tags, vec!["Night Pad"]);
    assert_eq!(rows[0].last_curated_at, Some(historical_curation));
    assert_eq!(
        db.collections_for_path(Path::new("two.wav")).unwrap(),
        expected_collections
    );
    assert!(!rows[0].missing);
    assert!(rows[0].content_hash.is_some());
    assert!(db.list_pending_renames().unwrap().is_empty());
    assert_eq!(
        sample_id_count(dir.path(), "features", "source::one.wav"),
        0
    );
    assert_eq!(
        sample_id_count(dir.path(), "features", "source::two.wav"),
        1
    );
}

#[cfg(any(unix, windows))]
#[test]
fn large_rename_before_initial_hash_uses_stable_file_identity() {
    let dir = tempdir().unwrap();
    let first_path = dir.path().join("one.wav");
    let second_path = dir.path().join("two.wav");
    std::fs::write(&first_path, vec![3u8; 9 * 1024 * 1024]).unwrap();

    let db = SourceDatabase::open_for_scan(dir.path()).unwrap();
    let initial = scan_once(&db).unwrap();
    assert_eq!(initial.hashes_pending, 1);
    assert_eq!(
        db.entry_for_path(Path::new("one.wav"))
            .unwrap()
            .unwrap()
            .content_hash,
        None
    );
    db.set_tag(Path::new("one.wav"), Rating::KEEP_1).unwrap();

    std::fs::rename(&first_path, &second_path).unwrap();
    let quick = scan_once(&db).unwrap();
    assert_eq!(quick.renames_reconciled, 0);
    assert_eq!(quick.hashes_pending, 1);

    let completed = complete_deferred_hashes(&db, quick).unwrap();

    assert_eq!(completed.renames_reconciled, 1);
    let renamed = db.entry_for_path(Path::new("two.wav")).unwrap().unwrap();
    assert_eq!(renamed.tag, Rating::KEEP_1);
    assert!(renamed.content_hash.is_some());
    assert!(db.list_pending_renames().unwrap().is_empty());
}

#[cfg(any(unix, windows))]
#[test]
fn copy_delete_before_initial_hash_does_not_transfer_identity() {
    let dir = tempdir().unwrap();
    let first_path = dir.path().join("one.wav");
    let second_path = dir.path().join("two.wav");
    std::fs::write(&first_path, vec![4u8; 9 * 1024 * 1024]).unwrap();

    let db = SourceDatabase::open_for_scan(dir.path()).unwrap();
    scan_once(&db).unwrap();
    db.set_tag(Path::new("one.wav"), Rating::KEEP_1).unwrap();

    std::fs::copy(&first_path, &second_path).unwrap();
    std::fs::remove_file(&first_path).unwrap();
    let quick = scan_once(&db).unwrap();
    let completed = complete_deferred_hashes(&db, quick).unwrap();

    assert_eq!(completed.renames_reconciled, 0);
    assert_eq!(
        db.entry_for_path(Path::new("two.wav"))
            .unwrap()
            .unwrap()
            .tag,
        Rating::NEUTRAL
    );
    assert!(
        db.list_pending_renames()
            .unwrap()
            .iter()
            .any(|entry| entry.relative_path == Path::new("one.wav"))
    );
}

#[test]
fn owned_deep_hash_uses_persisted_quick_scan_destinations() {
    let dir = tempdir().unwrap();
    let old = dir.path().join("old.wav");
    let new = dir.path().join("new.wav");
    std::fs::write(&old, vec![5_u8; 9 * 1024 * 1024]).unwrap();

    let db = SourceDatabase::open_for_scan(dir.path()).unwrap();
    hard_rescan(&db).unwrap();
    db.set_tag(Path::new("old.wav"), Rating::KEEP_1).unwrap();
    std::fs::rename(&old, &new).unwrap();

    let quick = scan_once(&db).unwrap();
    assert_eq!(quick.hashes_pending, 1);
    let deferred = crate::sample_sources::scanner::scan_hash::deep_hash_scan(
        &db,
        None,
        &HashSet::new(),
        crate::sample_sources::scanner::scan_hash::DeferredHashScope::AllUnhashed,
        None,
        None,
    )
    .unwrap();

    assert_eq!(deferred.renames_reconciled, 1);
    assert_eq!(
        db.entry_for_path(Path::new("new.wav"))
            .unwrap()
            .unwrap()
            .tag,
        Rating::KEEP_1
    );
    assert!(db.list_pending_renames().unwrap().is_empty());
}

#[test]
fn large_rename_reconciles_when_unchanged_duplicate_remains() {
    let dir = tempdir().unwrap();
    let original = dir.path().join("a.wav");
    let duplicate = dir.path().join("b.wav");
    let renamed = dir.path().join("c.wav");
    let payload = vec![7u8; 9 * 1024 * 1024];
    std::fs::write(&original, &payload).unwrap();
    std::fs::write(&duplicate, &payload).unwrap();

    let db = SourceDatabase::open_for_scan(dir.path()).unwrap();
    hard_rescan(&db).unwrap();
    db.set_tag(Path::new("a.wav"), Rating::KEEP_1).unwrap();
    db.set_user_tag(Path::new("a.wav"), Some("Original metadata"))
        .unwrap();

    std::fs::rename(&original, &renamed).unwrap();
    let stats = scan_once(&db).unwrap();
    assert_eq!(stats.renames_reconciled, 0);
    let deep = crate::sample_sources::scanner::scan_hash::deep_hash_scan(
        &db,
        None,
        &rename_candidates(&["c.wav"]),
        crate::sample_sources::scanner::scan_hash::DeferredHashScope::AllUnhashed,
        None,
        None,
    )
    .unwrap();
    assert_eq!(deep.renames_reconciled, 1);

    let duplicate_entry = db.entry_for_path(Path::new("b.wav")).unwrap().unwrap();
    assert_eq!(duplicate_entry.tag, Rating::NEUTRAL);
    let renamed_entry = db.entry_for_path(Path::new("c.wav")).unwrap().unwrap();
    assert_eq!(renamed_entry.tag, Rating::KEEP_1);
    assert_eq!(renamed_entry.user_tag.as_deref(), Some("Original metadata"));
}

#[cfg(unix)]
#[test]
fn size_and_mtime_coincidence_never_transfers_identity_or_metadata() {
    let dir = tempdir().unwrap();
    let old_path = dir.path().join("old.wav");
    let new_path = dir.path().join("new.wav");
    let file_size = 9 * 1024 * 1024;
    let timestamp = 1_700_000_000;
    std::fs::write(&old_path, vec![0_u8; file_size]).unwrap();
    set_file_times(&old_path, timestamp, 0);
    let db = SourceDatabase::open_for_scan(dir.path()).unwrap();
    hard_rescan(&db).unwrap();
    db.set_tag(Path::new("old.wav"), Rating::KEEP_1).unwrap();
    db.set_user_tag(Path::new("old.wav"), Some("Must stay pending"))
        .unwrap();
    insert_analysis_artifacts(dir.path(), "source::old.wav", "old.wav");

    std::fs::remove_file(&old_path).unwrap();
    std::fs::write(&new_path, vec![1_u8; file_size]).unwrap();
    set_file_times(&new_path, timestamp, 0);
    let quick = scan_once(&db).unwrap();

    assert_eq!(quick.renames_reconciled, 0);
    assert_eq!(quick.added, 1);
    assert_eq!(quick.missing, 1);
    let new_entry = db.entry_for_path(Path::new("new.wav")).unwrap().unwrap();
    assert_eq!(new_entry.tag, Rating::NEUTRAL);
    assert_eq!(new_entry.user_tag, None);
    assert_eq!(
        sample_id_count(dir.path(), "features", "source::old.wav"),
        1
    );
    assert_eq!(
        sample_id_count(dir.path(), "features", "source::new.wav"),
        0
    );
    drop(db);

    let db = SourceDatabase::open_for_scan(dir.path()).unwrap();
    let deep = crate::sample_sources::scanner::scan_hash::deep_hash_scan(
        &db,
        None,
        &rename_candidates(&["new.wav"]),
        crate::sample_sources::scanner::scan_hash::DeferredHashScope::AllUnhashed,
        None,
        None,
    )
    .unwrap();

    assert_eq!(deep.renames_reconciled, 0);
    let new_entry = db.entry_for_path(Path::new("new.wav")).unwrap().unwrap();
    assert_eq!(new_entry.tag, Rating::NEUTRAL);
    assert_eq!(new_entry.user_tag, None);
    let pending = db.list_pending_renames().unwrap();
    assert!(pending.iter().any(|entry| {
        entry.relative_path == Path::new("old.wav")
            && entry.metadata.tag == Rating::KEEP_1
            && entry.metadata.user_tag.as_deref() == Some("Must stay pending")
    }));
    assert_eq!(
        sample_id_count(dir.path(), "features", "source::old.wav"),
        1
    );
    assert_eq!(
        sample_id_count(dir.path(), "features", "source::new.wav"),
        0
    );
}

#[test]
fn deep_hash_scan_replays_pending_rename_metadata() {
    let dir = tempdir().unwrap();
    let old_path = dir.path().join("one.wav");
    let new_path = dir.path().join("two.wav");
    std::fs::write(&old_path, vec![0u8; 9 * 1024 * 1024]).unwrap();

    let db = SourceDatabase::open_for_scan(dir.path()).unwrap();
    hard_rescan(&db).unwrap();
    db.set_tag(Path::new("one.wav"), Rating::KEEP_1).unwrap();
    db.set_sound_type(Path::new("one.wav"), Some(SampleSoundType::Fx))
        .unwrap();
    db.set_user_tag(Path::new("one.wav"), Some("Sweep"))
        .unwrap();
    db.assign_tag_to_path(Path::new("one.wav"), "Sweep")
        .unwrap();

    let original_facts = {
        let entry = db.entry_for_path(Path::new("one.wav")).unwrap().unwrap();
        let facts = (entry.file_size, entry.modified_ns);
        let mut batch = db.write_batch().unwrap();
        batch.stage_pending_rename(&entry).unwrap();
        batch.remove_file(Path::new("one.wav")).unwrap();
        batch.commit().unwrap();
        facts
    };
    std::fs::rename(&old_path, &new_path).unwrap();
    let mut batch = db.write_batch().unwrap();
    batch
        .upsert_file_without_hash(Path::new("two.wav"), original_facts.0, original_facts.1)
        .unwrap();
    batch.commit().unwrap();

    let pending_before_deep = db.list_pending_renames().unwrap();
    assert_eq!(pending_before_deep.len(), 1);
    assert_eq!(
        pending_before_deep[0].metadata.sound_type,
        Some(SampleSoundType::Fx)
    );
    assert_eq!(
        pending_before_deep[0].metadata.user_tag.as_deref(),
        Some("Sweep")
    );
    assert_eq!(pending_before_deep[0].metadata.normal_tags, vec!["Sweep"]);

    let deep_stats = crate::sample_sources::scanner::scan_hash::deep_hash_scan(
        &db,
        None,
        &rename_candidates(&["two.wav"]),
        crate::sample_sources::scanner::scan_hash::DeferredHashScope::AllUnhashed,
        None,
        None,
    )
    .unwrap();
    assert_eq!(deep_stats.renames_reconciled, 1);

    let rows = db.list_files().unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].relative_path, Path::new("two.wav"));
    assert_eq!(rows[0].tag, Rating::KEEP_1);
    assert_eq!(rows[0].sound_type, Some(SampleSoundType::Fx));
    assert_eq!(rows[0].user_tag.as_deref(), Some("Sweep"));
    assert_eq!(rows[0].normal_tags, vec!["Sweep"]);
    assert!(db.list_pending_renames().unwrap().is_empty());
}

#[cfg(unix)]
#[test]
fn deep_hash_scan_uses_matching_facts_to_disambiguate_backfilled_duplicates() {
    let dir = tempdir().unwrap();
    let original = dir.path().join("a.wav");
    let duplicate = dir.path().join("b.wav");
    let renamed = dir.path().join("c.wav");
    let payload = vec![7u8; 9 * 1024 * 1024];
    std::fs::write(&original, &payload).unwrap();
    set_file_times(&original, 1_700_000_000, 0);

    let db = SourceDatabase::open_for_scan(dir.path()).unwrap();
    hard_rescan(&db).unwrap();
    db.set_tag(Path::new("a.wav"), Rating::KEEP_1).unwrap();

    std::fs::write(&duplicate, &payload).unwrap();
    set_file_times(&duplicate, 1_700_000_100, 0);
    let duplicate_scan = scan_once(&db).unwrap();
    assert_eq!(duplicate_scan.hashes_pending, 1);

    std::fs::rename(&original, &renamed).unwrap();
    let rename_scan = scan_once(&db).unwrap();
    assert_eq!(rename_scan.hashes_pending, 2);
    assert_eq!(rename_scan.renames_reconciled, 0);

    let deep = crate::sample_sources::scanner::scan_hash::deep_hash_scan(
        &db,
        None,
        &rename_candidates(&["b.wav", "c.wav"]),
        crate::sample_sources::scanner::scan_hash::DeferredHashScope::AllUnhashed,
        None,
        None,
    )
    .unwrap();
    assert_eq!(deep.hashes_computed, 2);
    assert_eq!(deep.renames_reconciled, 1);
    assert_eq!(
        db.entry_for_path(Path::new("b.wav")).unwrap().unwrap().tag,
        Rating::NEUTRAL
    );
    assert_eq!(
        db.entry_for_path(Path::new("c.wav")).unwrap().unwrap().tag,
        Rating::KEEP_1
    );
}

#[cfg(unix)]
#[test]
fn deferred_completion_reconciles_unique_hash_even_when_mtime_changes() {
    let dir = tempdir().unwrap();
    let old = dir.path().join("old.wav");
    let new = dir.path().join("new.wav");
    std::fs::write(&old, vec![7_u8; 9 * 1024 * 1024]).unwrap();
    set_file_times(&old, 1_700_000_000, 0);

    let db = SourceDatabase::open_for_scan(dir.path()).unwrap();
    hard_rescan(&db).unwrap();
    db.set_tag(Path::new("old.wav"), Rating::KEEP_1).unwrap();

    std::fs::copy(&old, &new).unwrap();
    set_file_times(&new, 1_700_000_100, 0);
    std::fs::remove_file(&old).unwrap();
    let removed = sync_paths(&db, &[PathBuf::from("new.wav"), PathBuf::from("old.wav")]).unwrap();
    assert_eq!(removed.missing, 1);
    assert_eq!(
        removed.rename_candidate_paths,
        vec![PathBuf::from("new.wav")]
    );
    assert!(
        db.list_pending_renames()
            .unwrap()
            .iter()
            .any(|entry| entry.content_hash.is_some())
    );

    let completed = complete_deferred_hashes(&db, removed).unwrap();
    assert_eq!(completed.renames_reconciled, 1);
    assert_eq!(completed.renamed_samples.len(), 1);
    assert_eq!(
        db.entry_for_path(Path::new("new.wav"))
            .unwrap()
            .unwrap()
            .tag,
        Rating::KEEP_1
    );
    assert!(db.list_pending_renames().unwrap().is_empty());
}

#[test]
fn targeted_split_batches_preserve_large_rename_destination() {
    let dir = tempdir().unwrap();
    let old = dir.path().join("old.wav");
    let new = dir.path().join("new.wav");
    std::fs::write(&old, vec![7_u8; 9 * 1024 * 1024]).unwrap();

    let db = SourceDatabase::open_for_scan(dir.path()).unwrap();
    hard_rescan(&db).unwrap();
    db.set_tag(Path::new("old.wav"), Rating::KEEP_1).unwrap();

    std::fs::copy(&old, &new).unwrap();
    let added = sync_paths(&db, &[PathBuf::from("new.wav")]).unwrap();
    let added = complete_deferred_hashes(&db, added).unwrap();
    assert_eq!(added.hashes_pending, 0);
    assert_eq!(
        db.list_pending_rename_destinations().unwrap(),
        vec![PathBuf::from("new.wav")]
    );
    scan_once(&db).unwrap();
    assert_eq!(
        db.list_pending_rename_destinations().unwrap(),
        vec![PathBuf::from("new.wav")],
        "a full quick scan between watcher halves must carry the destination"
    );

    std::fs::remove_file(&old).unwrap();
    let removed = sync_paths(&db, &[PathBuf::from("old.wav")]).unwrap();
    let completed = complete_deferred_hashes(&db, removed).unwrap();

    assert_eq!(completed.renames_reconciled, 1);
    assert_eq!(completed.targeted_manifest_query_count, 4);
    assert_eq!(completed.targeted_manifest_rows_read, 4);
    assert!(completed.committed_delta.created.is_empty());
    assert!(completed.committed_delta.deleted.is_empty());
    assert_eq!(completed.committed_delta.moved.len(), 1);
    assert_eq!(
        completed.committed_delta.moved[0].old_relative_path,
        PathBuf::from("old.wav")
    );
    assert_eq!(
        completed.committed_delta.moved[0].new_relative_path,
        PathBuf::from("new.wav")
    );
    assert_eq!(
        db.entry_for_path(Path::new("new.wav"))
            .unwrap()
            .unwrap()
            .tag,
        Rating::KEEP_1
    );
    assert!(db.list_pending_renames().unwrap().is_empty());
    assert!(db.list_pending_rename_destinations().unwrap().is_empty());
}

#[test]
fn targeted_destination_expires_after_two_full_quick_scans() {
    let dir = tempdir().unwrap();
    let old = dir.path().join("old.wav");
    let new = dir.path().join("new.wav");
    std::fs::write(&old, b"same-content").unwrap();

    let db = SourceDatabase::open_for_scan(dir.path()).unwrap();
    hard_rescan(&db).unwrap();
    std::fs::copy(&old, &new).unwrap();
    let added = sync_paths(&db, &[PathBuf::from("new.wav")]).unwrap();
    complete_deferred_hashes(&db, added).unwrap();

    scan_once(&db).unwrap();
    assert_eq!(
        db.list_pending_rename_destinations().unwrap(),
        vec![PathBuf::from("new.wav")]
    );
    scan_once(&db).unwrap();

    assert!(db.list_pending_rename_destinations().unwrap().is_empty());
}

#[test]
fn retained_destination_is_revalidated_before_identity_transfer() {
    let dir = tempdir().unwrap();
    let old = dir.path().join("old.wav");
    let new = dir.path().join("new.wav");
    std::fs::write(&old, b"original-content").unwrap();

    let db = SourceDatabase::open_for_scan(dir.path()).unwrap();
    hard_rescan(&db).unwrap();
    db.set_tag(Path::new("old.wav"), Rating::KEEP_1).unwrap();
    std::fs::copy(&old, &new).unwrap();
    let added = sync_paths(&db, &[PathBuf::from("new.wav")]).unwrap();
    complete_deferred_hashes(&db, added).unwrap();

    std::fs::write(&new, b"replacement-content").unwrap();
    std::fs::remove_file(&old).unwrap();
    let removed = sync_paths(&db, &[PathBuf::from("old.wav")]).unwrap();
    let completed = complete_deferred_hashes(&db, removed).unwrap();

    assert_eq!(completed.renames_reconciled, 0);
    assert_eq!(
        db.entry_for_path(Path::new("new.wav"))
            .unwrap()
            .unwrap()
            .tag,
        Rating::NEUTRAL
    );
}

#[test]
fn sole_eligible_destination_reconciles_when_unchanged_duplicate_exists() {
    let dir = tempdir().unwrap();
    let old = dir.path().join("old.wav");
    let duplicate = dir.path().join("duplicate.wav");
    let candidate = dir.path().join("candidate.wav");
    let payload = vec![7_u8; 9 * 1024 * 1024];
    std::fs::write(&old, &payload).unwrap();
    std::fs::write(&duplicate, &payload).unwrap();

    let db = SourceDatabase::open_for_scan(dir.path()).unwrap();
    hard_rescan(&db).unwrap();
    db.set_tag(Path::new("old.wav"), Rating::KEEP_1).unwrap();
    std::fs::remove_file(&old).unwrap();
    let removed = sync_paths(&db, &[PathBuf::from("old.wav")]).unwrap();
    complete_deferred_hashes(&db, removed).unwrap();

    std::fs::copy(&duplicate, &candidate).unwrap();
    let added = sync_paths(&db, &[PathBuf::from("candidate.wav")]).unwrap();
    let completed = complete_deferred_hashes(&db, added).unwrap();

    assert_eq!(completed.renames_reconciled, 1);
    assert_eq!(
        db.entry_for_path(Path::new("candidate.wav"))
            .unwrap()
            .unwrap()
            .tag,
        Rating::KEEP_1
    );
    assert!(db.list_pending_renames().unwrap().is_empty());
    assert_eq!(
        db.entry_for_path(Path::new("duplicate.wav"))
            .unwrap()
            .unwrap()
            .tag,
        Rating::NEUTRAL
    );
}

#[test]
fn rename_candidate_completion_does_not_backfill_unrelated_large_files() {
    let dir = tempdir().unwrap();
    let old = dir.path().join("old.wav");
    let new = dir.path().join("new.wav");
    let unrelated = dir.path().join("unrelated-large.wav");
    std::fs::write(&old, b"same-content").unwrap();
    std::fs::write(&unrelated, vec![9_u8; 9 * 1024 * 1024]).unwrap();

    let db = SourceDatabase::open_for_scan(dir.path()).unwrap();
    hard_rescan(&db).unwrap();
    let unrelated_entry = db
        .entry_for_path(Path::new("unrelated-large.wav"))
        .unwrap()
        .unwrap();
    let mut batch = db.write_batch().unwrap();
    batch
        .upsert_file_without_hash(
            &unrelated_entry.relative_path,
            unrelated_entry.file_size,
            unrelated_entry.modified_ns,
        )
        .unwrap();
    batch.commit().unwrap();

    std::fs::copy(&old, &new).unwrap();
    let added = sync_paths(&db, &[PathBuf::from("new.wav")]).unwrap();
    complete_deferred_hashes(&db, added).unwrap();
    std::fs::remove_file(&old).unwrap();
    let removed = sync_paths(&db, &[PathBuf::from("old.wav")]).unwrap();
    let completed = complete_deferred_hashes(&db, removed).unwrap();

    assert_eq!(completed.renames_reconciled, 1);
    assert_eq!(completed.hashes_computed, 0);
    assert!(
        db.entry_for_path(Path::new("unrelated-large.wav"))
            .unwrap()
            .unwrap()
            .content_hash
            .is_none()
    );
}

#[test]
fn targeted_destination_expires_after_an_unrelated_batch() {
    let dir = tempdir().unwrap();
    let old = dir.path().join("old.wav");
    let duplicate = dir.path().join("duplicate.wav");
    let unrelated = dir.path().join("unrelated.wav");
    std::fs::write(&old, b"same-content").unwrap();

    let db = SourceDatabase::open_for_scan(dir.path()).unwrap();
    hard_rescan(&db).unwrap();
    db.set_tag(Path::new("old.wav"), Rating::KEEP_1).unwrap();

    std::fs::copy(&old, &duplicate).unwrap();
    let added = sync_paths(&db, &[PathBuf::from("duplicate.wav")]).unwrap();
    complete_deferred_hashes(&db, added).unwrap();

    std::fs::write(&unrelated, b"different").unwrap();
    let unrelated_stats = sync_paths(&db, &[PathBuf::from("unrelated.wav")]).unwrap();
    complete_deferred_hashes(&db, unrelated_stats).unwrap();

    std::fs::remove_file(&old).unwrap();
    let removed = sync_paths(&db, &[PathBuf::from("old.wav")]).unwrap();
    let completed = complete_deferred_hashes(&db, removed).unwrap();

    assert_eq!(completed.renames_reconciled, 0);
    assert_eq!(
        db.entry_for_path(Path::new("duplicate.wav"))
            .unwrap()
            .unwrap()
            .tag,
        Rating::NEUTRAL
    );
}

#[test]
fn deferred_completion_does_not_treat_plain_delete_as_duplicate_rename() {
    let dir = tempdir().unwrap();
    let deleted = dir.path().join("deleted.wav");
    let duplicate = dir.path().join("duplicate.wav");
    let unrelated_large = dir.path().join("unrelated-large.wav");
    std::fs::write(&deleted, b"same-content").unwrap();
    std::fs::write(&duplicate, b"same-content").unwrap();
    std::fs::write(&unrelated_large, vec![9_u8; 9 * 1024 * 1024]).unwrap();

    let db = SourceDatabase::open_for_scan(dir.path()).unwrap();
    hard_rescan(&db).unwrap();
    let large = db
        .entry_for_path(Path::new("unrelated-large.wav"))
        .unwrap()
        .unwrap();
    let mut batch = db.write_batch().unwrap();
    batch
        .upsert_file_without_hash(&large.relative_path, large.file_size, large.modified_ns)
        .unwrap();
    batch.commit().unwrap();
    db.set_tag(Path::new("deleted.wav"), Rating::KEEP_1)
        .unwrap();
    std::fs::remove_file(&deleted).unwrap();

    let removed = sync_paths(&db, &[PathBuf::from("deleted.wav")]).unwrap();
    let completed = complete_deferred_hashes(&db, removed).unwrap();

    assert_eq!(completed.renames_reconciled, 0);
    assert_eq!(completed.hashes_computed, 0);
    assert!(
        db.entry_for_path(Path::new("unrelated-large.wav"))
            .unwrap()
            .unwrap()
            .content_hash
            .is_none(),
        "plain deletes must not trigger unrelated deferred backfill"
    );
    assert_eq!(
        db.entry_for_path(Path::new("duplicate.wav"))
            .unwrap()
            .unwrap()
            .tag,
        Rating::NEUTRAL
    );
    assert!(
        db.list_pending_renames()
            .unwrap()
            .iter()
            .any(|entry| entry.relative_path == Path::new("deleted.wav"))
    );
}

#[cfg(unix)]
#[test]
fn quick_scan_avoids_ambiguous_large_rename() {
    let dir = tempdir().unwrap();
    let first_path = dir.path().join("one.wav");
    let second_path = dir.path().join("two.wav");
    let third_path = dir.path().join("three.wav");
    let payload = vec![0u8; 9 * 1024 * 1024];
    std::fs::write(&first_path, &payload).unwrap();
    std::fs::write(&second_path, &payload).unwrap();

    let timestamp = 1_700_000_000i64;
    set_file_times(&first_path, timestamp, 0);
    set_file_times(&second_path, timestamp, 0);

    let db = SourceDatabase::open_for_scan(dir.path()).unwrap();
    hard_rescan(&db).unwrap();
    db.set_tag(Path::new("one.wav"), Rating::KEEP_1).unwrap();

    std::fs::remove_file(&first_path).unwrap();
    std::fs::remove_file(&second_path).unwrap();
    std::fs::write(&third_path, &payload).unwrap();
    set_file_times(&third_path, timestamp, 0);

    let stats = scan_once(&db).unwrap();
    assert_eq!(stats.renames_reconciled, 0);
    assert_eq!(stats.added, 1);
    assert_eq!(stats.missing, 2);

    let rows = db.list_files().unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].relative_path, Path::new("three.wav"));
    assert_eq!(rows[0].tag, Rating::NEUTRAL);
    let pending = db.list_pending_renames().unwrap();
    assert_eq!(pending.len(), 2);
    assert!(pending.iter().any(|entry| {
        entry.relative_path == Path::new("one.wav") && entry.metadata.tag == Rating::KEEP_1
    }));
}

#[cfg(any(unix, windows))]
#[test]
fn same_content_destinations_choose_unique_file_identity_across_batch_order() {
    for (moved_name, copy_name) in [("a-moved.wav", "z-copy.wav"), ("z-moved.wav", "a-copy.wav")] {
        let dir = tempdir().unwrap();
        let old = dir.path().join("old.wav");
        let moved = dir.path().join(moved_name);
        let copy = dir.path().join(copy_name);
        std::fs::write(&old, b"same-content").unwrap();
        let db = SourceDatabase::open_for_scan(dir.path()).unwrap();
        scan_once(&db).unwrap();
        db.set_tag(Path::new("old.wav"), Rating::KEEP_1).unwrap();
        db.set_user_tag(Path::new("old.wav"), Some("Original metadata"))
            .unwrap();
        insert_analysis_artifacts(dir.path(), "source::old.wav", "old.wav");

        std::fs::copy(&old, &copy).unwrap();
        std::fs::rename(&old, &moved).unwrap();
        let mut targets = vec![
            PathBuf::from("old.wav"),
            PathBuf::from(moved_name),
            PathBuf::from(copy_name),
        ];
        for index in 0..63 {
            let name = format!("m-filler-{index:02}.wav");
            std::fs::write(dir.path().join(&name), format!("filler-{index}")).unwrap();
            targets.push(PathBuf::from(name));
        }

        let stats = sync_paths(&db, &targets).unwrap();

        assert_eq!(stats.renames_reconciled, 1);
        let moved_entry = db.entry_for_path(Path::new(moved_name)).unwrap().unwrap();
        assert_eq!(moved_entry.tag, Rating::KEEP_1);
        assert_eq!(moved_entry.user_tag.as_deref(), Some("Original metadata"));
        let copy_entry = db.entry_for_path(Path::new(copy_name)).unwrap().unwrap();
        assert_eq!(copy_entry.tag, Rating::NEUTRAL);
        assert_eq!(copy_entry.user_tag, None);
        assert_eq!(
            sample_id_count(dir.path(), "features", "source::old.wav"),
            0
        );
        assert_eq!(
            sample_id_count(dir.path(), "features", &format!("source::{moved_name}")),
            1
        );
        assert_eq!(
            sample_id_count(dir.path(), "features", &format!("source::{copy_name}")),
            0
        );
    }
}

#[test]
fn ambiguous_same_content_destinations_remain_neutral_across_batches() {
    let dir = tempdir().unwrap();
    let old = dir.path().join("old.wav");
    let first = dir.path().join("a-copy.wav");
    let second = dir.path().join("z-copy.wav");
    std::fs::write(&old, b"same-content").unwrap();
    let db = SourceDatabase::open_for_scan(dir.path()).unwrap();
    scan_once(&db).unwrap();
    db.set_tag(Path::new("old.wav"), Rating::KEEP_1).unwrap();
    db.set_user_tag(Path::new("old.wav"), Some("Must remain pending"))
        .unwrap();
    insert_analysis_artifacts(dir.path(), "source::old.wav", "old.wav");

    std::fs::copy(&old, &first).unwrap();
    std::fs::copy(&old, &second).unwrap();
    std::fs::remove_file(&old).unwrap();
    let mut targets = vec![
        PathBuf::from("old.wav"),
        PathBuf::from("a-copy.wav"),
        PathBuf::from("z-copy.wav"),
    ];
    for index in 0..63 {
        let name = format!("m-filler-{index:02}.wav");
        std::fs::write(dir.path().join(&name), format!("filler-{index}")).unwrap();
        targets.push(PathBuf::from(name));
    }

    let quick = sync_paths(&db, &targets).unwrap();
    assert_eq!(quick.renames_reconciled, 0);
    let completed = complete_deferred_rename_candidates(&db, quick).unwrap();

    assert_eq!(completed.renames_reconciled, 0);
    for path in ["a-copy.wav", "z-copy.wav"] {
        let entry = db.entry_for_path(Path::new(path)).unwrap().unwrap();
        assert_eq!(entry.tag, Rating::NEUTRAL);
        assert_eq!(entry.user_tag, None);
        assert_eq!(
            sample_id_count(dir.path(), "features", &format!("source::{path}")),
            0
        );
    }
    assert!(db.list_pending_renames().unwrap().iter().any(|entry| {
        entry.relative_path == Path::new("old.wav")
            && entry.metadata.tag == Rating::KEEP_1
            && entry.metadata.user_tag.as_deref() == Some("Must remain pending")
    }));
    assert_eq!(
        sample_id_count(dir.path(), "features", "source::old.wav"),
        1
    );
}

fn insert_analysis_artifacts(root: &Path, sample_id: &str, relative_path: &str) {
    let conn = SourceDatabase::open_connection_for_background_job(root).unwrap();
    conn.execute(
        "INSERT INTO samples (
             sample_id, content_hash, size, mtime_ns, duration_seconds, sr_used, analysis_version
         ) VALUES (?1, 'hash-a', 1, 1, 1.0, 48000, 'analysis_v1_test')",
        [sample_id],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO features (sample_id, feat_version, vec_blob, light_dsp_blob, rms, computed_at)
         VALUES (?1, 1, x'00', x'00', 0.0, 1)",
        [sample_id],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO embeddings (sample_id, model_id, dim, dtype, l2_normed, vec, created_at)
         VALUES (?1, 'model', 1, 'f32', 1, x'00', 1)",
        [sample_id],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO analysis_jobs (
             sample_id, source_id, relative_path, job_type, content_hash, status, attempts, created_at
         ) VALUES (?1, 'source', ?2, 'analyze_sample', 'hash-a', 'done', 0, 1)",
        [sample_id, relative_path],
    )
    .unwrap();
}

fn sample_id_count(root: &Path, table: &str, sample_id: &str) -> i64 {
    let conn = SourceDatabase::open_connection_for_background_job(root).unwrap();
    conn.query_row(
        &format!("SELECT COUNT(*) FROM {table} WHERE sample_id = ?1"),
        [sample_id],
        |row| row.get(0),
    )
    .unwrap()
}

fn analysis_job_relative_path(root: &Path, sample_id: &str) -> String {
    let conn = SourceDatabase::open_connection_for_background_job(root).unwrap();
    conn.query_row(
        "SELECT relative_path FROM analysis_jobs WHERE sample_id = ?1",
        [sample_id],
        |row| row.get(0),
    )
    .unwrap()
}
