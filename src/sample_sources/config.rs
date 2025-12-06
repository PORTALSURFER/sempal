use std::path::{Path, PathBuf};

use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use super::{Collection, SampleSource};

/// Default filename used to store the app configuration.
pub const CONFIG_FILE_NAME: &str = "config.json";

/// Top-level app configuration persisted on disk.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AppConfig {
    pub sources: Vec<SampleSource>,
    #[serde(default)]
    pub collections: Vec<Collection>,
    #[serde(default)]
    pub feature_flags: FeatureFlags,
}

/// Toggleable features that can be persisted and evolve without breaking old configs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeatureFlags {
    #[serde(default = "default_true")]
    pub collections_enabled: bool,
}

impl Default for FeatureFlags {
    fn default() -> Self {
        Self {
            collections_enabled: true,
        }
    }
}

/// Errors that may occur while loading or saving app configuration.
#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("Unable to create config directory {path}: {source}")]
    CreateDir {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("Failed to read {path}: {source}")]
    Read {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("Failed to write {path}: {source}")]
    Write {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("Invalid config at {path}: {source}")]
    Parse {
        path: PathBuf,
        source: serde_json::Error,
    },
    #[error("No suitable config directory found")]
    NoConfigDir,
}

/// Resolve the configuration file path, ensuring the parent directory exists.
pub fn config_path() -> Result<PathBuf, ConfigError> {
    let dirs = ProjectDirs::from("com", "sempal", "sempal").ok_or(ConfigError::NoConfigDir)?;
    let dir = dirs.config_dir();
    std::fs::create_dir_all(dir).map_err(|source| ConfigError::CreateDir {
        path: dir.to_path_buf(),
        source,
    })?;
    Ok(dir.join(CONFIG_FILE_NAME))
}

/// Load configuration from disk, returning an empty default if missing.
pub fn load_or_default() -> Result<AppConfig, ConfigError> {
    let path = config_path()?;
    load_from(&path)
}

/// Persist configuration to disk, overwriting any previous contents.
pub fn save(config: &AppConfig) -> Result<(), ConfigError> {
    let path = config_path()?;
    save_to_path(config, &path)
}

/// Load configuration from a specific path, returning an empty default if missing.
pub fn load_from(path: &Path) -> Result<AppConfig, ConfigError> {
    if !path.exists() {
        return Ok(AppConfig::default());
    }
    let bytes = std::fs::read(path).map_err(|source| ConfigError::Read {
        path: path.to_path_buf(),
        source,
    })?;
    serde_json::from_slice(&bytes).map_err(|source| ConfigError::Parse {
        path: path.to_path_buf(),
        source,
    })
}

/// Save configuration to a specific path, creating parent directories as needed.
pub fn save_to_path(config: &AppConfig, path: &Path) -> Result<(), ConfigError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|source| ConfigError::CreateDir {
            path: parent.to_path_buf(),
            source,
        })?;
    }
    let data = serde_json::to_vec_pretty(config).map_err(|source| ConfigError::Parse {
        path: path.to_path_buf(),
        source,
    })?;
    std::fs::write(path, data).map_err(|source| ConfigError::Write {
        path: path.to_path_buf(),
        source,
    })
}

/// Utility to convert absolute paths to strings for serialization durability.
pub fn normalize_path(path: &Path) -> PathBuf {
    PathBuf::from_iter(path.components())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn save_and_load_from_custom_path() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("cfg.json");
        let cfg = AppConfig {
            sources: vec![SampleSource::new(dir.path().to_path_buf())],
            ..Default::default()
        };
        save_to_path(&cfg, &path).unwrap();
        let loaded = load_from(&path).unwrap();
        assert_eq!(loaded.sources.len(), 1);
        assert_eq!(loaded.sources[0].root, dir.path());
    }
}

fn default_true() -> bool {
    true
}
