//! Source hydration, folder projection, and related source-lane DTOs.

use super::*;
use crate::app::controller::LoadEntriesError;
use crate::app::controller::library::source_folders::{FolderProjectionView, FolderTreeSnapshot};
use crate::sample_sources::WavEntry;
use std::sync::{
    Mutex,
    atomic::{AtomicBool, Ordering},
};

/// Shared cancellation and source-write-fence ownership for one remap request.
#[derive(Debug, Default)]
pub(crate) struct SourceRemapWriteFence {
    canceled: AtomicBool,
    fence: Mutex<Option<crate::sample_sources::db::SourceDatabaseWriteFence>>,
}

/// Stable identity and metadata captured for one remap destination database artifact.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SourceRemapArtifactIdentity {
    /// Platform file identity when the operating system exposes one.
    pub(crate) stable_id: Option<String>,
    /// Artifact length from no-follow metadata.
    pub(crate) len: u64,
    /// Last-modified timestamp as nanoseconds after the Unix epoch, when available.
    pub(crate) modified_ns: Option<i64>,
    /// Whether the artifact itself is a symbolic link.
    pub(crate) is_symlink: bool,
}

/// Complete SQLite artifact identity for one current or legacy database name.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct SourceRemapDatabaseIdentity {
    /// Main SQLite database file.
    pub(crate) database: Option<SourceRemapArtifactIdentity>,
    /// Write-ahead log sidecar.
    pub(crate) wal: Option<SourceRemapArtifactIdentity>,
    /// Shared-memory sidecar.
    pub(crate) shm: Option<SourceRemapArtifactIdentity>,
    /// Rollback-journal sidecar.
    pub(crate) journal: Option<SourceRemapArtifactIdentity>,
}

impl SourceRemapDatabaseIdentity {
    /// Return whether any artifact exists under this database name.
    pub(crate) fn has_artifacts(&self) -> bool {
        self.database.is_some()
            || self.wal.is_some()
            || self.shm.is_some()
            || self.journal.is_some()
    }
}

impl SourceRemapWriteFence {
    /// Install a prepared source-write fence unless the remap was already canceled.
    pub(crate) fn install(
        &self,
        fence: crate::sample_sources::db::SourceDatabaseWriteFence,
    ) -> bool {
        let mut active = self
            .fence
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if self.canceled.load(Ordering::Acquire) {
            return false;
        }
        *active = Some(fence);
        true
    }

    /// Cancel the remap and immediately release any installed source-write fence.
    pub(crate) fn cancel(&self) {
        self.canceled.store(true, Ordering::Release);
        self.release();
    }

    /// Release an installed source-write fence without changing cancellation state.
    pub(crate) fn release(&self) {
        self.fence
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .take();
    }

    /// Return whether the controller canceled this remap request.
    pub(crate) fn is_canceled(&self) -> bool {
        self.canceled.load(Ordering::Acquire)
    }
}

/// Controller-owned source hydration lanes used to load source snapshots off the UI thread.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SourceHydrationKind {
    /// Hydrate the currently selected source that drives browser and waveform state.
    ActiveSelection,
    /// Hydrate a source assigned to the inactive retained folder pane.
    InactivePane,
}

/// Background source-hydration request that prepares one source snapshot for a pane.
#[derive(Debug)]
pub(crate) struct SourceHydrationJob {
    /// Monotonic request identifier used to discard stale results.
    pub(crate) request_id: u64,
    /// Sidebar pane that owns the source assignment.
    pub(crate) pane: FolderPaneId,
    /// Logical hydration lane for result application.
    pub(crate) kind: SourceHydrationKind,
    /// Source identifier that should be hydrated.
    pub(crate) source_id: SourceId,
    /// Source root used to open the DB and derive folder availability.
    pub(crate) source_root: PathBuf,
    /// Target page size for page-0 entry loading.
    pub(crate) page_size: usize,
    /// Optional cached page-0 entries cloned at dispatch time.
    pub(crate) cached_page: Option<Vec<WavEntry>>,
    /// Total entry count associated with `cached_page`, when present.
    pub(crate) cached_total: Option<usize>,
    /// Page size associated with `cached_page`, when present.
    pub(crate) cached_page_size: Option<usize>,
    /// Whether startup should stop after the minimum page-0 payload and defer follow-up work.
    pub(crate) defer_startup_follow_up_work: bool,
}

