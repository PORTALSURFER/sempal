use super::super::{DragDropController, file_metadata};
use crate::egui_app::state::DragSample;
use crate::egui_app::ui::style::StatusTone;
use crate::sample_sources::{SampleTag, SourceId, WavEntry};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use tracing::info;

impl DragDropController<'_> {
    pub(crate) fn handle_sample_drop_to_source(
        &mut self,
        source_id: SourceId,
        relative_path: PathBuf,
        target_source_id: SourceId,
    ) -> bool {
        if source_id == target_source_id {
            self.set_status("Sample is already in that source", StatusTone::Info);
            return false;
        }
        let Some(source) = self
            .library
            .sources
            .iter()
            .find(|s| s.id == source_id)
            .cloned()
        else {
            self.set_status("Source not available for move", StatusTone::Error);
            return false;
        };
        let Some(target_source) = self
            .library
            .sources
            .iter()
            .find(|s| s.id == target_source_id)
            .cloned()
        else {
            self.set_status("Target source not available for move", StatusTone::Error);
            return false;
        };
        if !target_source.root.is_dir() {
            self.set_status(
                format!(
                    "Target source folder missing: {}",
                    target_source.root.display()
                ),
                StatusTone::Error,
            );
            return false;
        }
        let absolute = source.root.join(&relative_path);
        if !absolute.exists() {
            self.set_status(
                format!("File missing: {}", relative_path.display()),
                StatusTone::Error,
            );
            return false;
        }
        let tag = match self.sample_tag_for(&source, &relative_path) {
            Ok(tag) => tag,
            Err(err) => {
                self.set_status(err, StatusTone::Error);
                return false;
            }
        };
        let target_relative = match unique_destination_path(&target_source.root, &relative_path) {
            Ok(path) => path,
            Err(err) => {
                self.set_status(err, StatusTone::Error);
                return false;
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
                return false;
            }
        }
        let target_absolute = target_source.root.join(&target_relative);
        if let Err(err) = move_sample_file(&absolute, &target_absolute) {
            self.set_status(err, StatusTone::Error);
            return false;
        }
        let (file_size, modified_ns) = match file_metadata(&target_absolute) {
            Ok(meta) => meta,
            Err(err) => {
                let _ = move_sample_file(&target_absolute, &absolute);
                self.set_status(err, StatusTone::Error);
                return false;
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
            return false;
        }
        if let Err(err) = self.remove_source_db_entry(&source, &relative_path) {
            let _ = self.remove_target_db_entry(&target_source, &target_relative);
            let _ = move_sample_file(&target_absolute, &absolute);
            self.set_status(err, StatusTone::Error);
            return false;
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
        true
    }

    pub(crate) fn handle_samples_drop_to_source(
        &mut self,
        samples: &[DragSample],
        target_source_id: SourceId,
    ) {
        let mut moved_sources = HashSet::new();
        for sample in samples {
            let moved = self.handle_sample_drop_to_source(
                sample.source_id.clone(),
                sample.relative_path.clone(),
                target_source_id.clone(),
            );
            if moved {
                moved_sources.insert(sample.source_id.clone());
                moved_sources.insert(target_source_id.clone());
            }
        }
        for source_id in moved_sources {
            let Some(source) = self
                .library
                .sources
                .iter()
                .find(|source| source.id == source_id)
                .cloned()
            else {
                continue;
            };
            self.invalidate_wav_entries_for_source_preserve_folders(&source);
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::egui_app::controller::EguiController;
    use crate::egui_app::controller::test_support::{sample_entry, write_test_wav};
    use crate::sample_sources::{SampleSource, SampleTag};
    use crate::waveform::WaveformRenderer;
    use tempfile::tempdir;

    #[test]
    fn moving_multiple_samples_to_source_clears_browser_rows() {
        let temp = tempdir().unwrap();
        let source_root = temp.path().join("source_a");
        let target_root = temp.path().join("source_b");
        std::fs::create_dir_all(&source_root).unwrap();
        std::fs::create_dir_all(&target_root).unwrap();
        let source = SampleSource::new(source_root);
        let target = SampleSource::new(target_root);
        let renderer = WaveformRenderer::new(10, 10);
        let mut controller = EguiController::new(renderer, None);
        controller.library.sources.push(source.clone());
        controller.library.sources.push(target.clone());
        controller.selection_state.ctx.selected_source = Some(source.id.clone());
        controller.cache_db(&source).unwrap();
        controller.cache_db(&target).unwrap();
        write_test_wav(&source.root.join("one.wav"), &[0.0, 0.1, -0.1]);
        write_test_wav(&source.root.join("two.wav"), &[0.0, 0.1, -0.1]);
        controller
            .ensure_sample_db_entry(&source, Path::new("one.wav"))
            .unwrap();
        controller
            .ensure_sample_db_entry(&source, Path::new("two.wav"))
            .unwrap();
        controller.set_wav_entries_for_tests(vec![
            sample_entry("one.wav", SampleTag::Neutral),
            sample_entry("two.wav", SampleTag::Neutral),
        ]);
        controller.rebuild_wav_lookup();
        controller.rebuild_browser_lists();

        let samples = vec![
            DragSample {
                source_id: source.id.clone(),
                relative_path: PathBuf::from("one.wav"),
            },
            DragSample {
                source_id: source.id.clone(),
                relative_path: PathBuf::from("two.wav"),
            },
        ];
        controller
            .drag_drop()
            .handle_samples_drop_to_source(&samples, target.id.clone());

        assert!(
            controller
                .wav_index_for_path(Path::new("one.wav"))
                .is_none()
        );
        assert!(
            controller
                .wav_index_for_path(Path::new("two.wav"))
                .is_none()
        );
    }
}
