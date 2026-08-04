use super::*;
use crate::sample_sources::scanner::UncoordinatedScanWriter;
use crate::sample_sources::scanner::scan_fs::{
    force_directory_entry_failure, force_directory_read_failure, force_file_type_failure,
};

#[test]
fn scan_add_update_and_prune_missing() {
    let dir = tempdir().unwrap();
    let file_path = dir.path().join("one.wav");
    std::fs::write(&file_path, b"one").unwrap();

    let db = SourceDatabase::open_for_scan(dir.path()).unwrap();
    let first = scan_once(&db).unwrap();
    assert_eq!(first.added, 1);
    assert_eq!(first.content_changed, 1);
    assert_eq!(first.changed_samples.len(), 1);
    let initial = db.list_files().unwrap();
    assert_eq!(initial.len(), 1);
    assert_eq!(initial[0].tag, Rating::NEUTRAL);

    std::fs::write(&file_path, b"longer-data").unwrap();
    let second = scan_once(&db).unwrap();
    assert_eq!(second.updated, 1);
    assert_eq!(second.content_changed, 1);
    assert_eq!(second.changed_samples.len(), 1);

    std::fs::remove_file(&file_path).unwrap();
    let third = scan_once(&db).unwrap();
    assert_eq!(third.missing, 1);
    assert!(db.list_files().unwrap().is_empty());
    let fourth = scan_once(&db).unwrap();
    assert_eq!(fourth.missing, 0);
    assert!(db.list_files().unwrap().is_empty());

    std::fs::write(&file_path, b"one").unwrap();
    let fifth = scan_once(&db).unwrap();
    assert_eq!(fifth.added, 1);
    assert_eq!(fifth.updated, 0);
    assert_eq!(fifth.content_changed, 1);
    assert_eq!(fifth.changed_samples.len(), 1);
    let rows = db.list_files().unwrap();
    assert_eq!(rows.len(), 1);
    assert!(!rows[0].missing);
}

#[test]
fn full_scan_preserves_an_unreadable_subtree_and_recovers_its_real_deletion() {
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
    let result = scan_once(&db);
    let ScanError::Incomplete { committed, error } = result.unwrap_err() else {
        panic!("an unreadable subtree must be retryable rather than authoritative");
    };
    assert!(error.contains("retry required"));
    assert!(committed.committed_delta.deleted.is_empty());
    assert!(
        committed
            .source_tree_snapshot
            .as_ref()
            .is_some_and(|snapshot| !snapshot.is_complete())
    );
    assert_eq!(
        db.entry_for_path(Path::new("protected/kick.wav"))
            .unwrap()
            .expect("unobserved descendant must remain indexed")
            .tag,
        Rating::KEEP_1
    );

    drop(failure);
    let recovered = scan_once(&db).expect("readable retry");
    assert_eq!(recovered.missing, 1);
    assert!(
        db.entry_for_path(Path::new("protected/kick.wav"))
            .unwrap()
            .is_none()
    );
}

#[test]
fn full_scan_preserves_an_unenumerated_subtree_after_a_directory_entry_failure() {
    let dir = tempdir().unwrap();
    let protected = dir.path().join("protected");
    std::fs::create_dir(&protected).unwrap();
    std::fs::write(protected.join("kick.wav"), b"kick").unwrap();
    // Keep one entry so the injected iterator failure is exercised after the
    // indexed audio file disappears during the simulated outage.
    std::fs::write(protected.join("notes.txt"), b"notes").unwrap();
    let db = SourceDatabase::open_for_scan(dir.path()).unwrap();
    scan_once(&db).unwrap();
    db.set_tag(Path::new("protected/kick.wav"), Rating::KEEP_1)
        .unwrap();

    std::fs::remove_file(protected.join("kick.wav")).unwrap();
    let failure = force_directory_entry_failure(&protected);
    let result = scan_once(&db);
    let ScanError::Incomplete { committed, .. } = result.unwrap_err() else {
        panic!("directory iterator failure must be retryable");
    };
    assert!(committed.committed_delta.deleted.is_empty());
    assert_eq!(
        db.entry_for_path(Path::new("protected/kick.wav"))
            .unwrap()
            .unwrap()
            .tag,
        Rating::KEEP_1
    );

    drop(failure);
    assert_eq!(scan_once(&db).unwrap().missing, 1);
}

#[test]
fn full_scan_preserves_descendants_after_an_entry_type_failure() {
    let dir = tempdir().unwrap();
    let protected = dir.path().join("protected");
    std::fs::create_dir(&protected).unwrap();
    std::fs::write(protected.join("snare.wav"), b"snare").unwrap();
    let db = SourceDatabase::open_for_scan(dir.path()).unwrap();
    scan_once(&db).unwrap();

    std::fs::remove_file(protected.join("snare.wav")).unwrap();
    let failure = force_file_type_failure(&protected);
    let result = scan_once(&db);
    let ScanError::Incomplete { committed, .. } = result.unwrap_err() else {
        panic!("entry type failure must be retryable");
    };
    assert!(committed.committed_delta.deleted.is_empty());
    assert!(
        db.entry_for_path(Path::new("protected/snare.wav"))
            .unwrap()
            .is_some()
    );

    drop(failure);
    assert_eq!(scan_once(&db).unwrap().missing, 1);
}

#[test]
fn scan_skips_analysis_when_hash_unchanged() {
    let dir = tempdir().unwrap();
    let file_path = dir.path().join("one.wav");
    std::fs::write(&file_path, b"one").unwrap();

    let db = SourceDatabase::open_for_scan(dir.path()).unwrap();
    let first = scan_once(&db).unwrap();
    assert_eq!(first.content_changed, 1);

    std::thread::sleep(Duration::from_millis(2));
    std::fs::write(&file_path, b"one").unwrap();

    let second = scan_once(&db).unwrap();
    assert_eq!(second.updated, 1);
    assert_eq!(second.content_changed, 0);
    assert!(second.changed_samples.is_empty());
}

