use std::path::PathBuf;

use thiserror::Error;

use crate::sample_sources::SourceDbError;

/// Errors that can occur while scanning a source folder.
#[derive(Debug, Error)]
pub enum ScanError {
    /// The provided root path is not a directory.
    #[error("Source root is not a directory: {0}")]
    InvalidRoot(PathBuf),
    /// Scan was canceled by the caller.
    #[error("Scan canceled")]
    Canceled,
    /// Failed to read a file or directory.
    #[error("Failed to read {path}: {source}")]
    Io {
        /// Path that failed to read.
        path: PathBuf,
        /// Underlying IO error.
        source: std::io::Error,
    },
    /// Database operation failed during scan.
    #[error("Database error: {0}")]
    Db(#[from] SourceDbError),
    /// Failed to convert filesystem time metadata.
    #[error("Time conversion failed for {path}")]
    Time {
        /// Path whose timestamp could not be converted.
        path: PathBuf,
    },
}
