use crate::native_app::sample_library::committed_file_mutations::ProcessLocalMutationCorrelationId;
use crate::native_app::sample_library::committed_file_mutations::RevisionFirstCursor;
use crate::native_app::transaction_history::HistoryFileIoDirection;
use crate::native_app::transaction_history::NativeTransactionHistory;
use crate::native_app::transaction_history::operation_journal::{
    JournalError, OperationJournalCoordinator, RecoverySummary,
};
use std::path::PathBuf;

pub(in crate::native_app) struct PendingHistoryCommit {
    pub(in crate::native_app) execution_id: u64,
    pub(in crate::native_app) transaction_id: u64,
    pub(in crate::native_app) direction: HistoryFileIoDirection,
    pub(in crate::native_app) through_target: Option<u64>,
    pub(in crate::native_app) correlation_id: ProcessLocalMutationCorrelationId,
    pub(in crate::native_app) waveform_paths: Vec<PathBuf>,
}

pub(in crate::native_app) struct TransactionState {
    pub(in crate::native_app) history: NativeTransactionHistory,
    pub(in crate::native_app) restoring: bool,
    pub(in crate::native_app) history_through_count: usize,
    pub(in crate::native_app) pending_history_commit: Option<PendingHistoryCommit>,
    pub(in crate::native_app) latest_committed_mutation:
        std::collections::HashMap<String, RevisionFirstCursor>,
    pub(in crate::native_app) operation_journal: OperationJournalLifecycle,
}

/// The profile-owned durable journal lifecycle held by native app state.
///
/// An unavailable journal is retained as a typed condition rather than being
/// silently retried or replaced. Future durable admission can therefore fail
/// closed while the rest of the app remains usable.
pub(in crate::native_app) struct OperationJournalLifecycle {
    coordinator: Option<OperationJournalCoordinator>,
    unavailable: Option<OperationJournalUnavailable>,
    recovery_summary: RecoverySummary,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::native_app) enum OperationJournalUnavailable {
    /// The profile journal is owned by another process or app instance.
    OwnedByAnotherProcess { path: PathBuf },
    /// The journal could not be opened for another fail-closed reason.
    OpenFailed(String),
    /// Unit-test fixtures intentionally do not acquire the live profile owner.
    DisabledForTests,
}

impl std::fmt::Display for OperationJournalUnavailable {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::OwnedByAnotherProcess { path } => {
                write!(formatter, "owned by another process: {}", path.display())
            }
            Self::OpenFailed(error) => formatter.write_str(error),
            Self::DisabledForTests => formatter.write_str("disabled for test fixture"),
        }
    }
}

impl OperationJournalUnavailable {
    fn from_error(error: JournalError) -> Self {
        match error {
            JournalError::OwnedByAnotherProcess { path } => Self::OwnedByAnotherProcess { path },
            error => Self::OpenFailed(error.to_string()),
        }
    }
}

impl OperationJournalLifecycle {
    fn unavailable(unavailable: OperationJournalUnavailable) -> Self {
        Self {
            coordinator: None,
            unavailable: Some(unavailable),
            recovery_summary: RecoverySummary::default(),
        }
    }

    fn open_current_profile() -> Self {
        match OperationJournalCoordinator::open_current_profile() {
            Ok(coordinator) => {
                let recovery_summary = coordinator.recovery_summary();
                Self {
                    coordinator: Some(coordinator),
                    unavailable: None,
                    recovery_summary,
                }
            }
            Err(error) => Self::unavailable(OperationJournalUnavailable::from_error(error)),
        }
    }

    #[cfg(test)]
    fn open(directory: PathBuf) -> Self {
        match OperationJournalCoordinator::open(directory) {
            Ok(coordinator) => {
                let recovery_summary = coordinator.recovery_summary();
                Self {
                    coordinator: Some(coordinator),
                    unavailable: None,
                    recovery_summary,
                }
            }
            Err(error) => Self::unavailable(OperationJournalUnavailable::from_error(error)),
        }
    }

    fn startup_diagnostic(&self) -> Option<String> {
        if let Some(unavailable) = self.unavailable.as_ref() {
            if matches!(unavailable, OperationJournalUnavailable::DisabledForTests) {
                return None;
            }
            return Some(format!(
                "Operation journal unavailable; durable operations disabled: {unavailable}"
            ));
        }
        let summary = &self.recovery_summary;
        summary.attention_required.then(|| {
            format!(
                "Operation journal needs attention: {} unresolved, {} malformed, {} unknown-version, {} oversized record(s)",
                summary.unresolved_count,
                summary.malformed_count,
                summary.unknown_version_count,
                summary.oversize_count,
            )
        })
    }

    fn release(&mut self) -> bool {
        self.coordinator.take().is_some()
    }

    #[cfg(test)]
    fn is_available(&self) -> bool {
        self.coordinator.is_some()
    }
}

impl Default for TransactionState {
    fn default() -> Self {
        Self {
            history: NativeTransactionHistory::default(),
            restoring: false,
            history_through_count: 0,
            pending_history_commit: None,
            latest_committed_mutation: std::collections::HashMap::new(),
            operation_journal: OperationJournalLifecycle::unavailable(
                OperationJournalUnavailable::DisabledForTests,
            ),
        }
    }
}