#[test]
fn scan_backfills_missing_identity_for_unchanged_row() {
    let dir = tempdir().unwrap();
    let relative = Path::new("missing-identity.wav");
    std::fs::write(dir.path().join(relative), b"sample").unwrap();
    let db = SourceDatabase::open_for_scan(dir.path()).unwrap();
    scan_once(&db).unwrap();
    let original = db.list_manifest_entries().unwrap().remove(0);
    assert!(original.file_identity.is_some());

    let mut batch = db.write_batch().unwrap();
    batch.set_file_identity(relative, None).unwrap();
    batch.commit().unwrap();

    let stats = scan_once(&db).unwrap();
    let repaired = db.list_manifest_entries().unwrap().remove(0);

    assert_eq!(repaired.content_hash, original.content_hash);
    assert!(repaired.file_identity.is_some());
    assert!(stats.committed_delta.created.is_empty());
    assert!(stats.committed_delta.deleted.is_empty());
}

#[test]
fn scan_adds_duplicate_content_when_original_path_still_exists() {
    let dir = tempdir().unwrap();
    let first_path = dir.path().join("one.wav");
    let duplicate_path = dir.path().join("two.wav");
    std::fs::write(&first_path, b"same-content").unwrap();

    let db = SourceDatabase::open_for_scan(dir.path()).unwrap();
    let first = scan_once(&db).unwrap();
    assert_eq!(first.added, 1);

    std::fs::write(&duplicate_path, b"same-content").unwrap();
    let second = scan_once(&db).unwrap();

    assert_eq!(second.added, 1);
    assert_eq!(second.renames_reconciled, 0);
    let mut paths = db
        .list_files()
        .unwrap()
        .into_iter()
        .map(|entry| entry.relative_path.to_string_lossy().to_string())
        .collect::<Vec<_>>();
    paths.sort();
    assert_eq!(paths, vec!["one.wav", "two.wav"]);
}

#[test]
fn scan_ignores_non_wav_and_counts_nested() {
    let dir = tempdir().unwrap();
    let nested = dir.path().join("nested");
    std::fs::create_dir_all(&nested).unwrap();
    std::fs::write(dir.path().join("one.wav"), b"one").unwrap();
    std::fs::write(nested.join("two.wav"), b"two").unwrap();
    std::fs::write(dir.path().join("later.aif"), b"aif").unwrap();
    std::fs::write(dir.path().join("later.aiff"), b"aiff").unwrap();
    std::fs::write(dir.path().join("unsupported.flac"), b"flac").unwrap();
    std::fs::write(dir.path().join("unsupported.mp3"), b"mp3").unwrap();
    std::fs::write(dir.path().join("ignore.txt"), b"text").unwrap();

    let db = SourceDatabase::open_for_scan(dir.path()).unwrap();
    let stats = scan_once(&db).unwrap();
    assert_eq!(stats.added, 2);
    assert_eq!(stats.total_files, 2);
    let rows = db.list_files().unwrap();
    assert_eq!(rows.len(), 2);
}

#[test]
fn scan_includes_dot_prefixed_files_and_nested_hidden_audio_by_default() {
    let dir = tempdir().unwrap();
    let hidden = dir.path().join(".hidden");
    std::fs::create_dir(&hidden).unwrap();
    std::fs::write(dir.path().join(".kick.wav"), b"kick").unwrap();
    std::fs::write(hidden.join("ignored.wav"), b"ignored").unwrap();

    let db = SourceDatabase::open_for_scan(dir.path()).unwrap();
    let stats = scan_once(&db).unwrap();

    assert_eq!(stats.added, 2);
    assert!(db.entry_for_path(Path::new(".kick.wav")).unwrap().is_some());
    assert!(
        db.entry_for_path(Path::new(".hidden/ignored.wav"))
            .unwrap()
            .is_some()
    );
}

#[test]
fn scan_in_background_finishes() {
    let dir = tempdir().unwrap();
    std::fs::write(dir.path().join("one.wav"), b"one").unwrap();
    let handle = scan_in_background(dir.path().to_path_buf());
    let stats = handle.join().unwrap().unwrap();
    assert_eq!(stats.added, 1);
}

#[test]
fn scan_with_progress_respects_cancel_flag() {
    use std::sync::atomic::AtomicBool;

    let dir = tempdir().unwrap();
    std::fs::write(dir.path().join("one.wav"), b"one").unwrap();
    let db = SourceDatabase::open_for_scan(dir.path()).unwrap();

    let cancel = AtomicBool::new(true);
    let mut progress_called = false;
    let result = scan_with_progress(&db, ScanMode::Quick, Some(&cancel), &mut |_, _| {
        progress_called = true;
    });
    assert!(matches!(result, Err(ScanError::Canceled)));
    assert!(!progress_called);
}

#[test]
fn scan_detects_missing_paths_without_double_counting() {
    let dir = tempdir().unwrap();
    let file_path = dir.path().join("one.wav");
    std::fs::write(&file_path, b"one").unwrap();

    let db = SourceDatabase::open_for_scan(dir.path()).unwrap();
    scan_once(&db).unwrap();

    std::fs::remove_file(&file_path).unwrap();
    let first = scan_once(&db).unwrap();
    assert_eq!(first.missing, 1);

    let second = scan_once(&db).unwrap();
    assert_eq!(second.missing, 0);
}

