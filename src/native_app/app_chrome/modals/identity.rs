use crate::native_app::transaction_history;

pub(super) const TRANSACTION_LIST_MODAL_ID: u64 = transaction_history::TRANSACTION_LIST_MODAL_ID;
pub(super) const TRANSACTION_LIST_SUMMARY_KEY: &str = "transaction-list-summary";
pub(super) const SHORTCUT_HELP_MODAL_KEY: &str = "shortcut-help-modal";
pub(super) const FILE_MOVE_CONFLICT_MODAL_KEY: &str = "file-move-conflict-modal";
pub(super) const FOLDER_DELETE_CONFIRMATION_MODAL_KEY: &str = "folder-delete-confirmation-modal";
pub(super) const WAVEFORM_DESTRUCTIVE_EDIT_MODAL_KEY: &str = "waveform-destructive-edit-modal";
pub(super) const PROTECTED_EXTRACTION_TARGET_SOURCE_MODAL_KEY: &str =
    "protected-extraction-target-source-modal";
pub(super) const TRASH_FOLDER_SETUP_MODAL_KEY: &str = "trash-folder-setup-modal";
pub(super) const UNSUPPORTED_FILES_MODAL_KEY: &str = "unsupported-files-modal";

pub(super) fn transaction_list_row_key(id: u64) -> String {
    format!("transaction-list-row-{id}")
}