impl TransactionState {
    #[cfg(test)]
    pub(in crate::native_app) fn for_tests() -> Self {
        Self::default()
    }

    pub(in crate::native_app) fn with_profile_operation_journal() -> Self {
        Self {
            operation_journal: OperationJournalLifecycle::open_current_profile(),
            ..Self::default()
        }
    }

    pub(in crate::native_app) fn startup_diagnostic(&self) -> Option<String> {
        self.operation_journal.startup_diagnostic()
    }

    pub(in crate::native_app) fn release_operation_journal(&mut self) -> bool {
        self.operation_journal.release()
    }

    /// Borrow the coordinator for a future durable admission, failing closed
    /// when startup could not establish profile ownership.
    #[allow(dead_code)]
    pub(in crate::native_app) fn operation_journal_for_admission(
        &mut self,
    ) -> Result<&mut OperationJournalCoordinator, OperationJournalUnavailable> {
        if let Some(coordinator) = self.operation_journal.coordinator.as_mut() {
            return Ok(coordinator);
        }
        Err(self.operation_journal.unavailable.clone().unwrap_or(
            OperationJournalUnavailable::OpenFailed(String::from(
                "operation journal coordinator is unavailable",
            )),
        ))
    }

    #[cfg(test)]
    pub(in crate::native_app) fn with_operation_journal_directory(directory: PathBuf) -> Self {
        Self {
            operation_journal: OperationJournalLifecycle::open(directory),
            ..Self::default()
        }
    }

    #[cfg(test)]
    pub(in crate::native_app) fn operation_journal_is_available(&self) -> bool {
        self.operation_journal.is_available()
    }

    #[cfg(test)]
    pub(in crate::native_app) fn operation_journal_unavailable(
        &self,
    ) -> Option<OperationJournalUnavailable> {
        self.operation_journal.unavailable.clone()
    }

    #[cfg(test)]
    pub(in crate::native_app) fn operation_journal_summary(&self) -> RecoverySummary {
        self.operation_journal.recovery_summary.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::native_app::transaction_history::operation_journal::{
        OperationActor, OperationIntent, OperationJournalCoordinator, OperationKind,
    };
    use serde_json::Value;
    use std::fs;

    fn intent() -> OperationIntent {
        OperationIntent {
            actor: OperationActor::User,
            kind: OperationKind::FileHistory,
            label: String::from("lifecycle test"),
        }
    }

    #[test]
    fn operation_journal_is_held_for_state_lifetime_and_released_on_drop() {
        let directory = tempfile::tempdir().expect("journal directory");
        let path = directory.path().to_path_buf();
        let state = TransactionState::with_operation_journal_directory(path.clone());
        assert!(state.operation_journal_is_available());
        assert!(OperationJournalCoordinator::open(path).is_err());
        drop(state);
        assert!(OperationJournalCoordinator::open(directory.path().to_path_buf()).is_ok());
    }

    #[test]
    fn shutdown_release_allows_a_new_profile_owner() {
        let directory = tempfile::tempdir().expect("journal directory");
        let path = directory.path().to_path_buf();
        let mut state = TransactionState::with_operation_journal_directory(path.clone());
        assert!(state.release_operation_journal());
        assert!(!state.operation_journal_is_available());
        assert!(OperationJournalCoordinator::open(path).is_ok());
    }

    #[test]
    fn startup_retains_unresolved_record_without_mutating_it() {
        let directory = tempfile::tempdir().expect("journal directory");
        let operation_id;
        {
            let mut journal = OperationJournalCoordinator::open(directory.path().to_path_buf())
                .expect("open journal");
            operation_id = journal.admit(intent(), Value::Null).expect("admit intent");
        }
        let record_path = directory.path().join(format!("{operation_id}.json"));
        let before = fs::read(&record_path).expect("read retained record");
        let state =
            TransactionState::with_operation_journal_directory(directory.path().to_path_buf());
        assert_eq!(state.operation_journal_summary().unresolved_count, 1);
        assert!(
            state
                .startup_diagnostic()
                .is_some_and(|diagnostic| diagnostic.contains("1 unresolved"))
        );
        assert_eq!(fs::read(record_path).expect("read retained record"), before);
    }

    #[test]
    fn owner_conflict_is_non_fatal_and_refuses_future_admission() {
        let directory = tempfile::tempdir().expect("journal directory");
        let path = directory.path().to_path_buf();
        let owner = OperationJournalCoordinator::open(path.clone()).expect("open owner");
        let mut state = TransactionState::with_operation_journal_directory(path);
        assert!(!state.operation_journal_is_available());
        assert!(matches!(
            state.operation_journal_unavailable(),
            Some(OperationJournalUnavailable::OwnedByAnotherProcess { .. })
        ));
        assert!(
            state
                .startup_diagnostic()
                .is_some_and(|diagnostic| diagnostic.contains("durable operations disabled"))
        );
        assert!(state.operation_journal_for_admission().is_err());
        drop(owner);
    }
}
