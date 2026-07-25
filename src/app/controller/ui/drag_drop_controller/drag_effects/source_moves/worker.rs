//! Background worker for cross-source sample moves.

use crate::app::controller::jobs::{
    FileOpMessage, SourceMoveRequest, SourceMoveResult, SourceMoveSuccess,
};
use crate::sample_sources::{SourceDatabase, SourceId};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc::Sender,
};

#[cfg(test)]
mod tests;
mod transaction;

use transaction::prepare_source_move_transaction;

trait SourceMoveHooks {
    fn before_target_db_stage(&mut self) -> Result<(), String> {
        Ok(())
    }

    fn before_finalize(&mut self) {}

    fn after_progress(&mut self, _completed: usize) {}
}

struct NoopSourceMoveHooks;

impl SourceMoveHooks for NoopSourceMoveHooks {}

#[cfg(test)]
#[derive(Default)]
struct SourceMoveTestHooks {
    before_target_db_stage: Option<Box<dyn FnMut() -> Result<(), String> + Send>>,
    before_finalize: Option<Box<dyn FnMut() + Send>>,
    after_progress: Option<Box<dyn FnMut(usize) + Send>>,
}

#[cfg(test)]
impl SourceMoveHooks for SourceMoveTestHooks {
    fn before_target_db_stage(&mut self) -> Result<(), String> {
        self.before_target_db_stage
            .take()
            .map_or(Ok(()), |mut hook| hook())
    }

    fn before_finalize(&mut self) {
        if let Some(mut hook) = self.before_finalize.take() {
            hook();
        }
    }

    fn after_progress(&mut self, completed: usize) {
        if let Some(hook) = self.after_progress.as_mut() {
            hook(completed);
        }
    }
}

/// Execute a batch of cross-source sample moves in the background worker.
pub(super) fn run_source_move_task(
    target_source_id: SourceId,
    target_root: PathBuf,
    requests: Vec<SourceMoveRequest>,
    errors: Vec<String>,
    cancel: Arc<AtomicBool>,
    sender: Option<&Sender<FileOpMessage>>,
) -> SourceMoveResult {
    run_source_move_task_with_hooks(
        target_source_id,
        target_root,
        requests,
        errors,
        cancel,
        sender,
        &mut NoopSourceMoveHooks,
    )
}

fn run_source_move_task_with_hooks(
    target_source_id: SourceId,
    target_root: PathBuf,
    requests: Vec<SourceMoveRequest>,
    mut errors: Vec<String>,
    cancel: Arc<AtomicBool>,
    sender: Option<&Sender<FileOpMessage>>,
    hooks: &mut impl SourceMoveHooks,
) -> SourceMoveResult {
    let mut progress = SourceMoveProgress::new(sender);
    let mut moved = Vec::new();
    let mut cancelled = false;
    if !target_root.is_dir() {
        errors.push(format!(
            "Target source folder missing: {}",
            target_root.display()
        ));
        return SourceMoveResult {
            target_source_id,
            moved,
            errors,
            cancelled,
        };
    }
    let target_db = match SourceDatabase::open_for_source_write(&target_root) {
        Ok(db) => db,
        Err(err) => {
            errors.push(format!("Failed to open target DB: {err}"));
            return SourceMoveResult {
                target_source_id,
                moved,
                errors,
                cancelled,
            };
        }
    };
    let mut source_dbs = HashMap::new();
    for request in requests {
        if cancel.load(Ordering::Relaxed) {
            cancelled = true;
            break;
        }
        let detail = Some(format!("Moving {}", request.relative_path.display()));
        if let Some(success) = run_source_move_request(
            &target_root,
            &target_db,
            &mut source_dbs,
            request,
            &mut errors,
            hooks,
        ) {
            moved.push(success);
        }
        progress.complete(detail);
        hooks.after_progress(progress.completed);
    }
    SourceMoveResult {
        target_source_id,
        moved,
        errors,
        cancelled,
    }
}

/// Progress reporter for the source-move worker.
struct SourceMoveProgress<'a> {
    sender: Option<&'a Sender<FileOpMessage>>,
    completed: usize,
}

impl<'a> SourceMoveProgress<'a> {
    fn new(sender: Option<&'a Sender<FileOpMessage>>) -> Self {
        Self {
            sender,
            completed: 0,
        }
    }

    fn complete(&mut self, detail: Option<String>) {
        self.completed = self.completed.saturating_add(1);
        report_progress(self.sender, self.completed, detail);
    }
}

/// Run one source-move request through its staged transaction pipeline.
fn run_source_move_request(
    target_root: &std::path::Path,
    target_db: &SourceDatabase,
    source_dbs: &mut HashMap<PathBuf, SourceDatabase>,
    request: SourceMoveRequest,
    errors: &mut Vec<String>,
    hooks: &mut impl SourceMoveHooks,
) -> Option<SourceMoveSuccess> {
    let mut transaction =
        match prepare_source_move_transaction(target_root, target_db, source_dbs, request) {
            Ok(transaction) => transaction,
            Err(err) => {
                errors.push(err);
                return None;
            }
        };
    if !transaction.commit_target_db_stage(errors, hooks) {
        return None;
    }
    if !transaction.commit_source_db_stage(errors) {
        return None;
    }
    if !transaction.finalize_filesystem_stage(errors, hooks) {
        return None;
    }
    Some(transaction.into_success(errors))
}

/// Forward one source-move progress update to the optional file-op channel sender.
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