#[test]
fn scan_detects_changed_content_hash() {
    let dir = tempdir().unwrap();
    let file_path = dir.path().join("one.wav");
    std::fs::write(&file_path, b"one").unwrap();

    let db = SourceDatabase::open_for_scan(dir.path()).unwrap();
    scan_once(&db).unwrap();

    std::fs::write(&file_path, b"two").unwrap();
    let stats = scan_once(&db).unwrap();
    assert_eq!(stats.content_changed, 1);
    assert_eq!(stats.changed_samples.len(), 1);
}

#[test]
fn scan_discovery_does_not_hold_the_source_writer() {
    let dir = tempdir().unwrap();
    std::fs::write(dir.path().join("one.wav"), b"one").unwrap();
    let db = SourceDatabase::open_for_scan(dir.path()).unwrap();
    let mut concurrent_write = None;

    scan_with_progress(&db, ScanMode::Quick, None, &mut |_, _| {
        if concurrent_write.is_some() {
            return;
        }
        std::thread::sleep(Duration::from_millis(25));
        let writer = SourceDatabase::open_for_user_metadata_write(dir.path()).unwrap();
        concurrent_write = Some(writer.set_metadata("scan_contention_probe", "complete"));
    })
    .unwrap();

    concurrent_write
        .expect("progress callback must run")
        .expect("metadata writer must complete while discovery is active");
    assert_eq!(
        db.get_metadata("scan_contention_probe").unwrap().as_deref(),
        Some("complete")
    );
}

#[test]
fn scan_revalidation_rejects_file_mutation_after_discovery() {
    let dir = tempdir().unwrap();
    let file_path = dir.path().join("one.wav");
    std::fs::write(&file_path, b"original").unwrap();
    let db = SourceDatabase::open_for_scan(dir.path()).unwrap();
    scan_once(&db).unwrap();
    let before = db.list_files().unwrap().remove(0);
    let mut mutated = false;

    let stale_scan = scan_with_progress(&db, ScanMode::Quick, None, &mut |_, _| {
        if !mutated {
            std::fs::write(&file_path, b"replacement-with-different-size").unwrap();
            mutated = true;
        }
    })
    .unwrap();

    assert!(mutated);
    assert_eq!(stale_scan.updated, 0);
    assert_eq!(stale_scan.missing, 0);
    let after_stale_scan = db.list_files().unwrap().remove(0);
    assert_eq!(after_stale_scan.file_size, before.file_size);
    assert_eq!(after_stale_scan.modified_ns, before.modified_ns);
    assert_eq!(after_stale_scan.content_hash, before.content_hash);

    let fresh_scan = scan_once(&db).unwrap();
    assert_eq!(fresh_scan.updated, 1);
    assert_ne!(db.list_files().unwrap()[0].file_size, before.file_size);
}

#[test]
fn scan_refreshes_noop_row_after_concurrent_removal() {
    let dir = tempdir().unwrap();
    let file_path = dir.path().join("one.wav");
    std::fs::write(&file_path, b"one").unwrap();
    let db = SourceDatabase::open_for_scan(dir.path()).unwrap();
    scan_once(&db).unwrap();
    let mut removed = false;

    scan_with_progress(&db, ScanMode::Quick, None, &mut |_, _| {
        if removed {
            return;
        }
        let writer = SourceDatabase::open_for_scan(dir.path()).unwrap();
        let mut batch = writer.write_batch().unwrap();
        batch.remove_file(Path::new("one.wav")).unwrap();
        batch.commit().unwrap();
        removed = true;
    })
    .unwrap();

    assert!(removed);
    assert!(db.entry_for_path(Path::new("one.wav")).unwrap().is_some());
}

#[test]
fn missing_stage_keeps_concurrently_restored_live_row() {
    let dir = tempdir().unwrap();
    let file_path = dir.path().join("restored.wav");
    std::fs::write(&file_path, b"old").unwrap();
    let db = SourceDatabase::open_for_scan(dir.path()).unwrap();
    scan_once(&db).unwrap();
    let stale = db
        .entry_for_path(Path::new("restored.wav"))
        .unwrap()
        .unwrap();
    std::fs::remove_file(&file_path).unwrap();

    std::fs::write(&file_path, b"new-longer").unwrap();
    let facts = super::super::super::scan_fs::read_facts(dir.path(), &file_path).unwrap();
    db.upsert_file(Path::new("restored.wav"), facts.size, facts.modified_ns)
        .unwrap();
    let mut stats = ScanStats::default();
    let mut batch = db.write_batch().unwrap();

    super::super::super::scan_diff::mark_missing(
        &db,
        db.source_traversal_policy().unwrap(),
        &mut batch,
        [stale],
        &mut stats,
    )
    .unwrap();
    batch.commit().unwrap();

    assert_eq!(stats.missing, 0);
    let restored = db
        .entry_for_path(Path::new("restored.wav"))
        .unwrap()
        .unwrap();
    assert_eq!(restored.file_size, facts.size);
    assert_eq!(restored.modified_ns, facts.modified_ns);
}

#[test]
fn missing_stage_prunes_path_replaced_by_directory() {
    let dir = tempdir().unwrap();
    let file_path = dir.path().join("replaced.wav");
    std::fs::write(&file_path, b"old").unwrap();
    let db = SourceDatabase::open_for_scan(dir.path()).unwrap();
    scan_once(&db).unwrap();
    std::fs::remove_file(&file_path).unwrap();
    std::fs::create_dir(&file_path).unwrap();

    let stats = scan_once(&db).unwrap();

    assert_eq!(stats.missing, 1);
    assert!(
        db.entry_for_path(Path::new("replaced.wav"))
            .unwrap()
            .is_none()
    );
}

