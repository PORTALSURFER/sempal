use super::super::{DragDropController, file_metadata};
use crate::egui_app::state::DragSample;
use crate::egui_app::ui::style::StatusTone;
use crate::sample_sources::{Rating, SourceId, WavEntry};
use std::path::{Path, PathBuf};
use tracing::{info, warn};

impl DragDropController<'_> {
    pub(crate) fn handle_sample_drop_to_drop_target(
        &mut self,
        source_id: SourceId,
        relative_path: PathBuf,
        target_path: PathBuf,
        copy_requested: bool,
    ) {
        info!(
            "Drop target requested: source_id={:?} path={} target={}",
            source_id,
            relative_path.display(),
            target_path.display()
        );
        let Some(source) = self
            .library
            .sources
            .iter()
            .find(|s| s.id == source_id)
            .cloned()
        else {
            warn!("Drop target: missing source {:?}", source_id);
            self.set_status("Source not available for drop", StatusTone::Error);
            return;
        };
        let Some(target) = self.resolve_drop_target_location(&target_path) else {
            self.set_status(
                "Drop target is not inside a configured source",
                StatusTone::Warning,
            );
            return;
        };
        let target_dir = target.source.root.join(&target.relative_folder);
        if !target_dir.is_dir() {
            self.set_status(
                format!("Drop target missing: {}", target_dir.display()),
                StatusTone::Error,
            );
            return;
        }
        if source.id == target.source.id && !copy_requested {
            self.handle_sample_drop_to_folder(source_id, relative_path, &target.relative_folder);
            return;
        }

        let file_name = match relative_path.file_name() {
            Some(name) => name.to_owned(),
            None => {
                self.set_status("Sample name unavailable for drop", StatusTone::Error);
                return;
            }
        };
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
        let last_played_at = self
            .sample_last_played_for(&source, &relative_path)
            .unwrap_or(None);
        if copy_requested {
            match copy_sample_to_target(
                self,
                &absolute,
                &target.source,
                &target.relative_folder,
                &file_name,
                tag,
                last_played_at,
            ) {
                Ok(path) => {
                    self.set_status(
                        format!("Copied to {}", path.display()),
                        StatusTone::Info,
                    );
                }
                Err(err) => self.set_status(err, StatusTone::Error),
            }
            return;
        }

        let destination_relative = match move_destination_relative(
            &target.source,
            &target.relative_folder,
            &file_name,
        ) {
            Ok(path) => path,
            Err(err) => {
                self.set_status(err, StatusTone::Error);
                return;
            }
        };
        let destination_absolute = target.source.root.join(&destination_relative);
        if let Err(err) = super::source_moves::move_sample_file(&absolute, &destination_absolute) {
            self.set_status(err, StatusTone::Error);
            return;
        }
        let (file_size, modified_ns) = match file_metadata(&destination_absolute) {
            Ok(meta) => meta,
            Err(err) => {
                let _ = super::source_moves::move_sample_file(&destination_absolute, &absolute);
                self.set_status(err, StatusTone::Error);
                return;
            }
        };
        if let Err(err) = self.register_moved_sample_for_source(
            &target.source,
            &destination_relative,
            file_size,
            modified_ns,
            tag,
            last_played_at,
        ) {
            let _ = super::source_moves::move_sample_file(&destination_absolute, &absolute);
            self.set_status(err, StatusTone::Error);
            return;
        }
        if let Err(err) = self.remove_source_db_entry(&source, &relative_path) {
            let _ = super::source_moves::move_sample_file(&destination_absolute, &absolute);
            self.set_status(err, StatusTone::Error);
            return;
        }
        self.prune_cached_sample(&source, &relative_path);
        let new_entry = WavEntry {
            relative_path: destination_relative.clone(),
            file_size,
            modified_ns,
            content_hash: None,
            tag,
            missing: false,
            last_played_at,
        };
        self.insert_cached_entry(&target.source, new_entry);
        if self.update_collections_for_source_move(
            &source.id,
            &target.source.id,
            &relative_path,
            &destination_relative,
        ) {
            let _ = self.persist_config("Failed to save collections after move");
        }
        self.set_status(
            format!("Moved to {}", target_dir.display()),
            StatusTone::Info,
        );
    }

    pub(crate) fn handle_samples_drop_to_drop_target(
        &mut self,
        samples: &[DragSample],
        target_path: PathBuf,
        copy_requested: bool,
    ) {
        for sample in samples {
            self.handle_sample_drop_to_drop_target(
                sample.source_id.clone(),
                sample.relative_path.clone(),
                target_path.clone(),
                copy_requested,
            );
        }
    }
}

