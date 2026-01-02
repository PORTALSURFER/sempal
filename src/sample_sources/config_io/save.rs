use std::path::Path;

use super::super::config_types::{AppConfig, AppSettings, ConfigError};
use super::load::config_path;

/// Persist configuration to disk, overwriting any previous contents.
///
/// Settings are written to TOML while sources/collections are stored in SQLite.
pub fn save(config: &AppConfig) -> Result<(), ConfigError> {
    let path = config_path()?;
    save_to_path(config, &path)
}

/// Save configuration to a specific path, creating parent directories as needed.
pub fn save_to_path(config: &AppConfig, path: &Path) -> Result<(), ConfigError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|source| ConfigError::CreateDir {
            path: parent.to_path_buf(),
            source,
        })?;
    }
    let settings = AppSettings::from(config);
    save_settings_to_path(&settings, path)?;
    crate::sample_sources::library::save(&crate::sample_sources::library::LibraryState {
        sources: config.sources.clone(),
        collections: config.collections.clone(),
    })?;
    Ok(())
}

pub(super) fn save_settings_to_path(
    settings: &AppSettings,
    path: &Path,
) -> Result<(), ConfigError> {
    let data = toml::to_string_pretty(settings).map_err(|source| ConfigError::SerializeToml {
        path: path.to_path_buf(),
        source,
    })?;
    std::fs::write(path, data).map_err(|source| ConfigError::Write {
        path: path.to_path_buf(),
        source,
    })
}
