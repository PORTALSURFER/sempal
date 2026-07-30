use std::path::{Path, PathBuf};

#[cfg(test)]
use super::file_move_progress::ignore_file_move_progress;
use super::{
    FileMoveConflict, FileMoveConflictBatch, FileMoveConflictCompletion,
    FileMoveConflictExecutionFailure, FileMoveConflictExecutionSuccess, FileMoveConflictResolution,
    FileMoveConflictResolutionRequest, FolderMoveCompletion, FolderMoveRequest, FolderMoveSuccess,
    drag_drop_relocation::{persist_moved_file_metadata, persist_moved_folders_metadata},
    drag_drop_sourced_files::{
        SourcedMovedFile, sourced_file_move_metadata_from_sourced_moves,
        sourced_moved_files_from_items,
    },
    file_move_progress::{
        FileMoveProgressReporter, file_move_conflict_progress_label,
        file_move_conflict_progress_total, folder_move_progress_label,
    },
    file_move_transaction::{
        FileMovePlan, file_move_items_plan_to_folder, file_move_plan_to_folder,
    },
    file_move_transaction::{
        move_existing_destination_to_backup, move_file_over_backup,
        move_file_to_unique_destination, remove_overwrite_backup,
        rename_files_with_rollback_and_progress, restore_overwrite_backup,
        transfer_files_with_rollback_and_progress_with_commit_callback, unique_destination,
    },
};
use crate::native_app::{
    app::FileMoveProgress, sample_library::file_actions::sample_path_label,
    waveform::remap_persisted_waveform_cache_after_move,
};
use wavecrate::sample_sources::{SourceFileEvidence, capture_source_file_evidence};

#[cfg(test)]
pub(in crate::native_app) fn execute_folder_move_request(
    request: FolderMoveRequest,
) -> FolderMoveCompletion {
    execute_folder_move_request_with_progress(request, 0, ignore_file_move_progress)
}

pub(in crate::native_app) fn execute_folder_move_request_with_progress(
    request: FolderMoveRequest,
    task_id: u64,
    emit: impl Fn(FileMoveProgress) -> bool,
) -> FolderMoveCompletion {
    let reporter =
        FileMoveProgressReporter::new(task_id, folder_move_progress_label(&request), emit);
    let result = execute_folder_move_request_result(&request, &reporter);
    FolderMoveCompletion {
        task_id,
        request,
        result,
    }
}

fn execute_folder_move_request_result<Emit>(
    request: &FolderMoveRequest,
    reporter: &FileMoveProgressReporter<Emit>,
) -> Result<FolderMoveSuccess, String>
where
    Emit: Fn(FileMoveProgress) -> bool,
{
    match request {
        FolderMoveRequest::Folder {
            source_root,
            source_database_root,
            moves,
            ..
        } => execute_folder_move(source_root, source_database_root, moves, reporter),
        FolderMoveRequest::Files {
            source_root,
            source_database_root,
            file_ids,
            target_folder,
            target_protected,
            remove_from_collection,
        } => execute_file_drop(
            source_root,
            source_database_root,
            file_ids,
            target_folder,
            *target_protected,
            *remove_from_collection,
            reporter,
        ),
        FolderMoveRequest::SourcedFiles {
            target_source_root,
            target_source_database_root,
            file_moves,
            target_folder,
            target_protected,
            remove_from_collection,
        } => execute_sourced_file_drop(
            target_source_root,
            target_source_database_root,
            file_moves,
            target_folder,
            *target_protected,
            *remove_from_collection,
            reporter,
        ),
        FolderMoveRequest::ExtractedFile {
            source_root,
            source_database_root,
            path,
            target_folder,
        } => execute_extracted_file_drop(
            source_root,
            source_database_root,
            path,
            target_folder,
            reporter,
        ),
    }
}

