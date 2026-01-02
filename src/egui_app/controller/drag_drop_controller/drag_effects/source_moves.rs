use super::super::{file_metadata, DragDropController};
use crate::egui_app::state::DragSample;
use crate::egui_app::ui::style::StatusTone;
use crate::sample_sources::{SampleTag, SourceId, WavEntry};
use std::path::{Path, PathBuf};
use tracing::info;

impl DragDropController<'_> {
    pub(in crate::egui_app::controller::drag_drop_controller) fn handle_sample_drop_to_source(
        &mut self,
        source_id: SourceId,
        relative_path: PathBuf,
        target_source_id: SourceId,
    ) {
        if source_id == target_source_id {
            self.set_status("Sample is already in that source", StatusTone::Info);
            return;
        }
        let Some(source) = self
            .library
            .sources
            .iter()
            .find(|s| s.id == source_id)
            .cloned()
        else {
            self.set_status("Source not available for move", StatusTone::Error);
            return;
        };
        let Some(target_source) = self
            .library
            .sources
            .iter()
            .find(|s| s.id == target_source_id)
            .cloned()
        else {
            self.set_status("Target source not available for move", StatusTone::Error);
            return;
        };
        if !target_source.root.is_dir() {
            self.set_status(
                format!("Target source folder missing: {}", target_source.root.display()),
                StatusTone::Error,
            );
            return;
        }
        let absolute = source.root.join(&relative_path);
        if !absolute.exists() {
            self.set_status(
                format!("File missing: {}", relative_path.display()),
                StatusTone::Error,
            );
            return;
        }
        let tag = match self.sample_tag_for(&source, &relative_path) {
            Ok(tag) => tag,
            Err(err) => {
                self.set_status(err, StatusTone::Error);
                return;
            }
        };
        let target_relative = match unique_destination_path(&target_source.root, &relative_path) {
            Ok(path) => path,
            Err(err) => {
                self.set_status(err, StatusTone::Error);
                return;
            }
        };
        if let Some(parent) = target_relative.parent() {
            let target_dir = target_source.root.join(parent);
            if let Err(err) = std::fs::create_dir_all(&target_dir) {
                self.set_status(
                    format!(
                        "Failed to create target folder {}: {err}",
                        target_dir.display()
                    ),
                    StatusTone::Error,
                );
                return;
            }
        }
        let target_absolute = target_source.root.join(&target_relative);
        if let Err(err) = move_sample_file(&absolute, &target_absolute) {
            self.set_status(err, StatusTone::Error);
            return;
        }
        let (file_size, modified_ns) = match file_metadata(&target_absolute) {
            Ok(meta) => meta,
            Err(err) => {
                let _ = move_sample_file(&target_absolute, &absolute);
                self.set_status(err, StatusTone::Error);
                return;
            }
        };
        if let Err(err) = self.register_moved_sample_for_source(
            &target_source,
            &target_relative,
            file_size,
            modified_ns,
            tag,
        ) {
            let _ = move_sample_file(&target_absolute, &absolute);
            self.set_status(err, StatusTone::Error);
            return;
        }
        if let Err(err) = self.remove_source_db_entry(&source, &relative_path) {
            let _ = self.remove_target_db_entry(&target_source, &target_relative);
            let _ = move_sample_file(&target_absolute, &absolute);
            self.set_status(err, StatusTone::Error);
            return;
        }
        self.prune_cached_sample(&source, &relative_path);
        let new_entry = WavEntry {
            relative_path: target_relative.clone(),
            file_size,
            modified_ns,
            content_hash: None,
            tag,
            missing: false,
        };
        self.insert_cached_entry(&target_source, new_entry);
        if self.update_collections_for_source_move(
            &source.id,
            &target_source.id,
            &relative_path,
            &target_relative,
        ) {
            let _ = self.persist_config("Failed to save collections after move");
        }
        info!(
            "Source move success: {} -> {}",
            relative_path.display(),
            target_relative.display()
        );
        self.set_status(
            format!("Moved to {}", target_source.root.display()),
            StatusTone::Info,
        );
    }

    pub(in crate::egui_app::controller::drag_drop_controller) fn handle_samples_drop_to_source(
        &mut self,
        samples: &[DragSample],
        target_source_id: SourceId,
    ) {
        for sample in samples {
            self.handle_sample_drop_to_source(
                sample.source_id.clone(),
                sample.relative_path.clone(),
                target_source_id.clone(),
            );
        }
    }

    fn register_moved_sample_for_source(
        &mut self,
        source: &crate::sample_sources::SampleSource,
        relative_path: &Path,
        file_size: u64,
        modified_ns: i64,
        tag: SampleTag,
    ) -> Result<(), String> {
        let db = self
            .database_for(source)
            .map_err(|err| format!("Database unavailable: {err}"))?;
        db.upsert_file(relative_path, file_size, modified_ns)
            .map_err(|err| format!("Failed to register file: {err}"))?;
        db.set_tag(relative_path, tag)
            .map_err(|err| format!("Failed to set tag: {err}"))?;
        Ok(())
    }

    fn remove_source_db_entry(
        &mut self,
        source: &crate::sample_sources::SampleSource,
        relative_path: &Path,
    ) -> Result<(), String> {
        let db = self
            .database_for(source)
            .map_err(|err| format!("Database unavailable: {err}"))?;
        db.remove_file(relative_path)
            .map_err(|err| format!("Failed to drop database row: {err}"))
    }

    fn remove_target_db_entry(
        &mut self,
        source: &crate::sample_sources::SampleSource,
        relative_path: &Path,
    ) -> Result<(), String> {
        let db = self
            .database_for(source)
            .map_err(|err| format!("Database unavailable: {err}"))?;
        db.remove_file(relative_path)
            .map_err(|err| format!("Failed to drop database row: {err}"))
    }

    fn update_collections_for_source_move(
        &mut self,
        from: &SourceId,
        to: &SourceId,
        old_relative: &Path,
        new_relative: &Path,
    ) -> bool {
        let new_path = new_relative.to_path_buf();
        let mut changed = false;
        for collection in &mut self.library.collections {
            if collection.contains(to, &new_path) {
                let before = collection.members.len();
                collection.members.retain(|member| {
                    if member.clip_root.is_some() {
                        return true;
                    }
                    &member.source_id != from || member.relative_path.as_path() != old_relative
                });
                if before != collection.members.len() {
                    changed = true;
                }
                continue;
            }
            for member in &mut collection.members {
                if member.clip_root.is_some() {
                    continue;
                }
                if &member.source_id == from && member.relative_path == old_relative {
                    member.source_id = to.clone();
                    member.relative_path = new_path.clone();
                    changed = true;
                }
            }
        }
        changed
    }
}

