use std::{
    collections::HashSet,
    path::{Path, PathBuf},
};

use super::{FileMoveConflict, FileMoveItem};
use crate::native_app::sample_library::exclusive_file_transfer::{
    CommittedFile, copy_file_to_unique_destination, move_file_no_replace,
    move_file_to_unique_destination as move_to_unique_destination, unique_copy_candidate,
};

#[derive(Debug, Default)]
pub(super) struct FileMovePlan {
    pub(super) ready: Vec<FileTransfer>,
    pub(super) conflicts: Vec<FileMoveConflict>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct FileTransfer {
    pub(super) source_path: PathBuf,
    pub(super) destination_path: PathBuf,
    pub(super) copy_only: bool,
}

impl FileTransfer {
    fn move_file(source_path: PathBuf, destination_path: PathBuf) -> Self {
        Self {
            source_path,
            destination_path,
            copy_only: false,
        }
    }

    fn copy_file(source_path: PathBuf, destination_path: PathBuf) -> Self {
        Self {
            source_path,
            destination_path,
            copy_only: true,
        }
    }
}

#[derive(Debug)]
pub(super) struct OverwriteBackup {
    destination_path: PathBuf,
    backup: CommittedFile,
}

#[derive(Clone, Debug)]
struct CompletedTransfer {
    transfer: FileTransfer,
    committed: CommittedFile,
}

pub(super) fn file_move_plan_to_folder(
    source_root: &Path,
    source_database_root: &Path,
    file_ids: &[String],
    target_path: &Path,
    target_protected: bool,
) -> Result<FileMovePlan, String> {
    let file_moves = file_ids
        .iter()
        .map(|file_id| FileMoveItem {
            source_root: source_root.to_path_buf(),
            source_database_root: source_database_root.to_path_buf(),
            file_id: file_id.clone(),
            copy_only: false,
        })
        .collect::<Vec<_>>();
    file_move_items_plan_to_folder(&file_moves, target_path, target_protected)
}

pub(super) fn file_move_items_plan_to_folder(
    file_moves: &[FileMoveItem],
    target_path: &Path,
    target_protected: bool,
) -> Result<FileMovePlan, String> {
    let mut plan = FileMovePlan::default();
    let mut seen = HashSet::new();
    let mut planned_destinations = HashSet::new();
    for item in file_moves {
        if !seen.insert(item.file_id.clone()) {
            continue;
        }
        let old_path = PathBuf::from(&item.file_id);
        if !old_path.is_file() {
            return Err(format!(
                "File move failed: {} is missing",
                old_path
                    .file_name()
                    .map(|name| name.to_string_lossy().to_string())
                    .unwrap_or_else(|| old_path.display().to_string())
            ));
        }
        if old_path.parent() == Some(target_path) {
            continue;
        }
        let Some(file_name) = old_path.file_name() else {
            return Err(String::from("File move failed: source file has no name"));
        };
        let mut new_path = target_path.join(file_name);
        if item.copy_only {
            new_path = unique_planned_destination(new_path, &mut planned_destinations);
            plan.ready.push(FileTransfer::copy_file(old_path, new_path));
        } else if new_path.exists() || !planned_destinations.insert(new_path.clone()) {
            plan.conflicts.push(FileMoveConflict {
                source_root: item.source_root.clone(),
                source_database_root: item.source_database_root.clone(),
                source_path: old_path,
                destination_path: new_path,
                destination_protected: target_protected,
            });
        } else {
            plan.ready.push(FileTransfer::move_file(old_path, new_path));
        }
    }
    Ok(plan)
}

#[cfg(test)]
pub(super) fn rename_files_with_rollback(
    moves: &[(PathBuf, PathBuf)],
) -> Result<Vec<(PathBuf, PathBuf)>, String> {
    rename_files_with_rollback_and_progress(moves, |_, _| {})
}

pub(super) fn rename_files_with_rollback_and_progress(
    moves: &[(PathBuf, PathBuf)],
    progress: impl FnMut(usize, &Path),
) -> Result<Vec<(PathBuf, PathBuf)>, String> {
    let transfers = moves
        .iter()
        .map(|(source_path, destination_path)| {
            FileTransfer::move_file(source_path.clone(), destination_path.clone())
        })
        .collect::<Vec<_>>();
    transfer_files_with_rollback_and_progress(&transfers, progress)
}

pub(super) fn transfer_files_with_rollback_and_progress(
    transfers: &[FileTransfer],
    progress: impl FnMut(usize, &Path),
) -> Result<Vec<(PathBuf, PathBuf)>, String> {
    transfer_files_with_rollback_and_progress_with_commit_callback(transfers, progress, |_, _| {})
}

pub(super) fn transfer_files_with_rollback_and_progress_with_commit_callback(
    transfers: &[FileTransfer],
    mut progress: impl FnMut(usize, &Path),
    mut on_commit: impl FnMut(&Path, &Path),
) -> Result<Vec<(PathBuf, PathBuf)>, String> {
    let mut completed = Vec::new();
    for transfer in transfers {
        let result = if transfer.copy_only {
            let first_candidate = transfer
                .source_path
                .file_name()
                .and_then(|file_name| {
                    transfer
                        .destination_path
                        .parent()
                        .map(|parent| parent.join(file_name))
                })
                .unwrap_or_else(|| transfer.destination_path.clone());
            copy_file_to_unique_destination(&transfer.source_path, &first_candidate)
                .map_err(|err| format!("File copy failed: {err}"))
        } else {
            move_file_with_prefix(
                &transfer.source_path,
                &transfer.destination_path,
                "File move failed",
            )
        };
        let committed = match result {
            Ok(committed) => committed,
            Err(error) => {
                rollback_completed_file_transfers(&completed);
                return Err(error);
            }
        };
        on_commit(&transfer.source_path, committed.path());
        progress(completed.len() + 1, committed.path());
        completed.push(CompletedTransfer {
            transfer: transfer.clone(),
            committed,
        });
    }
    Ok(completed
        .into_iter()
        .map(|completed| {
            (
                completed.transfer.source_path,
                completed.committed.path().to_path_buf(),
            )
        })
        .collect())
}

pub(super) fn move_file_to_unique_destination(
    source_path: &Path,
    target_folder: &Path,
    error_prefix: &'static str,
) -> Result<(PathBuf, PathBuf), String> {
    let Some(file_name) = source_path.file_name() else {
        return Err(format!("{error_prefix}: file has no name"));
    };
    let first_candidate = target_folder.join(file_name);
    let committed = move_to_unique_destination(source_path, &first_candidate)
        .map_err(|err| format!("{error_prefix}: {err}"))?;
    Ok((source_path.to_path_buf(), committed.path().to_path_buf()))
}

fn rollback_completed_file_transfers(completed: &[CompletedTransfer]) {
    for completed in completed.iter().rev() {
        if completed.transfer.copy_only {
            let _ = completed.committed.remove_if_owned();
        } else {
            let _ = completed
                .committed
                .move_back_if_owned(&completed.transfer.source_path);
        }
    }
}

fn move_file_with_prefix(
    source: &Path,
    destination: &Path,
    error_prefix: &'static str,
) -> Result<CommittedFile, String> {
    move_file_no_replace(source, destination).map_err(|err| format!("{error_prefix}: {err}"))
}

pub(super) fn unique_destination(first_candidate: &Path) -> PathBuf {
    for index in 0.. {
        let candidate = unique_copy_candidate(first_candidate, index);
        if !candidate.exists() {
            return candidate;
        }
    }
    unreachable!("unbounded copy suffix search should find a destination")
}

fn unique_planned_destination(
    first_candidate: PathBuf,
    planned_destinations: &mut HashSet<PathBuf>,
) -> PathBuf {
    if !first_candidate.exists() && planned_destinations.insert(first_candidate.clone()) {
        return first_candidate;
    }
    for count in 1.. {
        let candidate = unique_copy_candidate(&first_candidate, count);
        if !candidate.exists() && planned_destinations.insert(candidate.clone()) {
            return candidate;
        }
    }
    unreachable!("unbounded copy suffix search should find a destination")
}

pub(super) fn move_existing_destination_to_backup(
    destination_path: &Path,
) -> Result<OverwriteBackup, String> {
    for count in 1..10_000 {
        let backup_path = overwrite_backup_path(destination_path, count);
        match move_file_no_replace(destination_path, &backup_path) {
            Ok(backup) => {
                return Ok(OverwriteBackup {
                    destination_path: destination_path.to_path_buf(),
                    backup,
                });
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(format!("File overwrite failed: {error}")),
        }
    }
    Err(String::from(
        "File overwrite failed: could not reserve a backup path",
    ))
}

pub(super) fn move_file_over_backup(
    source_path: &Path,
    destination_path: &Path,
) -> Result<(), String> {
    move_file_with_prefix(source_path, destination_path, "File overwrite failed").map(|_| ())
}

pub(super) fn restore_overwrite_backup(backup: &OverwriteBackup) {
    let _ = backup.backup.move_back_if_owned(&backup.destination_path);
}

pub(super) fn remove_overwrite_backup(backup: &OverwriteBackup) {
    let _ = backup.backup.remove_if_owned();
}

fn overwrite_backup_path(destination_path: &Path, count: usize) -> PathBuf {
    let parent = destination_path.parent().unwrap_or_else(|| Path::new(""));
    let file_name = destination_path
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| String::from("file"));
    parent.join(format!(
        ".wavecrate-overwrite-backup-{count:03}-{file_name}"
    ))
}

#[cfg(test)]
mod tests;
