use crate::native_app::app::{
    GuiMessage, NativeAppState, OperationJournalRestoreCompletion, OperationJournalRestoreError,
    PendingHistoryCommit, PendingHistoryOwnerStaging,
};
use crate::native_app::transaction_history::operation_journal::{
    FilesystemStageOutcome, OperationActor, OperationId, OperationIntent, OperationKind,
};
use crate::native_app::transaction_history::{
    HistoryFileIoCommand, HistoryFileIoDirection, HistoryFileIoResult, HistoryFileIoRoute,
    TransactionContext, TransactionResult,
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
        if let Err(error) =
            self.start_history_file_request(HistoryFileIoDirection::Undo, None, context)
        {
            self.ui.status.sample = error;
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
        if let Err(error) =
            self.start_history_file_request(HistoryFileIoDirection::Redo, None, context)
        {
            self.ui.status.sample = error;
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
        let command = self
            .transactions
            .history
            .begin_file_io(direction, through_target, execution_id)
            .map_err(|error| {
                format!("{} not started: {error}", history_direction_verb(direction))
            })?;
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
        if command.route == HistoryFileIoRoute::OwnerWaveformRestore {
            self.start_owner_waveform_restore(command, context);
            return;
        }
        let gate = self.background.history_file_io_gate.clone();
        context.business().background("gui-history-file-io").run(
            move |_| {
                let _guard = gate.lock().expect("history file I/O owner lock");
                crate::native_app::transaction_history::file_io::execute_history_file_io(command)
            },
            GuiMessage::HistoryFileIoFinished,
        );
    }

    fn start_owner_waveform_restore(
        &mut self,
        command: HistoryFileIoCommand,
        context: &mut ui::UiUpdateContext<GuiMessage>,
    ) {
        if let Err(error) = self.background.waveform_recovery.clone() {
            let execution_id = command.execution_id;
            let transaction_id = command.transaction_id;
            let direction = command.direction;
            let through_target = command.through_target;
            self.restore_history_file_io_not_started(
                execution_id,
                transaction_id,
                direction,
                through_target,
                owner_restore_error_status(
                    direction,
                    OperationJournalRestoreError::RecoveryUnavailable(error),
                ),
            );
            return;
        }
        let intent = OperationIntent {
            actor: OperationActor::User,
            kind: OperationKind::FileHistory,
            label: command.label.clone(),
        };
        let action = command
            .actions
            .first()
            .cloned()
            .expect("owner-staging route has one waveform action");
        let owner_result = self
            .background
            .operation_journal
            .prepare_and_stage_bounded_waveform_restore(
                intent,
                owner_waveform_restore_payload(&command),
                command.direction,
                vec![action],
            );
        let execution_id = command.execution_id;
        let transaction_id = command.transaction_id;
        let direction = command.direction;
        let through_target = command.through_target;
        let label = command.label;
        match owner_result {
            Ok(receiver) => {
                context
                    .business()
                    .background("gui-history-owner-stage")
                    .run(
                        move |_| {
                            let result = receiver
                                .recv()
                                .map_err(|_| OperationJournalRestoreError::Closed)
                                .and_then(|result| result.map_err(Into::into));
                            OperationJournalRestoreCompletion {
                                execution_id,
                                transaction_id,
                                direction,
                                through_target,
                                label,
                                result,
                            }
                        },
                        GuiMessage::OperationJournalRestoreFinished,
                    );
            }
            Err(error) => {
                self.restore_history_file_io_not_started(
                    execution_id,
                    transaction_id,
                    direction,
                    through_target,
                    format!(
                        "{} not started: operation journal queue failed: {error:?}",
                        history_direction_verb(direction)
                    ),
                );
            }
        }
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

    pub(in crate::native_app) fn finish_operation_journal_restore(
        &mut self,
        completion: OperationJournalRestoreCompletion,
    ) {
        if self.transactions.completed_history_owner_matches(
            completion.execution_id,
            completion.transaction_id,
            completion.direction,
            completion.through_target,
        ) {
            return;
        }
        if self
            .transactions
            .pending_history_owner_staging
            .as_ref()
            .is_some_and(|pending| {
                pending.execution_id == completion.execution_id
                    && pending.transaction_id == completion.transaction_id
                    && pending.direction == completion.direction
                    && pending.through_target == completion.through_target
            })
        {
            return;
        }
        if !self.transactions.history.file_io_matches(
            completion.execution_id,
            completion.transaction_id,
            completion.direction,
            completion.through_target,
        ) {
            self.ui.status.sample = String::from(
                "Stale operation journal restore completion; history remains in flight",
            );
            return;
        }
        match completion.result {
            Ok(outcome) => {
                let operation_id = operation_id_for_stage_outcome(&outcome);
                self.transactions.pending_history_owner_staging =
                    Some(PendingHistoryOwnerStaging {
                        execution_id: completion.execution_id,
                        transaction_id: completion.transaction_id,
                        direction: completion.direction,
                        through_target: completion.through_target,
                        label: completion.label.clone(),
                        operation_id,
                        outcome: outcome.clone(),
                    });
                self.ui.status.sample =
                    owner_staging_status(completion.direction, &completion.label, &outcome);
            }
            Err(OperationJournalRestoreError::Journal(error)) => {
                self.ui.status.sample =
                    owner_ambiguous_journal_status(completion.direction, &completion.label, &error);
            }
            Err(error) => {
                self.restore_history_file_io_not_started(
                    completion.execution_id,
                    completion.transaction_id,
                    completion.direction,
                    completion.through_target,
                    owner_restore_error_status(completion.direction, error),
                );
            }
        }
    }

    fn restore_history_file_io_not_started(
        &mut self,
        execution_id: u64,
        transaction_id: u64,
        direction: HistoryFileIoDirection,
        through_target: Option<u64>,
        reason: String,
    ) {
        if !self.transactions.history.file_io_matches(
            execution_id,
            transaction_id,
            direction,
            through_target,
        ) {
            self.ui.status.sample =
                String::from("Stale operation journal failure; history remains in flight");
            return;
        }
        match self.transactions.history.finish_file_io(
            execution_id,
            transaction_id,
            direction,
            false,
        ) {
            Ok(_) => {
                self.transactions.retain_completed_history_owner(
                    execution_id,
                    transaction_id,
                    direction,
                    through_target,
                );
                self.ui.status.sample = reason;
            }
            Err(error) => self.ui.status.sample = format!("{reason}: {error}"),
        }
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

fn operation_id_for_stage_outcome(outcome: &FilesystemStageOutcome) -> OperationId {
    match outcome {
        FilesystemStageOutcome::FilesystemStaged(operation_id)
        | FilesystemStageOutcome::FilesystemPublished(operation_id)
        | FilesystemStageOutcome::PlatformQualificationRequired { operation_id, .. }
        | FilesystemStageOutcome::RetryPending { operation_id, .. }
        | FilesystemStageOutcome::AuditRequired { operation_id, .. }
        | FilesystemStageOutcome::JournalWriteFailed { operation_id, .. } => *operation_id,
    }
}

fn owner_waveform_restore_payload(command: &HistoryFileIoCommand) -> serde_json::Value {
    serde_json::json!({
        "execution_id": command.execution_id,
        "transaction_id": command.transaction_id,
        "direction": match command.direction {
            HistoryFileIoDirection::Undo => "undo",
            HistoryFileIoDirection::Redo => "redo",
        },
        "through_target": command.through_target,
    })
}

fn owner_staging_status(
    direction: HistoryFileIoDirection,
    label: &str,
    outcome: &FilesystemStageOutcome,
) -> String {
    let verb = match direction {
        HistoryFileIoDirection::Undo => "Undo",
        HistoryFileIoDirection::Redo => "Redo",
    };
    match outcome {
        FilesystemStageOutcome::FilesystemStaged(operation_id) => format!(
            "{verb} {label} staged (operation {operation_id}); final publication is pending"
        ),
        FilesystemStageOutcome::FilesystemPublished(operation_id) => format!(
            "{verb} {label} published (operation {operation_id}); history completion is pending"
        ),
        FilesystemStageOutcome::PlatformQualificationRequired { operation_id, .. } => format!(
            "{verb} {label}: safe replacement unavailable on the current platform/build (operation {operation_id}); staged recovery data preserved; retry requires platform/build or qualification-policy requalification"
        ),
        FilesystemStageOutcome::RetryPending {
            operation_id,
            reason,
        } => format!("{verb} {label} retry pending (operation {operation_id}): {reason}"),
        FilesystemStageOutcome::AuditRequired {
            operation_id,
            reason,
        } => format!("{verb} {label} requires audit (operation {operation_id}): {reason}"),
        FilesystemStageOutcome::JournalWriteFailed {
            operation_id,
            reason,
        } => format!("{verb} {label} journal write failed (operation {operation_id}): {reason}"),
    }
}

fn owner_restore_error_status(
    direction: HistoryFileIoDirection,
    error: OperationJournalRestoreError,
) -> String {
    let detail = match error {
        OperationJournalRestoreError::RejectedBeforeIntent(error) => {
            format!("rejected before durable intent: {error}")
        }
        OperationJournalRestoreError::Journal(error) => {
            unreachable!("ambiguous journal errors must retain history in flight: {error}")
        }
        OperationJournalRestoreError::RecoveryUnavailable(error) => {
            format!("destructive recovery unavailable: {error}")
        }
        OperationJournalRestoreError::Unavailable(error) => {
            format!("operation journal unavailable: {error}")
        }
        OperationJournalRestoreError::Closed => String::from("operation journal owner closed"),
    };
    format!(
        "{} not started: {detail}",
        history_direction_verb(direction)
    )
}

fn owner_ambiguous_journal_status(
    direction: HistoryFileIoDirection,
    label: &str,
    error: &str,
) -> String {
    format!(
        "{} {label} journal/recovery status is ambiguous; inspect recovery before retrying (history remains in flight): {error}",
        history_direction_verb(direction)
    )
}

fn history_direction_verb(direction: HistoryFileIoDirection) -> &'static str {
    match direction {
        HistoryFileIoDirection::Undo => "Undo",
        HistoryFileIoDirection::Redo => "Redo",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn owner_waveform_restore_payload_is_bounded_identity() {
        let command = HistoryFileIoCommand {
            execution_id: 19,
            transaction_id: 23,
            label: String::from("ignored from payload"),
            direction: HistoryFileIoDirection::Redo,
            through_target: Some(29),
            route: HistoryFileIoRoute::OwnerWaveformRestore,
            actions: Vec::new(),
        };

        assert_eq!(
            owner_waveform_restore_payload(&command),
            serde_json::json!({
                "execution_id": 19,
                "transaction_id": 23,
                "direction": "redo",
                "through_target": 29,
            })
        );
    }
}
