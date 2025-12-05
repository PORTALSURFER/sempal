use std::path::{Path, PathBuf};

use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use super::SampleSource;

/// Default filename used to store the app configuration.
pub const CONFIG_FILE_NAME: &str = "config.json";

/// Top-level app configuration persisted on disk.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AppConfig {
    pub sources: Vec<SampleSource>,
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
    if !path.exists() {
        return Ok(AppConfig::default());
    }
    let bytes = std::fs::read(&path).map_err(|source| ConfigError::Read {
        path: path.clone(),
        source,
    })?;
    serde_json::from_slice(&bytes).map_err(|source| ConfigError::Parse {
        path: path.clone(),
        source,
    })
}

/// Persist configuration to disk, overwriting any previous contents.
pub fn save(config: &AppConfig) -> Result<(), ConfigError> {
    let path = config_path()?;
    let data = serde_json::to_vec_pretty(config).map_err(|source| ConfigError::Parse {
        path: path.clone(),
        source,
    })?;
    std::fs::write(&path, data).map_err(|source| ConfigError::Write {
        path: path.clone(),
        source,
    })
}

/// Utility to convert absolute paths to strings for serialization durability.
pub fn normalize_path(path: &Path) -> PathBuf {
    PathBuf::from_iter(path.components())
}
