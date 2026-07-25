//! Shared scan-layer helpers built on top of the storage crate.

/// Scan tracking state to avoid duplicate work.
pub mod scan_state;
/// Source scanning logic.
pub mod scanner;

/// Per-source database helpers.
pub mod db {
    pub use wavecrate_library::sample_sources::db::*;
}

/// Global library database helpers.
pub mod library {
    pub use wavecrate_library::sample_sources::library::*;
}

pub use scan_state::ScanTracker;
pub use scanner::{
    ChangedSample, CommittedSourceDelta, ContentAuditActivity, ContentAuditBudget,
    ContentAuditStorage, DirectoryRepeatKind, ManifestIdentityDelta, MovedManifestIdentity,
    RenamedSample, ScanError, ScanMode, ScanStats, SourceTreeDiagnostic, SourceTreeFile,
    SourceTreeSnapshot, UpdatedSample,
};
pub use wavecrate_library::sample_sources::normalize_relative_path;
pub use wavecrate_library::sample_sources::{
    DB_FILE_NAME, LIBRARY_DB_FILE_NAME, LibraryError, LibraryState, Rating, SampleSource,
    SourceDatabase, SourceDbError, SourceId, WavEntry, database_path_for, normalize_path,
};
