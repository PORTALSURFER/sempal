use std::path::PathBuf;

use thiserror::Error;

/// Errors returned when managing a source database.
#[derive(Debug, Error)]
pub enum SourceDbError {
    /// The provided root path is not a directory.
    #[error("Source folder is not a directory: {0}")]
    InvalidRoot(PathBuf),
    /// SQLite query failed.
    #[error("Database query failed: {0}")]
    Sql(#[from] rusqlite::Error),
    /// Failed to create a parent directory.
    #[error("Could not write to {path}: {source}")]
    CreateDir {
        /// Path that could not be created.
        path: PathBuf,
        /// Underlying IO error.
        source: std::io::Error,
    },
    /// Provided path was not relative to the source root.
    #[error("Path must be relative to the source root: {0}")]
    PathMustBeRelative(PathBuf),
    /// Provided path contained disallowed components or was empty.
    #[error("Path contains invalid relative components: {0}")]
    InvalidRelativePath(PathBuf),
    /// A relative path could not be represented without losing filesystem identity.
    #[error("Path is not valid Unicode and cannot be persisted safely: {0}")]
    NonUnicodeRelativePath(PathBuf),
    /// Database is locked or busy.
    #[error("Database is busy, please retry")]
    Busy,
    /// A caller canceled an in-progress database operation.
    #[error("Database operation canceled")]
    Canceled,
    /// SQLite returned an unexpected result.
    #[error("SQLite returned an unexpected result")]
    Unexpected,
    /// Provided tag text cannot be normalized to a non-empty identity.
    #[error("Tag label cannot be empty")]
    EmptyTagLabel,
    /// Read-only mode requires an existing database file.
    #[error("Read-only source DB mode requires an existing database file: {0}")]
    ReadOnlyDatabaseMissing(PathBuf),
    /// Source database path policy rejected an unsafe local database path.
    #[error("Unsafe source database path {path}: {reason}")]
    UnsafeSourceDatabasePath {
        /// Path rejected by the source DB path policy.
        path: PathBuf,
        /// Stable reason suitable for user-facing status and diagnostics.
        reason: &'static str,
    },
    /// Failed to inspect a source database path before trusting it.
    #[error("Could not inspect source database path {path}: {source}")]
    InspectSourceDatabasePath {
        /// Path that could not be inspected.
        path: PathBuf,
        /// Underlying IO error.
        source: std::io::Error,
    },
    /// Failed to resolve a source database path before trusting it.
    #[error("Could not resolve source database path {path}: {source}")]
    ResolveSourceDatabasePath {
        /// Path that could not be resolved.
        path: PathBuf,
        /// Underlying IO error.
        source: std::io::Error,
    },
    /// Failed to resolve the app-owned metadata folder for a protected source.
    #[error("Could not resolve external metadata storage for {path}: {source}")]
    ExternalMetadataRoot {
        /// Source root whose external metadata folder was requested.
        path: PathBuf,
        /// Underlying application directory error.
        source: crate::app_dirs::AppDirError,
    },
    /// Failed to move a source DB from its legacy filename to the current filename.
    #[error("Could not migrate source database from {from} to {to}: {source}")]
    RenameLegacyDatabase {
        /// Legacy source DB path.
        from: PathBuf,
        /// Current source DB path.
        to: PathBuf,
        /// Underlying IO error.
        source: std::io::Error,
    },
}
