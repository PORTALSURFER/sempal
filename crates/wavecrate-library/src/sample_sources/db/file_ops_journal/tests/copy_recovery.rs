use super::*;

#[test]
fn reconcile_copy_from_staged_file() {
    let temp = TempDir::new().unwrap();
    let target_root = temp.path().join("target");
    std::fs::create_dir_all(&target_root).unwrap();
    let target_db = SourceDatabase::open_for_source_write(&target_root).unwrap();
    let source_path = temp.path().join("external.wav");
    write_wav(&source_path);
    let target_relative = PathBuf::from("copied.wav");
    let staged_relative = staged_relative_for_target(&target_relative, "copy").unwrap();
    let entry = FileOpJournalEntry::new_copy(
        String::from("copy-test"),
        CopyJournalEntryInit {
            target_relative: target_relative.clone(),
            staged_relative: staged_relative.clone(),
            tag: Rating::KEEP_1,
            looped: true,
            locked: true,
            last_played_at: Some(123),
            last_curated_at: Some(456),
        },
    )
    .unwrap();
    insert_entry(&target_db, &entry).unwrap();
    let staged_absolute = target_root.join(&staged_relative);
    std::fs::copy(&source_path, &staged_absolute).unwrap();
    let (file_size, modified_ns) = file_identity(&staged_absolute);
    update_stage(
        &target_db,
        &entry.id,
        FileOpStage::Staged,
        Some(file_size),
        Some(modified_ns),
    )
    .unwrap();

    let summary = reconcile_pending_ops_for_tests(&target_db).unwrap();
    assert_eq!(summary.completed, 1);
    assert!(target_root.join(&target_relative).exists());
    assert_eq!(
        target_db.tag_for_path(&target_relative).unwrap(),
        Some(Rating::KEEP_1)
    );
    assert_eq!(
        target_db.looped_for_path(&target_relative).unwrap(),
        Some(true)
    );
    assert_eq!(
        target_db.locked_for_path(&target_relative).unwrap(),
        Some(true)
    );
    assert_eq!(
        target_db.last_played_at_for_path(&target_relative).unwrap(),
        Some(123)
    );
    assert_no_journal_entries(&target_db);
}

#[test]
fn reconcile_copy_clears_stale_target_metadata_when_journal_defaults_are_empty() {
    let temp = TempDir::new().unwrap();
    let target_root = temp.path().join("target");
    std::fs::create_dir_all(&target_root).unwrap();
    let target_db = SourceDatabase::open_for_source_write(&target_root).unwrap();
    let source_path = temp.path().join("external.wav");
    write_wav(&source_path);
    let target_relative = PathBuf::from("copied.wav");
    let mut batch = target_db.write_batch().unwrap();
    batch
        .upsert_file_with_hash_and_tag(&target_relative, 8, 1, "stale-hash", Rating::KEEP_3, true)
        .unwrap();
    batch.commit().unwrap();
    target_db.set_looped(&target_relative, true).unwrap();
    target_db.set_locked(&target_relative, true).unwrap();
    target_db.set_last_played_at(&target_relative, 77).unwrap();

    let staged_relative = staged_relative_for_target(&target_relative, "copy").unwrap();
    let entry = FileOpJournalEntry::new_copy(
        String::from("copy-test"),
        CopyJournalEntryInit {
            target_relative: target_relative.clone(),
            staged_relative: staged_relative.clone(),
            tag: Rating::NEUTRAL,
            looped: false,
            locked: false,
            last_played_at: None,
            last_curated_at: None,
        },
    )
    .unwrap();
    insert_entry(&target_db, &entry).unwrap();
    let staged_absolute = target_root.join(&staged_relative);
    std::fs::copy(&source_path, &staged_absolute).unwrap();
    let (file_size, modified_ns) = file_identity(&staged_absolute);
    update_stage(
        &target_db,
        &entry.id,
        FileOpStage::Staged,
        Some(file_size),
        Some(modified_ns),
    )
    .unwrap();

    let summary = reconcile_pending_ops_for_tests(&target_db).unwrap();
    assert_eq!(summary.completed, 1);
    let restored = target_db.entry_for_path(&target_relative).unwrap().unwrap();
    assert_eq!(restored.tag, Rating::NEUTRAL);
    assert!(!restored.looped);
    assert!(!restored.locked);
    assert_eq!(restored.last_played_at, None);
    assert!(!restored.missing);
    assert_eq!(restored.content_hash, None);
    assert_no_journal_entries(&target_db);
}