fn execute_folder_move<Emit>(
    source_root: &Path,
    source_database_root: &Path,
    moves: &[(PathBuf, PathBuf)],
    reporter: &FileMoveProgressReporter<Emit>,
) -> Result<FolderMoveSuccess, String>
where
    Emit: Fn(FileMoveProgress) -> bool,
{
    for (old_path, new_path) in moves {
        if !old_path.is_dir() {
            return Err(format!(
                "Folder move failed: {} is missing",
                sample_path_label(old_path)
            ));
        }
        if new_path.exists() {
            let folder_name = new_path
                .file_name()
                .map(|name| name.to_string_lossy().to_string())
                .unwrap_or_else(|| new_path.display().to_string());
            return Err(format!("Folder move failed: {folder_name} already exists"));
        }
    }
    let total = moves.len().saturating_add(2).max(2);
    let mut moved_paths = Vec::with_capacity(moves.len());
    let mut moved_path_evidence = Vec::with_capacity(moves.len());
    for (index, (old_path, new_path)) in moves.iter().enumerate() {
        reporter.emit(
            index,
            total,
            format!("Moving {}", sample_path_label(old_path)),
        );
        if let Err(error) = std::fs::rename(old_path, new_path) {
            let rollback_error = rollback_moved_folders(&moved_paths);
            return Err(match rollback_error {
                Some(rollback_error) => {
                    format!("Folder move failed: {error}; rollback failed: {rollback_error}")
                }
                None => format!("Folder move failed: {error}"),
            });
        }
        let new_path = new_path.to_path_buf();
        moved_paths.push((old_path.to_path_buf(), new_path.clone()));
        moved_path_evidence.push((new_path.clone(), capture_source_file_evidence(&new_path)));
    }
    let completed_moves = moved_paths.len();
    reporter.emit(
        completed_moves,
        total,
        String::from("Preserving waveform cache"),
    );
    remap_persisted_waveform_cache_for_moves(&moved_paths);
    reporter.emit(
        completed_moves + 1,
        total,
        String::from("Updating metadata"),
    );
    let metadata_error =
        persist_moved_folders_metadata(source_root, source_database_root, &moved_paths).err();
    reporter.emit(total, total, String::from("Done"));
    Ok(FolderMoveSuccess {
        moved_paths,
        moved_path_evidence,
        conflicts: Vec::new(),
        metadata_error,
    })
}

fn rollback_moved_folders(moved_paths: &[(PathBuf, PathBuf)]) -> Option<String> {
    let mut errors = Vec::new();
    for (old_path, new_path) in moved_paths.iter().rev() {
        if let Err(error) = std::fs::rename(new_path, old_path) {
            errors.push(format!("{}: {error}", sample_path_label(new_path)));
        }
    }
    (!errors.is_empty()).then(|| errors.join("; "))
}

fn capture_moved_path_evidence(
    moved_paths: &[(PathBuf, PathBuf)],
) -> Vec<(PathBuf, SourceFileEvidence)> {
    moved_paths
        .iter()
        .map(|(_, after)| (after.clone(), capture_source_file_evidence(after)))
        .collect()
}

pub(in crate::native_app) fn execute_folder_move_transaction(
    source_root: &Path,
    source_database_root: &Path,
    moves: &[(PathBuf, PathBuf)],
) -> Result<(Vec<(PathBuf, PathBuf)>, Option<String>), String> {
    let completed = rename_folders_with_rollback(moves)?;
    let metadata_error =
        persist_moved_folders_metadata(source_root, source_database_root, &completed).err();
    Ok((completed, metadata_error))
}

fn rename_folders_with_rollback(
    moves: &[(PathBuf, PathBuf)],
) -> Result<Vec<(PathBuf, PathBuf)>, String> {
    let mut completed = Vec::new();
    for (old_path, new_path) in moves {
        if !old_path.is_dir() {
            rollback_moved_folders_for_transaction(&completed);
            return Err(format!(
                "Folder move failed: {} is missing",
                old_path.display()
            ));
        }
        if new_path.exists() {
            rollback_moved_folders_for_transaction(&completed);
            return Err(format!(
                "Folder move failed: {} already exists",
                new_path.display()
            ));
        }
        if let Some(parent) = new_path.parent() {
            std::fs::create_dir_all(parent).map_err(|err| format!("Folder move failed: {err}"))?;
        }
        if let Err(error) = std::fs::rename(old_path, new_path) {
            rollback_moved_folders_for_transaction(&completed);
            return Err(format!("Folder move failed: {error}"));
        }
        completed.push((old_path.clone(), new_path.clone()));
    }
    Ok(completed)
}

fn rollback_moved_folders_for_transaction(completed: &[(PathBuf, PathBuf)]) {
    for (old_path, new_path) in completed.iter().rev() {
        let _ = std::fs::rename(new_path, old_path);
    }
}