#[test]
fn configured_hidden_directory_exclusion_prunes_tracked_files() {
    let dir = tempdir().unwrap();
    let hidden = dir.path().join(".hidden");
    std::fs::create_dir(&hidden).unwrap();
    std::fs::write(hidden.join("one.wav"), b"hidden").unwrap();
    let db = SourceDatabase::open_for_scan(dir.path()).unwrap();
    db.upsert_file(Path::new(".hidden/one.wav"), 6, 1).unwrap();
    db.set_source_traversal_policy(
        wavecrate_library::sample_sources::SourceTraversalPolicy::exclude_hidden_directories(),
    )
    .unwrap();

    let stats = scan_once(&db).unwrap();

    assert_eq!(stats.missing, 1);
    assert!(
        db.entry_for_path(Path::new(".hidden/one.wav"))
            .unwrap()
            .is_none()
    );
}

#[test]
fn unsupported_replacement_after_discovery_remains_eligible_for_pruning() {
    let dir = tempdir().unwrap();
    let file_path = dir.path().join("one.wav");
    std::fs::write(&file_path, b"old").unwrap();
    let db = SourceDatabase::open_for_scan(dir.path()).unwrap();
    scan_once(&db).unwrap();
    std::fs::write(&file_path, b"changed-size").unwrap();
    let mut replaced = false;

    let stats = scan_with_progress(&db, ScanMode::Quick, None, &mut |_, _| {
        if replaced {
            return;
        }
        std::fs::remove_file(&file_path).unwrap();
        std::fs::create_dir(&file_path).unwrap();
        replaced = true;
    })
    .unwrap();

    assert!(replaced);
    assert_eq!(stats.missing, 1);
    assert!(db.entry_for_path(Path::new("one.wav")).unwrap().is_none());
}

#[test]
fn scan_rebases_noop_when_concurrent_writer_clears_hash() {
    let dir = tempdir().unwrap();
    let file_path = dir.path().join("one.wav");
    std::fs::write(&file_path, b"one").unwrap();
    let db = SourceDatabase::open_for_scan(dir.path()).unwrap();
    scan_once(&db).unwrap();
    let row = db.entry_for_path(Path::new("one.wav")).unwrap().unwrap();
    let mut cleared = false;

    let stats = scan_with_progress(&db, ScanMode::Quick, None, &mut |_, _| {
        if cleared {
            return;
        }
        let writer = SourceDatabase::open_for_scan(dir.path()).unwrap();
        let mut batch = writer.write_batch().unwrap();
        batch
            .upsert_file_without_hash(Path::new("one.wav"), row.file_size, row.modified_ns)
            .unwrap();
        batch.commit().unwrap();
        cleared = true;
    })
    .unwrap();

    assert!(cleared);
    assert_eq!(stats.hashes_computed, 1);
    assert!(
        db.entry_for_path(Path::new("one.wav"))
            .unwrap()
            .unwrap()
            .content_hash
            .is_some()
    );
}

#[test]
fn missing_repair_survives_concurrent_hash_clear() {
    let dir = tempdir().unwrap();
    let file_path = dir.path().join("one.wav");
    std::fs::write(&file_path, b"one").unwrap();
    let db = SourceDatabase::open_for_scan(dir.path()).unwrap();
    scan_once(&db).unwrap();
    let row = db.entry_for_path(Path::new("one.wav")).unwrap().unwrap();
    db.set_missing(Path::new("one.wav"), true).unwrap();
    let mut cleared = false;

    let stats = scan_with_progress(&db, ScanMode::Quick, None, &mut |_, _| {
        if cleared {
            return;
        }
        let writer = SourceDatabase::open_for_scan(dir.path()).unwrap();
        let mut batch = writer.write_batch().unwrap();
        batch
            .upsert_file_without_hash(Path::new("one.wav"), row.file_size, row.modified_ns)
            .unwrap();
        batch.commit().unwrap();
        cleared = true;
    })
    .unwrap();

    assert!(cleared);
    assert_eq!(stats.hashes_computed, 1);
    let repaired = db.entry_for_path(Path::new("one.wav")).unwrap().unwrap();
    assert!(!repaired.missing);
    assert!(repaired.content_hash.is_some());
}

#[cfg(unix)]
#[test]
fn scan_hashes_current_bytes_when_facts_are_preserved_after_discovery() {
    let dir = tempdir().unwrap();
    let file_path = dir.path().join("one.wav");
    std::fs::write(&file_path, b"old!").unwrap();
    let timestamp = 1_700_000_000;
    set_file_times(&file_path, timestamp, 0);
    let db = SourceDatabase::open_for_scan(dir.path()).unwrap();
    let mut replaced = false;

    scan_with_progress(&db, ScanMode::Quick, None, &mut |_, _| {
        if !replaced {
            std::fs::write(&file_path, b"new!").unwrap();
            set_file_times(&file_path, timestamp, 0);
            replaced = true;
        }
    })
    .unwrap();

    let row = db.entry_for_path(Path::new("one.wav")).unwrap().unwrap();
    assert_eq!(
        row.content_hash.as_deref(),
        Some(blake3::hash(b"new!").to_hex().as_str())
    );
}

