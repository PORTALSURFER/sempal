use std::collections::VecDeque;

use crate::native_app::app::NativeAppState;
use crate::native_app::transaction_history::{
    DEFAULT_TRANSACTION_LIMIT, HistoryFileAction, HistoryFileIoCommand, HistoryFileIoDirection,
    TransactionApplied, TransactionContext, TransactionListItem, TransactionListState,
    TransactionResult,
};

type NativeTransactionClosure = dyn for<'a> Fn(&mut TransactionContext<'a>) -> TransactionResult;

enum NativeTransactionActionKind {
    Closure(Box<NativeTransactionClosure>),
    File(HistoryFileAction),
}

struct NativeTransactionAction {
    label: String,
    undo: NativeTransactionActionKind,
    redo: NativeTransactionActionKind,
}

impl NativeTransactionAction {
    fn new(
        label: impl Into<String>,
        undo: impl for<'a> Fn(&mut TransactionContext<'a>) -> TransactionResult + 'static,
        redo: impl for<'a> Fn(&mut TransactionContext<'a>) -> TransactionResult + 'static,
    ) -> Self {
        Self {
            label: label.into(),
            undo: NativeTransactionActionKind::Closure(Box::new(undo)),
            redo: NativeTransactionActionKind::Closure(Box::new(redo)),
        }
    }

    fn file(
        label: impl Into<String>,
        undo_action: HistoryFileAction,
        redo_action: HistoryFileAction,
    ) -> Self {
        Self {
            label: label.into(),
            undo: NativeTransactionActionKind::File(undo_action),
            redo: NativeTransactionActionKind::File(redo_action),
        }
    }

    fn is_file(&self) -> bool {
        matches!(self.undo, NativeTransactionActionKind::File(_))
    }

    fn file_action(&self, direction: HistoryFileIoDirection) -> Option<HistoryFileAction> {
        match match direction {
            HistoryFileIoDirection::Undo => &self.undo,
            HistoryFileIoDirection::Redo => &self.redo,
        } {
            NativeTransactionActionKind::File(action) => Some(action.clone()),
            NativeTransactionActionKind::Closure(_) => None,
        }
    }
}

struct NativeTransaction {
    id: u64,
    label: String,
    actions: Vec<NativeTransactionAction>,
}

impl NativeTransaction {
    fn new(id: u64, label: String, actions: Vec<NativeTransactionAction>) -> Self {
        Self { id, label, actions }
    }

    fn undo(&self, state: &mut NativeAppState) -> TransactionResult {
        let mut context = TransactionContext { state };
        for action in self.actions.iter().rev() {
            match &action.undo {
                NativeTransactionActionKind::Closure(closure) => closure(&mut context)?,
                NativeTransactionActionKind::File(_) => {
                    return Err(String::from(
                        "file-backed transaction requires async execution",
                    ));
                }
            }
        }
        Ok(())
    }

    fn redo(&self, state: &mut NativeAppState) -> TransactionResult {
        let mut context = TransactionContext { state };
        for action in &self.actions {
            match &action.redo {
                NativeTransactionActionKind::Closure(closure) => closure(&mut context)?,
                NativeTransactionActionKind::File(_) => {
                    return Err(String::from(
                        "file-backed transaction requires async execution",
                    ));
                }
            }
        }
        Ok(())
    }

    fn snapshot(&self, state: TransactionListState) -> TransactionListItem {
        TransactionListItem {
            id: self.id,
            label: self.label.clone(),
            action_count: self.actions.len(),
            action_labels: self
                .actions
                .iter()
                .map(|action| action.label.clone())
                .collect(),
            state,
        }
    }
}

struct NativeTransactionDraft {
    label: String,
    actions: Vec<NativeTransactionAction>,
    depth: usize,
}

pub(in crate::native_app) struct PendingHistoryFileTransaction {
    transaction: NativeTransaction,
    direction: HistoryFileIoDirection,
    through_target: Option<u64>,
    execution_id: u64,
}

