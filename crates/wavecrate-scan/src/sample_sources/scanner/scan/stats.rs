use std::{fmt, path::PathBuf};

use wavecrate_library::sample_sources::db::{ContentAuditReport, PendingRenameDiagnostics};
use wavecrate_library::sample_sources::{SourceIndexEntry, SourceManifestEntry};

/// One bounded group of source-manifest rows after its database transaction commits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommittedScanBatch {
    /// Monotonic source-manifest revision assigned by the committed transaction.
    pub revision: u64,
    /// Source-relative paths changed by this transaction, in scan order.
    pub paths: Vec<PathBuf>,
}

/// Why a directory was not fully represented by a source traversal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SourceTreeDiagnostic {
    /// A directory could not be read or opened.
    DirectoryUnavailable {
        /// Relative path of the unavailable directory.
        path: PathBuf,
        /// Underlying failure text.
        error: String,
    },
    /// A directory entry could not be read.
    DirectoryEntryUnavailable {
        /// Relative path of the directory whose entry was unavailable.
        path: PathBuf,
        /// Underlying failure text.
        error: String,
    },
    /// An entry's type could not be classified.
    EntryTypeUnavailable {
        /// Relative path of the unclassifiable entry.
        path: PathBuf,
        /// Underlying failure text.
        error: String,
    },
    /// File metadata could not be read.
    FileMetadataUnavailable {
        /// Relative path of the file whose metadata was unavailable.
        path: PathBuf,
        /// Underlying failure text.
        error: String,
    },
    /// A directory descriptor could not produce a stable identity.
    DirectoryIdentityUnavailable {
        /// Relative path of the directory without a stable identity.
        path: PathBuf,
        /// Identity failure text, or `None` when the platform is unsupported.
        error: Option<String>,
    },
    /// A directory identity was already visited during this traversal generation.
    RepeatedDirectory {
        /// Relative path whose identity was already visited.
        path: PathBuf,
        /// First relative path at which the identity was visited.
        first_path: PathBuf,
        /// Whether the repeated identity is an ancestor cycle or another target.
        kind: DirectoryRepeatKind,
    },
    /// The source changed after an earlier scan checkpoint was committed.
    TraversalChanged,
}

impl fmt::Display for SourceTreeDiagnostic {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DirectoryUnavailable { path, error } => {
                write!(formatter, "directory unavailable {}: {error}", path.display())
            }
            Self::DirectoryEntryUnavailable { path, error } => write!(
                formatter,
                "directory entry unavailable {}: {error}",
                path.display()
            ),
            Self::EntryTypeUnavailable { path, error } => {
                write!(formatter, "entry type unavailable {}: {error}", path.display())
            }
            Self::FileMetadataUnavailable { path, error } => {
                write!(formatter, "file metadata unavailable {}: {error}", path.display())
            }
            Self::DirectoryIdentityUnavailable { path, error } => match error {
                Some(error) => write!(
                    formatter,
                    "directory identity unavailable {}: {error}",
                    path.display()
                ),
                None => write!(
                    formatter,
                    "directory identity unavailable {}: unsupported platform",
                    path.display()
                ),
            },
            Self::RepeatedDirectory {
                path,
                first_path,
                kind,
            } => write!(
                formatter,
                "repeated directory identity ({kind:?}) at {} (first visited at {})",
                path.display(),
                first_path.display()
            ),
            Self::TraversalChanged => formatter.write_str(
                "supported audio changed or became unavailable after an earlier scan batch committed",
            ),
        }
    }
}

/// Whether a repeated directory identity points back into the current ancestor path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DirectoryRepeatKind {
    /// The identity points back to an ancestor in this traversal.
    Cycle,
    /// The identity was reached through another non-ancestor path.
    RepeatedTarget,
}

/// One non-audio regular file observed during the authoritative source traversal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceTreeFile {
    /// File path relative to the source root.
    pub relative_path: PathBuf,
    /// File size observed without following symbolic links.
    pub file_size: u64,
}