#[test]
fn cancellation_after_first_committed_batch_stops_at_a_resumable_checkpoint() {
    use std::sync::atomic::{AtomicBool, Ordering};

    let dir = tempdir().unwrap();
    for index in 0..70 {
        std::fs::write(dir.path().join(format!("sample-{index:03}.wav")), b"x").unwrap();
    }
    let db = SourceDatabase::open_for_scan(dir.path()).unwrap();
    let cancel = AtomicBool::new(false);

    let result = scan_with_progress(&db, ScanMode::Quick, Some(&cancel), &mut |count, _| {
        if count == 65 {
            cancel.store(true, Ordering::Relaxed);
        }
    });

    let ScanError::Incomplete { committed, error } = result.unwrap_err() else {
        panic!("cancellation after a commit must return the checkpoint outcome");
    };
    let partial = *committed;
    assert_eq!(partial.committed_delta.created.len(), 64);
    assert!(partial.committed_delta.revision > 0);
    assert_eq!(error, "Scan canceled");
    assert_eq!(db.count_files().unwrap(), 64);

    cancel.store(false, Ordering::Relaxed);
    let resumed = scan_with_progress(&db, ScanMode::Quick, Some(&cancel), &mut |_, _| {})
        .expect("a later scan must resume from the partial checkpoint");
    assert_eq!(resumed.total_files, 70);
    assert_eq!(db.count_files().unwrap(), 70);
}

#[test]
fn interrupted_manifest_audit_resumes_checked_paths_and_finishes_deletion_reconciliation() {
    use std::sync::atomic::{AtomicBool, Ordering};

    let dir = tempdir().unwrap();
    for index in 0..70 {
        std::fs::write(dir.path().join(format!("sample-{index:03}.wav")), b"x").unwrap();
    }
    let db = SourceDatabase::open_for_scan(dir.path()).unwrap();
    let cancel = AtomicBool::new(false);

    let first =
        audit_source_and_record_with_progress(&db, Some(&cancel), 0, 100, &mut |checked, _| {
            if checked >= 64 {
                cancel.store(true, Ordering::Release);
            }
        });
    assert!(matches!(first, Err(ScanError::Incomplete { .. })));
    let checked = db
        .begin_or_resume_manifest_audit(101)
        .expect("load durable audit checkpoint");
    assert_eq!(checked.len(), 64);

    std::fs::remove_file(dir.path().join(&checked[0])).unwrap();
    cancel.store(false, Ordering::Release);
    let mut resumed_progress = Vec::new();
    let resumed =
        audit_source_and_record_with_progress(&db, Some(&cancel), 0, 200, &mut |checked, _| {
            resumed_progress.push(checked)
        })
        .expect("resume interrupted manifest audit");

    assert_eq!(resumed_progress.first().copied(), Some(64));
    assert_eq!(resumed.total_files, 70);
    assert!(
        db.entry_for_path(&checked[0]).unwrap().is_none(),
        "a path deleted after its checkpoint must still be reconciled at cycle completion"
    );
    assert!(
        db.begin_or_resume_manifest_audit(201)
            .expect("new audit cycle")
            .is_empty(),
        "completed audit must clear its durable checkpoint"
    );
    assert_eq!(
        db.get_metadata(crate::sample_sources::db::META_LAST_MANIFEST_AUDIT_AT)
            .unwrap()
            .as_deref(),
        Some("200")
    );
}

#[test]
fn manifest_audit_progress_is_published_after_durable_checkpoint() {
    let dir = tempdir().unwrap();
    let relative = Path::new("missed.wav");
    std::fs::write(dir.path().join(relative), b"missed watcher file").unwrap();
    let db = SourceDatabase::open_for_scan(dir.path()).unwrap();
    let mut progress = Vec::new();

    audit_source_and_record_with_progress(&db, None, 0, 100, &mut |checked, path| {
        let (checkpointed_paths, persisted_checked) = db
            .begin_or_resume_manifest_audit_batch(101, usize::MAX)
            .expect("read the checkpoint while publishing progress");
        progress.push((
            checked,
            persisted_checked,
            path.to_path_buf(),
            checkpointed_paths,
        ));
    })
    .expect("manifest audit should complete");

    assert_eq!(progress.len(), 1);
    let (checked, persisted_checked, path, checkpointed_paths) = &progress[0];
    assert_eq!(*checked, 1);
    assert_eq!(*persisted_checked, 1);
    assert_eq!(path, relative);
    assert_eq!(checkpointed_paths, &vec![relative.to_path_buf()]);
}

#[test]
fn interrupted_manifest_audit_revalidates_a_checkpointed_file() {
    use std::sync::atomic::{AtomicBool, Ordering};

    let dir = tempdir().unwrap();
    for index in 0..70 {
        std::fs::write(dir.path().join(format!("sample-{index:03}.wav")), b"x").unwrap();
    }
    let db = SourceDatabase::open_for_scan(dir.path()).unwrap();
    let cancel = AtomicBool::new(false);

    let first =
        audit_source_and_record_with_progress(&db, Some(&cancel), 0, 100, &mut |checked, _| {
            if checked >= 64 {
                cancel.store(true, Ordering::Release);
            }
        });
    assert!(matches!(first, Err(ScanError::Incomplete { .. })));

    let checked = db
        .begin_or_resume_manifest_audit(101)
        .expect("load durable audit checkpoint");
    assert_eq!(checked.len(), 64);
    let relative = &checked[0];
    let path = dir.path().join(relative);
    let original_modified = std::fs::metadata(&path).unwrap().modified().unwrap();
    let original_entry = db
        .entry_for_path(relative)
        .unwrap()
        .expect("checkpointed path is indexed");
    let original_modified_ns = original_entry.modified_ns;
    let original_hash = original_entry
        .content_hash
        .expect("small fixture is hashed during the quick scan");

    std::fs::write(&path, b"y").unwrap();
    let file = std::fs::OpenOptions::new().write(true).open(&path).unwrap();
    file.set_times(std::fs::FileTimes::new().set_modified(original_modified))
        .unwrap();
    cancel.store(false, Ordering::Release);
    let resumed = audit_source_and_record_with_progress(&db, Some(&cancel), 0, 200, &mut |_, _| {})
        .expect("resume interrupted manifest audit");
    let entry = db
        .entry_for_path(relative)
        .unwrap()
        .expect("checkpointed path remains indexed");

    assert_eq!(entry.file_size, 1);
    assert_eq!(entry.modified_ns, original_modified_ns);
    assert_ne!(entry.content_hash.as_deref(), Some(original_hash.as_str()));
    assert_eq!(resumed.updated, 1);
    assert!(resumed.content_changed >= 1);
    assert!(
        resumed
            .committed_delta
            .changed
            .iter()
            .any(|changed| changed.relative_path == *relative)
    );
    assert_eq!(
        db.get_metadata(crate::sample_sources::db::META_LAST_MANIFEST_AUDIT_AT)
            .unwrap()
            .as_deref(),
        Some("200")
    );
}

