use super::*;
use std::fs;
use tempfile::tempdir;
use wavecrate::sample_sources::capture_source_file_evidence;

#[test]
fn file_move_plan_deduplicates_ready_paths_and_reports_conflicts() {
    let root = tempdir().unwrap();
    let source = root.path().join("source");
    let target = root.path().join("target");
    fs::create_dir_all(&source).unwrap();
    fs::create_dir_all(&target).unwrap();
    let ready = source.join("ready.wav");
    let conflict = source.join("conflict.wav");
    let existing = target.join("conflict.wav");
    fs::write(&ready, b"ready").unwrap();
    fs::write(&conflict, b"source").unwrap();
    fs::write(&existing, b"existing").unwrap();

    let plan = file_move_plan_to_folder(
        root.path(),
        root.path(),
        &[
            ready.display().to_string(),
            ready.display().to_string(),
            conflict.display().to_string(),
        ],
        &target,
        true,
    )
    .unwrap();

    assert_eq!(
        plan.ready,
        vec![FileTransfer::move_file(
            ready.clone(),
            target.join("ready.wav")
        )]
    );
    assert_eq!(
        plan.conflicts,
        vec![FileMoveConflict {
            source_root: root.path().to_path_buf(),
            source_database_root: root.path().to_path_buf(),
            source_path: conflict,
            destination_path: existing,
            destination_protected: true,
        }]
    );
}

#[test]
fn planned_move_rejects_a_destination_created_before_commit() {
    let root = tempdir().unwrap();
    let source_folder = root.path().join("source");
    let target = root.path().join("target");
    fs::create_dir(&source_folder).unwrap();
    fs::create_dir(&target).unwrap();
    let source = source_folder.join("kick.wav");
    let destination = target.join("kick.wav");
    fs::write(&source, b"source").unwrap();
    let plan = file_move_plan_to_folder(
        root.path(),
        root.path(),
        &[source.display().to_string()],
        &target,
        false,
    )
    .unwrap();
    fs::write(&destination, b"late owner").unwrap();

    let error = transfer_files_with_rollback_and_progress(&plan.ready, |_, _| {}).unwrap_err();

    assert!(error.contains("File move failed"));
    assert_eq!(fs::read(source).unwrap(), b"source");
    assert_eq!(fs::read(destination).unwrap(), b"late owner");
}

#[test]
fn planned_copy_retries_a_destination_created_before_commit() {
    let root = tempdir().unwrap();
    let source_folder = root.path().join("source");
    let target = root.path().join("target");
    fs::create_dir(&source_folder).unwrap();
    fs::create_dir(&target).unwrap();
    let source = source_folder.join("kick.wav");
    let destination = target.join("kick.wav");
    fs::write(&source, b"source").unwrap();
    let plan = file_move_items_plan_to_folder(
        &[FileMoveItem {
            source_root: root.path().to_path_buf(),
            source_database_root: root.path().to_path_buf(),
            file_id: source.display().to_string(),
            copy_only: true,
        }],
        &target,
        false,
    )
    .unwrap();
    fs::write(&destination, b"late owner").unwrap();

    let completed = transfer_files_with_rollback_and_progress(&plan.ready, |_, _| {}).unwrap();

    let copied = target.join("kick_copy001.wav");
    assert_eq!(completed, vec![(source.clone(), copied.clone())]);
    assert_eq!(fs::read(source).unwrap(), b"source");
    assert_eq!(fs::read(destination).unwrap(), b"late owner");
    assert_eq!(fs::read(copied).unwrap(), b"source");
}

#[test]
fn planned_numbered_copy_continues_the_original_suffix_sequence() {
    let root = tempdir().unwrap();
    let source_folder = root.path().join("source");
    let target = root.path().join("target");
    fs::create_dir(&source_folder).unwrap();
    fs::create_dir(&target).unwrap();
    let source = source_folder.join("kick.wav");
    let direct_destination = target.join("kick.wav");
    let planned_destination = target.join("kick_copy001.wav");
    fs::write(&source, b"source").unwrap();
    fs::write(&direct_destination, b"existing").unwrap();
    let plan = file_move_items_plan_to_folder(
        &[FileMoveItem {
            source_root: root.path().to_path_buf(),
            source_database_root: root.path().to_path_buf(),
            file_id: source.display().to_string(),
            copy_only: true,
        }],
        &target,
        false,
    )
    .unwrap();
    assert_eq!(plan.ready[0].destination_path, planned_destination);
    fs::write(&planned_destination, b"late owner").unwrap();

    let completed = transfer_files_with_rollback_and_progress(&plan.ready, |_, _| {}).unwrap();

    let copied = target.join("kick_copy002.wav");
    assert_eq!(completed, vec![(source, copied.clone())]);
    assert_eq!(fs::read(direct_destination).unwrap(), b"existing");
    assert_eq!(fs::read(planned_destination).unwrap(), b"late owner");
    assert_eq!(fs::read(copied).unwrap(), b"source");
}

