use super::super::{file_metadata, DragDropController};
use crate::egui_app::state::DragSample;
use crate::egui_app::ui::style::StatusTone;
use crate::sample_sources::{SourceId, WavEntry};
use std::path::{Path, PathBuf};
use tracing::{info, warn};

impl DragDropController<'_> {
    pub(in crate::egui_app::controller::drag_drop_controller) fn handle_sample_drop_to_folder(
        &mut self,
        source_id: SourceId,
        relative_path: PathBuf,
        target_folder: &Path,
    ) {
        info!(
            "Folder drop requested: source_id={:?} path={} target={}",
            source_id,
            relative_path.display(),
            target_folder.display()
        );
        let Some(source) = self
            .library
            .sources
            .iter()
            .find(|s| s.id == source_id)
            .cloned()
        else {
            warn!("Folder move: missing source {:?}", source_id);
            self.set_status("Source not available for move", StatusTone::Error);
            return;
        };
        if self
            .selection_state
            .ctx
            .selected_source
            .as_ref()
            .is_some_and(|selected| selected != &source.id)
        {
            warn!(
                "Folder move blocked: selected source {:?} differs from sample source {:?}",
                self.selection_state.ctx.selected_source, source.id
            );
            self.set_status(
                "Switch to the sample's source before moving into its folders",
                StatusTone::Warning,
            );
            return;
        }
        let file_name = match relative_path.file_name() {
            Some(name) => name.to_owned(),
            None => {
                warn!(
                    "Folder move aborted: missing file name for {:?}",
                    relative_path
                );
                self.set_status("Sample name unavailable for move", StatusTone::Error);
                return;
            }
        };
        let new_relative = if target_folder.as_os_str().is_empty() {
            PathBuf::from(file_name)
        } else {
            target_folder.join(file_name)
        };
        if new_relative == relative_path {
            info!("Folder move skipped: already in target {:?}", target_folder);
            self.set_status("Sample is already in that folder", StatusTone::Info);
            return;
        }
        let destination_dir = source.root.join(target_folder);
        if !destination_dir.is_dir() {
            self.set_status(
                format!("Folder not found: {}", target_folder.display()),
                StatusTone::Error,
            );
            return;
        }
        let absolute = source.root.join(&relative_path);
        if !absolute.exists() {
            warn!(
                "Folder move aborted: missing source file {}",
                relative_path.display()
            );
            self.set_status(
                format!("File missing: {}", relative_path.display()),
                StatusTone::Error,
            );
            return;
        }
        let target_absolute = source.root.join(&new_relative);
        if target_absolute.exists() {
            warn!(
                "Folder move aborted: target already exists {}",
                new_relative.display()
            );
            self.set_status(
                format!("A file already exists at {}", new_relative.display()),
                StatusTone::Error,
            );
            return;
        }
        let tag = match self.sample_tag_for(&source, &relative_path) {
            Ok(tag) => tag,
            Err(err) => {
                warn!(
                    "Folder move aborted: failed to resolve tag for {}: {}",
                    relative_path.display(),
                    err
                );
                self.set_status(err, StatusTone::Error);
                return;
            }
        };
        if let Err(err) = std::fs::rename(&absolute, &target_absolute)
            .map_err(|err| format!("Failed to move file: {err}"))
        {
            warn!(
                "Folder move aborted: rename failed {} -> {} : {}",
                absolute.display(),
                target_absolute.display(),
                err
            );
            self.set_status(err, StatusTone::Error);
            return;
        }
        let (file_size, modified_ns) = match file_metadata(&target_absolute) {
            Ok(meta) => meta,
            Err(err) => {
                let _ = std::fs::rename(&target_absolute, &absolute);
                warn!(
                    "Folder move aborted: metadata failed for {} : {}",
                    target_absolute.display(),
                    err
                );
                self.set_status(err, StatusTone::Error);
                return;
            }
        };
        if let Err(err) = self.rewrite_db_entry_for_source(
            &source,
            &relative_path,
            &new_relative,
            file_size,
            modified_ns,
            tag,
        ) {
            let _ = std::fs::rename(&target_absolute, &absolute);
            warn!(
                "Folder move aborted: db rewrite failed {} -> {} : {}",
                relative_path.display(),
                new_relative.display(),
                err
            );
            self.set_status(err, StatusTone::Error);
            return;
        }
        let new_entry = WavEntry {
            relative_path: new_relative.clone(),
            file_size,
            modified_ns,
            content_hash: None,
            tag,
            missing: false,
        };
        self.update_cached_entry(&source, &relative_path, new_entry);
        if self.update_collections_for_rename(&source.id, &relative_path, &new_relative) {
            let _ = self.persist_config("Failed to save collections after move");
        }
        info!(
            "Folder move success: {} -> {}",
            relative_path.display(),
            new_relative.display()
        );
        self.set_status(
            format!("Moved to {}", target_folder.display()),
            StatusTone::Info,
        );
    }

    pub(in crate::egui_app::controller::drag_drop_controller) fn handle_samples_drop_to_folder(
        &mut self,
        samples: &[DragSample],
        target_folder: &Path,
    ) {
        for sample in samples {
            self.handle_sample_drop_to_folder(
                sample.source_id.clone(),
                sample.relative_path.clone(),
                target_folder,
            );
        }
    }

    pub(in crate::egui_app::controller::drag_drop_controller) fn handle_folder_drop_to_folder(
        &mut self,
        source_id: SourceId,
        folder: PathBuf,
        target_folder: &Path,
    ) {
        info!(
            "Folder drag requested: source_id={:?} folder={} target={}",
            source_id,
            folder.display(),
            target_folder.display()
        );
        let Some(source) = self
            .library
            .sources
            .iter()
            .find(|s| s.id == source_id)
            .cloned()
        else {
            warn!("Folder drag: missing source {:?}", source_id);
            self.set_status("Source not available for move", StatusTone::Error);
            return;
        };
        if folder.as_os_str().is_empty() {
            self.set_status("Root folder cannot be moved", StatusTone::Warning);
            return;
        }
        if target_folder == folder {
            self.set_status("Folder is already there", StatusTone::Info);
            return;
        }
        if target_folder.starts_with(&folder) {
            self.set_status("Cannot move a folder into itself", StatusTone::Warning);
            return;
        }
        if self
            .selection_state
            .ctx
            .selected_source
            .as_ref()
            .is_some_and(|selected| selected != &source.id)
        {
            warn!(
                "Folder drag blocked: selected source {:?} differs from folder source {:?}",
                self.selection_state.ctx.selected_source, source.id
            );
            self.set_status(
                "Switch to the folder's source before moving it",
                StatusTone::Warning,
            );
            return;
        }
        match self.move_folder_to_parent(&folder, target_folder) {
            Ok(new_relative) => {
                self.set_status(
                    format!("Moved folder to {}", new_relative.display()),
                    StatusTone::Info,
                );
            }
            Err(err) => {
                warn!(
                    "Folder drag aborted: move failed {} -> {} : {}",
                    folder.display(),
                    target_folder.display(),
                    err
                );
                self.set_status(err, StatusTone::Error);
            }
        }
    }
}
