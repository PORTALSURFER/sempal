use super::super::worker::{
    FolderMoveTestHooks, run_folder_move_task, run_folder_move_task_with_hooks,
};
use super::support::{
    Must, folder_move_request, folder_move_test_guard, setup_folder_move_fixture,
};
use crate::app::controller::AppController;
use crate::app::controller::ui::drag_drop_controller::DragDropController;
use crate::sample_sources::db::DB_FILE_NAME;
use crate::sample_sources::{Rating, SampleCollection, SourceDatabase};
use crate::waveform::WaveformRenderer;
use std::path::{Path, PathBuf};
use std::sync::{Arc, atomic::AtomicBool};
use std::time::Duration;

#[test]
/// Moving a folder relocates contained files and rewrites their source DB paths.
fn folder_move_updates_db_entries() {
    let _guard = folder_move_test_guard();
    let (_temp, source, source_root) = setup_folder_move_fixture();
    let request = folder_move_request(&source, &source_root, "old", "dest");
    let result = run_folder_move_task(request, Arc::new(AtomicBool::new(false)), None);

    assert!(result.errors.is_empty());
    assert_eq!(result.moved.len(), 1);
    let moved = result.moved.first().must();
    assert_eq!(
        moved.normal_tags,
        vec![String::from("Bright"), String::from("Riser FX")]
    );
    assert_eq!(moved.collection, Some(SampleCollection::new(1).must()));
    assert!(source_root.join("dest/old/one.wav").is_file());

    let db = SourceDatabase::open_for_test_fixture_source_write(&source_root).must();
    assert!(db.tag_for_path(Path::new("old/one.wav")).must().is_none());
    assert_eq!(
        db.tag_for_path(Path::new("dest/old/one.wav")).must(),
        Some(Rating::KEEP_1)
    );
    assert_eq!(
        db.looped_for_path(Path::new("dest/old/one.wav")).must(),
        Some(true)
    );
    assert_eq!(
        db.locked_for_path(Path::new("dest/old/one.wav")).must(),
        Some(true)
    );
    assert_eq!(
        db.last_played_at_for_path(Path::new("dest/old/one.wav"))
            .must(),
        Some(42)
    );
    assert_eq!(
        db.tag_labels_for_path(Path::new("dest/old/one.wav")).must(),
        vec![String::from("Bright"), String::from("Riser FX")]
    );
    assert_eq!(
        db.collection_for_path(Path::new("dest/old/one.wav")).must(),
        Some(SampleCollection::new(1).must())
    );
}

#[test]
/// Folder-tree folder moves register undo and redo on the legacy controller stack.
fn folder_tree_move_supports_undo_and_redo() {
    let _guard = folder_move_test_guard();
    let (_temp, source, source_root) = setup_folder_move_fixture();
    let mut controller = AppController::new(WaveformRenderer::new(16, 16), None);
    controller.library.sources.push(source.clone());
    controller.selection_state.ctx.selected_source = Some(source.id.clone());

    DragDropController::new(&mut controller).handle_folder_drop_to_folder(
        source.id.clone(),
        PathBuf::from("old"),
        Path::new("dest"),
    );

    assert!(source_root.join("dest/old/one.wav").is_file());
    assert!(!source_root.join("old").exists());
    let db = SourceDatabase::open_for_test_fixture_source_write(&source_root).must();
    assert!(db.tag_for_path(Path::new("old/one.wav")).must().is_none());
    assert_eq!(
        db.tag_for_path(Path::new("dest/old/one.wav")).must(),
        Some(Rating::KEEP_1)
    );

    controller.undo();

    assert!(source_root.join("old/one.wav").is_file());
    assert!(!source_root.join("dest/old").exists());
    assert_eq!(
        db.tag_for_path(Path::new("old/one.wav")).must(),
        Some(Rating::KEEP_1)
    );
    assert!(
        db.tag_for_path(Path::new("dest/old/one.wav"))
            .must()
            .is_none()
    );

    controller.redo();

    assert!(source_root.join("dest/old/one.wav").is_file());
    assert!(!source_root.join("old").exists());
    assert!(db.tag_for_path(Path::new("old/one.wav")).must().is_none());
    assert_eq!(
        db.tag_for_path(Path::new("dest/old/one.wav")).must(),
        Some(Rating::KEEP_1)
    );
}