#[test]
fn interrupted_manifest_audit_revalidates_checkpointed_paths_in_bounded_slices() {
    use std::sync::atomic::{AtomicBool, Ordering};

    let dir = tempdir().unwrap();
    for index in 0..128 {
        std::fs::write(dir.path().join(format!("sample-{index:03}.wav")), b"x").unwrap();
    }
    let db = SourceDatabase::open_for_scan(dir.path()).unwrap();
    let cancel = AtomicBool::new(false);
    let first =
        audit_source_and_record_with_progress(&db, Some(&cancel), 0, 100, &mut |checked, _| {
            if checked >= 128 {
                cancel.store(true, Ordering::Release);
            }
        });
    assert!(matches!(first, Err(ScanError::Incomplete { .. })));

    let checked = db
        .begin_or_resume_manifest_audit(101)
        .expect("load durable audit checkpoint");
    assert_eq!(checked.len(), 128);
    let relative = &checked[0];
    let path = dir.path().join(relative);
    let original_hash = db
        .entry_for_path(relative)
        .unwrap()
        .expect("checkpointed path is indexed")
        .content_hash
        .expect("small fixture is hashed during the quick scan");
    std::fs::write(&path, b"y").unwrap();

    let resumed = audit_source_and_record(&db, None, 0, 200);
    assert!(matches!(resumed, Err(ScanError::Incomplete { .. })));
    assert_eq!(
        db.begin_or_resume_manifest_audit(201)
            .expect("load remaining bounded checkpoint work")
            .len(),
        64
    );
    assert_ne!(
        db.entry_for_path(relative)
            .unwrap()
            .unwrap()
            .content_hash
            .as_deref(),
        Some(original_hash.as_str())
    );
    assert!(
        db.get_metadata(crate::sample_sources::db::META_LAST_MANIFEST_AUDIT_AT)
            .unwrap()
            .is_none(),
        "audit completion must remain pending while checkpoint slices remain"
    );

    audit_source_and_record(&db, None, 0, 300).expect("finish remaining checkpoint slice");
    assert_eq!(
        db.get_metadata(crate::sample_sources::db::META_LAST_MANIFEST_AUDIT_AT)
            .unwrap()
            .as_deref(),
        Some("300")
    );
}

#[test]
fn cancellation_after_walk_skips_missing_reconciliation_and_completion_publish() {
    use std::sync::atomic::{AtomicBool, Ordering};

    let dir = tempdir().unwrap();
    for index in 0..70 {
        std::fs::write(dir.path().join(format!("sample-{index:03}.wav")), b"x").unwrap();
    }
    let db = SourceDatabase::open_for_scan(dir.path()).unwrap();
    scan_once(&db).unwrap();
    std::fs::remove_file(dir.path().join("sample-000.wav")).unwrap();
    let cancel = AtomicBool::new(false);

    let result = scan_with_progress(&db, ScanMode::Quick, Some(&cancel), &mut |count, _| {
        if count == 69 {
            cancel.store(true, Ordering::Relaxed);
        }
    });

    assert!(matches!(result, Err(ScanError::Canceled)));
    assert!(
        db.entry_for_path(Path::new("sample-000.wav"))
            .unwrap()
            .is_some(),
        "cancellation before database reconciliation must leave missing rows for the next sweep"
    );

    cancel.store(false, Ordering::Relaxed);
    scan_with_progress(&db, ScanMode::Quick, Some(&cancel), &mut |_, _| {})
        .expect("a later scan must finish missing-row reconciliation");
    assert!(
        db.entry_for_path(Path::new("sample-000.wav"))
            .unwrap()
            .is_none()
    );
}

#[test]
fn unchanged_large_scan_only_commits_completion_metadata() {
    let dir = tempdir().unwrap();
    for index in 0..130 {
        std::fs::write(dir.path().join(format!("sample-{index:03}.wav")), b"x").unwrap();
    }
    let db = SourceDatabase::open_for_scan(dir.path()).unwrap();
    scan_once(&db).unwrap();
    let revision_before = db
        .get_metadata("revision")
        .unwrap()
        .unwrap()
        .parse::<u64>()
        .unwrap();

    let stats = scan_once(&db).unwrap();

    let revision_after = db
        .get_metadata("revision")
        .unwrap()
        .unwrap()
        .parse::<u64>()
        .unwrap();
    assert_eq!(stats.updated, 0);
    assert_eq!(stats.added, 0);
    assert_eq!(revision_after, revision_before + 1);
}

