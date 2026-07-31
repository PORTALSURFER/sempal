//! Derived application subdirectories below the resolved `.wavecrate` root.

use std::path::PathBuf;

use super::{AppDirError, app_root_dir};

pub(super) fn create_app_subdir(name: &str) -> Result<PathBuf, AppDirError> {
    let path = app_root_dir()?.join(name);
    std::fs::create_dir_all(&path).map_err(|source| AppDirError::CreateDir {
        path: path.clone(),
        source,
    })?;
    Ok(path)
}

/// Return the logs directory inside the `.wavecrate` root, creating it if needed.
pub fn logs_dir() -> Result<PathBuf, AppDirError> {
    create_app_subdir("logs")
}

/// Return the global handoff staging directory inside the `.wavecrate` root.
pub fn handoff_staging_dir() -> Result<PathBuf, AppDirError> {
    create_app_subdir("handoff_staging")
}

/// Return the profile-local durable operation-journal directory.
pub fn operation_journal_dir() -> Result<PathBuf, AppDirError> {
    create_app_subdir("operation_journal")
}

/// Return the root directory for rebuildable cache payloads.
pub fn rebuildable_cache_root_dir() -> Result<PathBuf, AppDirError> {
    create_app_subdir("cache")
}

/// Return the persistent waveform cache payload directory.
pub fn waveform_cache_dir() -> Result<PathBuf, AppDirError> {
    let path = rebuildable_cache_root_dir()?.join("waveforms");
    std::fs::create_dir_all(&path).map_err(|source| AppDirError::CreateDir {
        path: path.clone(),
        source,
    })?;
    Ok(path)
}

/// Clear all rebuildable cache payloads without touching logs, handoff staging,
/// source databases, or durable user metadata.
pub fn clear_rebuildable_cache_payloads() -> Result<PathBuf, String> {
    let path = app_root_dir().map_err(|err| err.to_string())?.join("cache");
    if path.exists() {
        if !path.is_dir() {
            return Err(format!(
                "Rebuildable cache path is not a directory: {}",
                path.display()
            ));
        }
        std::fs::remove_dir_all(&path).map_err(|err| {
            format!(
                "Failed to clear rebuildable caches at {}: {err}",
                path.display()
            )
        })?;
    }
    std::fs::create_dir_all(&path).map_err(|err| {
        format!(
            "Failed to recreate rebuildable cache directory at {}: {err}",
            path.display()
        )
    })?;
    Ok(path)
}
