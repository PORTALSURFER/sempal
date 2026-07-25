//! Folder-level move worker implementation.

use super::report_progress;
use crate::app::controller::jobs::{
    FileOpMessage, FolderEntryMove, FolderMoveRequest, FolderMoveResult,
};
use crate::sample_sources::{SampleCollection, SourceDatabase, WavEntry};
use std::path::{Path, PathBuf};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc::Sender,
};

mod metadata_rewrite;
mod request_validation;
mod result;
mod transaction;

use request_validation::prepare_folder_move_transaction;
use result::cancelled_result;

/// Precomputed filesystem paths for one folder move request.
struct PreparedFolderMove {
    new_relative: PathBuf,
    absolute_old: PathBuf,
    absolute_new: PathBuf,
}

/// Source DB row plus metadata that is not carried by `WavEntry`.
struct FolderMoveEntry {
    entry: WavEntry,
    collection: Option<SampleCollection>,
}

/// Prepared folder move transaction with explicit filesystem and database stages.
struct FolderMoveTransaction {
    request: FolderMoveRequest,
    prepared: PreparedFolderMove,
    db: SourceDatabase,
    entries: Vec<FolderMoveEntry>,
    moved: Vec<FolderEntryMove>,
}

/// Execute a background move for a folder dropped onto another folder.
pub(super) fn run_folder_move_task(
    request: FolderMoveRequest,
    cancel: Arc<AtomicBool>,
    sender: Option<&Sender<FileOpMessage>>,
    hooks: &mut impl super::FolderMoveHooks,
) -> FolderMoveResult {
    if cancel.load(Ordering::Relaxed) {
        return cancelled_result(&request);
    }

    let mut transaction = match prepare_folder_move_transaction(request) {
        Ok(transaction) => transaction,
        Err(result) => return result,
    };
    if let Err(result) = transaction.commit_filesystem_stage() {
        return result;
    }
    hooks.before_folder_move_batch();
    if let Err(result) = transaction.commit_db_stage() {
        return result;
    }
    report_progress(
        sender,
        1,
        Some(format!("Moved {}", transaction.request.folder.display())),
    );
    transaction.into_success()
}