/// Background source-add preparation request.
#[derive(Debug)]
pub(crate) struct SourceAddJob {
    /// Monotonic request identifier used to discard stale or duplicate results.
    pub(crate) request_id: u64,
    /// Source that will be committed after DB validation succeeds.
    pub(crate) source: crate::sample_sources::SampleSource,
}

/// Result of one background source-add preparation request.
#[derive(Debug)]
pub(crate) struct SourceAddPreparedResult {
    /// Request identifier echoed from [`SourceAddJob::request_id`].
    pub(crate) request_id: u64,
    /// Source that was prepared.
    pub(crate) source: crate::sample_sources::SampleSource,
    /// Worker time spent opening or migrating source state.
    pub(crate) elapsed: Duration,
    /// Preparation outcome.
    pub(crate) result: Result<(), String>,
}

/// Background source-remap snapshot request.
#[derive(Debug)]
pub(crate) struct SourceRemapJob {
    /// Monotonic request identifier used to reject stale completion.
    pub(crate) request_id: u64,
    /// Source before the remap.
    pub(crate) source: crate::sample_sources::SampleSource,
    /// Normalized destination root.
    pub(crate) new_root: PathBuf,
    /// Shared cancellation and source-write-fence ownership.
    pub(crate) write_fence: Arc<SourceRemapWriteFence>,
}

/// Result of one background source-remap snapshot request.
#[derive(Debug)]
pub(crate) struct SourceRemapPreparedResult {
    /// Request identifier echoed from [`SourceRemapJob::request_id`].
    pub(crate) request_id: u64,
    /// Source before the remap.
    pub(crate) source: crate::sample_sources::SampleSource,
    /// Normalized destination root.
    pub(crate) new_root: PathBuf,
    /// Request-owned snapshot staged outside the destination database path.
    pub(crate) staged_database: Option<PathBuf>,
    /// Prepared identity of the current destination database, when present.
    pub(crate) destination_current_database_identity: SourceRemapDatabaseIdentity,
    /// Prepared identity of the legacy destination database, when present.
    pub(crate) destination_legacy_database_identity: SourceRemapDatabaseIdentity,
    /// Source writer reservation retained until this result is published or discarded.
    pub(crate) write_fence: Arc<SourceRemapWriteFence>,
    /// Preparation outcome.
    pub(crate) result: Result<(), String>,
}

/// Compact source snapshot prepared off the UI thread for later controller apply.
#[derive(Debug)]
pub(crate) struct SourceHydrationSnapshot {
    /// Page-0 wav entries for the hydrated source.
    pub(crate) entries: Vec<WavEntry>,
    /// Total number of wav entries in the source.
    pub(crate) total: usize,
    /// Page size used to interpret `entries`.
    pub(crate) page_size: usize,
    /// Prebuilt normalized path lookup aligned with the hydrated page.
    pub(crate) path_lookup: HashMap<PathBuf, usize>,
    /// Folder availability derived from the hydrated page.
    pub(crate) available_folders: BTreeSet<PathBuf>,
    /// Immutable folder-tree snapshot derived from `available_folders`.
    pub(crate) folder_tree: FolderTreeSnapshot,
    /// Browser feature metadata aligned to `entries` for first-paint row badges.
    pub(crate) feature_cache: Option<crate::app::controller::FeatureCache>,
    /// Whether the snapshot reused the page-0 wav cache instead of querying the DB.
    pub(crate) from_cache: bool,
    /// Whether folder projection and feature metadata still need a deferred follow-up pass.
    pub(crate) deferred_follow_up_work: bool,
}

/// Result of one async browser feature-cache refresh.
#[derive(Debug)]
pub(crate) struct BrowserFeatureCacheRefreshResult {
    /// Request identifier used to drop stale refresh results.
    pub(crate) request_id: u64,
    /// Source whose browser feature metadata was refreshed.
    pub(crate) source_id: SourceId,
    /// Snapshot key the refreshed rows were built against.
    pub(crate) key: crate::app::controller::FeatureCacheKey,
    /// Refreshed cache payload or the terminal load error.
    pub(crate) result: Result<crate::app::controller::FeatureCache, String>,
}

