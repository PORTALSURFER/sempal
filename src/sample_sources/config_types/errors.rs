use std::path::PathBuf;

use thiserror::Error;

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
    ParseToml {
        path: PathBuf,
        source: toml::de::Error,
    },
    #[error("Invalid legacy config at {path}: {source}")]
    ParseJson {
        path: PathBuf,
        source: serde_json::Error,
    },
    #[error("Failed to serialize config to TOML at {path}: {source}")]
    SerializeToml {
        path: PathBuf,
        source: toml::ser::Error,
    },
    #[error("Failed to migrate legacy config from {path}: {source}")]
    LegacyMigration {
        path: PathBuf,
        source: Box<ConfigError>,
    },
    #[error("Failed to back up legacy config {path} to {backup_path}: {source}")]
    BackupLegacy {
        path: PathBuf,
        backup_path: PathBuf,
        source: std::io::Error,
    },
    #[error("No suitable config directory found")]
    NoConfigDir,
    #[error("Library database error: {0}")]
    Library(#[from] crate::sample_sources::library::LibraryError),
}
