use crate::native_app::transaction_history::TransactionContext;

#[cfg(test)]
use super::worker::AppliedWaveformEdit;
#[cfg(test)]
use std::path::Path;

impl TransactionContext<'_> {
    #[cfg(test)]
    pub(in crate::native_app) fn restore_edited_waveform(
        &mut self,
        backup_path: &Path,
        applied: &AppliedWaveformEdit,
    ) -> Result<(), String> {
        if let Some(error) = self
            .state
            .library
            .folder_browser
            .file_change_lock_error(&applied.absolute_path, "Undo")
        {
            return Err(error);
        }
        let _ = (backup_path, applied);
        Err(String::from(
            "file-backed waveform history must use the async owner",
        ))
    }
}
