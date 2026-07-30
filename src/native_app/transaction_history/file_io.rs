//! Typed, serialized filesystem work for file-backed transaction history.

use std::path::{Path, PathBuf};

use crate::native_app::sample_library::committed_file_mutations::{
    FileMutationProjection, PreparedCommittedFileMutationChange,
};
use crate::native_app::sample_library::folder_browser::commands::execute_folder_move_transaction;
use crate::native_app::waveform_edits::{
    AppliedWaveformEdit, restore_edited_waveform, restore_extracted_file_for_transaction,
};
use wavecrate::sample_sources::{SourceFileEvidence, capture_source_file_evidence};

/// A file-backed history action. It is deliberately separate from the closure action used by
/// ordinary in-memory history, so filesystem work can never run through the UI closure path.
#[derive(Clone, Debug, PartialEq)]
pub(in crate::native_app) enum HistoryFileAction {
    FolderMove {
        source_root: PathBuf,
        source_database_root: PathBuf,
        moves: Vec<(PathBuf, PathBuf)>,
    },
    WaveformRestore {
        backup_path: PathBuf,
        applied: AppliedWaveformEdit,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::native_app) enum HistoryFileIoDirection {
    Undo,
    Redo,
}

#[derive(Clone, Debug, PartialEq)]
pub(in crate::native_app) struct HistoryFileIoCommand {
    pub(in crate::native_app) execution_id: u64,
    pub(in crate::native_app) transaction_id: u64,
    pub(in crate::native_app) direction: HistoryFileIoDirection,
    pub(in crate::native_app) through_target: Option<u64>,
    pub(in crate::native_app) actions: Vec<HistoryFileAction>,
}

#[derive(Clone, Debug, PartialEq)]
pub(in crate::native_app) struct HistoryFileIoOutput {
    pub(in crate::native_app) changes: Vec<PreparedCommittedFileMutationChange>,
    pub(in crate::native_app) failures: Vec<(Option<String>, String)>,
    pub(in crate::native_app) waveform_paths: Vec<PathBuf>,
}

#[derive(Clone, Debug, PartialEq)]
pub(in crate::native_app) struct HistoryFileIoResult {
    pub(in crate::native_app) execution_id: u64,
    pub(in crate::native_app) transaction_id: u64,
    pub(in crate::native_app) direction: HistoryFileIoDirection,
    pub(in crate::native_app) through_target: Option<u64>,
    pub(in crate::native_app) result: Result<HistoryFileIoOutput, String>,
}

/// Execute one command on the history I/O owner. Callers hold the owner's serialization gate.
pub(in crate::native_app) fn execute_history_file_io(
    command: HistoryFileIoCommand,
) -> HistoryFileIoResult {
    let identity = (
        command.execution_id,
        command.transaction_id,
        command.direction,
        command.through_target,
    );
    let result = execute_actions(command.actions);
    HistoryFileIoResult {
        execution_id: identity.0,
        transaction_id: identity.1,
        direction: identity.2,
        through_target: identity.3,
        result,
    }
}

fn execute_actions(actions: Vec<HistoryFileAction>) -> Result<HistoryFileIoOutput, String> {
    let mut changes = Vec::new();
    let mut failures = Vec::new();
    let mut waveform_paths = Vec::new();
    for action in actions {
        match action {
            HistoryFileAction::FolderMove {
                source_root,
                source_database_root,
                moves,
            } => {
                let (completed, metadata_error) =
                    execute_folder_move_transaction(&source_root, &source_database_root, &moves)?;
                if let Some(error) = metadata_error {
                    failures.push((None, error));
                }
                let mut move_changes = completed
                    .iter()
                    .map(|(before, after)| {
                        PreparedCommittedFileMutationChange::path_only_move(
                            before.clone(),
                            after.clone(),
                            capture_source_file_evidence(after),
                        )
                    })
                    .collect::<Vec<_>>();
                if let Some((target_path, _)) = completed.first() {
                    let target_path = completed
                        .first()
                        .map(|(_, after)| after.clone())
                        .unwrap_or_else(|| target_path.clone());
                    let projection = FileMutationProjection::MoveTransaction {
                        target_path,
                        source_root,
                        source_database_root,
                        moves: completed.clone(),
                    };
                    if let Some(first) = move_changes.first_mut() {
                        *first = first.clone().with_projection(projection);
                    }
                }
                changes.extend(move_changes);
            }
            HistoryFileAction::WaveformRestore {
                backup_path,
                applied,
            } => {
                let before_content_identity = restore_edited_waveform(&backup_path, &applied)?;
                let evidence = capture_source_file_evidence(&applied.absolute_path);
                changes.push(
                    PreparedCommittedFileMutationChange::content_changed(
                        applied.absolute_path.clone(),
                        evidence,
                    )
                    .with_before_content_identity(before_content_identity),
                );
                waveform_paths.push(applied.absolute_path.clone());
                if let Some(extracted) = applied.extracted.as_ref() {
                    restore_extracted_file_for_transaction(&backup_path, &applied, extracted)?;
                    let change = if backup_path == applied.backup.before.as_path() {
                        PreparedCommittedFileMutationChange::deleted(
                            extracted.path.clone(),
                            extracted.evidence.clone(),
                        )
                    } else {
                        PreparedCommittedFileMutationChange::created(
                            extracted.path.clone(),
                            capture_source_file_evidence(&extracted.path),
                        )
                    };
                    changes.push(change);
                }
            }
        }
    }
    Ok(HistoryFileIoOutput {
        changes,
        failures,
        waveform_paths,
    })
}

#[allow(dead_code)]
fn _evidence(path: &Path) -> SourceFileEvidence {
    capture_source_file_evidence(path)
}
