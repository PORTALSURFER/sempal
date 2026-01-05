use super::super::*;
use crate::sample_sources::SourceDatabase;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

pub(super) struct CollectionMoveRequest {
    pub(super) source_id: SourceId,
    pub(super) source_root: PathBuf,
    pub(super) relative_path: PathBuf,
}

pub(super) fn unique_destination_name(root: &Path, path: &Path) -> Result<PathBuf, String> {
    let file_name = path
        .file_name()
        .ok_or_else(|| "Sample has no file name".to_string())?;
    let candidate = PathBuf::from(file_name);
    if !root.join(&candidate).exists() {
        return Ok(candidate);
    }
    let stem = path
        .file_stem()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| "sample".to_string());
    let extension = path
        .extension()
        .map(|ext| ext.to_string_lossy().to_string());
    for index in 1..=999 {
        let suffix = format!("{stem}_move{index:03}");
        let file_name = if let Some(ext) = &extension {
            format!("{suffix}.{ext}")
        } else {
            suffix
        };
        let candidate = PathBuf::from(file_name);
        if !root.join(&candidate).exists() {
            return Ok(candidate);
        }
    }
    Err("Failed to find destination file name".into())
}

pub(super) fn move_sample_file(source: &Path, destination: &Path) -> Result<(), String> {
    match fs::rename(source, destination) {
        Ok(()) => Ok(()),
        Err(rename_err) => {
            if let Err(copy_err) = fs::copy(source, destination) {
                return Err(format!(
                    "Failed to move file: {rename_err}; copy failed: {copy_err}"
                ));
            }
            if let Err(remove_err) = fs::remove_file(source) {
                let _ = fs::remove_file(destination);
                return Err(format!("Failed to remove original file: {remove_err}"));
            }
            Ok(())
        }
    }
}

pub(super) fn run_collection_move_task(
    collection_id: CollectionId,
    clip_root: PathBuf,
    requests: Vec<CollectionMoveRequest>,
) -> crate::egui_app::controller::jobs::CollectionMoveResult {
    let mut moved = Vec::new();
    let mut errors = Vec::new();
    let clip_db = match SourceDatabase::open(&clip_root) {
        Ok(db) => db,
        Err(err) => {
            errors.push(format!("Failed to open collection database: {err}"));
            return crate::egui_app::controller::jobs::CollectionMoveResult {
                collection_id,
                moved,
                errors,
            };
        }
    };
    let mut source_dbs: std::collections::HashMap<PathBuf, SourceDatabase> =
        std::collections::HashMap::new();
    for request in requests {
        let absolute = request.source_root.join(&request.relative_path);
        if !absolute.is_file() {
            errors.push(format!(
                "File missing: {}",
                request.relative_path.display()
            ));
            continue;
        }
        let clip_relative = match unique_destination_name(&clip_root, &request.relative_path) {
            Ok(path) => path,
            Err(err) => {
                errors.push(err);
                continue;
            }
        };
        let clip_absolute = clip_root.join(&clip_relative);
        if let Err(err) = move_sample_file(&absolute, &clip_absolute) {
            errors.push(err);
            continue;
        }
        let clip_metadata = match fs::metadata(&clip_absolute) {
            Ok(metadata) => metadata,
            Err(err) => {
                let _ = move_sample_file(&clip_absolute, &absolute);
                errors.push(format!("Missing clip metadata: {err}"));
                continue;
            }
        };
        let modified_ns = match clip_metadata.modified() {
            Ok(modified) => match modified.duration_since(SystemTime::UNIX_EPOCH) {
                Ok(duration) => duration.as_nanos() as i64,
                Err(_) => {
                    let _ = move_sample_file(&clip_absolute, &absolute);
                    errors.push("File modified time is before epoch".to_string());
                    continue;
                }
            },
            Err(err) => {
                let _ = move_sample_file(&clip_absolute, &absolute);
                errors.push(format!("Missing mtime for collection: {err}"));
                continue;
            }
        };
        if let Err(err) = clip_db.upsert_file(
            &clip_relative,
            clip_metadata.len(),
            modified_ns,
        ) {
            let _ = move_sample_file(&clip_absolute, &absolute);
            errors.push(format!("Failed to sync collection entry: {err}"));
            continue;
        }
        let db = match source_dbs.entry(request.source_root.clone()) {
            std::collections::hash_map::Entry::Occupied(entry) => entry.into_mut(),
            std::collections::hash_map::Entry::Vacant(entry) => match SourceDatabase::open(&request.source_root) {
                Ok(db) => entry.insert(db),
                Err(err) => {
                    let _ = clip_db.remove_file(&clip_relative);
                    let _ = move_sample_file(&clip_absolute, &absolute);
                    errors.push(format!("Failed to open source database: {err}"));
                    continue;
                }
            },
        };
        if let Err(err) = db.remove_file(&request.relative_path) {
            let _ = clip_db.remove_file(&clip_relative);
            let _ = move_sample_file(&clip_absolute, &absolute);
            errors.push(format!("Failed to drop source database row: {err}"));
            continue;
        }
        moved.push(crate::egui_app::controller::jobs::CollectionMoveSuccess {
            source_id: request.source_id,
            relative_path: request.relative_path,
            clip_root: clip_root.clone(),
            clip_relative,
        });
    }
    crate::egui_app::controller::jobs::CollectionMoveResult {
        collection_id,
        moved,
        errors,
    }
}