#[test]
fn rename_files_with_rollback_restores_completed_moves_after_later_failure() {
    let root = tempdir().unwrap();
    let source = root.path().join("source");
    let target = root.path().join("target");
    fs::create_dir_all(&source).unwrap();
    fs::create_dir_all(&target).unwrap();
    let first_source = source.join("first.wav");
    let second_source = source.join("second.wav");
    let first_destination = target.join("first.wav");
    let missing_destination = root.path().join("missing-parent").join("second.wav");
    fs::write(&first_source, b"first").unwrap();
    fs::write(&second_source, b"second").unwrap();

    let result = rename_files_with_rollback(&[
        (first_source.clone(), first_destination.clone()),
        (second_source.clone(), missing_destination),
    ]);

    assert!(result.is_err());
    assert_eq!(fs::read(&first_source).unwrap(), b"first");
    assert!(!first_destination.exists());
    assert_eq!(fs::read(&second_source).unwrap(), b"second");
}

#[test]
fn rollback_restores_source_without_touching_a_replaced_destination() {
    let root = tempdir().unwrap();
    let source_folder = root.path().join("source");
    let target = root.path().join("target");
    fs::create_dir(&source_folder).unwrap();
    fs::create_dir(&target).unwrap();
    let first_source = source_folder.join("first.wav");
    let second_source = source_folder.join("second.wav");
    let first_destination = target.join("first.wav");
    let missing_destination = root.path().join("missing-parent").join("second.wav");
    fs::write(&first_source, b"first").unwrap();
    fs::write(&second_source, b"second").unwrap();
    let transfers = [
        FileTransfer::move_file(first_source.clone(), first_destination.clone()),
        FileTransfer::move_file(second_source.clone(), missing_destination),
    ];

    let result = transfer_files_with_rollback_and_progress(&transfers, |completed, path| {
        if completed == 1 {
            fs::remove_file(path).unwrap();
            fs::write(path, b"replacement").unwrap();
        }
    });

    assert!(result.is_err());
    assert_eq!(fs::read(first_source).unwrap(), b"first");
    assert_eq!(fs::read(first_destination).unwrap(), b"replacement");
    assert_eq!(fs::read(second_source).unwrap(), b"second");
}

#[test]
fn commit_callback_captures_evidence_before_a_later_observer_rewrite() {
    let root = tempdir().unwrap();
    let source = root.path().join("source");
    let target = root.path().join("target");
    fs::create_dir_all(&source).unwrap();
    fs::create_dir_all(&target).unwrap();
    let first_source = source.join("first.wav");
    let second_source = source.join("second.wav");
    let first_destination = target.join("first.wav");
    let second_destination = target.join("second.wav");
    fs::write(&first_source, b"first").unwrap();
    fs::write(&second_source, b"second").unwrap();
    let original_evidence = capture_source_file_evidence(&first_source);
    let transfers = [
        FileTransfer::move_file(first_source, first_destination.clone()),
        FileTransfer::move_file(second_source, second_destination),
    ];
    let mut evidence = Vec::new();

    transfer_files_with_rollback_and_progress_with_commit_callback(
        &transfers,
        |_, _| {},
        |_, after| {
            evidence.push(capture_source_file_evidence(after));
            if after == first_destination {
                fs::write(after, b"replacement").unwrap();
            }
        },
    )
    .expect("moves succeed");

    assert_eq!(evidence[0], original_evidence);
    assert_ne!(
        evidence[0],
        capture_source_file_evidence(&first_destination)
    );
}

#[test]
fn move_file_to_unique_destination_renames_conflicting_extracted_file() {
    let root = tempdir().unwrap();
    let source = root.path().join("source");
    let target = root.path().join("target");
    fs::create_dir_all(&source).unwrap();
    fs::create_dir_all(&target).unwrap();
    let extracted = source.join("loop.wav");
    let existing = target.join("loop.wav");
    fs::write(&extracted, b"extracted").unwrap();
    fs::write(&existing, b"existing").unwrap();

    let moved = move_file_to_unique_destination(&extracted, &target, "Extraction move failed")
        .expect("move extracted file");

    let renamed = target.join("loop_copy001.wav");
    assert_eq!(moved, (extracted.clone(), renamed.clone()));
    assert!(!extracted.exists());
    assert_eq!(fs::read(existing).unwrap(), b"existing");
    assert_eq!(fs::read(renamed).unwrap(), b"extracted");
}