fn move_destination_relative(
    target: &crate::sample_sources::SampleSource,
    target_folder: &Path,
    file_name: &std::ffi::OsStr,
) -> Result<PathBuf, String> {
    let relative = if target_folder.as_os_str().is_empty() {
        PathBuf::from(file_name)
    } else {
        target_folder.join(file_name)
    };
    let destination = target.root.join(&relative);
    if destination.exists() {
        return Err(format!(
            "A file already exists at {}",
            destination.display()
        ));
    }
    Ok(relative)
}

fn copy_sample_to_target(
    controller: &mut DragDropController<'_>,
    absolute: &Path,
    target: &crate::sample_sources::SampleSource,
    target_folder: &Path,
    file_name: &std::ffi::OsStr,
    tag: Rating,
    last_played_at: Option<i64>,
) -> Result<PathBuf, String> {
    let destination_relative =
        copy_destination_relative(target, target_folder, file_name)?;
    let destination_absolute = target.root.join(&destination_relative);
    if let Some(parent) = destination_relative.parent() {
        if !parent.as_os_str().is_empty() {
            let target_dir = target.root.join(parent);
            if let Err(err) = std::fs::create_dir_all(&target_dir) {
                return Err(format!(
                    "Failed to create target folder {}: {err}",
                    target_dir.display()
                ));
            }
        }
    }
    if let Err(err) = std::fs::copy(absolute, &destination_absolute) {
        return Err(format!("Failed to copy sample: {err}"));
    }
    let (file_size, modified_ns) = match file_metadata(&destination_absolute) {
        Ok(meta) => meta,
        Err(err) => {
            let _ = std::fs::remove_file(&destination_absolute);
            return Err(err);
        }
    };
    if let Err(err) = controller.register_moved_sample_for_source(
        target,
        &destination_relative,
        file_size,
        modified_ns,
        tag,
        last_played_at,
    ) {
        let _ = std::fs::remove_file(&destination_absolute);
        return Err(err);
    }
    let new_entry = WavEntry {
        relative_path: destination_relative.clone(),
        file_size,
        modified_ns,
        content_hash: None,
        tag,
        missing: false,
        last_played_at,
    };
    controller.insert_cached_entry(target, new_entry);
    Ok(destination_relative)
}

fn copy_destination_relative(
    target: &crate::sample_sources::SampleSource,
    target_folder: &Path,
    file_name: &std::ffi::OsStr,
) -> Result<PathBuf, String> {
    let base = if target_folder.as_os_str().is_empty() {
        PathBuf::from(file_name)
    } else {
        target_folder.join(file_name)
    };
    if !target.root.join(&base).exists() {
        return Ok(base);
    }
    let stem = Path::new(file_name)
        .file_stem()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| "sample".to_string());
    let extension = Path::new(file_name)
        .extension()
        .map(|ext| ext.to_string_lossy().to_string());
    for index in 1..=999 {
        let suffix = format!("{stem}_copy{index:03}");
        let file_name = if let Some(ext) = &extension {
            format!("{suffix}.{ext}")
        } else {
            suffix
        };
        let candidate = if target_folder.as_os_str().is_empty() {
            PathBuf::from(&file_name)
        } else {
            target_folder.join(&file_name)
        };
        if !target.root.join(&candidate).exists() {
            return Ok(candidate);
        }
    }
    Err("Failed to find destination file name".into())
}