#[test]
fn reconcile_copy_preserves_staged_file_when_target_path_was_reused() {
    let temp = TempDir::new().unwrap();
    let target_root = temp.path().join("target");
    std::fs::create_dir_all(&target_root).unwrap();
    let target_db = SourceDatabase::open_for_source_write(&target_root).unwrap();
    let source_path = temp.path().join("external.wav");
    write_wav(&source_path);
    let target_relative = PathBuf::from("copied.wav");
    let staged_relative = staged_relative_for_target(&target_relative, "copy").unwrap();
    let entry = FileOpJournalEntry::new_copy(
        String::from("copy-test"),
        CopyJournalEntryInit {
            target_relative: target_relative.clone(),
            staged_relative: staged_relative.clone(),
            tag: Rating::KEEP_1,
            looped: true,
            locked: true,
            last_played_at: Some(123),
            last_curated_at: Some(456),
        },
    )
    .unwrap();
    insert_entry(&target_db, &entry).unwrap();
    let staged_absolute = target_root.join(&staged_relative);
    std::fs::copy(&source_path, &staged_absolute).unwrap();
    let (file_size, modified_ns) = file_identity(&staged_absolute);
    update_stage(
        &target_db,
        &entry.id,
        FileOpStage::Staged,
        Some(file_size),
        Some(modified_ns),
    )
    .unwrap();

    std::fs::write(target_root.join(&target_relative), [7u8; 8]).unwrap();
    let mut batch = target_db.write_batch().unwrap();
    batch
        .upsert_file_with_hash_and_tag(
            &target_relative,
            8,
            2,
            "reused-hash",
            Rating::TRASH_3,
            false,
        )
        .unwrap();
    batch.commit().unwrap();
    target_db.set_looped(&target_relative, false).unwrap();
    target_db.set_locked(&target_relative, false).unwrap();
    target_db.set_last_played_at(&target_relative, 77).unwrap();

    let summary = reconcile_pending_ops_for_tests(&target_db).unwrap();
    assert_eq!(summary.completed, 0);
    assert_eq!(list_entries(&target_db).unwrap().entries.len(), 1);
    assert!(staged_absolute.exists());
    assert_eq!(
        std::fs::read(target_root.join(&target_relative)).unwrap(),
        vec![7u8; 8]
    );
    assert_eq!(
        target_db.tag_for_path(&target_relative).unwrap(),
        Some(Rating::TRASH_3)
    );
    assert_eq!(
        target_db.last_played_at_for_path(&target_relative).unwrap(),
        Some(77)
    );
    assert!(
        summary
            .errors
            .iter()
            .any(|err| err.contains("target path was reused before recovery replay")),
        "unexpected reconcile errors: {:?}",
        summary.errors
    );
}

#[test]
fn reconcile_copy_defers_when_target_exists_and_journal_identity_is_incomplete() {
    let temp = TempDir::new().unwrap();
    let target_root = temp.path().join("target");
    std::fs::create_dir_all(&target_root).unwrap();
    let target_db = SourceDatabase::open_for_source_write(&target_root).unwrap();
    let source_path = temp.path().join("external.wav");
    write_wav(&source_path);
    let target_relative = PathBuf::from("copied.wav");
    let staged_relative = staged_relative_for_target(&target_relative, "copy").unwrap();
    let entry = FileOpJournalEntry::new_copy(
        String::from("copy-test"),
        CopyJournalEntryInit {
            target_relative: target_relative.clone(),
            staged_relative: staged_relative.clone(),
            tag: Rating::KEEP_1,
            looped: true,
            locked: true,
            last_played_at: Some(123),
            last_curated_at: Some(456),
        },
    )
    .unwrap();
    insert_entry(&target_db, &entry).unwrap();
    let staged_absolute = target_root.join(&staged_relative);
    std::fs::copy(&source_path, &staged_absolute).unwrap();
    update_stage(&target_db, &entry.id, FileOpStage::Staged, None, None).unwrap();

    std::fs::write(target_root.join(&target_relative), [7u8; 8]).unwrap();
    let mut batch = target_db.write_batch().unwrap();
    batch
        .upsert_file_with_hash_and_tag(
            &target_relative,
            8,
            2,
            "reused-hash",
            Rating::TRASH_3,
            false,
        )
        .unwrap();
    batch.commit().unwrap();
    target_db.set_looped(&target_relative, false).unwrap();
    target_db.set_locked(&target_relative, false).unwrap();
    target_db.set_last_played_at(&target_relative, 77).unwrap();

    let summary = reconcile_pending_ops_for_tests(&target_db).unwrap();
    assert_eq!(summary.completed, 0);
    assert_eq!(list_entries(&target_db).unwrap().entries.len(), 1);
    assert!(staged_absolute.exists());
    assert_eq!(
        std::fs::read(target_root.join(&target_relative)).unwrap(),
        vec![7u8; 8]
    );
    assert_eq!(
        target_db.tag_for_path(&target_relative).unwrap(),
        Some(Rating::TRASH_3)
    );
    assert_eq!(
        target_db.looped_for_path(&target_relative).unwrap(),
        Some(false)
    );
    assert_eq!(
        target_db.locked_for_path(&target_relative).unwrap(),
        Some(false)
    );
    assert_eq!(
        target_db.last_played_at_for_path(&target_relative).unwrap(),
        Some(77)
    );
    assert!(
        summary
            .errors
            .iter()
            .any(|err| err.contains("journaled identity is incomplete")),
        "unexpected reconcile errors: {:?}",
        summary.errors
    );
}

