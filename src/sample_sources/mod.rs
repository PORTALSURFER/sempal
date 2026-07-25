//! Compatibility facade for sample-source persistence, scanning, and library models.
//!
//! The implementation is split across `wavecrate-library` for database-owned
//! types and `wavecrate-scan` for scan orchestration. This module preserves the
//! existing Wavecrate-facing API while keeping that ownership visible at each
//! re-export site.

/// User configuration loading/saving for sample sources.
pub mod config;
#[doc(hidden)]
pub mod duplicate_file_ops;
mod file_move_metadata;
#[doc(hidden)]
pub mod harvest_file_ops;
mod harvest_seen;
mod starmap_layout;
/// Scan tracking state to avoid duplicate work.
pub mod scan_state {
    pub use wavecrate_scan::sample_sources::scan_state::ScanTracker;
}
/// Source scanning logic.
pub mod scanner {
    pub use wavecrate_scan::sample_sources::scanner::{
        ChangedSample, CommittedSourceDelta, ContentAuditActivity, ContentAuditBudget,
        ContentAuditStorage, ManifestIdentityDelta, MovedManifestIdentity, RenamedSample,
        ScanError, ScanMode, ScanStats, UpdatedSample, audit_source, audit_source_and_record,
        audit_source_and_record_with_budget_and_progress,
        audit_source_and_record_with_budget_and_progress_and_writer,
        audit_source_and_record_with_progress, audit_source_with_budget, complete_deferred_hashes,
        complete_deferred_hashes_with_cancel, complete_deferred_rename_candidates,
        complete_deferred_rename_candidates_with_cancel, complete_pending_deep_hash_for_path,
        complete_pending_deep_hashes, hard_rescan, scan_in_background, scan_once,
        scan_with_progress, sync_paths, sync_paths_with_progress,
    };
}

/// Per-source database helpers.
pub mod db {
    #[cfg(debug_assertions)]
    pub use wavecrate_library::sample_sources::db::test_scope_source_db_open_total_count;
    #[cfg(debug_assertions)]
    pub use wavecrate_library::sample_sources::db::test_source_db_open_total_count;
    pub use wavecrate_library::sample_sources::db::{
        BrowserMetadataSnapshot, ContentAuditCheckpoint, ContentAuditEntryState,
        ContentAuditReport, ContentAuditSkipReason, DB_FILE_NAME, LEGACY_DB_FILE_NAME,
        META_DEFERRED_MAINTENANCE_REVISION, META_DEFERRED_MAINTENANCE_SCHEMA,
        META_LAST_MANIFEST_AUDIT_AT, META_LAST_SCAN_COMPLETED_AT,
        META_READINESS_DUPLICATE_IDENTITY, META_SOURCE_TRAVERSAL_POLICY,
        META_WAV_IDENTITIES_REVISION, META_WAV_PATHS_REVISION, PendingRenameEntry, Rating,
        SOURCE_DB_READ_ONLY_ENV, SampleCollection, SampleSoundType, SourceCollectionWrite,
        SourceContentHashWrite, SourceDatabase, SourceDatabaseConnectionRole,
        SourceDatabaseWriteFence, SourceDbError, SourceFileWrite, SourceTag, SourceTagUsage,
        SourceTagWrite, SourceWriteBatch, SourceWriteCommand, WavEntry, file_ops_journal,
        normalize_relative_path, read, schema, tags, util, write,
    };
}

/// Global library database helpers.
pub mod library {
    pub use wavecrate_library::sample_sources::library::{
        HarvestDerivationOperation, HarvestDerivationRecord, HarvestFileIdentity, HarvestFileKey,
        HarvestFileRecord, HarvestMetadataSnapshot, HarvestSourceRange, HarvestState,
        LIBRARY_DB_FILE_NAME, LibraryError, LibraryState, NewHarvestDerivation,
        harvest_derivations_for_parent, harvest_derivative_count,
        harvest_derivative_counts_for_source, harvest_file, harvest_files_for_source,
        harvest_parents_for_child, load, lookup_retained_source_for_root,
        lookup_source_id_for_root, mark_harvest_seen, mark_harvest_touched, open_connection,
        record_harvest_derivation, remap_harvest_file_key, remap_harvest_file_prefix,
        retained_sources, save, set_harvest_state, upsert_harvest_file, upsert_harvest_files,
    };
}

pub use duplicate_file_ops::{
    ContextSampleDoubleResult, ContextSampleSameResult, DuplicateDoubleRequest,
    DuplicateSameRequest, WholeFileHarvestExtractionCopy, WholeFileHarvestExtractionFailure,
    WholeFileHarvestExtractionPlan, WholeFileHarvestExtractionRequest,
    WholeFileHarvestExtractionResult, execute_duplicate_context_sample_double,
    execute_duplicate_context_sample_same, execute_whole_file_harvest_extraction,
};
#[doc(hidden)]
pub use file_move_metadata::{
    SourcedFileMoveMetadata, persist_copied_file_metadata, persist_sourced_moved_file_metadata,
};
pub use harvest_seen::{HarvestSeenPersistRequest, HarvestSeenPersistResult, persist_harvest_seen};
pub use starmap_layout::{
    STARMAP_LAYOUT_UMAP_VERSION, StarmapLayoutLoadRequest, StarmapLayoutLoadResult,
    StarmapLayoutPoint, StarmapLayoutSample, StarmapSourceLayoutRequest, load_starmap_layout,
};
pub use wavecrate_library::sample_sources::db::{SampleCollection, SampleSoundType};
#[cfg(any(test, feature = "legacy-controller"))]
pub(crate) use wavecrate_library::sample_sources::is_supported_audio;
pub use wavecrate_library::sample_sources::normalize_relative_path;
pub use wavecrate_library::sample_sources::readiness;
pub use wavecrate_library::sample_sources::{
    BrowserMetadataSnapshot, DB_FILE_NAME, HarvestDerivationOperation, HarvestDerivationRecord,
    HarvestFileIdentity, HarvestFileKey, HarvestFileRecord, HarvestMetadataSnapshot,
    HarvestSourceRange, HarvestState, LIBRARY_DB_FILE_NAME, LibraryError, LibraryState,
    NewHarvestDerivation, Rating, SampleSource, SourceCollectionWrite, SourceContentHashWrite,
    SourceDatabase, SourceDatabaseConnectionRole, SourceDbError, SourceFileWrite, SourceId,
    SourceMetadataStorage, SourceRole, SourceTagWrite, SourceWriteCommand, WavEntry,
    database_path_for, default_primary_import_folder, normalize_path,
};
pub use wavecrate_library::sample_sources::{HiddenDirectoryPolicy, SourceTraversalPolicy};
pub use wavecrate_scan::sample_sources::ScanTracker;
pub use wavecrate_scan::sample_sources::{
    ChangedSample, CommittedSourceDelta, ManifestIdentityDelta, MovedManifestIdentity,
    RenamedSample, ScanError, ScanMode, ScanStats, UpdatedSample,
};
