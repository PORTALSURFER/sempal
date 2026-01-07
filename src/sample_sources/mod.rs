use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

mod audio_support;
pub mod collections;
pub mod config;
pub mod db;
pub mod library;
pub mod scan_state;
pub mod scanner;

pub(crate) use audio_support::{is_supported_audio, supported_audio_where_clause};
pub use collections::{Collection, CollectionId};
pub use db::{DB_FILE_NAME, SampleTag, SourceDatabase, SourceDbError, WavEntry};
pub use scan_state::ScanTracker;
pub use scanner::{ScanError, ScanMode, ScanStats};

/// Identifier for a configured sample source.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SourceId(String);

impl SourceId {
    /// Create a new unique source identifier.
    pub fn new() -> Self {
        Self(Uuid::new_v4().to_string())
    }

    /// Rehydrate a source identifier from a stored string.
    pub fn from_string(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    /// Borrow the identifier as a string.
    #[allow(dead_code)]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Default for SourceId {
    fn default() -> Self {
        Self::new()
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

    /// Create a sample source with an existing id (used when re-attaching a known root).
    pub fn new_with_id(id: SourceId, root: PathBuf) -> Self {
        Self { id, root }
    }

    /// Location of the SQLite database for this source.
    #[allow(dead_code)]
    pub fn db_path(&self) -> PathBuf {
        database_path_for(&self.root)
    }

    /// Open the SQLite database for this source, creating it if necessary.
    #[allow(dead_code)]
    pub fn open_db(&self) -> Result<SourceDatabase, SourceDbError> {
        SourceDatabase::open(&self.root)
    }
}

/// Name the per-source database using a hidden file inside the chosen folder.
#[allow(dead_code)]
pub fn database_path_for(root: &Path) -> PathBuf {
    root.join(DB_FILE_NAME)
}
