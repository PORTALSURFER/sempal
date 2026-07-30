use crate::native_app::app::GuiMessage;
use crate::native_app::app::NativeAppState;
use crate::native_app::app::PendingHistoryCommit;
use crate::native_app::transaction_history::{
    HistoryFileIoCommand, HistoryFileIoDirection, HistoryFileIoResult, TransactionContext,
    TransactionResult,
};
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

    pub(in crate::native_app) fn register_file_transaction_action(
        &mut self,
        label: impl Into<String>,
        undo: crate::native_app::transaction_history::HistoryFileAction,
        redo: crate::native_app::transaction_history::HistoryFileAction,
    ) {
        if self.transactions.restoring {
            return;
        }
        self.transactions
            .history
            .register_file_action(label, undo, redo);
    }

    pub(in crate::native_app) fn undo_transaction(
        &mut self,
        context: &mut ui::UiUpdateContext<GuiMessage>,
    ) {
        if self
            .start_history_file_request(HistoryFileIoDirection::Undo, None, context)
            .is_err()
        {
            self.ui.status.sample = String::from("Undo already in progress");
            return;
        }
        if self.history_file_request_started(HistoryFileIoDirection::Undo, None) {
            return;
        }
        let mut history = std::mem::take(&mut self.transactions.history);
        let was_restoring = self.transactions.restoring;
        self.transactions.restoring = true;
        let result = history.undo(self);
        self.transactions.restoring = was_restoring;
        self.transactions.history = history;
        self.background.rating_persist.schedule_if_idle(context);
        match result {
            Ok(Some(applied)) => self.ui.status.sample = format!("Undid {}", applied.label),
            Ok(None) => self.ui.status.sample = String::from("Nothing to undo"),
            Err(error) => self.ui.status.sample = format!("Undo failed: {error}"),
        }
    }

    pub(in crate::native_app) fn redo_transaction(
        &mut self,
        context: &mut ui::UiUpdateContext<GuiMessage>,
    ) {
        if self
            .start_history_file_request(HistoryFileIoDirection::Redo, None, context)
            .is_err()
        {
            self.ui.status.sample = String::from("Redo already in progress");
            return;
        }
        if self.history_file_request_started(HistoryFileIoDirection::Redo, None) {
            return;
        }
        let mut history = std::mem::take(&mut self.transactions.history);
        let was_restoring = self.transactions.restoring;
        self.transactions.restoring = true;
        let result = history.redo(self);
        self.transactions.restoring = was_restoring;
        self.transactions.history = history;
        self.background.rating_persist.schedule_if_idle(context);
        match result {
            Ok(Some(applied)) => self.ui.status.sample = format!("Redid {}", applied.label),
            Ok(None) => self.ui.status.sample = String::from("Nothing to redo"),
            Err(error) => self.ui.status.sample = format!("Redo failed: {error}"),
        }
    }

    pub(in crate::native_app) fn undo_transactions_through(
        &mut self,
        target_id: u64,
        context: &mut ui::UiUpdateContext<GuiMessage>,
    ) {
        self.continue_history_through(HistoryFileIoDirection::Undo, target_id, context);
    }

    pub(in crate::native_app) fn redo_transactions_through(
        &mut self,
        target_id: u64,
        context: &mut ui::UiUpdateContext<GuiMessage>,
    ) {
        self.continue_history_through(HistoryFileIoDirection::Redo, target_id, context);
    }

    fn continue_history_through(
        &mut self,
        direction: HistoryFileIoDirection,
        target_id: u64,
        context: &mut ui::UiUpdateContext<GuiMessage>,
    ) {
        if !self
            .transactions
            .history
            .has_transaction_on_stack(direction, target_id)
        {
            self.ui.status.sample = format!(
                "Transaction #{target_id} is not {}able",
                match direction {
                    HistoryFileIoDirection::Undo => "undo",
                    HistoryFileIoDirection::Redo => "redo",
                }
            );
            return;
        }
        loop {
            let started = match self.start_history_file_request(direction, Some(target_id), context)
            {
                Ok(started) => started,
                Err(error) => {
                    self.ui.status.sample = error;
                    return;
                }
            };
            if started {
                return;
            }
            let mut history = std::mem::take(&mut self.transactions.history);
            let was_restoring = self.transactions.restoring;
            self.transactions.restoring = true;
            let result = match direction {
                HistoryFileIoDirection::Undo => history.undo(self),
                HistoryFileIoDirection::Redo => history.redo(self),
            };
            self.transactions.restoring = was_restoring;
            self.transactions.history = history;
            match result {
                Ok(Some(applied)) => {
                    self.transactions.history_through_count =
                        self.transactions.history_through_count.saturating_add(1);
                    let reached = !self
                        .transactions
                        .history
                        .has_transaction_on_stack(direction, target_id);
                    if reached {
                        let count = std::mem::take(&mut self.transactions.history_through_count);
                        self.ui.status.sample = match direction {
                            HistoryFileIoDirection::Undo => {
                                format!("Undid {} through {}", count, applied.label)
                            }
                            HistoryFileIoDirection::Redo => {
                                format!("Redid {} through {}", count, applied.label)
                            }
                        };
                        return;
                    }
                }
                Ok(None) => {
                    self.ui.status.sample = format!(
                        "Transaction #{target_id} is not {}able",
                        match direction {
                            HistoryFileIoDirection::Undo => "undo",
                            HistoryFileIoDirection::Redo => "redo",
                        }
                    );
                    return;
                }
                Err(error) => {
                    self.ui.status.sample = format!("History failed: {error}");
                    return;
                }
            }
        }
    }

    fn start_history_file_request(
        &mut self,
        direction: HistoryFileIoDirection,
        through_target: Option<u64>,
        context: &mut ui::UiUpdateContext<GuiMessage>,
    ) -> Result<bool, String> {
        let execution_id = self.background.next_task_id();
        let command =
            self.transactions
                .history
                .begin_file_io(direction, through_target, execution_id)?;
        let Some(command) = command else {
            return Ok(false);
        };
        context.emit(GuiMessage::HistoryFileIoRequested(command));
        Ok(true)
    }

    fn history_file_request_started(
        &self,
        direction: HistoryFileIoDirection,
        through_target: Option<u64>,
    ) -> bool {
        self.transactions.history.file_io_in_flight()
            && through_target.is_none()
            && matches!(
                direction,
                HistoryFileIoDirection::Undo | HistoryFileIoDirection::Redo
            )
    }

    pub(in crate::native_app) fn start_history_file_io(
        &mut self,
        command: HistoryFileIoCommand,
        context: &mut ui::UiUpdateContext<GuiMessage>,
    ) {
        let gate = self.background.history_file_io_gate.clone();
        context.business().background("gui-history-file-io").run(
            move |_| {
                let _guard = gate.lock().expect("history file I/O owner lock");
                crate::native_app::transaction_history::file_io::execute_history_file_io(command)
            },
            GuiMessage::HistoryFileIoFinished,
        );
    }

    pub(in crate::native_app) fn finish_history_file_io(
        &mut self,
        result: HistoryFileIoResult,
        context: &mut ui::UiUpdateContext<GuiMessage>,
    ) {
        let success = result.result.is_ok();
        let output = result.result.ok();
        let direction = result.direction;
        let through_target = result.through_target;
        let transaction_id = result.transaction_id;
        if let Some(output) = output {
            let correlation_id = crate::native_app::sample_library::committed_file_mutations::ProcessLocalMutationCorrelationId::from_raw(self.background.next_task_id());
            let waveform_paths = output.waveform_paths;
            self.transactions.pending_history_commit = Some(PendingHistoryCommit {
                execution_id: result.execution_id,
                transaction_id,
                direction,
                through_target,
                correlation_id,
                waveform_paths,
            });
            let queued = self.queue_prepared_partially_committed_file_mutation_with_correlation(
                match direction {
                    HistoryFileIoDirection::Undo => {
                        crate::native_app::sample_library::committed_file_mutations::FileMutationOperation::Undo
                    }
                    HistoryFileIoDirection::Redo => {
                        crate::native_app::sample_library::committed_file_mutations::FileMutationOperation::Redo
                    }
                },
                output.changes,
                output.failures,
                correlation_id,
                context,
            );
            if queued.is_none() {
                self.finalize_history_file_io(correlation_id, false, context);
            }
            if self.transactions.pending_history_commit.is_none() {
                return;
            }
            return;
        }
        if !success {
            let _ = self.transactions.history.finish_file_io(
                result.execution_id,
                transaction_id,
                direction,
                false,
            );
            self.ui.status.sample = format!(
                "{} failed",
                match direction {
                    HistoryFileIoDirection::Undo => "Undo",
                    HistoryFileIoDirection::Redo => "Redo",
                }
            );
            return;
        }
        let _ = through_target;
    }

    pub(in crate::native_app) fn finalize_history_file_io(
        &mut self,
        correlation_id: crate::native_app::sample_library::committed_file_mutations::ProcessLocalMutationCorrelationId,
        accepted: bool,
        context: &mut ui::UiUpdateContext<GuiMessage>,
    ) {
        let Some(pending) = self.transactions.pending_history_commit.take() else {
            return;
        };
        if pending.correlation_id != correlation_id {
            self.transactions.pending_history_commit = Some(pending);
            return;
        }
        let applied = match self.transactions.history.finish_file_io(
            pending.execution_id,
            pending.transaction_id,
            pending.direction,
            accepted,
        ) {
            Ok(applied) => applied,
            Err(error) => {
                self.ui.status.sample = format!("History failed: {error}");
                return;
            }
        };
        if !accepted {
            self.ui.status.sample = String::from("History reconciliation failed");
            return;
        }
        for path in pending.waveform_paths {
            self.evict_waveform_cache_path(&path);
            if let Err(error) = self.reload_waveform_path_now_if_loaded(&path) {
                self.ui.status.sample =
                    format!("History applied but waveform reload failed: {error}");
            }
        }
        if let Some(target_id) = pending.through_target {
            if target_id != pending.transaction_id {
                self.continue_history_through(pending.direction, target_id, context);
                return;
            }
            self.transactions.history_through_count =
                self.transactions.history_through_count.saturating_add(1);
            let count = std::mem::take(&mut self.transactions.history_through_count);
            self.ui.status.sample = match pending.direction {
                HistoryFileIoDirection::Undo => {
                    format!("Undid {count} through {}", applied.0.label)
                }
                HistoryFileIoDirection::Redo => {
                    format!("Redid {count} through {}", applied.0.label)
                }
            };
            return;
        }
        self.ui.status.sample = match pending.direction {
            HistoryFileIoDirection::Undo => format!("Undid {}", applied.0.label),
            HistoryFileIoDirection::Redo => format!("Redid {}", applied.0.label),
        };
        self.background.rating_persist.schedule_if_idle(context);
    }

    pub(in crate::native_app) fn toggle_transaction_list(&mut self) {
        self.ui.chrome.transaction_list_open = !self.ui.chrome.transaction_list_open;
    }
}
