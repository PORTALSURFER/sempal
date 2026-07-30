use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

mod audio_support;
/// Per-source database helpers.
pub mod db;
/// Global library database helpers.
pub mod library;
/// Durable source-readiness and convergence coordination contracts.
pub mod readiness;
mod source_entry;

pub use audio_support::{
    is_apple_double_sidecar, is_recognized_audio, is_supported_audio, supported_audio_where_clause,
};
pub use db::normalize_relative_path;
pub use db::{
    BrowserFileMetadata, BrowserMetadataCursor, BrowserMetadataPage, BrowserMetadataSnapshot,
    DB_FILE_NAME, ExistingFileMetadataUpdate, ManifestCommitResult, Rating, SampleCollection,
    SourceCollectionWrite, SourceContentHashWrite, SourceDatabase, SourceDatabaseConnectionRole,
    SourceDbError, SourceFileWrite, SourceIndexClassification, SourceIndexDiagnostic,
    SourceIndexEntry, SourceIndexSnapshot, SourceManifestEntry, SourceTag, SourceTagUsage,
    SourceTagWrite, SourceWriteCommand, WavEntry,
};
pub use library::{
    HarvestDerivationOperation, HarvestDerivationRecord, HarvestFileIdentity, HarvestFileKey,
    HarvestFileRecord, HarvestMetadataSnapshot, HarvestSourceRange, HarvestState,
    LIBRARY_DB_FILE_NAME, LibraryError, LibraryState, NewHarvestDerivation,
};
pub use source_entry::{
    HiddenDirectoryPolicy, SOURCE_FORMAT_POLICY_VERSION, SourceEntryClassification,
    SourceEntryFileType, SourceEntryKind, SourceEntryProbeError, SourceEntryRejection,
    SourceFileClassification, SourceTraversalPolicy, classify_path_without_following,
    classify_path_without_following_with_policy, classify_source_entry,
    classify_source_entry_with_policy, is_rejected_source_file_path,
};

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

/// Role assigned to a configured source.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum SourceRole {
    /// Current writable source behavior.
    #[default]
    Normal,
    /// Read/play source where Wavecrate should not mutate audio files.
    Protected,
    /// Writable default destination for protected-source edits/imports.
    Primary,
}

impl SourceRole {
    /// Stable database/config representation.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Normal => "normal",
            Self::Protected => "protected",
            Self::Primary => "primary",
        }
    }

    /// Parse a stored source role, defaulting unknown values to normal.
    pub fn from_stored(value: &str) -> Self {
        match value {
            "protected" => Self::Protected,
            "primary" => Self::Primary,
            _ => Self::Normal,
        }
    }

    /// Whether file mutations inside this source are forbidden.
    pub fn protects_files(self) -> bool {
        matches!(self, Self::Protected)
    }
}

/// Where a source's Wavecrate metadata database is stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum SourceMetadataStorage {
    /// Store `.wavecrate.db` inside the source folder.
    #[default]
    SourceFolder,
    /// Store source metadata in Wavecrate's app data folder.
    AppData,
}

impl SourceMetadataStorage {
    /// Stable database/config representation.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::SourceFolder => "source_folder",
            Self::AppData => "app_data",
        }
    }

    /// Parse a stored metadata policy, defaulting unknown values to source-local.
    pub fn from_stored(value: &str) -> Self {
        match value {
            "app_data" => Self::AppData,
            _ => Self::SourceFolder,
        }
    }
}

/// User-selected folder that owns wav files and Wavecrate metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SampleSource {
    /// Stable identifier for the source.
    pub id: SourceId,
    /// Root folder path for the source.
    pub root: PathBuf,
    /// Source role for write guards and default destination behavior.
    #[serde(default)]
    pub role: SourceRole,
    /// Metadata storage policy for this source.
    #[serde(default)]
    pub metadata_storage: SourceMetadataStorage,
    /// Relative import folder used when this source is the primary library.
    #[serde(default = "default_primary_import_folder")]
    pub primary_import_folder: PathBuf,
}

impl SampleSource {
    /// Create a new sample source for the given directory.
    pub fn new(root: PathBuf) -> Self {
        Self {
            id: SourceId::new(),
            root,
            role: SourceRole::Normal,
            metadata_storage: SourceMetadataStorage::SourceFolder,
            primary_import_folder: default_primary_import_folder(),
        }
    }

    /// Create a sample source with an existing id.
    pub fn new_with_id(id: SourceId, root: PathBuf) -> Self {
        Self {
            id,
            root,
            role: SourceRole::Normal,
            metadata_storage: SourceMetadataStorage::SourceFolder,
            primary_import_folder: default_primary_import_folder(),
        }
    }

    /// Return a copy configured as protected.
    pub fn protected(mut self) -> Self {
        self.role = SourceRole::Protected;
        self.metadata_storage = SourceMetadataStorage::AppData;
        self
    }

    /// Return a copy configured as primary.
    pub fn primary(mut self) -> Self {
        self.role = SourceRole::Primary;
        self.metadata_storage = SourceMetadataStorage::SourceFolder;
        self
    }

    /// Whether the source blocks audio-file mutations.
    pub fn is_protected(&self) -> bool {
        self.role.protects_files()
    }

    /// Whether the source is the primary writable destination.
    pub fn is_primary(&self) -> bool {
        self.role == SourceRole::Primary
    }

    /// Folder used for generated imports when this source is primary.
    pub fn primary_import_path(&self) -> PathBuf {
        self.root.join(&self.primary_import_folder)
    }

    /// Root folder that contains the Wavecrate metadata database.
    pub fn database_root(&self) -> Result<PathBuf, crate::app_dirs::AppDirError> {
        match self.metadata_storage {
            SourceMetadataStorage::SourceFolder => Ok(self.root.clone()),
            SourceMetadataStorage::AppData => Ok(external_metadata_root_for_source(&self.id)?),
        }
    }

    /// Location of the SQLite database for this source.
    pub fn db_path(&self) -> Result<PathBuf, crate::app_dirs::AppDirError> {
        Ok(database_path_for(&self.database_root()?))
    }

    /// Open the SQLite database for this source, creating it if necessary.
    pub fn open_db(&self) -> Result<SourceDatabase, SourceDbError> {
        let database_root =
            self.database_root()
                .map_err(|source| SourceDbError::ExternalMetadataRoot {
                    path: self.root.clone(),
                    source,
                })?;
        SourceDatabase::open_for_source_write_with_database_root(&self.root, database_root)
    }
}

/// Normalize a path for durable storage by preserving only its path components.
pub fn normalize_path(path: &Path) -> PathBuf {
    PathBuf::from_iter(path.components())
}

/// Name the per-source database using a hidden file inside the chosen folder.
pub fn database_path_for(root: &Path) -> PathBuf {
    root.join(DB_FILE_NAME)
}

/// Default relative inbox folder for primary-source imports.
pub fn default_primary_import_folder() -> PathBuf {
    PathBuf::from("_Wavecrate Inbox")
}

fn external_metadata_root_for_source(
    id: &SourceId,
) -> Result<PathBuf, crate::app_dirs::AppDirError> {
    Ok(crate::app_dirs::app_root_dir()?
        .join("source-metadata")
        .join(id.as_str()))
}
