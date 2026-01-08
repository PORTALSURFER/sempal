use crate::egui_app::controller::library::collection_items_helpers::file_metadata;
use super::super::undo;
use super::super::*;
use std::path::{Path, PathBuf};

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
                std::fs::copy(&before, &undo_absolute)
                    .map_err(|err| format!("Failed to restore audio: {err}"))?;
                controller.sync_after_audio_overwrite(&undo_source_id, &undo_relative)?;
                Ok(())
            },
            move |controller: &mut EguiController| {
                std::fs::copy(&after, &redo_absolute)
                    .map_err(|err| format!("Failed to reapply audio: {err}"))?;
                controller.sync_after_audio_overwrite(&redo_source_id, &redo_relative)?;
                Ok(())
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
        tag: SampleTag,
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
                let db = controller
                    .database_for(&source)
                    .map_err(|err| format!("Database unavailable: {err}"))?;
                let _ = std::fs::remove_file(&undo_absolute);
                let _ = db.remove_file(&undo_relative);
                controller.prune_cached_sample(&source, &undo_relative);
                Ok(())
            },
            move |controller: &mut EguiController| {
                let source = controller
                    .library
                    .sources
                    .iter()
                    .find(|s| s.id == redo_source_id)
                    .cloned()
                    .ok_or_else(|| "Source not available".to_string())?;
                let db = controller
                    .database_for(&source)
                    .map_err(|err| format!("Database unavailable: {err}"))?;
                if let Some(parent) = redo_absolute.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                std::fs::copy(&after, &redo_absolute)
                    .map_err(|err| format!("Failed to restore crop file: {err}"))?;
                let (file_size, modified_ns) = file_metadata(&redo_absolute)?;
                db.upsert_file(&redo_relative, file_size, modified_ns)
                    .map_err(|err| format!("Failed to sync database entry: {err}"))?;
                db.set_tag(&redo_relative, tag)
                    .map_err(|err| format!("Failed to sync tag: {err}"))?;
                controller.insert_cached_entry(
                    &source,
                    WavEntry {
                        relative_path: redo_relative.clone(),
                        file_size,
                        modified_ns,
                        content_hash: None,
                        tag,
                        missing: false,
                    },
                );
                controller.refresh_waveform_for_sample(&source, &redo_relative);
                controller.reexport_collections_for_sample(&source.id, &redo_relative);
                Ok(())
            },
        )
        .with_cleanup_dir(backup_dir)
    }

    pub(crate) fn sync_after_audio_overwrite(
        &mut self,
        source_id: &SourceId,
        relative_path: &Path,
    ) -> Result<(), String> {
        let source = self
            .library
            .sources
            .iter()
            .find(|s| &s.id == source_id)
            .cloned()
            .ok_or_else(|| "Source not available".to_string())?;
        let absolute_path = source.root.join(relative_path);
        let (file_size, modified_ns) = file_metadata(&absolute_path)?;
        let tag = self.sample_tag_for(&source, relative_path)?;
        let entry = WavEntry {
            relative_path: relative_path.to_path_buf(),
            file_size,
            modified_ns,
            content_hash: None,
            tag,
            missing: false,
        };
        self.update_cached_entry(&source, relative_path, entry);
        self.refresh_waveform_for_sample(&source, relative_path);
        self.reexport_collections_for_sample(&source.id, relative_path);
        Ok(())
    }
}
