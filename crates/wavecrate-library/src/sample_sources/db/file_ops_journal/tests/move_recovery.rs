use super::*;

#[test]
fn reconcile_move_from_staged_file() {
    let fixture = MoveRecoveryFixture::new();
    std::fs::rename(fixture.source_absolute(), fixture.staged_absolute()).unwrap();
    let (file_size, modified_ns) = file_identity(&fixture.staged_absolute());
    update_stage(
        &fixture.target_db,
        &fixture.entry.id,
        FileOpStage::Staged,
        Some(file_size),
        Some(modified_ns),
    )
    .unwrap();

    let summary = reconcile_pending_ops_for_tests(&fixture.target_db).unwrap();
    assert_eq!(summary.completed, 1);
    assert!(!fixture.staged_absolute().exists());
    assert!(fixture.target_absolute().exists());
    assert!(
        fixture
            .source_db
            .tag_for_path(&fixture.source_relative)
            .unwrap()
            .is_none()
    );
    assert_target_metadata_preserved(&fixture);
    assert_no_journal_entries(&fixture.target_db);
}

#[test]
fn reconcile_same_source_move_from_staged_file() {
    let temp = TempDir::new().unwrap();
    let source_root = temp.path().join("source");
    std::fs::create_dir_all(&source_root).unwrap();
    let db = SourceDatabase::open_for_source_write(&source_root).unwrap();
    let source_relative = PathBuf::from("one.wav");
    let source_absolute = source_root.join(&source_relative);
    write_wav(&source_absolute);
    db.upsert_file(&source_relative, 16, 1).unwrap();
    db.set_tag(&source_relative, Rating::KEEP_1).unwrap();
    db.set_looped(&source_relative, true).unwrap();
    db.set_locked(&source_relative, true).unwrap();
    db.set_last_played_at(&source_relative, 123).unwrap();
    let target_relative = PathBuf::from("moved.wav");
    let staged_relative = staged_relative_for_target(&target_relative, "test").unwrap();
    let entry = FileOpJournalEntry::new_move(
        String::from("move-test"),
        MoveJournalEntryInit {
            source_root: source_root.clone(),
            source_relative: source_relative.clone(),
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
    insert_entry(&db, &entry).unwrap();
    let staged_absolute = source_root.join(&staged_relative);
    std::fs::rename(&source_absolute, &staged_absolute).unwrap();
    let (file_size, modified_ns) = file_identity(&staged_absolute);
    update_stage(
        &db,
        &entry.id,
        FileOpStage::Staged,
        Some(file_size),
        Some(modified_ns),
    )
    .unwrap();

    let summary = reconcile_pending_ops_for_tests(&db).unwrap();
    assert_eq!(summary.completed, 1);
    assert!(!staged_absolute.exists());
    assert!(source_root.join(&target_relative).exists());
    assert!(db.tag_for_path(&source_relative).unwrap().is_none());
    assert_eq!(
        db.tag_for_path(&target_relative).unwrap(),
        Some(Rating::KEEP_1)
    );
    assert_eq!(db.looped_for_path(&target_relative).unwrap(), Some(true));
    assert_eq!(db.locked_for_path(&target_relative).unwrap(), Some(true));
    assert_eq!(
        db.last_played_at_for_path(&target_relative).unwrap(),
        Some(123)
    );
    assert_no_journal_entries(&db);
}

#[test]
fn reconcile_target_db_stage_removes_orphaned_source_row() {
    let fixture = MoveRecoveryFixture::new();
    std::fs::rename(fixture.source_absolute(), fixture.target_absolute()).unwrap();
    fixture.stage_target_db();

    let summary = reconcile_pending_ops_for_tests(&fixture.target_db).unwrap();
    assert_eq!(summary.completed, 1);
    assert!(fixture.target_absolute().exists());
    assert!(
        fixture
            .source_db
            .tag_for_path(&fixture.source_relative)
            .unwrap()
            .is_none()
    );
    assert_target_metadata_preserved(&fixture);
    assert_no_journal_entries(&fixture.target_db);
}

#[test]
fn reconcile_source_db_stage_is_idempotent_after_move_completion() {
    let fixture = MoveRecoveryFixture::new();
    std::fs::rename(fixture.source_absolute(), fixture.target_absolute()).unwrap();
    fixture
        .target_db
        .upsert_file(&fixture.target_relative, 16, 1)
        .unwrap();
    fixture
        .target_db
        .set_tag(&fixture.target_relative, Rating::KEEP_1)
        .unwrap();
    fixture
        .target_db
        .set_looped(&fixture.target_relative, true)
        .unwrap();
    fixture
        .target_db
        .set_locked(&fixture.target_relative, true)
        .unwrap();
    fixture
        .target_db
        .set_last_played_at(&fixture.target_relative, 123)
        .unwrap();
    fixture
        .source_db
        .remove_file(&fixture.source_relative)
        .unwrap();
    fixture.stage_source_db();

    let summary = reconcile_pending_ops_for_tests(&fixture.target_db).unwrap();
    assert_eq!(summary.completed, 1);
    assert!(fixture.target_absolute().exists());
    assert_target_metadata_preserved(&fixture);
    assert_no_journal_entries(&fixture.target_db);
}

#[test]
fn reconcile_missing_staged_file_keeps_original_source_when_target_missing() {
    let fixture = MoveRecoveryFixture::new();
    let (file_size, modified_ns) = file_identity(&fixture.source_absolute());
    update_stage(
        &fixture.target_db,
        &fixture.entry.id,
        FileOpStage::Staged,
        Some(file_size),
        Some(modified_ns),
    )
    .unwrap();
    fixture
        .target_db
        .upsert_file(&fixture.target_relative, 16, 1)
        .unwrap();

    let summary = reconcile_pending_ops_for_tests(&fixture.target_db).unwrap();
    assert_eq!(summary.completed, 1);
    assert!(fixture.source_absolute().exists());
    assert!(!fixture.target_absolute().exists());
    assert_eq!(
        fixture
            .source_db
            .tag_for_path(&fixture.source_relative)
            .unwrap(),
        Some(Rating::KEEP_1)
    );
    assert!(
        fixture
            .target_db
            .tag_for_path(&fixture.target_relative)
            .unwrap()
            .is_none()
    );
    assert_no_journal_entries(&fixture.target_db);
}

#[test]
fn reconcile_move_defers_when_target_exists_and_journal_identity_is_incomplete() {
    let fixture = MoveRecoveryFixture::new();
    std::fs::rename(fixture.source_absolute(), fixture.staged_absolute()).unwrap();
    update_stage(
        &fixture.target_db,
        &fixture.entry.id,
        FileOpStage::Staged,
        None,
        None,
    )
    .unwrap();

    std::fs::write(fixture.target_absolute(), [7u8; 8]).unwrap();
    let mut batch = fixture.target_db.write_batch().unwrap();
    batch
        .upsert_file_with_hash_and_tag(
            &fixture.target_relative,
            8,
            2,
            "reused-hash",
            Rating::TRASH_3,
            false,
        )
        .unwrap();
    batch.commit().unwrap();
    fixture
        .target_db
        .set_looped(&fixture.target_relative, false)
        .unwrap();
    fixture
        .target_db
        .set_locked(&fixture.target_relative, false)
        .unwrap();
    fixture
        .target_db
        .set_last_played_at(&fixture.target_relative, 77)
        .unwrap();

    let summary = reconcile_pending_ops_for_tests(&fixture.target_db).unwrap();
    assert_eq!(summary.completed, 0);
    assert_eq!(list_entries(&fixture.target_db).unwrap().entries.len(), 1);
    assert!(fixture.staged_absolute().exists());
    assert!(!fixture.source_absolute().exists());
    assert_eq!(
        std::fs::read(fixture.target_absolute()).unwrap(),
        vec![7u8; 8]
    );
    assert_eq!(
        fixture
            .target_db
            .tag_for_path(&fixture.target_relative)
            .unwrap(),
        Some(Rating::TRASH_3)
    );
    assert_eq!(
        fixture
            .target_db
            .last_played_at_for_path(&fixture.target_relative)
            .unwrap(),
        Some(77)
    );
    assert_eq!(
        fixture
            .source_db
            .tag_for_path(&fixture.source_relative)
            .unwrap(),
        Some(Rating::KEEP_1)
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
