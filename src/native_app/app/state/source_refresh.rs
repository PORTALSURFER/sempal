use std::{collections::BTreeSet, path::PathBuf, time::Instant};

use crate::native_app::sample_library::source_watcher::WatcherContinuityProof;

pub(super) struct QueuedSourceRefresh {
    pub(super) source_id: String,
    pub(super) selection_requested: bool,
    pub(super) scan_required: bool,
    pub(super) cause: SourceRefreshCause,
    pub(super) lifecycle_generation: Option<u64>,
    pub(super) enqueued_at: Instant,
}

pub(super) struct QueuedTargetedSourceSync {
    pub(super) source_id: String,
    pub(super) paths: BTreeSet<PathBuf>,
    pub(super) lifecycle_generation: Option<u64>,
    pub(super) journal_checkpoint_event_id: Option<u64>,
    pub(super) watcher_continuity_proof: Option<WatcherContinuityProof>,
    pub(super) proofless_evidence_seen: bool,
    pub(super) audit_required: bool,
    pub(super) enqueued_at: Instant,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::native_app) enum SourceRefreshCause {
    DeferredSourceAdd,
    DeferredSelection,
    WatcherOverflow,
    ManifestAudit { committed_revision: u64 },
    ProjectionRevisionGap { committed_revision: u64 },
    FilesystemSyncIncomplete,
    FilesystemSyncFailed,
    ScanCancelled,
}

impl SourceRefreshCause {
    pub(in crate::native_app) fn label(self) -> &'static str {
        match self {
            Self::DeferredSourceAdd => "deferred_source_add",
            Self::DeferredSelection => "deferred_selection",
            Self::WatcherOverflow => "watcher_overflow",
            Self::ManifestAudit { .. } => "manifest_audit",
            Self::ProjectionRevisionGap { .. } => "projection_revision_gap",
            Self::FilesystemSyncIncomplete => "filesystem_sync_incomplete",
            Self::FilesystemSyncFailed => "filesystem_sync_failed",
            Self::ScanCancelled => "scan_cancelled",
        }
    }

    pub(in crate::native_app) fn committed_revision(self) -> Option<u64> {
        match self {
            Self::ManifestAudit { committed_revision }
            | Self::ProjectionRevisionGap { committed_revision } => Some(committed_revision),
            Self::DeferredSourceAdd
            | Self::DeferredSelection
            | Self::WatcherOverflow
            | Self::FilesystemSyncIncomplete
            | Self::FilesystemSyncFailed
            | Self::ScanCancelled => None,
        }
    }

    pub(super) fn merge(self, incoming: Self) -> Self {
        match (self.committed_revision(), incoming.committed_revision()) {
            (Some(current), Some(incoming_revision)) => {
                if incoming_revision >= current {
                    incoming
                } else {
                    self
                }
            }
            (None, Some(_)) => self,
            (Some(_), None) => incoming,
            (None, None) => incoming,
        }
    }
}

pub(in crate::native_app) struct PendingSourceRefresh {
    pub(in crate::native_app) source_id: String,
    pub(in crate::native_app) cause: SourceRefreshCause,
    pub(in crate::native_app) lifecycle_generation: Option<u64>,
    pub(in crate::native_app) enqueued_at: Instant,
}

pub(in crate::native_app) struct PendingTargetedSourceSync {
    pub(in crate::native_app) source_id: String,
    pub(in crate::native_app) paths: Vec<PathBuf>,
    pub(in crate::native_app) lifecycle_generation: Option<u64>,
    pub(in crate::native_app) journal_checkpoint_event_id: Option<u64>,
    pub(in crate::native_app) watcher_continuity_proof: Option<WatcherContinuityProof>,
    pub(in crate::native_app) proofless_evidence_seen: bool,
    pub(in crate::native_app) audit_required: bool,
    pub(in crate::native_app) enqueued_at: Instant,
}
