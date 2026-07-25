use super::*;
use crate::timestamps::system_time_to_unix_nanos;
use rusqlite::params;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

use crate::sample_sources::{Rating, SourceDatabase};

fn reconcile_pending_ops_for_tests(db: &SourceDatabase) -> Result<FileOpReconcileSummary, String> {
    super::reconcile::reconcile_pending_ops_with_test_finalizer(db)
}

mod copy_recovery;
mod malformed_rows;
mod move_recovery;
mod offline_source_retry;

struct MoveRecoveryFixture {
    _temp: TempDir,
    source_root: PathBuf,
    target_root: PathBuf,
    source_db: SourceDatabase,
    target_db: SourceDatabase,
    source_relative: PathBuf,
    target_relative: PathBuf,
    staged_relative: PathBuf,
    entry: FileOpJournalEntry,
}

impl MoveRecoveryFixture {
    fn new() -> Self {
        let temp = TempDir::new().unwrap();
        let source_root = temp.path().join("source");
        let target_root = temp.path().join("target");
        std::fs::create_dir_all(&source_root).unwrap();
        std::fs::create_dir_all(&target_root).unwrap();
        let source_db = SourceDatabase::open_for_source_write(&source_root).unwrap();
        let target_db = SourceDatabase::open_for_source_write(&target_root).unwrap();
        let source_relative = PathBuf::from("one.wav");
        let source_absolute = source_root.join(&source_relative);
        write_wav(&source_absolute);
        source_db.upsert_file(&source_relative, 16, 1).unwrap();
        source_db.set_tag(&source_relative, Rating::KEEP_1).unwrap();
        source_db.set_looped(&source_relative, true).unwrap();
        source_db.set_locked(&source_relative, true).unwrap();
        source_db.set_last_played_at(&source_relative, 123).unwrap();
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
        insert_entry(&target_db, &entry).unwrap();
        Self {
            _temp: temp,
            source_root,
            target_root,
            source_db,
            target_db,
            source_relative,
            target_relative,
            staged_relative,
            entry,
        }
    }

    fn source_absolute(&self) -> PathBuf {
        self.source_root.join(&self.source_relative)
    }

    fn target_absolute(&self) -> PathBuf {
        self.target_root.join(&self.target_relative)
    }

    fn staged_absolute(&self) -> PathBuf {
        self.target_root.join(&self.staged_relative)
    }

    fn stage_target_db(&self) {
        let (file_size, modified_ns) = file_identity(&self.target_absolute());
        update_stage(
            &self.target_db,
            &self.entry.id,
            FileOpStage::TargetDb,
            Some(file_size),
            Some(modified_ns),
        )
        .unwrap();
    }

    fn stage_source_db(&self) {
        let (file_size, modified_ns) = file_identity(&self.target_absolute());
        update_stage(
            &self.target_db,
            &self.entry.id,
            FileOpStage::SourceDb,
            Some(file_size),
            Some(modified_ns),
        )
        .unwrap();
    }
}

fn write_wav(path: &Path) {
    std::fs::write(path, [0u8; 16]).unwrap();
}

fn file_identity(path: &Path) -> (u64, i64) {
    let metadata = std::fs::metadata(path).unwrap();
    let modified_ns = system_time_to_unix_nanos(metadata.modified().unwrap());
    (metadata.len(), modified_ns)
}

fn assert_no_journal_entries(db: &SourceDatabase) {
    let listed = list_entries(db).unwrap();
    assert!(listed.entries.is_empty());
    assert!(listed.malformed.is_empty());
}

fn assert_target_metadata_preserved(fixture: &MoveRecoveryFixture) {
    assert_eq!(
        fixture
            .target_db
            .tag_for_path(&fixture.target_relative)
            .unwrap(),
        Some(Rating::KEEP_1)
    );
    assert_eq!(
        fixture
            .target_db
            .looped_for_path(&fixture.target_relative)
            .unwrap(),
        Some(true)
    );
    assert_eq!(
        fixture
            .target_db
            .locked_for_path(&fixture.target_relative)
            .unwrap(),
        Some(true)
    );
    assert_eq!(
        fixture
            .target_db
            .last_played_at_for_path(&fixture.target_relative)
            .unwrap(),
        Some(123)
    );
}