#[test]
fn bounded_manifest_audit_repairs_same_size_closed_app_edit() {
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
    let stats = audit_source_and_record(&db, None, 8, 1_234).unwrap();
    let current_hash = db
        .entry_for_path(Path::new("same.wav"))
        .unwrap()
        .unwrap()
        .content_hash
        .unwrap();

    assert_ne!(current_hash, original_hash);
    assert_eq!(stats.committed_delta.changed.len(), 1);
    assert_eq!(stats.hashes_computed, 1);
    assert_eq!(
        db.get_metadata(crate::sample_sources::db::META_LAST_MANIFEST_AUDIT_AT)
            .unwrap()
            .as_deref(),
        Some("1234")
    );
    assert_eq!(stats.committed_delta.revision, db.get_revision().unwrap());
}

#[test]
fn manifest_audit_keeps_an_unreadable_subtree_due_for_retry() {
    let dir = tempdir().unwrap();
    let protected = dir.path().join("protected");
    std::fs::create_dir(&protected).unwrap();
    std::fs::write(protected.join("kick.wav"), b"kick").unwrap();
    let db = SourceDatabase::open_for_scan(dir.path()).unwrap();
    scan_once(&db).unwrap();

    std::fs::remove_file(protected.join("kick.wav")).unwrap();
    let failure = force_directory_read_failure(&protected);
    let result = audit_source_and_record(&db, None, 8, 1_234);
    let ScanError::Incomplete { committed, error } = result.unwrap_err() else {
        panic!("partial manifest audit must remain retryable");
    };
    assert!(error.contains("retry required"));
    assert!(committed.committed_delta.deleted.is_empty());
    assert!(
        db.get_metadata(crate::sample_sources::db::META_LAST_MANIFEST_AUDIT_AT)
            .unwrap()
            .is_none()
    );

    drop(failure);
    let recovered = audit_source_and_record(&db, None, 8, 1_234).unwrap();
    assert_eq!(recovered.missing, 1);
    assert_eq!(
        db.get_metadata(crate::sample_sources::db::META_LAST_MANIFEST_AUDIT_AT)
            .unwrap()
            .as_deref(),
        Some("1234")
    );
}

#[test]
fn typed_manifest_audit_outcome_retains_coverage_barrier_for_unreadable_traversal() {
    let dir = tempdir().unwrap();
    let protected = dir.path().join("protected");
    std::fs::create_dir(&protected).unwrap();
    std::fs::write(protected.join("kick.wav"), b"kick").unwrap();
    let db = SourceDatabase::open_for_scan(dir.path()).unwrap();
    scan_once(&db).unwrap();

    std::fs::remove_file(protected.join("kick.wav")).unwrap();
    let failure = force_directory_read_failure(&protected);
    let outcome = audit_source_and_record_with_budget_and_progress_and_writer_outcome(
        &db,
        None,
        ContentAuditBudget::entry_limited(8),
        1_234,
        &mut |_, _| {},
        &UncoordinatedScanWriter,
    )
    .unwrap();
    drop(failure);

    let ManifestAuditOutcome::Incomplete {
        committed, error, ..
    } = outcome
    else {
        panic!("uncertain traversal must retain the manifest coverage barrier");
    };
    assert!(error.contains("retry required"));
    assert!(committed.committed_delta.deleted.is_empty());
    assert!(
        db.get_metadata(crate::sample_sources::db::META_LAST_MANIFEST_AUDIT_AT)
            .unwrap()
            .is_none(),
        "incomplete traversal must not publish authoritative audit completion"
    );
}

#[test]
fn manifest_audit_publishes_scan_repair_when_content_verification_is_cancelled() {
    let dir = tempdir().unwrap();
    let db = SourceDatabase::open_for_scan(dir.path()).unwrap();
    std::fs::write(dir.path().join("missed.wav"), b"missed watcher event").unwrap();
    let cancel = std::sync::atomic::AtomicBool::new(false);
    let generation_before = db
        .pending_rename_diagnostics()
        .unwrap()
        .authoritative_generation;

    let result = audit_source_and_record_with_post_scan_hook(&db, Some(&cancel), 8, 1_234, || {
        cancel.store(true, std::sync::atomic::Ordering::Release)
    });
    let ScanError::Incomplete { committed, error } = result.unwrap_err() else {
        panic!("cancelled verification must return the committed manifest repair");
    };
    let stats = *committed;

    assert_eq!(error, "Scan canceled");
    assert_eq!(stats.committed_delta.created.len(), 1);
    assert_eq!(
        stats.committed_delta.created[0].relative_path,
        Path::new("missed.wav")
    );
    assert_eq!(
        db.get_metadata(crate::sample_sources::db::META_LAST_MANIFEST_AUDIT_AT)
            .unwrap()
            .as_deref(),
        Some("1234"),
        "manifest traversal completion is independent from content coverage"
    );
    let coverage = db.content_audit_report(1_234).unwrap();
    assert_eq!(coverage.remaining_entries, 1);
    assert_eq!(coverage.verified_entries, 0);
    assert_eq!(
        db.pending_rename_diagnostics()
            .unwrap()
            .authoritative_generation,
        generation_before,
        "failed audit verification must not authorize retention pruning"
    );
}

#[test]
fn typed_manifest_audit_outcome_keeps_coverage_complete_when_content_pauses() {
    let dir = tempdir().unwrap();
    let db = SourceDatabase::open_for_scan(dir.path()).unwrap();
    std::fs::write(dir.path().join("missed.wav"), b"missed watcher event").unwrap();
    let cancel = std::sync::atomic::AtomicBool::new(false);

    let outcome =
        audit_source_and_record_with_post_scan_hook_outcome(&db, Some(&cancel), 8, 1_234, || {
            cancel.store(true, std::sync::atomic::Ordering::Release)
        })
        .unwrap();

    let ManifestAuditOutcome::Complete {
        stats,
        content_incomplete,
        ..
    } = outcome
    else {
        panic!("content verification pause must not downgrade manifest coverage");
    };
    assert_eq!(content_incomplete.as_deref(), Some("Scan canceled"));
    assert_eq!(stats.committed_delta.created.len(), 1);
    assert_eq!(
        db.get_metadata(crate::sample_sources::db::META_LAST_MANIFEST_AUDIT_AT)
            .unwrap()
            .as_deref(),
        Some("1234")
    );
}

