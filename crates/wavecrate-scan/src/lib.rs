#![deny(missing_docs)]
#![deny(warnings)]

//! Shared sample-source scanning and scan-state helpers.

/// Sample-source scan state and filesystem synchronization helpers.
pub mod sample_sources;

pub use sample_sources::ScanTracker;
pub use sample_sources::scanner::{
    ChangedSample, CommittedScanBatch, CommittedSourceDelta, DirectoryRepeatKind,
    ManifestIdentityDelta, MovedManifestIdentity, RenamedSample, ScanError, ScanMode, ScanStats,
    SourceTreeDiagnostic, SourceTreeFile, SourceTreeSnapshot, UpdatedSample, audit_source,
    audit_source_and_record, audit_source_and_record_with_progress, complete_deferred_hashes,
    complete_deferred_rename_candidates, complete_deferred_rename_candidates_with_cancel,
    complete_pending_deep_hash_for_path, complete_pending_deep_hashes, hard_rescan,
    scan_in_background, scan_once, scan_with_progress,
    scan_with_progress_and_writer_and_committed_batch, sync_paths, sync_paths_with_progress,
};