#[cfg(unix)]
#[test]
fn reconcile_copy_retains_staged_file_when_final_target_is_replaced_by_symlink() {
    use std::os::unix::fs::symlink;

    let temp = TempDir::new().unwrap();
    let target_root = temp.path().join("target");
    let outside = temp.path().join("outside.wav");
    std::fs::create_dir_all(&target_root).unwrap();
    write_wav(&outside);
    let target_db = SourceDatabase::open_for_source_write(&target_root).unwrap();
    let target_relative = PathBuf::from("copied.wav");
    let staged_relative = staged_relative_for_target(&target_relative, "symlink").unwrap();
    let entry = FileOpJournalEntry::new_copy(
        String::from("copy-symlink-target"),
        CopyJournalEntryInit {
            target_relative: target_relative.clone(),
            staged_relative: staged_relative.clone(),
            tag: Rating::KEEP_1,
            looped: false,
            locked: false,
            last_played_at: None,
            last_curated_at: None,
        },
    )
    .unwrap();
    insert_entry(&target_db, &entry).unwrap();
    let staged_absolute = target_root.join(&staged_relative);
    write_wav(&staged_absolute);
    let (file_size, modified_ns) = file_identity(&staged_absolute);
    update_stage(
        &target_db,
        &entry.id,
        FileOpStage::Staged,
        Some(file_size),
        Some(modified_ns),
    )
    .unwrap();
    symlink(&outside, target_root.join(&target_relative)).unwrap();

    let summary = reconcile_pending_ops_for_tests(&target_db).unwrap();

    assert_eq!(summary.completed, 0);
    assert!(staged_absolute.exists());
    assert!(target_root.join(&target_relative).read_link().is_ok());
    assert_eq!(list_entries(&target_db).unwrap().entries.len(), 1);
    assert_eq!(std::fs::read(&outside).unwrap(), vec![0; 16]);
}

#[cfg(unix)]
#[test]
fn reconcile_copy_retries_after_ancestor_symlink_replacement() {
    use std::os::unix::fs::symlink;

    let temp = TempDir::new().unwrap();
    let target_root = temp.path().join("target");
    let outside = temp.path().join("outside");
    let moved_ancestor = temp.path().join("nested-moved");
    std::fs::create_dir_all(target_root.join("nested")).unwrap();
    std::fs::create_dir_all(&outside).unwrap();
    let target_db = SourceDatabase::open_for_source_write(&target_root).unwrap();
    let target_relative = PathBuf::from("nested/copied.wav");
    let staged_relative = staged_relative_for_target(&target_relative, "ancestor").unwrap();
    let entry = FileOpJournalEntry::new_copy(
        String::from("copy-symlink-ancestor"),
        CopyJournalEntryInit {
            target_relative: target_relative.clone(),
            staged_relative: staged_relative.clone(),
            tag: Rating::KEEP_1,
            looped: false,
            locked: false,
            last_played_at: None,
            last_curated_at: None,
        },
    )
    .unwrap();
    insert_entry(&target_db, &entry).unwrap();
    let staged_absolute = target_root.join(&staged_relative);
    write_wav(&staged_absolute);
    let (file_size, modified_ns) = file_identity(&staged_absolute);
    update_stage(
        &target_db,
        &entry.id,
        FileOpStage::Staged,
        Some(file_size),
        Some(modified_ns),
    )
    .unwrap();
    std::fs::rename(target_root.join("nested"), &moved_ancestor).unwrap();
    symlink(&outside, target_root.join("nested")).unwrap();

    let deferred = reconcile_pending_ops_for_tests(&target_db).unwrap();
    assert_eq!(deferred.completed, 0);
    assert_eq!(list_entries(&target_db).unwrap().entries.len(), 1);
    assert!(
        moved_ancestor
            .join(staged_relative.file_name().unwrap())
            .exists()
    );

    std::fs::remove_file(target_root.join("nested")).unwrap();
    std::fs::rename(&moved_ancestor, target_root.join("nested")).unwrap();
    let retried = reconcile_pending_ops_for_tests(&target_db).unwrap();
    assert_eq!(retried.completed, 1);
    assert!(target_root.join(target_relative).exists());
    assert_eq!(list_entries(&target_db).unwrap().entries.len(), 0);
}