impl NativeTransactionDraft {
    fn new(label: String) -> Self {
        Self {
            label,
            actions: Vec::new(),
            depth: 1,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::native_app::transaction_history::HistoryFileIoRoute;
    use crate::native_app::waveform_edits::waveform_restore_action_for_capacity_tests;

    #[test]
    fn stale_history_file_completion_cannot_finish_new_execution() {
        let mut history = NativeTransactionHistory::new();
        let action = HistoryFileAction::FolderMove {
            source_root: std::path::PathBuf::from("/tmp/source"),
            source_database_root: std::path::PathBuf::from("/tmp/source/.db"),
            moves: vec![(
                std::path::PathBuf::from("/tmp/source/old"),
                std::path::PathBuf::from("/tmp/source/new"),
            )],
        };
        history.register_file_action("Move", action.clone(), action);
        let command = history
            .begin_file_io(HistoryFileIoDirection::Undo, None, 17)
            .expect("start file history")
            .expect("file action");
        assert!(
            history
                .finish_file_io(
                    command.execution_id + 1,
                    command.transaction_id,
                    command.direction,
                    false,
                )
                .is_err()
        );
        assert!(history.file_io_in_flight());
        history
            .finish_file_io(
                command.execution_id,
                command.transaction_id,
                command.direction,
                false,
            )
            .expect("current completion should finish");
        assert!(!history.file_io_in_flight());
        assert!(history.can_undo());
    }

    #[test]
    fn non_extracted_waveform_restore_uses_owner_route_with_bounded_identity() {
        let mut history = NativeTransactionHistory::new();
        let action = waveform_restore_action_for_capacity_tests(
            "/tmp/before.wav".into(),
            "/tmp/target.wav".into(),
            false,
        );
        history.register_file_action("Owner restore", action.clone(), action);
        let command = history
            .begin_file_io(HistoryFileIoDirection::Undo, Some(7), 11)
            .expect("begin owner history")
            .expect("owner history command");
        assert_eq!(command.route, HistoryFileIoRoute::OwnerWaveformRestore);
        assert_eq!(command.execution_id, 11);
        assert_eq!(command.transaction_id, 1);
        assert_eq!(command.direction, HistoryFileIoDirection::Undo);
        assert_eq!(command.through_target, Some(7));
        assert_eq!(command.label, "Owner restore");
    }

    #[test]
    fn mixed_non_extracted_waveform_restore_is_rejected_before_stack_removal() {
        let mut history = NativeTransactionHistory::new();
        let restore = waveform_restore_action_for_capacity_tests(
            "/tmp/before.wav".into(),
            "/tmp/target.wav".into(),
            false,
        );
        let move_action = HistoryFileAction::FolderMove {
            source_root: "/tmp/source".into(),
            source_database_root: "/tmp/source/.db".into(),
            moves: vec![("/tmp/source/old".into(), "/tmp/source/new".into())],
        };
        history.begin_transaction("Mixed restore");
        history.register_file_action("Restore", restore.clone(), restore);
        history.register_file_action("Move", move_action.clone(), move_action);
        assert!(history.commit_transaction());
        let error = history
            .begin_file_io(HistoryFileIoDirection::Undo, None, 12)
            .expect_err("mixed owner route should fail before dispatch");
        assert!(error.contains("unsupported mixed history file operation"));
        assert!(!history.file_io_in_flight());
        assert!(history.can_undo());
    }

    #[test]
    fn extracted_waveform_restore_stays_on_generic_worker_route() {
        let mut history = NativeTransactionHistory::new();
        let action = waveform_restore_action_for_capacity_tests(
            "/tmp/before.wav".into(),
            "/tmp/target.wav".into(),
            true,
        );
        history.register_file_action("Extracted restore", action.clone(), action);
        let command = history
            .begin_file_io(HistoryFileIoDirection::Undo, None, 13)
            .expect("begin extracted history")
            .expect("generic history command");
        assert_eq!(command.route, HistoryFileIoRoute::GenericWorker);
    }
}

pub(in crate::native_app) struct NativeTransactionHistory {
    undo: VecDeque<NativeTransaction>,
    redo: VecDeque<NativeTransaction>,
    active: Option<NativeTransactionDraft>,
    next_id: u64,
    limit: usize,
    in_flight_file: Option<PendingHistoryFileTransaction>,
}

impl Default for NativeTransactionHistory {
    fn default() -> Self {
        Self::new()
    }
}

impl NativeTransactionHistory {
    pub(in crate::native_app) fn new() -> Self {
        Self {
            undo: VecDeque::new(),
            redo: VecDeque::new(),
            active: None,
            next_id: 1,
            limit: DEFAULT_TRANSACTION_LIMIT,
            in_flight_file: None,
        }
    }

    pub(in crate::native_app) fn begin_transaction(&mut self, label: impl Into<String>) {
        if let Some(active) = self.active.as_mut() {
            active.depth += 1;
            return;
        }
        self.active = Some(NativeTransactionDraft::new(label.into()));
    }

    pub(in crate::native_app) fn commit_transaction(&mut self) -> bool {
        let Some(active) = self.active.as_mut() else {
            return false;
        };
        active.depth = active.depth.saturating_sub(1);
        if active.depth > 0 {
            return false;
        }
        let Some(active) = self.active.take() else {
            return false;
        };
        if active.actions.is_empty() {
            return false;
        }
        self.push_transaction(active.label, active.actions);
        true
    }

    pub(in crate::native_app) fn register_action(
        &mut self,
        label: impl Into<String>,
        undo: impl for<'a> Fn(&mut TransactionContext<'a>) -> TransactionResult + 'static,
        redo: impl for<'a> Fn(&mut TransactionContext<'a>) -> TransactionResult + 'static,
    ) {
        let label = label.into();
        let action = NativeTransactionAction::new(label.clone(), undo, redo);
        if let Some(active) = self.active.as_mut() {
            if active.actions.iter().any(NativeTransactionAction::is_file) {
                return;
            }
            active.actions.push(action);
        } else {
            self.push_transaction(label, vec![action]);
        }
    }

