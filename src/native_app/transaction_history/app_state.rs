use crate::native_app::app::GuiMessage;
use crate::native_app::app::NativeAppState;
use crate::native_app::sample_library::committed_file_mutations::FileMutationOperation;
use crate::native_app::transaction_history::{TransactionContext, TransactionResult};
use radiant::prelude as ui;

impl NativeAppState {
    pub(in crate::native_app) fn begin_transaction(&mut self, label: impl Into<String>) {
        if !self.transactions.restoring {
            self.transactions.history.begin_transaction(label);
        }
    }

    pub(in crate::native_app) fn commit_transaction(&mut self) -> bool {
        if self.transactions.restoring {
            return false;
        }
        self.transactions.history.commit_transaction()
    }

    pub(in crate::native_app) fn register_transaction_action(
        &mut self,
        label: impl Into<String>,
        undo: impl for<'a> Fn(&mut TransactionContext<'a>) -> TransactionResult + 'static,
        redo: impl for<'a> Fn(&mut TransactionContext<'a>) -> TransactionResult + 'static,
    ) {
        if self.transactions.restoring {
            return;
        }
        self.transactions.history.register_action(label, undo, redo);
    }

    pub(in crate::native_app) fn undo_transaction(
        &mut self,
        context: &mut ui::UiUpdateContext<GuiMessage>,
    ) {
        self.transactions.pending_file_mutations.clear();
        self.transactions.pending_file_mutation_failures.clear();
        self.transactions.pending_file_mutation_attempted = false;
        let mut history = std::mem::take(&mut self.transactions.history);
        let was_restoring = self.transactions.restoring;
        self.transactions.restoring = true;
        let result = history.undo(self);
        self.transactions.restoring = was_restoring;
        self.transactions.history = history;
        self.finish_transaction_file_mutations(
            FileMutationOperation::Undo,
            result.as_ref().err().cloned(),
            context,
        );
        self.background.rating_persist.schedule_if_idle(context);
        match result {
            Ok(Some(applied)) => {
                self.ui.status.sample = format!("Undid {}", applied.label);
            }
            Ok(None) => {
                self.ui.status.sample = String::from("Nothing to undo");
            }
            Err(error) => {
                self.ui.status.sample = format!("Undo failed: {error}");
            }
        }
    }

    pub(in crate::native_app) fn undo_transactions_through(
        &mut self,
        target_id: u64,
        context: &mut ui::UiUpdateContext<GuiMessage>,
    ) {
        self.transactions.pending_file_mutations.clear();
        self.transactions.pending_file_mutation_failures.clear();
        self.transactions.pending_file_mutation_attempted = false;
        let mut history = std::mem::take(&mut self.transactions.history);
        let was_restoring = self.transactions.restoring;
        self.transactions.restoring = true;
        let result = history.undo_through(target_id, self);
        self.transactions.restoring = was_restoring;
        self.transactions.history = history;
        self.finish_transaction_file_mutations(
            FileMutationOperation::Undo,
            result.as_ref().err().cloned(),
            context,
        );
        self.background.rating_persist.schedule_if_idle(context);
        match result {
            Ok(applied) if applied.is_empty() => {
                self.ui.status.sample = format!("Transaction #{target_id} is not undoable");
            }
            Ok(applied) => {
                let count = applied.len();
                let label = applied
                    .last()
                    .map(|transaction| transaction.label.as_str())
                    .unwrap_or("transaction");
                self.ui.status.sample = format!("Undid {count} through {label}");
            }
            Err(error) => {
                self.ui.status.sample = format!("Undo failed: {error}");
            }
        }
    }

    pub(in crate::native_app) fn redo_transaction(
        &mut self,
        context: &mut ui::UiUpdateContext<GuiMessage>,
    ) {
        self.transactions.pending_file_mutations.clear();
        self.transactions.pending_file_mutation_failures.clear();
        self.transactions.pending_file_mutation_attempted = false;
        let mut history = std::mem::take(&mut self.transactions.history);
        let was_restoring = self.transactions.restoring;
        self.transactions.restoring = true;
        let result = history.redo(self);
        self.transactions.restoring = was_restoring;
        self.transactions.history = history;
        self.finish_transaction_file_mutations(
            FileMutationOperation::Redo,
            result.as_ref().err().cloned(),
            context,
        );
        self.background.rating_persist.schedule_if_idle(context);
        match result {
            Ok(Some(applied)) => {
                self.ui.status.sample = format!("Redid {}", applied.label);
            }
            Ok(None) => {
                self.ui.status.sample = String::from("Nothing to redo");
            }
            Err(error) => {
                self.ui.status.sample = format!("Redo failed: {error}");
            }
        }
    }

    pub(in crate::native_app) fn redo_transactions_through(
        &mut self,
        target_id: u64,
        context: &mut ui::UiUpdateContext<GuiMessage>,
    ) {
        self.transactions.pending_file_mutations.clear();
        self.transactions.pending_file_mutation_failures.clear();
        self.transactions.pending_file_mutation_attempted = false;
        let mut history = std::mem::take(&mut self.transactions.history);
        let was_restoring = self.transactions.restoring;
        self.transactions.restoring = true;
        let result = history.redo_through(target_id, self);
        self.transactions.restoring = was_restoring;
        self.transactions.history = history;
        self.finish_transaction_file_mutations(
            FileMutationOperation::Redo,
            result.as_ref().err().cloned(),
            context,
        );
        self.background.rating_persist.schedule_if_idle(context);
        match result {
            Ok(applied) if applied.is_empty() => {
                self.ui.status.sample = format!("Transaction #{target_id} is not redoable");
            }
            Ok(applied) => {
                let count = applied.len();
                let label = applied
                    .last()
                    .map(|transaction| transaction.label.as_str())
                    .unwrap_or("transaction");
                self.ui.status.sample = format!("Redid {count} through {label}");
            }
            Err(error) => {
                self.ui.status.sample = format!("Redo failed: {error}");
            }
        }
    }

    pub(in crate::native_app) fn toggle_transaction_list(&mut self) {
        self.ui.chrome.transaction_list_open = !self.ui.chrome.transaction_list_open;
    }

    fn finish_transaction_file_mutations(
        &mut self,
        operation: FileMutationOperation,
        operation_error: Option<String>,
        context: &mut ui::UiUpdateContext<GuiMessage>,
    ) {
        let changes = std::mem::take(&mut self.transactions.pending_file_mutations);
        let mut failures = std::mem::take(&mut self.transactions.pending_file_mutation_failures);
        if std::mem::take(&mut self.transactions.pending_file_mutation_attempted) {
            failures.extend(operation_error);
        }
        self.queue_partially_committed_file_mutation(
            operation,
            changes,
            failures.into_iter().map(|error| (None, error)).collect(),
            context,
        );
    }
}
