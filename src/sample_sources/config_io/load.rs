use std::path::{Path, PathBuf};

use serde::de::Error as SerdeDeError;

use crate::app_dirs;

use super::super::config_types::{AppConfig, AppSettings, ConfigError};
use super::CONFIG_FILE_NAME;
use super::legacy::{legacy_config_path, migrate_legacy_config};
use super::map_app_dir_error;
use super::save::save_settings_to_path;

/// Resolve the configuration file path, ensuring the parent directory exists.
pub fn config_path() -> Result<PathBuf, ConfigError> {
    let dir = app_dirs::app_root_dir().map_err(map_app_dir_error)?;
    Ok(dir.join(CONFIG_FILE_NAME))
}

/// Load configuration from disk, returning defaults if missing.
///
/// This pulls settings from a TOML file and data from the SQLite library database.
/// If a legacy `config.json` exists, it will be migrated into the new layout.
pub fn load_or_default() -> Result<AppConfig, ConfigError> {
    let settings_path = config_path()?;
    let legacy_path = legacy_config_path()?;
    let mut settings = if settings_path.exists() {
        load_settings_from(&settings_path)?
    } else {
        migrate_legacy_config(&legacy_path, &settings_path)?
    };
    apply_app_data_dir(&settings_path, &mut settings)?;

    let library = crate::sample_sources::library::load()?;
    Ok(AppConfig::from((settings, library)))
}

/// Utility to convert absolute paths to strings for serialization durability.
pub fn normalize_path(path: &Path) -> PathBuf {
    PathBuf::from_iter(path.components())
}

pub(super) fn load_settings_from(path: &Path) -> Result<AppSettings, ConfigError> {
    if !path.exists() {
        return Ok(AppSettings::default());
    }
    let bytes = std::fs::read(path).map_err(|source| ConfigError::Read {
        path: path.to_path_buf(),
        source,
    })?;
    let text = String::from_utf8(bytes).map_err(|source| ConfigError::ParseToml {
        path: path.to_path_buf(),
        source: SerdeDeError::custom(source),
    })?;
    let mut value: toml::Value = toml::from_str(&text).map_err(|source| ConfigError::ParseToml {
        path: path.to_path_buf(),
        source,
    })?;
    if let Some(root) = value.as_table_mut() {
        if let Some(core) = root
            .get("core")
            .and_then(|core| core.as_table())
            .cloned()
        {
            for (key, value) in core {
                root.entry(key).or_insert(value);
            }
            root.remove("core");
        }
    }
    value
        .try_into()
        .map_err(|source| ConfigError::ParseToml {
            path: path.to_path_buf(),
            source,
        })
        .map(AppSettings::normalized)
}

pub(super) fn apply_app_data_dir(
    settings_path: &Path,
    settings: &mut AppSettings,
) -> Result<(), ConfigError> {
    let Some(app_data_dir) = settings.core.app_data_dir.clone() else {
        return Ok(());
    };
    let override_path = app_data_dir.join(CONFIG_FILE_NAME);
    if override_path != settings_path && override_path.exists() {
        *settings = load_settings_from(&override_path)?;
    } else if override_path != settings_path && settings_path.exists() {
        save_settings_to_path(settings, &override_path)?;
    }
    settings.core.app_data_dir = Some(app_data_dir.clone());
    app_dirs::set_app_root_override(app_data_dir).map_err(map_app_dir_error)?;
    Ok(())
}