    pub(in crate::native_app) fn register_file_action(
        &mut self,
        label: impl Into<String>,
        undo_action: HistoryFileAction,
        redo_action: HistoryFileAction,
    ) {
        let action = NativeTransactionAction::file(label, undo_action, redo_action);
        if let Some(active) = self.active.as_mut() {
            if active.actions.iter().any(|existing| !existing.is_file()) {
                return;
            }
            active.actions.push(action);
        } else {
            self.push_transaction(action.label.clone(), vec![action]);
        }
    }

    pub(in crate::native_app) fn begin_file_io(
        &mut self,
        direction: HistoryFileIoDirection,
        through_target: Option<u64>,
        execution_id: u64,
    ) -> Result<Option<HistoryFileIoCommand>, String> {
        if self.in_flight_file.is_some() {
            return Err(String::from(
                "A history file operation is already in flight",
            ));
        }
        let stack = match direction {
            HistoryFileIoDirection::Undo => &mut self.undo,
            HistoryFileIoDirection::Redo => &mut self.redo,
        };
        let Some(transaction) = stack.back() else {
            return Ok(None);
        };
        if transaction.actions.iter().any(|action| !action.is_file()) {
            return Ok(None);
        }
        let actions = transaction
            .actions
            .iter()
            .filter_map(|action| action.file_action(direction))
            .collect::<Vec<_>>();
        if actions.is_empty() {
            return Ok(None);
        }
        let route = super::file_io::classify_history_file_actions(&actions)?;
        let transaction = stack.pop_back().expect("history stack entry exists");
        let transaction_id = transaction.id;
        let label = transaction.label.clone();
        self.in_flight_file = Some(PendingHistoryFileTransaction {
            transaction,
            direction,
            through_target,
            execution_id,
        });
        Ok(Some(HistoryFileIoCommand {
            execution_id,
            transaction_id,
            label,
            direction,
            through_target,
            route,
            actions,
        }))
    }

    pub(in crate::native_app) fn finish_file_io(
        &mut self,
        execution_id: u64,
        transaction_id: u64,
        direction: HistoryFileIoDirection,
        success: bool,
    ) -> Result<(TransactionApplied, Option<u64>), String> {
        let Some(pending) = self.in_flight_file.take() else {
            return Err(String::from("No history file operation is in flight"));
        };
        if pending.execution_id != execution_id
            || pending.transaction.id != transaction_id
            || pending.direction != direction
        {
            self.in_flight_file = Some(pending);
            return Err(String::from("Stale history file operation completion"));
        }
        let applied = TransactionApplied {
            label: pending.transaction.label.clone(),
            action_count: pending.transaction.actions.len(),
        };
        let through_target = pending.through_target;
        if !success {
            match direction {
                HistoryFileIoDirection::Undo => self.undo.push_back(pending.transaction),
                HistoryFileIoDirection::Redo => self.redo.push_back(pending.transaction),
            }
            return Ok((applied, through_target));
        }
        match direction {
            HistoryFileIoDirection::Undo => self.redo.push_back(pending.transaction),
            HistoryFileIoDirection::Redo => self.undo.push_back(pending.transaction),
        }
        Ok((applied, through_target))
    }

    pub(in crate::native_app) fn file_io_in_flight(&self) -> bool {
        self.in_flight_file.is_some()
    }

