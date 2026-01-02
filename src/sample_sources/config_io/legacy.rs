use std::path::{Path, PathBuf};

use crate::app_dirs;

use super::super::config_types::{AppConfig, AppSettings, ConfigError};
use super::LEGACY_CONFIG_FILE_NAME;
use super::map_app_dir_error;
use super::save::save_settings_to_path;

pub(super) fn legacy_config_path() -> Result<PathBuf, ConfigError> {
    let dir = app_dirs::app_root_dir().map_err(map_app_dir_error)?;
    Ok(dir.join(LEGACY_CONFIG_FILE_NAME))
}

pub(super) fn migrate_legacy_config(
    legacy_path: &Path,
    new_path: &Path,
) -> Result<AppSettings, ConfigError> {
    if !legacy_path.exists() {
        return Ok(AppSettings::default());
    }
    let legacy = load_legacy_from(legacy_path).map_err(|source| ConfigError::LegacyMigration {
        path: legacy_path.to_path_buf(),
        source: Box::new(source),
    })?;
    crate::sample_sources::library::save(&crate::sample_sources::library::LibraryState {
        sources: legacy.sources.clone(),
        collections: legacy.collections.clone(),
    })?;
    let settings = AppSettings::from(&legacy).normalized();
    save_settings_to_path(&settings, new_path)?;
    backup_legacy_file(legacy_path)?;
    Ok(settings)
}

fn backup_legacy_file(path: &Path) -> Result<(), ConfigError> {
    let backup_path = path.with_extension("json.bak");
    if backup_path.exists() {
        std::fs::remove_file(&backup_path).map_err(|source| ConfigError::BackupLegacy {
            path: path.to_path_buf(),
            backup_path: backup_path.clone(),
            source,
        })?;
    }
    std::fs::rename(path, &backup_path).map_err(|source| ConfigError::BackupLegacy {
        path: path.to_path_buf(),
        backup_path,
        source,
    })
}

fn load_legacy_from(path: &Path) -> Result<AppConfig, ConfigError> {
    let bytes = std::fs::read(path).map_err(|source| ConfigError::Read {
        path: path.to_path_buf(),
        source,
    })?;
    serde_json::from_slice(&bytes).map_err(|source| ConfigError::ParseJson {
        path: path.to_path_buf(),
        source,
    })
}
