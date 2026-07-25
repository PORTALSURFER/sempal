use super::*;
use crate::sample_sources::{Rating, SourceDatabase};
use std::path::PathBuf;
use tempfile::TempDir;

#[test]
fn reconcile_target_db_stage_defers_until_source_root_returns() {
    let temp = TempDir::new().unwrap();
    let source_root = temp.path().join("source");
    let parked_root = temp.path().join("source-offline");
    let target_root = temp.path().join("target");
    std::fs::create_dir_all(&source_root).unwrap();
    std::fs::create_dir_all(&target_root).unwrap();

    {
        let source_db = SourceDatabase::open_for_source_write(&source_root).unwrap();
        let source_relative = PathBuf::from("one.wav");
        let source_absolute = source_root.join(&source_relative);
        write_wav(&source_absolute);
        source_db.upsert_file(&source_relative, 16, 1).unwrap();
        source_db.set_tag(&source_relative, Rating::KEEP_1).unwrap();
        source_db.set_looped(&source_relative, true).unwrap();
        source_db.set_locked(&source_relative, true).unwrap();
        source_db.set_last_played_at(&source_relative, 123).unwrap();
    }

    let target_db = SourceDatabase::open_for_source_write(&target_root).unwrap();
    let source_relative = PathBuf::from("one.wav");
    let target_relative = PathBuf::from("moved.wav");
    let entry = FileOpJournalEntry::new_move(
        String::from("move-test"),
        MoveJournalEntryInit {
            source_root: source_root.clone(),
            source_relative: source_relative.clone(),
            target_relative: target_relative.clone(),
            staged_relative: staged_relative_for_target(&target_relative, "test").unwrap(),
            tag: Rating::KEEP_1,
            looped: true,
            locked: true,
            last_played_at: Some(123),
            last_curated_at: Some(456),
        },
    )
    .unwrap();
    insert_entry(&target_db, &entry).unwrap();

    std::fs::rename(
        source_root.join(&source_relative),
        target_root.join(&target_relative),
    )
    .unwrap();
    let (file_size, modified_ns) = file_identity(&target_root.join(&target_relative));
    update_stage(
        &target_db,
        &entry.id,
        FileOpStage::TargetDb,
        Some(file_size),
        Some(modified_ns),
    )
    .unwrap();
    std::fs::rename(&source_root, &parked_root).unwrap();

    let first = reconcile_pending_ops_for_tests(&target_db).unwrap();
    assert_eq!(first.completed, 0);
    assert_eq!(first.errors.len(), 1);
    assert!(first.errors[0].contains("Deferred move recovery"));
    assert!(target_root.join(&target_relative).is_file());
    assert_eq!(
        list_entries(&target_db).unwrap().entries.len(),
        1,
        "journal entry should remain pending while the source is offline"
    );

    std::fs::rename(&parked_root, &source_root).unwrap();
    let source_db = SourceDatabase::open_for_source_write(&source_root).unwrap();
    assert_eq!(
        source_db.tag_for_path(&source_relative).unwrap(),
        Some(Rating::KEEP_1)
    );

    let second = reconcile_pending_ops_for_tests(&target_db).unwrap();
    assert_eq!(second.completed, 1);
    assert!(second.errors.is_empty());
    assert!(
        source_db.tag_for_path(&source_relative).unwrap().is_none(),
        "source row should be removed once replay can reach the source DB again"
    );
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
