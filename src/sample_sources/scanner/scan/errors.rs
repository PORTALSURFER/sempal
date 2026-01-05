use std::path::PathBuf;

use thiserror::Error;

use crate::sample_sources::SourceDbError;

/// Errors that can occur while scanning a source folder.
#[derive(Debug, Error)]
pub enum ScanError {
    #[error("Source root is not a directory: {0}")]
    InvalidRoot(PathBuf),
    #[error("Scan canceled")]
    Canceled,
    #[error("Failed to read {path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("Database error: {0}")]
    Db(#[from] SourceDbError),
    #[error("Time conversion failed for {path}")]
    Time { path: PathBuf },
}
