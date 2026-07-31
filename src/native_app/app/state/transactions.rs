use crate::native_app::sample_library::committed_file_mutations::ProcessLocalMutationCorrelationId;
use crate::native_app::sample_library::committed_file_mutations::RevisionFirstCursor;
use crate::native_app::transaction_history::HistoryFileIoDirection;
use crate::native_app::transaction_history::NativeTransactionHistory;
use crate::native_app::transaction_history::operation_journal::FilesystemStageOutcome;
use std::path::PathBuf;
use uuid::Uuid;

pub(in crate::native_app) struct PendingHistoryCommit {
    pub(in crate::native_app) execution_id: u64,
    pub(in crate::native_app) transaction_id: u64,
    pub(in crate::native_app) direction: HistoryFileIoDirection,
    pub(in crate::native_app) through_target: Option<u64>,
    pub(in crate::native_app) correlation_id: ProcessLocalMutationCorrelationId,
    pub(in crate::native_app) waveform_paths: Vec<PathBuf>,
}

#[allow(dead_code)]
pub(in crate::native_app) struct PendingHistoryOwnerStaging {
    pub(in crate::native_app) execution_id: u64,
    pub(in crate::native_app) transaction_id: u64,
    pub(in crate::native_app) direction: HistoryFileIoDirection,
    pub(in crate::native_app) through_target: Option<u64>,
    pub(in crate::native_app) label: String,
    pub(in crate::native_app) operation_id: Uuid,
    pub(in crate::native_app) outcome: FilesystemStageOutcome,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct CompletedHistoryOwnerFence {
    execution_id: u64,
    transaction_id: u64,
    direction: HistoryFileIoDirection,
    through_target: Option<u64>,
}

pub(in crate::native_app) struct TransactionState {
    pub(in crate::native_app) history: NativeTransactionHistory,
    pub(in crate::native_app) restoring: bool,
    pub(in crate::native_app) history_through_count: usize,
    pub(in crate::native_app) pending_history_commit: Option<PendingHistoryCommit>,
    pub(in crate::native_app) pending_history_owner_staging: Option<PendingHistoryOwnerStaging>,
    completed_history_owner_fence: Option<CompletedHistoryOwnerFence>,
    pub(in crate::native_app) latest_committed_mutation:
        std::collections::HashMap<String, RevisionFirstCursor>,
}

impl Default for TransactionState {
    fn default() -> Self {
        Self {
            history: NativeTransactionHistory::default(),
            restoring: false,
            history_through_count: 0,
            pending_history_commit: None,
            pending_history_owner_staging: None,
            completed_history_owner_fence: None,
            latest_committed_mutation: std::collections::HashMap::new(),
        }
    }
}

impl TransactionState {
    #[cfg(test)]
    pub(in crate::native_app) fn for_tests() -> Self {
        Self::default()
    }

    pub(in crate::native_app) fn completed_history_owner_matches(
        &self,
        execution_id: u64,
        transaction_id: u64,
        direction: HistoryFileIoDirection,
        through_target: Option<u64>,
    ) -> bool {
        self.completed_history_owner_fence
            == Some(CompletedHistoryOwnerFence {
                execution_id,
                transaction_id,
                direction,
                through_target,
            })
    }

    pub(in crate::native_app) fn retain_completed_history_owner(
        &mut self,
        execution_id: u64,
        transaction_id: u64,
        direction: HistoryFileIoDirection,
        through_target: Option<u64>,
    ) {
        self.completed_history_owner_fence = Some(CompletedHistoryOwnerFence {
            execution_id,
            transaction_id,
            direction,
            through_target,
        });
    }
}
