//! Background job helpers for undo/redo file operations.

use crate::egui_app::controller::jobs::{FileOpMessage, UndoFileJob, UndoFileOpResult, UndoFileOutcome};
use crate::egui_app::controller::library::collection_items_helpers::file_metadata;
use crate::sample_sources::SourceDatabase;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc::Sender,
};

/// Run an undo/redo filesystem job with optional progress updates.
pub(crate) fn run_undo_file_job(
    job: UndoFileJob,
    cancel: Arc<AtomicBool>,
    sender: Option<&Sender<FileOpMessage>>,
) -> UndoFileOpResult {
    if cancel.load(Ordering::Relaxed) {
        return UndoFileOpResult {
            result: Err("Undo cancelled".to_string()),
            cancelled: true,
        };
    }
    let result = match job {
        UndoFileJob::Overwrite {
            source_id,
            source_root,
            relative_path,
            absolute_path,
            backup_path,
        } => {
            if let Some(parent) = absolute_path.parent() {
                if let Err(err) = std::fs::create_dir_all(parent) {
                    return UndoFileOpResult {
                        result: Err(format!(
                            "Failed to create folder {}: {err}",
                            parent.display()
                        )),
                        cancelled: false,
                    };
                }
            }
            std::fs::copy(&backup_path, &absolute_path)
                .map_err(|err| format!("Failed to restore audio: {err}"))
                .and_then(|_| {
                    let (file_size, modified_ns) = file_metadata(&absolute_path)?;
                    let db = SourceDatabase::open(&source_root)
                        .map_err(|err| format!("Database unavailable: {err}"))?;
                    let tag = db
                        .tag_for_path(&relative_path)
                        .map_err(|err| format!("Failed to read database: {err}"))?
                        .ok_or_else(|| "Sample not found in database".to_string())?;
                    let looped = db
                        .looped_for_path(&relative_path)
                        .map_err(|err| format!("Failed to read database: {err}"))?
                        .ok_or_else(|| "Sample not found in database".to_string())?;
                    let last_played_at = db
                        .last_played_at_for_path(&relative_path)
                        .map_err(|err| format!("Failed to read database: {err}"))?;
                    Ok(UndoFileOutcome::Overwrite {
                        source_id,
                        relative_path,
                        file_size,
                        modified_ns,
                        tag,
                        looped,
                        last_played_at,
                    })
                })
        }
        UndoFileJob::RemoveSample {
            source_id,
            source_root,
            relative_path,
            absolute_path,
        } => {
            let db = SourceDatabase::open(&source_root)
                .map_err(|err| format!("Database unavailable: {err}"))?;
            let _ = std::fs::remove_file(&absolute_path);
            let _ = db.remove_file(&relative_path);
            Ok(UndoFileOutcome::Removed {
                source_id,
                relative_path,
            })
        }
        UndoFileJob::RestoreSample {
            source_id,
            source_root,
            relative_path,
            absolute_path,
            backup_path,
            tag,
        } => {
            if let Some(parent) = absolute_path.parent() {
                if let Err(err) = std::fs::create_dir_all(parent) {
                    return UndoFileOpResult {
                        result: Err(format!(
                            "Failed to create folder {}: {err}",
                            parent.display()
                        )),
                        cancelled: false,
                    };
                }
            }
            std::fs::copy(&backup_path, &absolute_path)
                .map_err(|err| format!("Failed to restore audio: {err}"))
                .and_then(|_| {
                    let (file_size, modified_ns) = file_metadata(&absolute_path)?;
                    let db = SourceDatabase::open(&source_root)
                        .map_err(|err| format!("Database unavailable: {err}"))?;
                    db.upsert_file(&relative_path, file_size, modified_ns)
                        .map_err(|err| format!("Failed to sync database entry: {err}"))?;
                    db.set_tag(&relative_path, tag)
                        .map_err(|err| format!("Failed to sync tag: {err}"))?;
                    Ok(UndoFileOutcome::Restored {
                        source_id,
                        relative_path,
                        file_size,
                        modified_ns,
                        tag,
                        looped: false,
                        last_played_at: None,
                    })
                })
        }
    };
    if let Some(tx) = sender {
        let _ = tx.send(FileOpMessage::Progress {
            completed: 1,
            detail: None,
        });
    }
    UndoFileOpResult {
        result,
        cancelled: false,
    }
}