/// Browser layout facts captured by the same traversal that reconciles the source manifest.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct SourceTreeSnapshot {
    /// All visible directories, relative to the source root, including the empty root path.
    pub directories: Vec<PathBuf>,
    /// Visible regular files that are not authoritative supported-audio manifest rows.
    pub other_files: Vec<SourceTreeFile>,
    /// Typed index-only file facts captured by this traversal.
    #[doc(hidden)]
    pub index_entries: Vec<SourceIndexEntry>,
    /// Bounded diagnostics for entries that could not be classified or enumerated.
    pub diagnostics: Vec<SourceTreeDiagnostic>,
    /// Relative directory or entry prefixes whose descendants were not
    /// authoritatively observed during this traversal.
    ///
    /// This is internal scan state carried to missing-row reconciliation. It
    /// is deliberately unbounded: dropping a prefix here could turn an I/O
    /// failure into a false deletion.
    #[doc(hidden)]
    pub uncertain_prefixes: Vec<PathBuf>,
}

impl SourceTreeSnapshot {
    /// A projection is safe to publish only when every encountered entry was classified.
    pub fn is_complete(&self) -> bool {
        self.diagnostics.is_empty()
    }
}

/// Summary of a scan run.
#[derive(Debug, Default, Clone)]
pub struct ScanStats {
    /// Authoritative identity delta observed at the final committed source revision.
    pub committed_delta: CommittedSourceDelta,
    /// Typed index-only changes proven by one bounded committed source write.
    pub committed_source_index_delta: CommittedSourceIndexDelta,
    /// Number of newly discovered files.
    pub added: usize,
    /// Number of files updated in-place.
    pub updated: usize,
    /// Number of files now missing from disk.
    pub missing: usize,
    /// Total number of files scanned.
    pub total_files: usize,
    /// Number of files with changed content hashes.
    pub content_changed: usize,
    /// Number of files whose content hashes were computed during the scan.
    pub hashes_computed: usize,
    /// Number of files whose content hashes were deferred during the scan.
    pub hashes_pending: usize,
    /// Durable content-verification coverage after this scan.
    pub content_audit: Option<ContentAuditReport>,
    /// Number of missing rows reconciled to renamed files.
    pub renames_reconciled: usize,
    /// Current destination candidates examined through indexed pending-source lookups.
    pub pending_renames_considered: usize,
    /// Expired pending-source candidates removed by authoritative completion.
    pub pending_renames_pruned: usize,
    /// Aggregate pending-source population after authoritative completion.
    pub pending_rename_diagnostics: Option<PendingRenameDiagnostics>,
    /// Detailed list of files whose source-visible metadata was updated in place.
    pub updated_samples: Vec<UpdatedSample>,
    /// Detailed list of source-visible rename reconciliations.
    pub renamed_samples: Vec<RenamedSample>,
    /// Detailed list of changed samples.
    pub changed_samples: Vec<ChangedSample>,
    /// Newly inserted paths from this scan that are eligible as rename destinations.
    #[doc(hidden)]
    pub rename_candidate_paths: Vec<PathBuf>,
    #[doc(hidden)]
    pub manifest_before: Vec<SourceManifestEntry>,
    #[doc(hidden)]
    pub manifest_after: Vec<SourceManifestEntry>,
    /// Manifest rows updated by a bounded deferred operation.
    #[doc(hidden)]
    pub manifest_updates: Vec<SourceManifestEntry>,
    /// Filesystem layout captured by the authoritative full traversal.
    #[doc(hidden)]
    pub source_tree_snapshot: Option<SourceTreeSnapshot>,
    /// Bounded diagnostics from targeted traversal, which has no full browser-layout snapshot.
    pub traversal_diagnostics: Vec<SourceTreeDiagnostic>,
    /// Number of manifest rows materialized by a targeted scan's scoped reads.
    pub targeted_manifest_rows_read: usize,
    /// Number of exact-subtree manifest read statements issued by a targeted scan.
    pub targeted_manifest_query_count: usize,
    /// Number of canonical watcher targets after ancestor collapse.
    pub targeted_manifest_scope_count: usize,
    /// Wall-clock duration of targeted synchronization, in microseconds.
    pub targeted_sync_elapsed_us: u64,
}