/// Background folder projection request for one pane-scoped folder browser.
#[derive(Debug)]
pub(crate) struct FolderProjectionJob {
    /// Monotonic request identifier used to discard stale results.
    pub(crate) request_id: u64,
    /// Sidebar pane whose folder browser rows are being projected.
    pub(crate) pane: FolderPaneId,
    /// Source identifier that owns the folder browser state.
    pub(crate) source_id: SourceId,
    /// Immutable retained folder model snapshot captured on the controller thread.
    pub(crate) model: crate::app::controller::library::source_folders::FolderBrowserModel,
    /// Projection workload to execute off the UI thread.
    pub(crate) work: FolderProjectionWork,
    /// Whether a source is assigned to the pane during projection.
    pub(crate) has_source: bool,
}

/// Off-thread folder projection workload kinds.
#[derive(Debug)]
pub(crate) enum FolderProjectionWork {
    /// Rebuild available folders, reconcile the retained model, and project rows.
    RefreshAvailable {
        /// Source root used to validate folder paths against disk.
        source_root: PathBuf,
        /// Currently loaded relative wav paths used to derive folder availability.
        loaded_relative_paths: Vec<PathBuf>,
        /// Cached disk folders retained for the source.
        disk_folders: BTreeSet<PathBuf>,
        /// Cached available folders used for empty-entry reuse semantics.
        cached_available: BTreeSet<PathBuf>,
        /// Visibility mode associated with `cached_available`.
        cached_available_show_all_folders: bool,
        /// Whether a waveform load for this source is still pending.
        pending_wav_load: bool,
    },
    /// Reproject rows from an existing immutable tree snapshot.
    Reproject {
        /// Immutable tree snapshot reused across row projections.
        snapshot: FolderTreeSnapshot,
    },
}

/// Result payload for one completed folder projection request.
#[derive(Debug)]
pub(crate) struct FolderProjectionSnapshot {
    /// Reconciled folder-browser model snapshot that should replace the retained cache entry.
    pub(crate) model: crate::app::controller::library::source_folders::FolderBrowserModel,
    /// Immutable tree snapshot aligned to `model.available`.
    pub(crate) tree: FolderTreeSnapshot,
    /// Projected folder-browser view fields ready for UI apply.
    pub(crate) view: FolderProjectionView,
}

/// Result of one background folder projection request.
#[derive(Debug)]
pub(crate) struct FolderProjectionResult {
    /// Request identifier echoed from [`FolderProjectionJob::request_id`].
    pub(crate) request_id: u64,
    /// Sidebar pane whose folder browser rows are being projected.
    pub(crate) pane: FolderPaneId,
    /// Source identifier that owns the folder browser state.
    pub(crate) source_id: SourceId,
    /// Worker time spent hydrating or projecting the folder rows.
    pub(crate) elapsed: Duration,
    /// Folder projection payload prepared off the UI thread.
    pub(crate) snapshot: FolderProjectionSnapshot,
}

/// Result of one background source hydration request.
#[derive(Debug)]
pub(crate) struct SourceHydrationResult {
    /// Request identifier echoed from [`SourceHydrationJob::request_id`].
    pub(crate) request_id: u64,
    /// Sidebar pane that owns the source assignment.
    pub(crate) pane: FolderPaneId,
    /// Logical hydration lane for result application.
    pub(crate) kind: SourceHydrationKind,
    /// Source identifier associated with this hydration attempt.
    pub(crate) source_id: SourceId,
    /// Worker time spent loading/building the snapshot.
    pub(crate) elapsed: Duration,
    /// Hydrated snapshot or the terminal load error.
    pub(crate) result: Result<SourceHydrationSnapshot, LoadEntriesError>,
}

/// Result of a background folder scan for a source root.
#[derive(Debug)]
pub(crate) struct FolderScanResult {
    /// Request identifier for this scan.
    pub(crate) request_id: u64,
    /// Source identifier associated with the scan.
    pub(crate) source_id: SourceId,
    /// Relative folder paths discovered on disk.
    pub(crate) folders: BTreeSet<PathBuf>,
}