fn unique_destination_path(root: &Path, relative: &Path) -> Result<PathBuf, String> {
    if !root.join(relative).exists() {
        return Ok(relative.to_path_buf());
    }
    let parent = relative.parent().unwrap_or_else(|| Path::new(""));
    let file_name = relative
        .file_name()
        .ok_or_else(|| "Sample has no file name".to_string())?;
    let stem = Path::new(file_name)
        .file_stem()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| "sample".to_string());
    let extension = Path::new(file_name)
        .extension()
        .map(|ext| ext.to_string_lossy().to_string());
    for index in 1..=999 {
        let suffix = format!("{stem}_move{index:03}");
        let file_name = if let Some(ext) = &extension {
            format!("{suffix}.{ext}")
        } else {
            suffix
        };
        let candidate = parent.join(file_name);
        if !root.join(&candidate).exists() {
            return Ok(candidate);
        }
    }
    Err("Failed to find destination file name".into())
}

fn move_sample_file(source: &Path, destination: &Path) -> Result<(), String> {
    match std::fs::rename(source, destination) {
        Ok(()) => Ok(()),
        Err(rename_err) => {
            if let Err(copy_err) = std::fs::copy(source, destination) {
                return Err(format!(
                    "Failed to move file: {rename_err}; copy failed: {copy_err}"
                ));
            }
            if let Err(remove_err) = std::fs::remove_file(source) {
                let _ = std::fs::remove_file(destination);
                return Err(format!("Failed to remove original file: {remove_err}"));
            }
            Ok(())
        }
    }
}
