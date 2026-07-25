use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

use super::source_scan_cache::FolderScanCacheUpdate;
use super::{FileEntry, FolderEntry, collections::MissingCollectionSnapshot};
use wavecrate::sample_sources::{
    config::DEFAULT_RATING_DECAY_WEEKS, scanner::CommittedSourceDelta,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::native_app) struct FolderScanRequest {
    pub(in crate::native_app) task_id: u64,
    pub(in crate::native_app) source_id: String,
    pub(in crate::native_app) label: String,
    pub(in crate::native_app) root: PathBuf,
    pub(in crate::native_app) database_root: PathBuf,
    pub(in crate::native_app) rating_decay_weeks: u16,
}

impl FolderScanRequest {
    pub(in crate::native_app) fn default_rating_decay_weeks() -> u16 {
        DEFAULT_RATING_DECAY_WEEKS
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::native_app) struct FolderScanProgress {
    pub(in crate::native_app) task_id: u64,
    pub(in crate::native_app) source_id: String,
    pub(in crate::native_app) label: String,
    pub(in crate::native_app) lifecycle: FolderScanLifecycle,
    pub(in crate::native_app) completed: usize,
    pub(in crate::native_app) total: usize,
    pub(in crate::native_app) detail: String,
    pub(in crate::native_app) queued_at: Instant,
    pub(in crate::native_app) state_changed_at: Instant,
    pub(in crate::native_app) last_progress_at: Instant,
    pub(in crate::native_app) lifecycle_generation: Option<u64>,
    pub(in crate::native_app) retry_count: u32,
}

pub(in crate::native_app) const SOURCE_SCAN_LONG_WAIT_THRESHOLD: Duration = Duration::from_secs(30);

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::native_app) enum FolderScanLifecycle {
    Queued,
    WaitingForSourceRegistration,
    WaitingForScanCapacity { current_owner: Option<String> },
    Scanning,
    ApplyingResults,
    PersistingResults,
    RetryScheduled,
    Canceled,
    Failed,
    CompleteWithWarnings,
    Complete,
}

impl FolderScanLifecycle {
    pub(in crate::native_app) fn label(&self) -> &'static str {
        match self {
            Self::Queued => "Queued",
            Self::WaitingForSourceRegistration => "Waiting for source update",
            Self::WaitingForScanCapacity { .. } => "Waiting for scan capacity",
            Self::Scanning => "Scanning",
            Self::ApplyingResults => "Applying results",
            Self::PersistingResults => "Saving results",
            Self::RetryScheduled => "Retry scheduled",
            Self::Canceled => "Canceled",
            Self::Failed => "Failed",
            Self::CompleteWithWarnings => "Complete with warnings",
            Self::Complete => "Complete",
        }
    }

    pub(in crate::native_app) fn is_waiting(&self) -> bool {
        matches!(
            self,
            Self::Queued
                | Self::WaitingForSourceRegistration
                | Self::WaitingForScanCapacity { .. }
                | Self::RetryScheduled
        )
    }

    pub(in crate::native_app) fn is_terminal(&self) -> bool {
        matches!(
            self,
            Self::Canceled | Self::Failed | Self::CompleteWithWarnings | Self::Complete
        )
    }
}

impl FolderScanProgress {
    pub(in crate::native_app) fn new(
        task_id: u64,
        source_id: String,
        label: String,
        lifecycle: FolderScanLifecycle,
        completed: usize,
        total: usize,
        detail: String,
    ) -> Self {
        let now = Instant::now();
        Self {
            task_id,
            source_id,
            label,
            lifecycle,
            completed,
            total,
            detail,
            queued_at: now,
            state_changed_at: now,
            last_progress_at: now,
            lifecycle_generation: None,
            retry_count: 0,
        }
    }

    pub(in crate::native_app) fn transition(
        task_id: u64,
        source_id: String,
        label: String,
        lifecycle: FolderScanLifecycle,
        detail: impl Into<String>,
    ) -> Self {
        Self::new(task_id, source_id, label, lifecycle, 0, 0, detail.into())
    }

    pub(in crate::native_app) fn reconcile_timing_from(&mut self, previous: &Self) {
        let meaningful_progress = self.lifecycle != previous.lifecycle
            || self.completed != previous.completed
            || self.total != previous.total
            || self.detail != previous.detail;
        self.queued_at = previous.queued_at;
        if self.lifecycle == previous.lifecycle {
            self.state_changed_at = previous.state_changed_at;
        }
        if !meaningful_progress {
            self.last_progress_at = previous.last_progress_at;
        }
        self.retry_count = self.retry_count.max(previous.retry_count);
        self.lifecycle_generation = self.lifecycle_generation.or(previous.lifecycle_generation);
    }

    pub(in crate::native_app) fn queue_age_at(&self, now: Instant) -> Duration {
        now.saturating_duration_since(self.queued_at)
    }

    pub(in crate::native_app) fn last_progress_age_at(&self, now: Instant) -> Duration {
        now.saturating_duration_since(self.last_progress_at)
    }

