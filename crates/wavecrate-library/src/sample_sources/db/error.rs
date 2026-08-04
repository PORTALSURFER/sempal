use std::path::PathBuf;

use thiserror::Error;

use super::types::SourceDirectoryTruthUnavailableReason;

/// Typed fail-closed errors from the directory-truth publication contract.
#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum SourceDirectoryTruthError {
    /// A generation number is not a valid positive source-local generation.
    #[error("directory truth generation {generation} is invalid")]
    InvalidGeneration {
        /// Invalid generation supplied by the caller.
        generation: u64,
    },
    /// An entry count cannot be represented safely by the source database.
    #[error("directory truth entry count {count} is invalid")]
    InvalidEntryCount {
        /// Invalid entry count supplied by the caller.
        count: u64,
    },
    /// The caller's expected source revision no longer matches the writer transaction.
    #[error("directory truth expected source revision {expected}, found {actual}")]
    StaleRevision {
        /// Revision supplied by the caller.
        expected: u64,
        /// Revision observed under the writer lock.
        actual: u64,
    },
    /// The requested generation does not exist.
    #[error("directory truth generation {generation} does not exist")]
    GenerationMissing {
        /// Generation requested by the caller.
        generation: u64,
    },
    /// The staged generation does not contain its declared number of entries.
    #[error(
        "directory truth generation {generation} is incomplete: expected {expected}, staged {staged}"
    )]
    Incomplete {
        /// Generation being finalized.
        generation: u64,
        /// Declared entry count.
        expected: u64,
        /// Persisted entry count.
        staged: u64,
    },
    /// A batch contains the same normalized path more than once.
    #[error("directory truth path is duplicated: {path}")]
    DuplicatePath {
        /// Duplicate normalized path.
        path: PathBuf,
    },
    /// A generation already contains the same path.
    #[error("directory truth path already exists in the generation: {path}")]
    ExistingPath {
        /// Existing normalized path.
        path: PathBuf,
    },
    /// A batch or generation contains the same stable directory identity more than once.
    #[error("directory truth directory identity is duplicated: {identity}")]
    DuplicateDirectoryIdentity {
        /// Duplicate stable identity.
        identity: String,
    },
    /// A requested generation collides with a generation in another lifecycle state.
    #[error("directory truth generation {generation} has already been published or retired")]
    GenerationCollision {
        /// Colliding generation.
        generation: u64,
    },
    /// A batch would exceed the generation's declared entry count.
    #[error("directory truth generation {generation} would exceed its declared entry count")]
    EntryCountExceeded {
        /// Generation receiving the batch.
        generation: u64,
    },
    /// A batch exceeds the bounded staging limit.
    #[error("directory truth staging batch exceeds the bounded limit")]
    BatchTooLarge,
    /// A path or identity cannot be persisted without losing its meaning.
    #[error("directory truth entry is invalid: {path}")]
    InvalidPath {
        /// Invalid path supplied by the caller.
        path: PathBuf,
    },
    /// A directory identity is empty or contains unsupported control data.
    #[error("directory truth directory identity is invalid")]
    InvalidDirectoryIdentity,
    /// The database shape is not safe to write.
    #[error("directory truth schema is unavailable or malformed")]
    SchemaUnavailable,
    /// A read cursor belongs to a different active generation or revision.
    #[error("directory truth cursor is stale")]
    StaleCursor,
    /// Persisted directory state requires an audit before mutation.
    #[error("directory truth requires an audit: {reason:?}")]
    RequiresAudit {
        /// Fail-closed reason.
        reason: SourceDirectoryTruthUnavailableReason,
    },
}

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
    /// Directory-truth storage rejected a stale, malformed, or conflicting operation.
    #[error(transparent)]
    DirectoryTruth(#[from] SourceDirectoryTruthError),
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