#[test]
/// Cancelling before folder processing starts leaves filesystem and DB state unchanged.
fn folder_move_cancelled_before_processing_keeps_source_unchanged() {
    let _guard = folder_move_test_guard();
    let (_temp, source, source_root) = setup_folder_move_fixture();
    let request = folder_move_request(&source, &source_root, "old", "dest");
    let result = run_folder_move_task(request, Arc::new(AtomicBool::new(true)), None);

    assert!(result.cancelled);
    assert!(!result.folder_moved);
    assert!(result.moved.is_empty());
    assert!(result.errors.is_empty());
    assert!(source_root.join("old/one.wav").is_file());
    assert!(!source_root.join("dest/old").exists());
    let db = SourceDatabase::open_for_test_fixture_source_write(&source_root).must();
    assert_eq!(
        db.tag_for_path(Path::new("old/one.wav")).must(),
        Some(Rating::KEEP_1)
    );
    assert_eq!(
        db.locked_for_path(Path::new("old/one.wav")).must(),
        Some(true)
    );
}

#[test]
/// Moving a folder into one of its descendants is rejected without touching the source tree.
fn folder_move_rejects_descendant_target() {
    let _guard = folder_move_test_guard();
    let (_temp, source, source_root) = setup_folder_move_fixture();
    std::fs::create_dir_all(source_root.join("old/child")).must();
    let request = folder_move_request(&source, &source_root, "old", "old/child");
    let result = run_folder_move_task(request, Arc::new(AtomicBool::new(false)), None);

    assert!(!result.folder_moved);
    assert!(result.moved.is_empty());
    assert_eq!(
        result.errors,
        vec!["Cannot move a folder into itself".to_string()]
    );
    assert!(source_root.join("old/one.wav").is_file());
    assert!(source_root.join("old/child").is_dir());
}

#[test]
/// An existing destination folder rejects the move before any filesystem rename occurs.
fn folder_move_rejects_existing_destination_folder() {
    let _guard = folder_move_test_guard();
    let (_temp, source, source_root) = setup_folder_move_fixture();
    std::fs::create_dir_all(source_root.join("dest/old")).must();
    let request = folder_move_request(&source, &source_root, "old", "dest");
    let result = run_folder_move_task(request, Arc::new(AtomicBool::new(false)), None);

    assert!(!result.folder_moved);
    assert!(result.moved.is_empty());
    assert_eq!(
        result.errors,
        vec![format!(
            "Folder already exists: {}",
            PathBuf::from("dest").join("old").display()
        )]
    );
    assert!(source_root.join("old/one.wav").is_file());
    assert!(source_root.join("dest/old").is_dir());
    let db = SourceDatabase::open_for_test_fixture_source_write(&source_root).must();
    assert_eq!(
        db.tag_for_path(Path::new("old/one.wav")).must(),
        Some(Rating::KEEP_1)
    );
    assert_eq!(
        db.locked_for_path(Path::new("old/one.wav")).must(),
        Some(true)
    );
}

#[test]
/// A database lock after the filesystem rename rolls the folder back to its original path.
fn folder_move_db_write_failure_rolls_back_source_and_db_state() {
    let _guard = folder_move_test_guard();
    let (_temp, source, source_root) = setup_folder_move_fixture();
    let (lock_release_tx, lock_release_rx) = std::sync::mpsc::channel();
    let (lock_done_tx, lock_done_rx) = std::sync::mpsc::channel();
    let mut lock_release_rx = Some(lock_release_rx);
    let mut lock_done_tx = Some(lock_done_tx);
    let source_root_for_hook = source_root.clone();
    let mut hooks = FolderMoveTestHooks {
        before_folder_move_batch: Some(Box::new(move || {
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
    let request = folder_move_request(&source, &source_root, "old", "dest");
    let result = run_folder_move_task_with_hooks(
        request,
        Arc::new(AtomicBool::new(false)),
        None,
        &mut hooks,
    );
    let _ = lock_release_tx.send(());
    lock_done_rx.recv_timeout(Duration::from_secs(1)).must();

    assert!(!result.folder_moved);
    assert!(result.moved.is_empty());
    assert_eq!(result.new_folder, PathBuf::from("dest/old"));
    assert!(result.errors.iter().any(|err| {
        err.contains("Failed to start database update")
            || err.contains("Failed to drop old entry")
            || err.contains("Failed to register moved file")
            || err.contains("Failed to copy tag")
            || err.contains("Failed to copy loop marker")
            || err.contains("Failed to copy keep lock")
            || err.contains("Failed to copy playback age")
            || err.contains("Failed to copy normal tags")
            || err.contains("Failed to copy collection")
            || err.contains("Failed to save folder move")
    }));
    assert!(source_root.join("old/one.wav").is_file());
    assert!(!source_root.join("dest/old").exists());
    let db = SourceDatabase::open_for_test_fixture_source_write(&source_root).must();
    assert_eq!(
        db.tag_for_path(Path::new("old/one.wav")).must(),
        Some(Rating::KEEP_1)
    );
    assert_eq!(
        db.locked_for_path(Path::new("old/one.wav")).must(),
        Some(true)
    );
    assert!(
        db.tag_for_path(Path::new("dest/old/one.wav"))
            .must()
            .is_none()
    );
}