fn execute_file_drop<Emit>(
    source_root: &Path,
    source_database_root: &Path,
    file_ids: &[String],
    target_folder: &Path,
    target_protected: bool,
    remove_from_collection: Option<wavecrate::sample_sources::SampleCollection>,
    reporter: &FileMoveProgressReporter<Emit>,
) -> Result<FolderMoveSuccess, String>
where
    Emit: Fn(FileMoveProgress) -> bool,
{
    if !target_folder.is_dir() {
        return Err(String::from("File move failed: target folder is missing"));
    }
    let plan = file_move_plan_to_folder(
        source_root,
        source_database_root,
        file_ids,
        target_folder,
        target_protected,
    )?;
    let total = file_move_work_total(&plan);
    reporter.emit(0, total, String::from("Moving files"));
    let mut moved_path_evidence = Vec::new();
    let moved_paths = transfer_files_with_rollback_and_progress_with_commit_callback(
        &plan.ready,
        |completed, path| {
            reporter.emit(
                completed,
                total,
                format!("Moved {}", sample_path_label(path)),
            );
        },
        |_, after| {
            moved_path_evidence.push((after.to_path_buf(), capture_source_file_evidence(after)));
        },
    )?;
    let checked = moved_paths.len().saturating_add(plan.conflicts.len());
    reporter.emit(checked, total, String::from("Preserving waveform cache"));
    remap_persisted_waveform_cache_for_moves(&moved_paths);
    reporter.emit(checked, total, String::from("Updating metadata"));
    let metadata_error = persist_moved_file_metadata(
        source_root,
        source_database_root,
        &moved_paths,
        remove_from_collection,
    )
    .err();
    reporter.emit(total, total, String::from("Done"));
    Ok(FolderMoveSuccess {
        moved_paths,
        moved_path_evidence,
        conflicts: plan.conflicts,
        metadata_error,
    })
}

fn execute_sourced_file_drop<Emit>(
    target_source_root: &Path,
    target_source_database_root: &Path,
    file_moves: &[super::FileMoveItem],
    target_folder: &Path,
    target_protected: bool,
    remove_from_collection: Option<wavecrate::sample_sources::SampleCollection>,
    reporter: &FileMoveProgressReporter<Emit>,
) -> Result<FolderMoveSuccess, String>
where
    Emit: Fn(FileMoveProgress) -> bool,
{
    if !target_folder.is_dir() {
        return Err(String::from("File move failed: target folder is missing"));
    }
    let plan = file_move_items_plan_to_folder(file_moves, target_folder, target_protected)?;
    let total = file_move_work_total(&plan);
    reporter.emit(0, total, String::from("Moving files"));
    let mut moved_path_evidence = Vec::new();
    let moved_paths = transfer_files_with_rollback_and_progress_with_commit_callback(
        &plan.ready,
        |completed, path| {
            reporter.emit(
                completed,
                total,
                format!("Moved {}", sample_path_label(path)),
            );
        },
        |_, after| {
            moved_path_evidence.push((after.to_path_buf(), capture_source_file_evidence(after)));
        },
    )?;
    let checked = moved_paths.len().saturating_add(plan.conflicts.len());
    reporter.emit(checked, total, String::from("Preserving waveform cache"));
    remap_persisted_waveform_cache_for_moves(&moved_paths);
    reporter.emit(checked, total, String::from("Updating metadata"));
    let sourced_moves = sourced_moved_files_from_items(file_moves, &moved_paths);
    let metadata_moves = sourced_file_move_metadata_from_sourced_moves(&sourced_moves);
    let metadata_error = wavecrate::sample_sources::persist_sourced_moved_file_metadata(
        target_source_root,
        target_source_database_root,
        &metadata_moves,
        remove_from_collection,
    )
    .err();
    reporter.emit(total, total, String::from("Done"));
    Ok(FolderMoveSuccess {
        moved_paths,
        moved_path_evidence,
        conflicts: plan.conflicts,
        metadata_error,
    })
}

