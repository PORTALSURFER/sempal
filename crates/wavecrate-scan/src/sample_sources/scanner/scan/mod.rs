mod context;
mod errors;
mod runner;
mod stats;

pub use super::scan_hash::{ContentAuditActivity, ContentAuditBudget, ContentAuditStorage};
pub(crate) use context::ScanContext;
pub use errors::ScanError;
pub use runner::{
    ManifestAuditOutcome, ScanMode, audit_source, audit_source_and_record,
    audit_source_and_record_with_budget_and_progress,
    audit_source_and_record_with_budget_and_progress_and_writer,
    audit_source_and_record_with_budget_and_progress_and_writer_outcome,
    audit_source_and_record_with_budget_and_progress_and_writer_with_request,
    audit_source_and_record_with_progress, audit_source_with_budget, complete_deferred_hashes,
    complete_deferred_hashes_with_cancel, complete_deferred_rename_candidates,
    complete_deferred_rename_candidates_with_cancel,
    complete_deferred_rename_candidates_with_cancel_and_writer,
    complete_pending_deep_hash_for_path, complete_pending_deep_hashes, hard_rescan,
    scan_in_background, scan_once, scan_with_progress, scan_with_progress_and_writer,
    scan_with_progress_and_writer_and_committed_batch,
};
#[cfg(test)]
pub(crate) use runner::{
    audit_source_and_record_with_post_scan_hook,
    audit_source_and_record_with_post_scan_hook_outcome,
    audit_source_and_record_with_pre_record_hook,
};
pub(crate) use runner::{finish_scan_result, reconcile_scan_renames};
pub use stats::{
    ChangedSample, CommittedScanBatch, CommittedSourceDelta, DirectoryRepeatKind,
    ManifestIdentityDelta, MovedManifestIdentity, RenamedSample, ScanStats, SourceTreeDiagnostic,
    SourceTreeFile, SourceTreeSnapshot, UpdatedSample,
};

#[cfg(test)]
mod tests;
