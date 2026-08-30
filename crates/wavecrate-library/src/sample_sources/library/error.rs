use std::path::PathBuf;

use thiserror::Error;

use crate::app_dirs;

/// Errors returned when operating on the library database.
#[derive(Debug, Error)]
pub enum LibraryError {
    /// No suitable application directory was available.
    #[error("No suitable config directory available for library database")]
    NoConfigDir,
    /// Failed to create the directory for the database file.
    #[error("Could not create library directory {path}: {source}")]
    CreateDir {
        /// Path that could not be created.
        path: PathBuf,
        /// Underlying IO error.
        source: std::io::Error,
    },
    /// Failed to resolve the configured persistence profile.
    #[error("Invalid library persistence profile '{profile}'")]
    InvalidProfile {
        /// Rejected profile name.
        profile: String,
    },
    /// The profile ownership capability could not be used for a library operation.
    #[error(transparent)]
    ProfileOwnership(#[from] app_dirs::ProfileOwnershipError),
    /// Profile ownership changed while an accepted library command was in flight.
    #[error("profile ownership changed at {path}: {reason}")]
    ProfileOwnershipChanged {
        /// Profile root whose ownership boundary changed.
        path: PathBuf,
        /// Stable reason for the fail-closed result.
        reason: String,
    },
    /// Failed to open or query the database.
    #[error("Library database query failed: {0}")]
    Sql(#[from] rusqlite::Error),
    /// Failed to deserialize JSON metadata from the DB.
    #[error("Library metadata parse failed: {0}")]
    Json(#[from] serde_json::Error),
    /// Failed to deserialize one named JSON metadata value from the DB.
    #[error("Library metadata key '{key}' parse failed: {source}")]
    MetadataJson {
        /// Metadata key that contained invalid JSON.
        key: &'static str,
        /// Underlying JSON error.
        source: serde_json::Error,
    },
    /// A retained source descriptor contained a present but unsupported enum value.
    #[error("Retained source '{root}' has invalid {field} value '{value}'")]
    InvalidRetainedSourceDescriptor {
        /// Retained source root used to identify the corrupt record.
        root: PathBuf,
        /// Descriptor field containing the unsupported value.
        field: &'static str,
        /// Unsupported stored value.
        value: String,
    },
}

pub(super) fn map_sql_error(err: rusqlite::Error) -> LibraryError {
    LibraryError::Sql(err)
}

pub(super) fn map_app_dir_error(error: app_dirs::AppDirError) -> LibraryError {
    match error {
        app_dirs::AppDirError::NoBaseDir => LibraryError::NoConfigDir,
        app_dirs::AppDirError::CreateDir { path, source } => {
            LibraryError::CreateDir { path, source }
        }
        app_dirs::AppDirError::InvalidProfileName { profile } => {
            LibraryError::InvalidProfile { profile }
        }
    }
}