    pub(in crate::native_app) fn file_io_matches(
        &self,
        execution_id: u64,
        transaction_id: u64,
        direction: HistoryFileIoDirection,
        through_target: Option<u64>,
    ) -> bool {
        self.in_flight_file.as_ref().is_some_and(|pending| {
            pending.execution_id == execution_id
                && pending.transaction.id == transaction_id
                && pending.direction == direction
                && pending.through_target == through_target
        })
    }

    pub(in crate::native_app) fn has_transaction_on_stack(
        &self,
        direction: HistoryFileIoDirection,
        transaction_id: u64,
    ) -> bool {
        if self.in_flight_file.is_some() {
            return false;
        }
        let stack = match direction {
            HistoryFileIoDirection::Undo => &self.undo,
            HistoryFileIoDirection::Redo => &self.redo,
        };
        stack
            .iter()
            .any(|transaction| transaction.id == transaction_id)
    }

    fn push_transaction(
        &mut self,
        label: impl Into<String>,
        actions: Vec<NativeTransactionAction>,
    ) {
        if actions.is_empty() {
            return;
        }
        let transaction = NativeTransaction::new(self.next_id, label.into(), actions);
        self.next_id += 1;
        self.redo.clear();
        self.undo.push_back(transaction);
        while self.undo.len() > self.limit {
            self.undo.pop_front();
        }
    }

    pub(in crate::native_app) fn undo(
        &mut self,
        state: &mut NativeAppState,
    ) -> Result<Option<TransactionApplied>, String> {
        let Some(transaction) = self.undo.pop_back() else {
            return Ok(None);
        };
        if let Err(error) = transaction.undo(state) {
            self.undo.push_back(transaction);
            return Err(error);
        }
        let applied = TransactionApplied {
            label: transaction.label.clone(),
            action_count: transaction.actions.len(),
        };
        self.redo.push_back(transaction);
        Ok(Some(applied))
    }

    pub(in crate::native_app) fn redo(
        &mut self,
        state: &mut NativeAppState,
    ) -> Result<Option<TransactionApplied>, String> {
        let Some(transaction) = self.redo.pop_back() else {
            return Ok(None);
        };
        if let Err(error) = transaction.redo(state) {
            self.redo.push_back(transaction);
            return Err(error);
        }
        let applied = TransactionApplied {
            label: transaction.label.clone(),
            action_count: transaction.actions.len(),
        };
        self.undo.push_back(transaction);
        Ok(Some(applied))
    }

    pub(in crate::native_app) fn can_undo(&self) -> bool {
        self.in_flight_file.is_none() && !self.undo.is_empty()
    }

    pub(in crate::native_app) fn remove_transactions_with_action_label(&mut self, label: &str) {
        self.undo
            .retain(|transaction| !transaction.has_action_label(label));
        self.redo
            .retain(|transaction| !transaction.has_action_label(label));
        if self
            .active
            .as_ref()
            .is_some_and(|draft| draft.has_action_label(label))
        {
            self.active = None;
        }
    }

    pub(in crate::native_app) fn can_redo(&self) -> bool {
        self.in_flight_file.is_none() && !self.redo.is_empty()
    }

    pub(in crate::native_app) fn is_transaction_open(&self) -> bool {
        self.active.is_some()
    }

    pub(in crate::native_app) fn list_items(&self) -> Vec<TransactionListItem> {
        let active = self.active.iter().map(|draft| TransactionListItem {
            id: 0,
            label: draft.label.clone(),
            action_count: draft.actions.len(),
            action_labels: draft
                .actions
                .iter()
                .map(|action| action.label.clone())
                .collect(),
            state: TransactionListState::Active,
        });
        let undo = self.undo.iter().rev().map(|transaction| {
            transaction.snapshot(if self.in_flight_file.is_some() {
                TransactionListState::Unavailable
            } else {
                TransactionListState::Undoable
            })
        });
        let redo = self.redo.iter().rev().map(|transaction| {
            transaction.snapshot(if self.in_flight_file.is_some() {
                TransactionListState::Unavailable
            } else {
                TransactionListState::Redoable
            })
        });
        active.chain(undo).chain(redo).collect()
    }
}

impl NativeTransaction {
    fn has_action_label(&self, label: &str) -> bool {
        self.actions.iter().any(|action| action.label == label)
    }
}

impl NativeTransactionDraft {
    fn has_action_label(&self, label: &str) -> bool {
        self.actions.iter().any(|action| action.label == label)
    }
}
