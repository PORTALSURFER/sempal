use super::super::worker::{
    FolderMoveTestHooks, run_folder_sample_move_task, run_folder_sample_move_task_with_hooks,
};
use super::support::{Must, folder_move_test_guard, read_file_metadata};
use crate::app::controller::jobs::FolderSampleMoveRequest;
use crate::app::controller::test_support::write_test_wav;
use crate::sample_sources::db::{DB_FILE_NAME, file_ops_journal};
use crate::sample_sources::{Rating, SampleSource, SourceDatabase};
use std::path::{Path, PathBuf};
use std::sync::{Arc, atomic::AtomicBool};
use std::time::Duration;
use tempfile::tempdir;

#[test]
/// Moving a single file updates filesystem state, DB metadata, and clears journal entries.
fn folder_sample_move_updates_db_entry() {
    let _guard = folder_move_test_guard();
    let temp = tempdir().must();
    let source_root = temp.path().join("source");
    let target_dir = source_root.join("folder");
    std::fs::create_dir_all(&target_dir).must();
    let source = SampleSource::new(source_root.clone());
    let wav_path = source_root.join("one.wav");
    write_test_wav(&wav_path, &[0.0, 0.1, -0.1]);
    let (file_size, modified_ns) = read_file_metadata(&wav_path);
    let db = SourceDatabase::open_for_test_fixture_source_write(&source_root).must();
    let mut batch = db.write_batch().must();
    batch
        .upsert_file(Path::new("one.wav"), file_size, modified_ns)
        .must();
    batch.set_tag(Path::new("one.wav"), Rating::KEEP_1).must();
    batch.set_looped(Path::new("one.wav"), true).must();
    batch.set_locked(Path::new("one.wav"), true).must();
    batch.set_last_played_at(Path::new("one.wav"), 42).must();
    batch.commit().must();

    let request = FolderSampleMoveRequest {
        relative_path: PathBuf::from("one.wav"),
        target_relative: PathBuf::from("folder/one.wav"),
    };
    let result = run_folder_sample_move_task(
        source.id.clone(),
        source_root.clone(),
        vec![request],
        Vec::new(),
        Arc::new(AtomicBool::new(false)),
        None,
    );

    assert!(result.errors.is_empty());
    assert_eq!(result.moved.len(), 1);
    assert!(source_root.join("folder/one.wav").is_file());

    let db = SourceDatabase::open_for_test_fixture_source_write(&source_root).must();
    assert!(db.tag_for_path(Path::new("one.wav")).must().is_none());
    assert_eq!(
        db.tag_for_path(Path::new("folder/one.wav")).must(),
        Some(Rating::KEEP_1)
    );
    assert_eq!(
        db.looped_for_path(Path::new("folder/one.wav")).must(),
        Some(true)
    );
    assert_eq!(
        db.locked_for_path(Path::new("folder/one.wav")).must(),
        Some(true)
    );
    assert_eq!(
        db.last_played_at_for_path(Path::new("folder/one.wav"))
            .must(),
        Some(42)
    );
    let entries = file_ops_journal::list_entries(&db).must();
    assert!(entries.entries.is_empty());
    assert!(entries.malformed.is_empty());
}

