use crate::native_app::app::{NativeAppState, ShortcutHelpSection, shortcut_help_sections};
use crate::native_app::transaction_history::{TransactionListItem, TransactionListState};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct TransactionListProjection {
    pub(super) summary: String,
    pub(super) rows: Vec<TransactionListRowProjection>,
    pub(super) empty_title: &'static str,
    pub(super) empty_detail: &'static str,
}

impl TransactionListProjection {
    pub(super) fn from_state(state: &NativeAppState) -> Self {
        Self {
            summary: transaction_list_summary(state),
            rows: state
                .transactions
                .history
                .list_items()
                .into_iter()
                .map(TransactionListRowProjection::from_item)
                .collect(),
            empty_title: "No transactions registered",
            empty_detail: "Undoable actions will appear here.",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct TransactionListRowProjection {
    pub(super) id: u64,
    pub(super) order_label: String,
    pub(super) label: String,
    pub(super) action_summary: String,
    pub(super) state: TransactionListState,
}

impl TransactionListRowProjection {
    fn from_item(item: TransactionListItem) -> Self {
        let action_summary = transaction_action_summary(&item);
        let order_label = match item.state {
            TransactionListState::Active => String::from("Draft"),
            TransactionListState::Unavailable
            | TransactionListState::Undoable
            | TransactionListState::Redoable => {
                format!("#{}", item.id)
            }
        };
        Self {
            id: item.id,
            order_label,
            label: item.label,
            action_summary,
            state: item.state,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct ShortcutHelpProjection {
    pub(super) intro: &'static str,
    pub(super) sections: Vec<ShortcutHelpSection>,
}

impl ShortcutHelpProjection {
    pub(super) fn from_state(state: &NativeAppState) -> Self {
        Self {
            intro: "Context-aware keyboard shortcuts. Press Esc or Command-/ to close.",
            sections: shortcut_help_sections(state),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct FileMoveConflictProjection {
    pub(super) summary: String,
    pub(super) file_name: String,
    pub(super) destination: String,
    pub(super) apply_to_remaining: bool,
}

impl FileMoveConflictProjection {
    pub(super) fn from_state(state: &NativeAppState) -> Self {
        let conflict = state
            .library
            .folder_browser
            .pending_file_move_conflict_view()
            .expect("file move conflict modal requires pending conflict state");
        Self {
            summary: format!(
                "Conflict {} of {}",
                conflict.current_number, conflict.total_count
            ),
            file_name: conflict.file_name,
            destination: format!("Destination: {}", conflict.destination_folder),
            apply_to_remaining: state
                .ui
                .browser_interaction
                .file_move_conflict_apply_to_remaining,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct FolderDeleteConfirmationProjection {
    pub(super) name: String,
    pub(super) question: &'static str,
    pub(super) detail: &'static str,
}

impl FolderDeleteConfirmationProjection {
    pub(super) fn from_state(state: &NativeAppState) -> Self {
        let target = state
            .ui
            .browser_interaction
            .pending_folder_delete
            .as_ref()
            .expect("folder delete modal requires pending folder delete state");
        Self {
            name: target.name.clone(),
            question: "Move folder contents to the configured trash folder?",
            detail: "The folder tree will update after the move completes.",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct WaveformDestructiveEditProjection {
    pub(super) title: String,
    pub(super) message: String,
}

impl WaveformDestructiveEditProjection {
    pub(super) fn from_state(state: &NativeAppState) -> Self {
        let pending = state
            .ui
            .browser_interaction
            .pending_waveform_destructive_edit
            .as_ref()
            .expect("waveform destructive modal requires pending edit state");
        Self {
            title: pending.prompt.title.clone(),
            message: pending.prompt.message.clone(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct ProtectedExtractionTargetSourceProjection {
    pub(super) title: String,
    pub(super) message: String,
}

impl ProtectedExtractionTargetSourceProjection {
    pub(super) fn from_state(state: &NativeAppState) -> Self {
        let pending = state
            .ui
            .browser_interaction
            .pending_protected_extraction_target_source
            .as_ref()
            .expect("protected extraction target modal requires pending target source state");
        Self {
            title: pending.title.clone(),
            message: pending.message.clone(),
        }
    }
}

fn transaction_list_summary(state: &NativeAppState) -> String {
    format!(
        "{} | {} | {}",
        undo_summary(state),
        redo_summary(state),
        active_transaction_summary(state)
    )
}

fn undo_summary(state: &NativeAppState) -> &'static str {
    if state.transactions.history.can_undo() {
        "undo ready"
    } else {
        "no undo"
    }
}

fn redo_summary(state: &NativeAppState) -> &'static str {
    if state.transactions.history.can_redo() {
        "redo ready"
    } else {
        "no redo"
    }
}

fn active_transaction_summary(state: &NativeAppState) -> &'static str {
    if state.transactions.history.is_transaction_open() {
        "open transaction"
    } else {
        "closed"
    }
}

fn transaction_action_summary(item: &TransactionListItem) -> String {
    let action_label = match item.action_count {
        1 => String::from("1 action"),
        count => format!("{count} actions"),
    };
    if item.action_labels.is_empty() {
        action_label
    } else {
        format!("{}: {}", action_label, item.action_labels.join(", "))
    }
}
