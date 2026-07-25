use self::folder_sample_move_task::prepare_folder_sample_move_transaction;
use crate::app::controller::jobs::{
    FileOpMessage, FolderMoveRequest, FolderMoveResult, FolderSampleMoveRequest,
    FolderSampleMoveResult,
};
use crate::sample_sources::{SourceDatabase, SourceId};
use std::path::PathBuf;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc::Sender,
};

/// Folder-level move worker implementation split from the sample-move worker.
mod folder_move_task;
/// Staged folder-sample move transaction helpers split from the main worker loop.
mod folder_sample_move_task;

/// Delegate folder-level move execution to the dedicated folder worker module.
pub(super) fn run_folder_move_task(
    request: FolderMoveRequest,
    cancel: Arc<AtomicBool>,
    sender: Option<&Sender<FileOpMessage>>,
) -> FolderMoveResult {
    folder_move_task::run_folder_move_task(request, cancel, sender, &mut NoopFolderMoveHooks)
}

#[cfg(test)]
pub(super) fn run_folder_move_task_with_hooks(
    request: FolderMoveRequest,
    cancel: Arc<AtomicBool>,
    sender: Option<&Sender<FileOpMessage>>,
    hooks: &mut FolderMoveTestHooks,
) -> FolderMoveResult {
    folder_move_task::run_folder_move_task(request, cancel, sender, hooks)
}

pub(super) trait FolderMoveHooks {
    fn before_folder_move_batch(&mut self) {}

    fn before_folder_sample_batch(&mut self) {}

    fn before_folder_sample_finalize(&mut self) {}
}

struct NoopFolderMoveHooks;

impl FolderMoveHooks for NoopFolderMoveHooks {}

#[cfg(test)]
#[derive(Default)]
pub(super) struct FolderMoveTestHooks {
    pub(super) before_folder_move_batch: Option<Box<dyn FnMut() + Send>>,
    pub(super) before_folder_sample_batch: Option<Box<dyn FnMut() + Send>>,
    pub(super) before_folder_sample_finalize: Option<Box<dyn FnMut() + Send>>,
}

#[cfg(test)]
impl FolderMoveHooks for FolderMoveTestHooks {
    fn before_folder_move_batch(&mut self) {
        if let Some(mut hook) = self.before_folder_move_batch.take() {
            hook();
        }
    }

    fn before_folder_sample_batch(&mut self) {
        if let Some(mut hook) = self.before_folder_sample_batch.take() {
            hook();
        }
    }

    fn before_folder_sample_finalize(&mut self) {
        if let Some(mut hook) = self.before_folder_sample_finalize.take() {
            hook();
        }
    }
}

/// Execute a background batch of sample moves inside one source folder.
pub(super) fn run_folder_sample_move_task(
    source_id: SourceId,
    source_root: PathBuf,
    requests: Vec<FolderSampleMoveRequest>,
    errors: Vec<String>,
    cancel: Arc<AtomicBool>,
    sender: Option<&Sender<FileOpMessage>>,
) -> FolderSampleMoveResult {
    run_folder_sample_move_task_with_hooks(
        source_id,
        source_root,
        requests,
        errors,
        cancel,
        sender,
        &mut NoopFolderMoveHooks,
    )
}

pub(super) fn run_folder_sample_move_task_with_hooks(
    source_id: SourceId,
    source_root: PathBuf,
    requests: Vec<FolderSampleMoveRequest>,
    mut errors: Vec<String>,
    cancel: Arc<AtomicBool>,
    sender: Option<&Sender<FileOpMessage>>,
    hooks: &mut impl FolderMoveHooks,
) -> FolderSampleMoveResult {
    let mut moved = Vec::new();
    let mut completed = 0usize;
    let mut cancelled = false;
    if !source_root.is_dir() {
        errors.push(format!("Source folder missing: {}", source_root.display()));
        return FolderSampleMoveResult {
            source_id,
            moved,
            errors,
            cancelled,
        };
    }
    let db = match SourceDatabase::open_for_source_write(&source_root) {
        Ok(db) => db,
        Err(err) => {
            errors.push(format!("Failed to open source DB: {err}"));
            return FolderSampleMoveResult {
                source_id,
                moved,
                errors,
                cancelled,
            };
        }
    };
    for request in requests {
        if cancel.load(Ordering::Relaxed) {
            cancelled = true;
            break;
        }
        let detail = Some(format!("Moving {}", request.relative_path.display()));
        let transaction = match prepare_folder_sample_move_transaction(&db, &source_root, request) {
            Ok(transaction) => transaction,
            Err(err) => {
                errors.push(err);
                completed += 1;
                report_progress(sender, completed, detail);
                continue;
            }
        };
        hooks.before_folder_sample_batch();
        if !transaction.commit_db_stage(&mut errors) {
            completed += 1;
            report_progress(sender, completed, detail);
            continue;
        }
        hooks.before_folder_sample_finalize();
        if !transaction.finalize_filesystem_stage(&mut errors) {
            completed += 1;
            report_progress(sender, completed, detail);
            continue;
        }
        moved.push(transaction.into_success(&mut errors));
        completed += 1;
        report_progress(sender, completed, detail);
    }
    FolderSampleMoveResult {
        source_id,
        moved,
        errors,
        cancelled,
    }
}

/// Forward one progress update to the optional file-op channel sender.
pub(super) fn report_progress(
    sender: Option<&Sender<FileOpMessage>>,
    completed: usize,
    detail: Option<String>,
) {
    if let Some(tx) = sender {
        let _ = tx.send(FileOpMessage::Progress {
            completed,
            detail,
            item: None,
        });
    }
}
