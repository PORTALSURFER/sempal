mod completion;
mod entrypoints;
mod prompt;
mod protected_copy;
mod queue;
mod transaction;
mod worker;

pub(in crate::native_app) use worker::WaveformDestructiveEditResult;
pub(in crate::native_app) use worker::{
    AppliedWaveformEdit, restore_edited_waveform, restore_extracted_file_for_transaction,
};
#[cfg(test)]
pub(in crate::native_app) use worker::{
    destructive_edit_before_backup_path_for_tests, execute_destructive_edit_for_tests,
    waveform_restore_action_for_capacity_tests,
};