#[cfg(unix)]
#[test]
fn reconcile_copy_retains_staged_file_when_ancestor_permission_is_uncertain() {
    use std::os::unix::fs::PermissionsExt;

    let temp = TempDir::new().unwrap();
    let target_root = temp.path().join("target");
    let nested = target_root.join("nested");
    std::fs::create_dir_all(&nested).unwrap();
    let target_db = SourceDatabase::open_for_source_write(&target_root).unwrap();
    let target_relative = PathBuf::from("nested/copied.wav");
    let staged_relative = staged_relative_for_target(&target_relative, "permission").unwrap();
    let entry = FileOpJournalEntry::new_copy(
        String::from("copy-permission-uncertainty"),
        CopyJournalEntryInit {
            target_relative: target_relative.clone(),
            staged_relative: staged_relative.clone(),
            tag: Rating::KEEP_1,
            looped: false,
            locked: false,
            last_played_at: None,
            last_curated_at: None,
        },
    )
    .unwrap();
    insert_entry(&target_db, &entry).unwrap();
    let staged_absolute = target_root.join(&staged_relative);
    write_wav(&staged_absolute);
    let (file_size, modified_ns) = file_identity(&staged_absolute);
    update_stage(
        &target_db,
        &entry.id,
        FileOpStage::Staged,
        Some(file_size),
        Some(modified_ns),
    )
    .unwrap();

    let original_mode = std::fs::metadata(&nested).unwrap().permissions().mode();
    std::fs::set_permissions(&nested, std::fs::Permissions::from_mode(0o000)).unwrap();
    if std::fs::read_dir(&nested).is_ok() {
        std::fs::set_permissions(&nested, std::fs::Permissions::from_mode(original_mode)).unwrap();
        return;
    }

    let summary = reconcile_pending_ops_for_tests(&target_db).unwrap();

    std::fs::set_permissions(&nested, std::fs::Permissions::from_mode(original_mode)).unwrap();
    assert_eq!(summary.completed, 0);
    assert!(staged_absolute.exists());
    assert_eq!(list_entries(&target_db).unwrap().entries.len(), 1);
}

#[test]
fn reconcile_copy_rejects_same_size_and_mtime_regular_target_replacement() {
    let temp = TempDir::new().unwrap();
    let target_root = temp.path().join("target");
    std::fs::create_dir_all(&target_root).unwrap();
    let target_db = SourceDatabase::open_for_source_write(&target_root).unwrap();
    let target_relative = PathBuf::from("copied.wav");
    let staged_relative = staged_relative_for_target(&target_relative, "identity").unwrap();
    let entry = FileOpJournalEntry::new_copy(
        String::from("copy-identity-replacement"),
        CopyJournalEntryInit {
            target_relative: target_relative.clone(),
            staged_relative: staged_relative.clone(),
            tag: Rating::KEEP_1,
            looped: false,
            locked: false,
            last_played_at: None,
            last_curated_at: None,
        },
    )
    .unwrap();
    insert_entry(&target_db, &entry).unwrap();
    let staged_absolute = target_root.join(&staged_relative);
    write_wav(&staged_absolute);
    let (file_size, modified_ns) = file_identity(&staged_absolute);
    update_stage(
        &target_db,
        &entry.id,
        FileOpStage::Staged,
        Some(file_size),
        Some(modified_ns),
    )
    .unwrap();
    let target_absolute = target_root.join(&target_relative);
    write_wav(&target_absolute);
    filetime::set_file_mtime(
        &target_absolute,
        filetime::FileTime::from_unix_time(
            modified_ns / 1_000_000_000,
            (modified_ns % 1_000_000_000) as u32,
        ),
    )
    .unwrap();

    let summary = reconcile_pending_ops_for_tests(&target_db).unwrap();

    assert_eq!(summary.completed, 0);
    assert!(staged_absolute.exists());
    assert_eq!(list_entries(&target_db).unwrap().entries.len(), 1);
    assert_eq!(std::fs::read(target_absolute).unwrap(), vec![0; 16]);
}
