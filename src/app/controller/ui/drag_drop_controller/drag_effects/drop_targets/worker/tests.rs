use super::super::DroppedSampleMetadata;
use crate::sample_sources::db::file_ops_journal::{self, FileOpStage};
use crate::sample_sources::{Rating, SourceDatabase};
use std::path::{Path, PathBuf};
use tempfile::TempDir;
use wavecrate_library::timestamps::system_time_to_unix_nanos;

use super::super::super::move_transaction::{
    move_sample_file, prepare_staged_copy, prepare_staged_move,
};
use super::super::transactions::register_drop_target_target_entry;
use super::super::transactions::sample_move_metadata;

struct DropTargetRecoveryFixture {
    _temp: TempDir,
    source_root: PathBuf,
    target_root: PathBuf,
    source_db: SourceDatabase,
    target_db: SourceDatabase,
    source_relative: PathBuf,
    source_absolute: PathBuf,
}

impl DropTargetRecoveryFixture {
    fn new() -> Self {
        let temp = TempDir::new().unwrap();
        let source_root = temp.path().join("source");
        let target_root = temp.path().join("target");
        std::fs::create_dir_all(&source_root).unwrap();
        std::fs::create_dir_all(&target_root).unwrap();
        let source_db = SourceDatabase::open_for_test_fixture_source_write(&source_root).unwrap();
        let target_db = SourceDatabase::open_for_test_fixture_source_write(&target_root).unwrap();
        let source_relative = PathBuf::from("one.wav");
        let source_absolute = source_root.join(&source_relative);
        std::fs::write(&source_absolute, [0u8; 16]).unwrap();
        let metadata = std::fs::metadata(&source_absolute).unwrap();
        source_db
            .upsert_file(
                &source_relative,
                metadata.len(),
                modified_ns(&source_absolute),
            )
            .unwrap();
        source_db.set_tag(&source_relative, Rating::KEEP_1).unwrap();
        source_db.set_looped(&source_relative, true).unwrap();
        source_db.set_locked(&source_relative, true).unwrap();
        source_db.set_last_played_at(&source_relative, 42).unwrap();
        Self {
            _temp: temp,
            source_root,
            target_root,
            source_db,
            target_db,
            source_relative,
            source_absolute,
        }
    }

    fn metadata(&self) -> DroppedSampleMetadata {
        DroppedSampleMetadata {
            tag: Rating::KEEP_1,
            looped: true,
            locked: true,
            last_played_at: Some(42),
            last_curated_at: Some(99),
            sound_type: None,
            user_tag: Some("Vintage FX".into()),
            normal_tags: Vec::new(),
            collection: None,
        }
    }
}

fn modified_ns(path: &Path) -> i64 {
    system_time_to_unix_nanos(std::fs::metadata(path).unwrap().modified().unwrap())
}

fn journal_entry(db: &SourceDatabase) -> file_ops_journal::FileOpJournalEntry {
    let listed = file_ops_journal::list_entries(db).unwrap();
    assert!(listed.malformed.is_empty());
    assert_eq!(listed.entries.len(), 1);
    listed.entries.into_iter().next().unwrap()
}

fn assert_target_metadata(
    db: &SourceDatabase,
    target_relative: &Path,
    metadata: &DroppedSampleMetadata,
) {
    assert_eq!(
        db.tag_for_path(target_relative).unwrap(),
        Some(metadata.tag)
    );
    assert_eq!(
        db.looped_for_path(target_relative).unwrap(),
        Some(metadata.looped)
    );
    assert_eq!(
        db.locked_for_path(target_relative).unwrap(),
        Some(metadata.locked)
    );
    assert_eq!(
        db.last_played_at_for_path(target_relative).unwrap(),
        metadata.last_played_at
    );
    assert_eq!(
        db.user_tag_for_path(target_relative).unwrap(),
        metadata.user_tag.clone()
    );
}

fn assert_no_journal_entries(db: &SourceDatabase) {
    let listed = file_ops_journal::list_entries(db).unwrap();
    assert!(listed.entries.is_empty());
    assert!(listed.malformed.is_empty());
}

