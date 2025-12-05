use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub mod config;
pub mod db;
pub mod scanner;

pub use db::{DB_FILE_NAME, SourceDatabase, SourceDbError, WavEntry};
pub use scanner::{ScanError, ScanStats, scan_in_background};

/// Identifier for a configured sample source.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SourceId(String);

impl SourceId {
    /// Create a new unique source identifier.
    pub fn new() -> Self {
        Self(Uuid::new_v4().to_string())
    }

    /// Borrow the identifier as a string.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for SourceId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// User-selected folder that owns its own SQLite database of wav files.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SampleSource {
    pub id: SourceId,
    pub root: PathBuf,
}

impl SampleSource {
    /// Create a new sample source for the given directory.
    pub fn new(root: PathBuf) -> Self {
        Self {
            id: SourceId::new(),
            root,
        }
    }

    /// Location of the SQLite database for this source.
    pub fn db_path(&self) -> PathBuf {
        database_path_for(&self.root)
    }

    /// Open the SQLite database for this source, creating it if necessary.
    pub fn open_db(&self) -> Result<SourceDatabase, SourceDbError> {
        SourceDatabase::open(&self.root)
    }
}

/// Name the per-source database using a hidden file inside the chosen folder.
pub fn database_path_for(root: &Path) -> PathBuf {
    root.join(DB_FILE_NAME)
}