#[test]
/// Cancellation before the batch starts leaves source file and DB entries unchanged.
fn folder_sample_move_cancelled_before_processing_keeps_source_unchanged() {
    let _guard = folder_move_test_guard();
    let temp = tempdir().must();
    let source_root = temp.path().join("source");
    let target_dir = source_root.join("folder");
    std::fs::create_dir_all(&target_dir).must();
    let source = SampleSource::new(source_root.clone());
    let wav_path = source_root.join("one.wav");
    write_test_wav(&wav_path, &[0.0, 0.1, -0.1]);
    let (file_size, modified_ns) = read_file_metadata(&wav_path);
    let db = SourceDatabase::open_for_test_fixture_source_write(&source_root).must();
    db.upsert_file(Path::new("one.wav"), file_size, modified_ns)
        .must();
    db.set_tag(Path::new("one.wav"), Rating::KEEP_1).must();
    db.set_locked(Path::new("one.wav"), true).must();

    let cancel = Arc::new(AtomicBool::new(true));
    let result = run_folder_sample_move_task(
        source.id.clone(),
        source_root.clone(),
        vec![FolderSampleMoveRequest {
            relative_path: PathBuf::from("one.wav"),
            target_relative: PathBuf::from("folder/one.wav"),
        }],
        Vec::new(),
        cancel,
        None,
    );

    assert!(result.cancelled);
    assert!(result.moved.is_empty());
    assert!(result.errors.is_empty());
    assert!(source_root.join("one.wav").is_file());
    assert!(!source_root.join("folder/one.wav").exists());
    let db = SourceDatabase::open_for_test_fixture_source_write(&source_root).must();
    assert_eq!(
        db.tag_for_path(Path::new("one.wav")).must(),
        Some(Rating::KEEP_1)
    );
    assert_eq!(db.locked_for_path(Path::new("one.wav")).must(), Some(true));
    let entries = file_ops_journal::list_entries(&db).must();
    assert!(entries.entries.is_empty());
    assert!(entries.malformed.is_empty());
}

#[test]
/// A DB-write failure rolls the file back to source and keeps staged journal data for recovery.
fn folder_sample_move_db_write_failure_rolls_back_source_and_keeps_journal_for_recovery() {
    let _guard = folder_move_test_guard();
    let temp = tempdir().must();
    let source_root = temp.path().join("source");
    let target_dir = source_root.join("folder");
    std::fs::create_dir_all(&target_dir).must();
    let source = SampleSource::new(source_root.clone());
    let wav_path = source_root.join("one.wav");
    write_test_wav(&wav_path, &[0.0, 0.1, -0.1]);
    let (file_size, modified_ns) = read_file_metadata(&wav_path);
    let db = SourceDatabase::open_for_test_fixture_source_write(&source_root).must();
    db.upsert_file(Path::new("one.wav"), file_size, modified_ns)
        .must();
    db.set_tag(Path::new("one.wav"), Rating::KEEP_1).must();
    db.set_looped(Path::new("one.wav"), true).must();
    db.set_locked(Path::new("one.wav"), true).must();
    db.set_last_played_at(Path::new("one.wav"), 42).must();

    let (lock_release_tx, lock_release_rx) = std::sync::mpsc::channel();
    let (lock_done_tx, lock_done_rx) = std::sync::mpsc::channel();
    let mut lock_release_rx = Some(lock_release_rx);
    let mut lock_done_tx = Some(lock_done_tx);
    let source_root_for_hook = source_root.clone();
    let mut hooks = FolderMoveTestHooks {
        before_folder_sample_batch: Some(Box::new(move || {
            let (locked_tx, locked_rx) = std::sync::mpsc::channel();
            let db_file = source_root_for_hook.join(DB_FILE_NAME);
            let lock_release_rx = lock_release_rx.take().must();
            let lock_done_tx = lock_done_tx.take().must();
            std::thread::spawn(move || {
                let conn = rusqlite::Connection::open(db_file).must();
                conn.execute_batch("BEGIN IMMEDIATE").must();
                let _ = locked_tx.send(());
                let _ = lock_release_rx.recv();
                let _ = conn.execute_batch("COMMIT");
                drop(conn);
                let _ = lock_done_tx.send(());
            });
            locked_rx.recv().must();
        })),
        ..Default::default()
    };

    let result = run_folder_sample_move_task_with_hooks(
        source.id.clone(),
        source_root.clone(),
        vec![FolderSampleMoveRequest {
            relative_path: PathBuf::from("one.wav"),
            target_relative: PathBuf::from("folder/one.wav"),
        }],
        Vec::new(),
        Arc::new(AtomicBool::new(false)),
        None,
        &mut hooks,
    );
    let _ = lock_release_tx.send(());
    lock_done_rx.recv_timeout(Duration::from_secs(1)).must();

    assert!(result.moved.is_empty());
    assert!(result.errors.iter().any(|err| {
        err.contains("Failed to start database update")
            || err.contains("Failed to drop old entry")
            || err.contains("Failed to register moved file")
            || err.contains("Failed to copy tag")
            || err.contains("Failed to copy loop marker")
            || err.contains("Failed to copy keep lock")
            || err.contains("Failed to copy playback age")
            || err.contains("Failed to save move")
            || err.contains("Failed to update move journal")
            || err.contains("Failed to finalize move")
    }));
    assert!(source_root.join("one.wav").is_file());
    assert!(!source_root.join("folder/one.wav").exists());
    let db = SourceDatabase::open_for_test_fixture_source_write(&source_root).must();
    assert_eq!(
        db.tag_for_path(Path::new("one.wav")).must(),
        Some(Rating::KEEP_1)
    );
    assert_eq!(db.locked_for_path(Path::new("one.wav")).must(), Some(true));
    let entries = file_ops_journal::list_entries(&db).must();
    assert!(entries.malformed.is_empty());
    assert_eq!(entries.entries.len(), 1);
    assert_eq!(
        entries.entries[0].stage,
        file_ops_journal::FileOpStage::Staged
    );
    assert_eq!(entries.entries[0].locked, Some(true));
    assert_eq!(
        entries.entries[0].target_relative,
        PathBuf::from("folder/one.wav")
    );
}