#[test]
fn copy_finalize_failure_keeps_target_db_stage_until_reconcile() {
    let fixture = DropTargetRecoveryFixture::new();
    let target_relative = PathBuf::from("blocked/copied.wav");
    let metadata = fixture.metadata();
    let prepared = prepare_staged_copy(
        &fixture.target_db,
        &fixture.source_absolute,
        &fixture.target_root,
        &target_relative,
        sample_move_metadata(&metadata),
    )
    .unwrap();
    register_drop_target_target_entry(
        &fixture.target_db,
        &target_relative,
        prepared.file_size,
        prepared.modified_ns,
        &metadata,
    )
    .unwrap();
    file_ops_journal::update_stage(
        &fixture.target_db,
        &prepared.op_id,
        FileOpStage::TargetDb,
        None,
        None,
    )
    .unwrap();
    std::fs::create_dir_all(&prepared.target_absolute).unwrap();

    let finalize_err =
        move_sample_file(&prepared.staged_absolute, &prepared.target_absolute).unwrap_err();
    assert!(finalize_err.contains("Failed to move file"));

    let entry = journal_entry(&fixture.target_db);
    assert_eq!(entry.stage, FileOpStage::TargetDb);
    let staged_absolute = prepared.staged_absolute.clone();
    assert!(staged_absolute.is_file());
    assert!(!fixture.target_root.join(&target_relative).is_file());
    assert_target_metadata(&fixture.target_db, &target_relative, &metadata);
    assert!(fixture.source_absolute.is_file());

    std::fs::remove_dir(&prepared.target_absolute).unwrap();
    let summary = file_ops_journal::reconcile_pending_ops(&fixture.target_db).unwrap();
    assert_eq!(summary.completed, 1);
    assert!(summary.errors.is_empty());
    assert!(fixture.target_root.join(&target_relative).is_file());
    assert!(!staged_absolute.exists());
    assert_target_metadata(&fixture.target_db, &target_relative, &metadata);
    assert_no_journal_entries(&fixture.target_db);
}

#[test]
fn move_finalize_failure_keeps_source_db_stage_until_reconcile() {
    let fixture = DropTargetRecoveryFixture::new();
    let target_relative = PathBuf::from("moved.wav");
    let metadata = fixture.metadata();
    let prepared = prepare_staged_move(
        &fixture.target_db,
        &fixture.source_root,
        &fixture.source_relative,
        &fixture.target_root,
        &target_relative,
        sample_move_metadata(&metadata),
    )
    .unwrap();
    register_drop_target_target_entry(
        &fixture.target_db,
        &target_relative,
        prepared.file_size,
        prepared.modified_ns,
        &metadata,
    )
    .unwrap();
    fixture
        .source_db
        .remove_file(&fixture.source_relative)
        .unwrap();
    file_ops_journal::update_stage(
        &fixture.target_db,
        &prepared.op_id,
        FileOpStage::SourceDb,
        None,
        None,
    )
    .unwrap();
    std::fs::create_dir_all(&prepared.target_absolute).unwrap();

    let finalize_err =
        move_sample_file(&prepared.staged_absolute, &prepared.target_absolute).unwrap_err();
    assert!(finalize_err.contains("Failed to move file"));

    let entry = journal_entry(&fixture.target_db);
    assert_eq!(entry.stage, FileOpStage::SourceDb);
    let staged_absolute = prepared.staged_absolute.clone();
    assert!(staged_absolute.is_file());
    assert!(!fixture.source_absolute.exists());
    assert!(!fixture.target_root.join(&target_relative).is_file());
    assert!(
        fixture
            .source_db
            .tag_for_path(&fixture.source_relative)
            .unwrap()
            .is_none()
    );
    assert_target_metadata(&fixture.target_db, &target_relative, &metadata);

    std::fs::remove_dir(&prepared.target_absolute).unwrap();
    let summary = file_ops_journal::reconcile_pending_ops(&fixture.target_db).unwrap();
    assert_eq!(summary.completed, 1);
    assert!(summary.errors.is_empty());
    assert!(fixture.target_root.join(&target_relative).is_file());
    assert!(!staged_absolute.exists());
    assert!(
        fixture
            .source_db
            .tag_for_path(&fixture.source_relative)
            .unwrap()
            .is_none()
    );
    assert_target_metadata(&fixture.target_db, &target_relative, &metadata);
    assert_no_journal_entries(&fixture.target_db);
}