impl ScanStats {
    pub(crate) fn merge_deferred_hashes(&mut self, mut deferred: Self) {
        self.hashes_computed += deferred.hashes_computed;
        self.updated += deferred.updated;
        self.content_changed += deferred.content_changed;
        self.content_audit = deferred.content_audit.take().or(self.content_audit.take());
        self.hashes_pending = self.hashes_pending.saturating_sub(deferred.hashes_computed);
        if self.targeted_manifest_query_count > 0 || deferred.targeted_manifest_query_count > 0 {
            self.targeted_manifest_rows_read = self
                .targeted_manifest_rows_read
                .saturating_add(deferred.targeted_manifest_rows_read);
            self.targeted_manifest_query_count = self
                .targeted_manifest_query_count
                .saturating_add(deferred.targeted_manifest_query_count);
        }
        self.renames_reconciled += deferred.renames_reconciled;
        self.pending_renames_considered += deferred.pending_renames_considered;
        self.pending_renames_pruned += deferred.pending_renames_pruned;
        self.pending_rename_diagnostics = deferred
            .pending_rename_diagnostics
            .take()
            .or(self.pending_rename_diagnostics.take());
        self.updated_samples.append(&mut deferred.updated_samples);
        self.renamed_samples.append(&mut deferred.renamed_samples);
        self.changed_samples.append(&mut deferred.changed_samples);
        self.committed_source_index_delta
            .merge(deferred.committed_source_index_delta);
        let has_manifest_snapshot =
            !self.manifest_before.is_empty() || !self.manifest_after.is_empty();
        if !has_manifest_snapshot {
            self.manifest_updates.extend(deferred.manifest_updates);
            self.committed_delta
                .created
                .extend(deferred.committed_delta.created);
            self.committed_delta
                .changed
                .extend(deferred.committed_delta.changed);
            self.committed_delta
                .moved
                .extend(deferred.committed_delta.moved);
            self.committed_delta
                .deleted
                .extend(deferred.committed_delta.deleted);
            self.committed_delta.revision = deferred.committed_delta.revision;
        } else if !deferred.manifest_after.is_empty() {
            self.manifest_after = deferred.manifest_after;
            self.committed_delta = super::super::manifest::build_targeted_committed_delta(
                &self.manifest_before,
                &self.manifest_after,
                deferred.committed_delta.revision,
                &self.renamed_samples,
            );
        } else if !deferred.manifest_updates.is_empty() {
            for update in deferred.manifest_updates {
                if let Ok(index) = self
                    .manifest_after
                    .binary_search_by(|entry| entry.relative_path.cmp(&update.relative_path))
                {
                    self.manifest_after[index] = update;
                } else {
                    let index = self
                        .manifest_after
                        .partition_point(|entry| entry.relative_path < update.relative_path);
                    self.manifest_after.insert(index, update);
                }
            }
            self.committed_delta = super::super::manifest::build_targeted_committed_delta(
                &self.manifest_before,
                &self.manifest_after,
                deferred.committed_delta.revision,
                &self.renamed_samples,
            );
        } else if deferred.committed_delta.revision > 0 {
            if self.committed_delta.revision == 0 {
                self.committed_delta = deferred.committed_delta;
            } else {
                self.committed_delta.revision = deferred.committed_delta.revision;
            }
        }
        if deferred.source_tree_snapshot.is_some() {
            self.source_tree_snapshot = deferred.source_tree_snapshot;
        }
    }

    pub(crate) fn record_rename_candidate(&mut self, path: PathBuf) {
        self.rename_candidate_paths.push(path);
    }
}

/// Typed source-index changes published only after their owning source write commits.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct CommittedSourceIndexDelta {
    /// Generic source revision assigned by the bounded write that changed the index.
    pub revision: u64,
    /// Source-index revision assigned by that same bounded write.
    pub index_revision: u64,
    /// Visible typed rows upserted by the committed write.
    pub upserted_entries: Vec<SourceIndexEntry>,
    /// Visible typed rows removed by the committed write.
    pub removed_paths: Vec<PathBuf>,
}