#[test]
fn typed_manifest_audit_outcome_retains_barrier_for_noncheckpoint_verification_error() {
    let dir = tempdir().unwrap();
    let db = SourceDatabase::open_for_scan(dir.path()).unwrap();
    std::fs::write(dir.path().join("missed.wav"), b"missed watcher event").unwrap();
    let displaced_parent = tempdir().unwrap();
    let displaced = displaced_parent.path().join("displaced-root");
    let source_root = dir.path().to_path_buf();

    let outcome = audit_source_and_record_with_post_scan_hook_outcome(&db, None, 8, 1_234, || {
        std::fs::rename(&source_root, &displaced).unwrap();
        std::fs::create_dir(&source_root).unwrap();
    })
    .unwrap();

    std::fs::remove_dir(&source_root).unwrap();
    std::fs::rename(&displaced, &source_root).unwrap();

    let ManifestAuditOutcome::Incomplete {
        committed, error, ..
    } = outcome
    else {
        panic!("non-checkpoint verification errors must retain the recovery barrier");
    };
    assert!(error.contains("generation changed"));
    assert_eq!(committed.committed_delta.created.len(), 1);
    assert!(committed.content_audit.is_none());
    assert_eq!(
        db.get_metadata(crate::sample_sources::db::META_LAST_MANIFEST_AUDIT_AT)
            .unwrap()
            .as_deref(),
        Some("1234"),
        "manifest repair may be durable, but the caller must retry the failed verification"
    );
}

#[test]
fn manifest_audit_publishes_unchanged_committed_revision_when_verification_is_cancelled() {
    let dir = tempdir().unwrap();
    std::fs::write(dir.path().join("known.wav"), b"known").unwrap();
    let db = SourceDatabase::open_for_scan(dir.path()).unwrap();
    scan_once(&db).unwrap();
    let cancel = std::sync::atomic::AtomicBool::new(false);
    let generation_before = db
        .pending_rename_diagnostics()
        .unwrap()
        .authoritative_generation;

    let result = audit_source_and_record_with_post_scan_hook(&db, Some(&cancel), 8, 1_234, || {
        cancel.store(true, std::sync::atomic::Ordering::Release)
    });
    let ScanError::Incomplete { committed, error } = result.unwrap_err() else {
        panic!("cancelled verification must return the committed unchanged checkpoint");
    };

    assert_eq!(error, "Scan canceled");
    assert!(committed.committed_delta.is_empty());
    assert_eq!(
        committed.committed_delta.revision,
        db.get_revision().unwrap()
    );
    assert!(committed.committed_delta.revision > 0);
    assert_eq!(
        db.pending_rename_diagnostics()
            .unwrap()
            .authoritative_generation,
        generation_before
    );
}

#[cfg(unix)]
#[test]
fn manifest_audit_root_swap_after_scan_returns_committed_checkpoint() {
    let parent = tempdir().unwrap();
    let root = parent.path().join("source");
    std::fs::create_dir(&root).unwrap();
    std::fs::write(root.join("known.wav"), b"known").unwrap();
    let db = SourceDatabase::open_for_scan(&root).unwrap();

    let old_root = parent.path().join("old-source");
    let result = audit_source_and_record_with_pre_record_hook(&db, None, 0, 1_234, || {
        std::fs::rename(&root, &old_root).unwrap();
        std::fs::create_dir(&root).unwrap();
    });
    let ScanError::Incomplete { committed, error } = result.unwrap_err() else {
        panic!("stale audit completion must return the committed checkpoint");
    };

    assert!(error.contains("Source root generation changed"));
    assert_eq!(committed.committed_delta.created.len(), 1);
    assert!(db.entry_for_path(Path::new("known.wav")).unwrap().is_some());
    assert!(
        db.get_metadata(crate::sample_sources::db::META_LAST_MANIFEST_AUDIT_AT)
            .unwrap()
            .is_none()
    );
}

#[test]
fn skipped_existing_file_is_not_used_as_a_rename_source() {
    let dir = tempdir().unwrap();
    let hidden = dir.path().join(".hidden");
    std::fs::create_dir(&hidden).unwrap();
    std::fs::write(hidden.join("old.wav"), b"same").unwrap();
    std::fs::write(dir.path().join("new.wav"), b"same").unwrap();
    let db = SourceDatabase::open_for_scan(dir.path()).unwrap();
    let facts =
        super::super::super::scan_fs::read_facts(dir.path(), &hidden.join("old.wav")).unwrap();
    let hash = blake3::hash(b"same").to_hex().to_string();
    let mut batch = db.write_batch().unwrap();
    batch
        .upsert_file_with_hash(
            Path::new(".hidden/old.wav"),
            facts.size,
            facts.modified_ns,
            &hash,
        )
        .unwrap();
    batch
        .set_tag(Path::new(".hidden/old.wav"), Rating::KEEP_1)
        .unwrap();
    batch.commit().unwrap();

    let stats = scan_once(&db).unwrap();

    assert_eq!(stats.renames_reconciled, 0);
    let new_entry = db.entry_for_path(Path::new("new.wav")).unwrap().unwrap();
    assert_eq!(new_entry.tag, Rating::NEUTRAL);
}