fn execute_extracted_file_drop<Emit>(
    source_root: &Path,
    source_database_root: &Path,
    path: &Path,
    target_folder: &Path,
    reporter: &FileMoveProgressReporter<Emit>,
) -> Result<FolderMoveSuccess, String>
where
    Emit: Fn(FileMoveProgress) -> bool,
{
    if !path.is_file() {
        return Err(format!(
            "Sample move failed: {} is missing",
            path.file_name()
                .map(|name| name.to_string_lossy().to_string())
                .unwrap_or_else(|| path.display().to_string())
        ));
    }
    if !target_folder.is_dir() {
        return Err(String::from("Sample move failed: target folder is missing"));
    }
    reporter.emit(0, 2, format!("Moving {}", sample_path_label(path)));
    let completed = move_file_to_unique_destination(path, target_folder, "Sample move failed")?;
    let moved_paths = vec![completed];
    let moved_path_evidence = capture_moved_path_evidence(&moved_paths);
    reporter.emit(1, 2, String::from("Preserving waveform cache"));
    remap_persisted_waveform_cache_for_moves(&moved_paths);
    reporter.emit(1, 2, String::from("Updating metadata"));
    let metadata_error =
        persist_moved_file_metadata(source_root, source_database_root, &moved_paths, None).err();
    reporter.emit(2, 2, String::from("Done"));
    Ok(FolderMoveSuccess {
        moved_paths,
        moved_path_evidence,
        conflicts: Vec::new(),
        metadata_error,
    })
}

#[cfg(test)]
pub(in crate::native_app) fn execute_file_move_conflict_request(
    batch: FileMoveConflictBatch,
    request: FileMoveConflictResolutionRequest,
) -> FileMoveConflictCompletion {
    execute_file_move_conflict_request_with_progress(batch, request, 0, ignore_file_move_progress)
}

pub(in crate::native_app) fn execute_file_move_conflict_request_with_progress(
    mut batch: FileMoveConflictBatch,
    request: FileMoveConflictResolutionRequest,
    task_id: u64,
    emit: impl Fn(FileMoveProgress) -> bool,
) -> FileMoveConflictCompletion {
    let reporter = FileMoveProgressReporter::new(
        task_id,
        file_move_conflict_progress_label(&batch, request),
        emit,
    );
    if request.apply_to_remaining {
        batch.batch_policy = Some(request.resolution);
    }

    let total = file_move_conflict_progress_total(&batch, request);
    let mut moved_paths = Vec::new();
    let mut moved_path_evidence = Vec::new();
    let mut last_resolution = request.resolution;
    loop {
        let Some(conflict) = batch.conflicts.get(batch.current_index).cloned() else {
            break;
        };
        let completed_conflicts = batch.resolved_count.saturating_add(batch.skipped_count);
        reporter.emit(
            completed_conflicts,
            total,
            format!("Resolving {}", sample_path_label(&conflict.source_path)),
        );
        let resolution = batch.batch_policy.unwrap_or(request.resolution);
        let completed = match execute_file_move_conflict(&conflict, resolution) {
            Ok(completed) => completed,
            Err(error) => {
                batch.batch_policy = None;
                let metadata_error =
                    persist_conflict_moved_file_metadata(&batch, &moved_paths).err();
                return FileMoveConflictCompletion {
                    task_id,
                    result: Err(FileMoveConflictExecutionFailure {
                        batch,
                        moved_paths,
                        moved_path_evidence,
                        error,
                        metadata_error,
                    }),
                };
            }
        };
        moved_path_evidence.extend(capture_moved_path_evidence(&completed));
        remap_persisted_waveform_cache_for_moves(&completed);
        match resolution {
            FileMoveConflictResolution::Overwrite | FileMoveConflictResolution::Rename => {
                batch.resolved_count += 1;
            }
            FileMoveConflictResolution::Skip => {
                batch.skipped_count += 1;
            }
        }
        batch.current_index += 1;
        last_resolution = resolution;
        moved_paths.extend(completed);
        let completed_conflicts = batch.resolved_count.saturating_add(batch.skipped_count);
        reporter.emit(
            completed_conflicts,
            total,
            String::from("Resolving conflicts"),
        );
        if batch.batch_policy.is_none() {
            break;
        }
    }
    let completed_conflicts = batch.resolved_count.saturating_add(batch.skipped_count);
    reporter.emit(
        completed_conflicts,
        total,
        String::from("Updating metadata"),
    );
    let metadata_error = persist_conflict_moved_file_metadata(&batch, &moved_paths).err();
    reporter.emit(total, total, String::from("Done"));

    FileMoveConflictCompletion {
        task_id,
        result: Ok(FileMoveConflictExecutionSuccess {
            batch,
            moved_paths,
            moved_path_evidence,
            last_resolution,
            metadata_error,
        }),
    }
}