impl CommittedSourceIndexDelta {
    /// Return true when no source-index projection change was committed.
    pub fn is_empty(&self) -> bool {
        self.upserted_entries.is_empty() && self.removed_paths.is_empty()
    }

    pub(crate) fn record_commit(
        &mut self,
        revision: u64,
        index_revision: u64,
        upserted_entries: Vec<SourceIndexEntry>,
        removed_paths: Vec<PathBuf>,
    ) {
        if self.is_empty() {
            self.revision = revision;
            self.index_revision = index_revision;
        } else if self.revision != revision || self.index_revision != index_revision {
            // A single scan must not publish facts from multiple source revisions as one
            // targeted projection. Preserve the facts for diagnostics, but poison the
            // authority so the browser worker falls back to full recovery.
            self.revision = 0;
            self.index_revision = 0;
        }
        self.upserted_entries.extend(upserted_entries);
        self.removed_paths.extend(removed_paths);
    }

    fn merge(&mut self, deferred: Self) {
        if deferred.is_empty() {
            return;
        }
        self.record_commit(
            deferred.revision,
            deferred.index_revision,
            deferred.upserted_entries,
            deferred.removed_paths,
        );
    }
}

/// One current or retired identity in a committed source-manifest delta.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManifestIdentityDelta {
    /// Stable identity used to fence downstream work.
    pub identity: String,
    /// Source-relative path at this revision.
    pub relative_path: PathBuf,
    /// Full hash or explicit pending generation for this identity.
    pub content_generation: String,
    /// Whether source-visible size or modification metadata changed.
    pub source_metadata_changed: bool,
}

/// One identity whose committed source-relative path changed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MovedManifestIdentity {
    /// Stable identity used to fence downstream work.
    pub identity: String,
    /// Previous source-relative path.
    pub old_relative_path: PathBuf,
    /// Current source-relative path.
    pub new_relative_path: PathBuf,
    /// Current full hash or explicit pending generation.
    pub content_generation: String,
}

/// Structured source-manifest delta published only after the authoritative commit.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct CommittedSourceDelta {
    /// Monotonic committed source-path revision.
    pub revision: u64,
    /// Identities newly present at this revision.
    pub created: Vec<ManifestIdentityDelta>,
    /// Identities whose content generation changed at this revision.
    pub changed: Vec<ManifestIdentityDelta>,
    /// Identities whose path changed without losing stable ownership.
    pub moved: Vec<MovedManifestIdentity>,
    /// Identities no longer present at this revision.
    pub deleted: Vec<ManifestIdentityDelta>,
}

impl CommittedSourceDelta {
    /// Return true when the committed manifest did not change.
    pub fn is_empty(&self) -> bool {
        self.created.is_empty()
            && self.changed.is_empty()
            && self.moved.is_empty()
            && self.deleted.is_empty()
    }
}

/// Metadata describing a sample whose tracked file facts changed without moving.
#[derive(Debug, Clone)]
pub struct UpdatedSample {
    /// Path relative to the source root.
    pub relative_path: PathBuf,
    /// File size in bytes.
    pub file_size: u64,
    /// Last modified timestamp in epoch nanoseconds.
    pub modified_ns: i64,
    /// Updated content hash when the scan computed one.
    pub content_hash: Option<String>,
}

/// Metadata describing a sample whose path was reconciled as a rename.
#[derive(Debug, Clone)]
pub struct RenamedSample {
    /// Previous path relative to the source root.
    pub old_relative_path: PathBuf,
    /// Current path relative to the source root.
    pub new_relative_path: PathBuf,
    /// File size in bytes at the current path.
    pub file_size: u64,
    /// Last modified timestamp in epoch nanoseconds at the current path.
    pub modified_ns: i64,
    /// Updated content hash when the scan computed or reused one.
    pub content_hash: Option<String>,
}

/// Metadata describing a sample whose content changed.
#[derive(Debug, Clone)]
pub struct ChangedSample {
    /// Path relative to the source root.
    pub relative_path: PathBuf,
    /// File size in bytes.
    pub file_size: u64,
    /// Last modified timestamp in epoch nanoseconds.
    pub modified_ns: i64,
    /// Updated content hash.
    pub content_hash: String,
}
