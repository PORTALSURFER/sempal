use crate::egui_app::controller::jobs::UndoFileJob;
use super::super::undo;
use super::super::*;
use std::path::PathBuf;

impl EguiController {
    pub(crate) fn selection_edit_undo_entry(
        &self,
        label: String,
        source_id: SourceId,
        relative_path: PathBuf,
        absolute_path: PathBuf,
        backup: undo::OverwriteBackup,
    ) -> undo::UndoEntry<EguiController> {
        let before = backup.before.clone();
        let after = backup.after.clone();
        let backup_dir = backup.dir.clone();
        let undo_source_id = source_id.clone();
        let redo_source_id = source_id;
        let undo_relative = relative_path.clone();
        let redo_relative = relative_path;
        let undo_absolute = absolute_path.clone();
        let redo_absolute = absolute_path;
        undo::UndoEntry::<EguiController>::new(
            label,
            move |controller: &mut EguiController| {
                let source = controller
                    .library
                    .sources
                    .iter()
                    .find(|s| s.id == undo_source_id)
                    .cloned()
                    .ok_or_else(|| "Source not available".to_string())?;
                Ok(undo::UndoExecution::Deferred(UndoFileJob::Overwrite {
                    source_id: undo_source_id.clone(),
                    source_root: source.root,
                    relative_path: undo_relative.clone(),
                    absolute_path: undo_absolute.clone(),
                    backup_path: before.clone(),
                }))
            },
            move |controller: &mut EguiController| {
                let source = controller
                    .library
                    .sources
                    .iter()
                    .find(|s| s.id == redo_source_id)
                    .cloned()
                    .ok_or_else(|| "Source not available".to_string())?;
                Ok(undo::UndoExecution::Deferred(UndoFileJob::Overwrite {
                    source_id: redo_source_id.clone(),
                    source_root: source.root,
                    relative_path: redo_relative.clone(),
                    absolute_path: redo_absolute.clone(),
                    backup_path: after.clone(),
                }))
            },
        )
        .with_cleanup_dir(backup_dir)
    }

    pub(crate) fn crop_new_sample_undo_entry(
        &self,
        label: String,
        source_id: SourceId,
        relative_path: PathBuf,
        absolute_path: PathBuf,
        tag: crate::sample_sources::Rating,
        backup: undo::OverwriteBackup,
    ) -> undo::UndoEntry<EguiController> {
        let after = backup.after.clone();
        let backup_dir = backup.dir.clone();
        let undo_source_id = source_id.clone();
        let redo_source_id = source_id;
        let undo_relative = relative_path.clone();
        let redo_relative = relative_path;
        let undo_absolute = absolute_path.clone();
        let redo_absolute = absolute_path;
        undo::UndoEntry::<EguiController>::new(
            label,
            move |controller: &mut EguiController| {
                let source = controller
                    .library
                    .sources
                    .iter()
                    .find(|s| s.id == undo_source_id)
                    .cloned()
                    .ok_or_else(|| "Source not available".to_string())?;
                Ok(undo::UndoExecution::Deferred(UndoFileJob::RemoveSample {
                    source_id: undo_source_id.clone(),
                    source_root: source.root,
                    relative_path: undo_relative.clone(),
                    absolute_path: undo_absolute.clone(),
                }))
            },
            move |controller: &mut EguiController| {
                let source = controller
                    .library
                    .sources
                    .iter()
                    .find(|s| s.id == redo_source_id)
                    .cloned()
                    .ok_or_else(|| "Source not available".to_string())?;
                Ok(undo::UndoExecution::Deferred(UndoFileJob::RestoreSample {
                    source_id: redo_source_id.clone(),
                    source_root: source.root,
                    relative_path: redo_relative.clone(),
                    absolute_path: redo_absolute.clone(),
                    backup_path: after.clone(),
                    tag,
                }))
            },
        )
        .with_cleanup_dir(backup_dir)
    }

}