fn execute_file_move_conflict(
    conflict: &FileMoveConflict,
    resolution: FileMoveConflictResolution,
) -> Result<Vec<(PathBuf, PathBuf)>, String> {
    match resolution {
        FileMoveConflictResolution::Overwrite => execute_overwrite_conflict(conflict),
        FileMoveConflictResolution::Rename => execute_rename_conflict(conflict),
        FileMoveConflictResolution::Skip => Ok(Vec::new()),
    }
}

fn execute_overwrite_conflict(
    conflict: &FileMoveConflict,
) -> Result<Vec<(PathBuf, PathBuf)>, String> {
    if conflict.destination_protected {
        return Err(String::from(
            "This source is protected. Copy to Primary and continue?",
        ));
    }
    let backup = move_existing_destination_to_backup(&conflict.destination_path)?;
    if let Err(error) = move_file_over_backup(&conflict.source_path, &conflict.destination_path) {
        restore_overwrite_backup(&backup);
        return Err(error);
    }

    let completed = vec![(
        conflict.source_path.clone(),
        conflict.destination_path.clone(),
    )];
    remove_overwrite_backup(&backup);
    Ok(completed)
}

fn execute_rename_conflict(conflict: &FileMoveConflict) -> Result<Vec<(PathBuf, PathBuf)>, String> {
    let destination = unique_destination(&conflict.destination_path);
    let move_pair = (conflict.source_path.clone(), destination);
    rename_files_with_rollback_and_progress(std::slice::from_ref(&move_pair), |_, _| {})
}

fn persist_conflict_moved_file_metadata(
    batch: &FileMoveConflictBatch,
    moved_paths: &[(PathBuf, PathBuf)],
) -> Result<(), String> {
    let sourced_moves = sourced_moved_files_from_conflicts(&batch.conflicts, moved_paths);
    let metadata_moves = sourced_file_move_metadata_from_sourced_moves(&sourced_moves);
    wavecrate::sample_sources::persist_sourced_moved_file_metadata(
        &batch.source_root,
        &batch.source_database_root,
        &metadata_moves,
        batch.remove_from_collection,
    )
}

pub(super) fn sourced_moved_files_from_conflicts(
    conflicts: &[FileMoveConflict],
    moved_paths: &[(PathBuf, PathBuf)],
) -> Vec<SourcedMovedFile> {
    let source_roots = conflicts
        .iter()
        .map(|conflict| {
            (
                conflict.source_path.clone(),
                (
                    conflict.source_root.clone(),
                    conflict.source_database_root.clone(),
                ),
            )
        })
        .collect::<std::collections::HashMap<_, _>>();
    moved_paths
        .iter()
        .filter_map(|(old_path, new_path)| {
            source_roots
                .get(old_path)
                .cloned()
                .map(|(source_root, source_database_root)| SourcedMovedFile {
                    source_root,
                    source_database_root,
                    old_path: old_path.clone(),
                    new_path: new_path.clone(),
                    copy_only: false,
                })
        })
        .collect()
}

fn file_move_work_total(plan: &FileMovePlan) -> usize {
    plan.ready
        .len()
        .saturating_add(plan.conflicts.len())
        .saturating_add(1)
        .max(1)
}