    pub(in crate::native_app) fn taking_longer_than_expected_at(&self, now: Instant) -> bool {
        !self.lifecycle.is_terminal()
            && self.last_progress_age_at(now) >= SOURCE_SCAN_LONG_WAIT_THRESHOLD
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::native_app) enum FolderScanItem {
    ResetFolder,
    Folder(FolderEntry),
    File(FileEntry),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::native_app) struct FolderScanDiscovery {
    pub(in crate::native_app) task_id: u64,
    pub(in crate::native_app) source_id: String,
    pub(in crate::native_app) committed_revision: Option<u64>,
    pub(in crate::native_app) parent_id: String,
    pub(in crate::native_app) item: FolderScanItem,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::native_app) struct FolderScanDiscoveryBatch {
    pub(in crate::native_app) task_id: u64,
    pub(in crate::native_app) source_id: String,
    pub(in crate::native_app) committed_revision: Option<u64>,
    pub(in crate::native_app) lifecycle_generation: Option<u64>,
    pub(in crate::native_app) sequence: u64,
    pub(in crate::native_app) events: Vec<FolderScanDiscovery>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::native_app) struct FolderScanResult {
    pub(in crate::native_app) task_id: u64,
    pub(in crate::native_app) source_id: String,
    pub(in crate::native_app) label: String,
    pub(in crate::native_app) folder: FolderEntry,
    pub(in crate::native_app::sample_library::folder_browser) missing_collection_snapshot:
        MissingCollectionSnapshot,
    pub(in crate::native_app) file_count: usize,
    pub(in crate::native_app) folder_count: usize,
    pub(in crate::native_app) source_db_error: Option<String>,
    pub(in crate::native_app) metadata_hydration: MetadataHydrationStatus,
    pub(in crate::native_app) committed_delta: Option<CommittedSourceDelta>,
    pub(in crate::native_app) source_root_available: bool,
    pub(in crate::native_app) cancelled: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::native_app) enum MetadataHydrationStatus {
    Complete { revision: u64 },
    Failed { error: String },
    NotAttempted,
}

impl MetadataHydrationStatus {
    pub(in crate::native_app) fn error(&self) -> Option<&str> {
        match self {
            Self::Failed { error } => Some(error),
            Self::Complete { .. } | Self::NotAttempted => None,
        }
    }

    pub(in crate::native_app) fn revision(&self) -> Option<u64> {
        match self {
            Self::Complete { revision } => Some(*revision),
            Self::Failed { .. } | Self::NotAttempted => None,
        }
    }
}

impl FolderScanResult {
    pub(in crate::native_app) fn audio_file_paths(&self) -> Vec<PathBuf> {
        self.folder
            .all_files()
            .into_iter()
            .filter(|file| file.is_audio() && !file.is_missing())
            .map(|file| PathBuf::from(&file.id))
            .collect()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::native_app) struct PreparedFolderScanResult {
    pub(in crate::native_app) scan: FolderScanResult,
    pub(in crate::native_app) audio_file_paths: Vec<PathBuf>,
    pub(in crate::native_app) scan_cache_update: FolderScanCacheUpdate,
    pub(in crate::native_app) lifecycle_generation: Option<u64>,
    pub(in crate::native_app) terminal_failure: Option<String>,
    pub(in crate::native_app) rating_decay_maintenance: Option<RatingDecayMaintenanceRequest>,
}

impl From<FolderScanResult> for PreparedFolderScanResult {
    fn from(scan: FolderScanResult) -> Self {
        let audio_file_paths = scan.audio_file_paths();
        let scan_cache_update = super::source_scan_cache::prepare_folder_scan_cache_update(&scan);
        Self {
            scan,
            audio_file_paths,
            scan_cache_update,
            lifecycle_generation: None,
            terminal_failure: None,
            rating_decay_maintenance: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::native_app) struct RatingDecayMaintenanceRequest {
    pub(in crate::native_app) source_id: String,
    pub(in crate::native_app) root: PathBuf,
    pub(in crate::native_app) database_root: PathBuf,
    pub(in crate::native_app) rating_decay_weeks: u16,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::native_app) struct FolderTreeRefreshRequest {
    pub(in crate::native_app) source_id: String,
    pub(in crate::native_app) label: String,
    pub(in crate::native_app) root: PathBuf,
    pub(in crate::native_app) database_root: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::native_app) struct FolderTreeRefreshResult {
    pub(in crate::native_app) source_id: String,
    pub(in crate::native_app) label: String,
    pub(in crate::native_app) folder: FolderEntry,
    pub(in crate::native_app) folder_count: usize,
    pub(in crate::native_app) source_root_available: bool,
}

/// Request for verifying that a selected folder still matches its cached child state.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::native_app) struct FolderVerifyRequest {
    pub(in crate::native_app) source_id: String,
    pub(in crate::native_app) source_root: PathBuf,
    pub(in crate::native_app) database_root: PathBuf,
    pub(in crate::native_app) folder_path: PathBuf,
    pub(in crate::native_app) cached_child_ids: Vec<String>,
    pub(in crate::native_app) cached_file_signatures: Vec<(String, u64)>,
}

/// Fresh filesystem snapshot used to detect drift in a cached folder view.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::native_app) struct FolderVerifySnapshot {
    pub(in crate::native_app) child_paths: Vec<PathBuf>,
    pub(in crate::native_app) files: Vec<FileEntry>,
}

/// Result of a folder verification pass.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::native_app) enum FolderVerifyOutcome {
    Unchanged,
    Missing,
    Changed(FolderVerifySnapshot),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::native_app) struct FolderVerifyResult {
    pub(in crate::native_app) source_id: String,
    pub(in crate::native_app) folder_path: PathBuf,
    pub(in crate::native_app) outcome: FolderVerifyOutcome,
}