#[test]
/// A final rename failure after DB commit rolls the DB and staged file back immediately.
fn folder_sample_move_finalize_failure_rolls_back_db_file_and_journal() {
    let _guard = folder_move_test_guard();
    let temp = tempdir().must();
    let source_root = temp.path().join("source");
    let target_dir = source_root.join("folder");
    std::fs::create_dir_all(&target_dir).must();
    let source = SampleSource::new(source_root.clone());
    let wav_path = source_root.join("one.wav");
    write_test_wav(&wav_path, &[0.0, 0.1, -0.1]);
    let (file_size, modified_ns) = read_file_metadata(&wav_path);
    let db = SourceDatabase::open_for_test_fixture_source_write(&source_root).must();
    let mut batch = db.write_batch().must();
    batch
        .upsert_file(Path::new("one.wav"), file_size, modified_ns)
        .must();
    batch.set_tag(Path::new("one.wav"), Rating::KEEP_1).must();
    batch.set_looped(Path::new("one.wav"), true).must();
    batch.set_locked(Path::new("one.wav"), true).must();
    batch.set_last_played_at(Path::new("one.wav"), 42).must();
    batch.commit().must();

    let target_blocker = source_root.join("folder/one.wav");
    let mut hooks = FolderMoveTestHooks {
        before_folder_sample_finalize: Some(Box::new(move || {
            std::fs::create_dir(&target_blocker).must();
        })),
        ..Default::default()
    };

    let result = run_folder_sample_move_task_with_hooks(
        source.id.clone(),
        source_root.clone(),
        vec![FolderSampleMoveRequest {
            relative_path: PathBuf::from("one.wav"),
            target_relative: PathBuf::from("folder/one.wav"),
        }],
        Vec::new(),
        Arc::new(AtomicBool::new(false)),
        None,
        &mut hooks,
    );

    assert!(result.moved.is_empty());
    assert!(
        result
            .errors
            .iter()
            .any(|err| err.contains("Failed to finalize move"))
    );
    assert!(source_root.join("one.wav").is_file());
    assert!(source_root.join("folder/one.wav").is_dir());
    let db = SourceDatabase::open_for_test_fixture_source_write(&source_root).must();
    assert_eq!(
        db.tag_for_path(Path::new("one.wav")).must(),
        Some(Rating::KEEP_1)
    );
    assert_eq!(db.looped_for_path(Path::new("one.wav")).must(), Some(true));
    assert_eq!(db.locked_for_path(Path::new("one.wav")).must(), Some(true));
    assert_eq!(
        db.last_played_at_for_path(Path::new("one.wav")).must(),
        Some(42)
    );
    assert!(
        db.tag_for_path(Path::new("folder/one.wav"))
            .must()
            .is_none()
    );
    let entries = file_ops_journal::list_entries(&db).must();
    assert!(entries.entries.is_empty());
    assert!(entries.malformed.is_empty());
}