fn remap_persisted_waveform_cache_for_moves(moved_paths: &[(PathBuf, PathBuf)]) {
    for (old_path, new_path) in moved_paths {
        remap_persisted_waveform_cache_after_move(old_path, new_path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs,
        time::{SystemTime, UNIX_EPOCH},
    };

    fn temp_dir(name: &str) -> PathBuf {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock before epoch")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("{name}-{suffix}"));
        fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }

    #[test]
    fn ready_file_moves_execute_as_worker_request() {
        let root = temp_dir("wavecrate-file-move-execution-ready-rollback");
        let source_dir = root.join("source");
        let target_dir = root.join("target");
        fs::create_dir_all(&source_dir).expect("create source");
        fs::create_dir_all(&target_dir).expect("create target");
        let source = source_dir.join("kick.wav");
        let destination = target_dir.join("kick.wav");
        fs::write(&source, b"source").expect("write source");

        let request = FolderMoveRequest::Files {
            source_root: root.clone(),
            source_database_root: root.clone(),
            file_ids: vec![source.display().to_string()],
            target_folder: target_dir,
            target_protected: false,
            remove_from_collection: None,
        };
        let result = execute_folder_move_request(request).result;

        let success = result.expect("move succeeds");
        assert_eq!(
            success.moved_paths,
            vec![(source.clone(), destination.clone())]
        );
        assert_eq!(
            success.moved_path_evidence,
            vec![(
                destination.clone(),
                capture_source_file_evidence(&destination)
            )]
        );
        assert!(!source.exists());
        assert_eq!(fs::read(&destination).expect("read destination"), b"source");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn ready_file_moves_emit_progress_messages() {
        let root = temp_dir("wavecrate-file-move-execution-progress");
        let source_dir = root.join("source");
        let target_dir = root.join("target");
        fs::create_dir_all(&source_dir).expect("create source");
        fs::create_dir_all(&target_dir).expect("create target");
        let first = source_dir.join("first.wav");
        let second = source_dir.join("second.wav");
        fs::write(&first, b"first").expect("write first");
        fs::write(&second, b"second").expect("write second");
        let request = FolderMoveRequest::Files {
            source_root: root.clone(),
            source_database_root: root.clone(),
            file_ids: vec![first.display().to_string(), second.display().to_string()],
            target_folder: target_dir,
            target_protected: false,
            remove_from_collection: None,
        };
        let (sender, receiver) = std::sync::mpsc::channel();

        let completion = execute_folder_move_request_with_progress(request, 42, move |progress| {
            sender.send(progress).is_ok()
        });

        assert!(
            completion
                .result
                .expect("move succeeds")
                .metadata_error
                .is_none()
        );
        let progress = receiver.try_iter().collect::<Vec<_>>();
        assert!(
            progress.iter().any(|progress| progress.completed > 0),
            "file move worker should stream progress: {progress:?}"
        );
        assert!(
            progress
                .last()
                .is_some_and(|progress| progress.completed == progress.total),
            "file move worker should finish progress at total: {progress:?}"
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn extracted_file_move_captures_evidence_before_preservation_progress() {
        let root = temp_dir("wavecrate-extracted-file-move-evidence");
        let source_dir = root.join("source");
        let target_dir = root.join("target");
        fs::create_dir_all(&source_dir).expect("create source");
        fs::create_dir_all(&target_dir).expect("create target");
        let source = source_dir.join("kick.wav");
        let destination = target_dir.join("kick.wav");
        fs::write(&source, b"source").expect("write source");
        let original_evidence = capture_source_file_evidence(&source);
        let request = FolderMoveRequest::ExtractedFile {
            source_root: root.clone(),
            source_database_root: root.clone(),
            path: source,
            target_folder: target_dir,
        };

        let completion = execute_folder_move_request_with_progress(request, 0, |progress| {
            if progress.completed == 1 {
                fs::write(&destination, b"replacement").expect("rewrite destination");
            }
            true
        });
        let success = completion.result.expect("move succeeds");

        assert_eq!(
            success.moved_path_evidence,
            vec![(destination, original_evidence)]
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn folder_move_transaction_renames_folders_and_creates_missing_parent() {
        let root = temp_dir("wavecrate-folder-move-transaction-success");
        let source = root.join("source");
        let destination = root.join("missing-parent").join("source");
        fs::create_dir_all(&source).expect("create source");
        fs::write(source.join("kick.wav"), b"kick").expect("write nested sample");

        let (moved, metadata_error) =
            execute_folder_move_transaction(&root, &root, &[(source.clone(), destination.clone())])
                .expect("move folder transaction");

        assert_eq!(moved, vec![(source.clone(), destination.clone())]);
        assert!(metadata_error.is_none());
        assert!(!source.exists());
        assert_eq!(
            fs::read(destination.join("kick.wav")).expect("read moved sample"),
            b"kick"
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn folder_move_transaction_reports_destination_conflict() {
        let root = temp_dir("wavecrate-folder-move-transaction-conflict");
        let source = root.join("source");
        let destination = root.join("destination");
        fs::create_dir_all(&source).expect("create source");
        fs::create_dir_all(&destination).expect("create destination");

        let error =
            execute_folder_move_transaction(&root, &root, &[(source.clone(), destination.clone())])
                .expect_err("destination conflict should fail");

        assert!(error.contains("already exists"), "{error}");
        assert!(source.exists());
        assert!(destination.exists());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn folder_move_transaction_rolls_back_completed_folders_after_later_failure() {
        let root = temp_dir("wavecrate-folder-move-transaction-rollback");
        let first_source = root.join("first");
        let second_source = root.join("missing");
        let first_destination = root.join("target").join("first");
        let second_destination = root.join("target").join("missing");
        fs::create_dir_all(&first_source).expect("create first source");
        fs::write(first_source.join("kick.wav"), b"kick").expect("write nested sample");

        let error = execute_folder_move_transaction(
            &root,
            &root,
            &[
                (first_source.clone(), first_destination.clone()),
                (second_source.clone(), second_destination),
            ],
        )
        .expect_err("later missing folder should fail");

        assert!(error.contains("is missing"), "{error}");
        assert_eq!(
            fs::read(first_source.join("kick.wav")).expect("first folder rolled back"),
            b"kick"
        );
        assert!(!first_destination.exists());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn overwrite_conflict_executes_as_worker_request() {
        let root = temp_dir("wavecrate-file-move-execution-overwrite-rollback");
        let source_dir = root.join("source");
        let target_dir = root.join("target");
        fs::create_dir_all(&source_dir).expect("create source");
        fs::create_dir_all(&target_dir).expect("create target");
        let source = source_dir.join("kick.wav");
        let destination = target_dir.join("kick.wav");
        fs::write(&source, b"source").expect("write source");
        fs::write(&destination, b"destination").expect("write destination");
        let conflict = FileMoveConflict {
            source_root: root.clone(),
            source_database_root: root.clone(),
            source_path: source.clone(),
            destination_path: destination.clone(),
            destination_protected: false,
        };

        let batch = FileMoveConflictBatch {
            source_root: root.clone(),
            source_database_root: root.clone(),
            target_folder: target_dir.clone(),
            remove_from_collection: None,
            conflicts: vec![conflict],
            current_index: 0,
            resolved_count: 0,
            skipped_count: 0,
            batch_policy: None,
        };
        let result = execute_file_move_conflict_request(
            batch,
            FileMoveConflictResolutionRequest::new(FileMoveConflictResolution::Overwrite, false),
        );

        let success = result.result.expect("overwrite succeeds");
        assert_eq!(
            success.moved_paths,
            vec![(source.clone(), destination.clone())]
        );
        assert_eq!(
            success.moved_path_evidence,
            vec![(
                destination.clone(),
                capture_source_file_evidence(&destination)
            )]
        );
        assert!(!source.exists());
        assert_eq!(fs::read(&destination).expect("read destination"), b"source");
        assert!(
            fs::read_dir(&target_dir)
                .expect("read target")
                .all(|entry| !entry
                    .expect("entry")
                    .file_name()
                    .to_string_lossy()
                    .starts_with(".wavecrate-overwrite-backup-")),
            "overwrite backup should be restored and removed from view"
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn protected_destination_overwrite_conflict_is_blocked() {
        let root = temp_dir("wavecrate-file-move-execution-protected-overwrite");
        let source_dir = root.join("source");
        let target_dir = root.join("target");
        fs::create_dir_all(&source_dir).expect("create source");
        fs::create_dir_all(&target_dir).expect("create target");
        let source = source_dir.join("kick.wav");
        let destination = target_dir.join("kick.wav");
        fs::write(&source, b"source").expect("write source");
        fs::write(&destination, b"destination").expect("write destination");
        let conflict = FileMoveConflict {
            source_root: root.clone(),
            source_database_root: root.clone(),
            source_path: source.clone(),
            destination_path: destination.clone(),
            destination_protected: true,
        };

        let batch = FileMoveConflictBatch {
            source_root: root.clone(),
            source_database_root: root.clone(),
            target_folder: target_dir.clone(),
            remove_from_collection: None,
            conflicts: vec![conflict],
            current_index: 0,
            resolved_count: 0,
            skipped_count: 0,
            batch_policy: None,
        };
        let result = execute_file_move_conflict_request(
            batch,
            FileMoveConflictResolutionRequest::new(FileMoveConflictResolution::Overwrite, false),
        );

        let failure = result.result.expect_err("protected overwrite is blocked");
        assert_eq!(
            failure.error,
            "This source is protected. Copy to Primary and continue?"
        );
        assert_eq!(fs::read(&source).expect("read source"), b"source");
        assert_eq!(
            fs::read(&destination).expect("read destination"),
            b"destination"
        );
        let _ = fs::remove_dir_all(root);
    }
}
