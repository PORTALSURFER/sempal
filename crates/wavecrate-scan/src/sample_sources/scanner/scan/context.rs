use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::PathBuf;

use crate::sample_sources::SourceDatabase;
use crate::sample_sources::db::{SourceIndexEntry, SourceWriteBatch, WavEntry};
use wavecrate_library::sample_sources::{SourceManifestEntry, SourceTraversalPolicy};

use super::{ScanError, ScanMode, ScanStats};

const MANIFEST_AUDIT_CHECKPOINT_SIZE: usize = 64;

pub(crate) struct ScanContext {
    pub(crate) existing: HashMap<PathBuf, WavEntry>,
    pub(crate) stats: ScanStats,
    pub(crate) mode: ScanMode,
    pub(crate) rename_candidate_generation: Option<u64>,
    existing_index_entries: BTreeMap<PathBuf, SourceIndexEntry>,
    observed_index_entries: BTreeMap<PathBuf, SourceIndexEntry>,
    committed_manifest: BTreeMap<PathBuf, SourceManifestEntry>,
    committed_manifest_revision: u64,
    pub(crate) last_committed_revision: Option<u64>,
    manifest_audit: Option<ManifestAuditCheckpoint>,
    targeted_manifest_scope: Option<Vec<PathBuf>>,
    source_tree_incomplete: bool,
    uncertain_prefixes: BTreeSet<PathBuf>,
    traversal_policy: SourceTraversalPolicy,
}

struct ManifestAuditCheckpoint {
    revalidation_pending: HashSet<PathBuf>,
    revalidated_pending: Vec<PathBuf>,
    revalidation_remaining: usize,
    pending: Vec<PathBuf>,
    expected_total: usize,
}

impl ScanContext {
    pub(super) fn new(
        db: &SourceDatabase,
        mode: ScanMode,
        manifest_revision: u64,
        manifest: Vec<SourceManifestEntry>,
    ) -> Result<Self, ScanError> {
        let existing = index_existing(db)?;
        let mut context = Self::from_existing(existing, mode, manifest_revision, manifest);
        context.existing_index_entries = db
            .list_source_index_entries()?
            .into_iter()
            .map(|entry| (entry.relative_path.clone(), entry))
            .collect();
        Ok(context)
    }

    pub(in crate::sample_sources::scanner) fn from_existing(
        existing: HashMap<PathBuf, WavEntry>,
        mode: ScanMode,
        manifest_revision: u64,
        manifest: Vec<SourceManifestEntry>,
    ) -> Self {
        Self {
            existing,
            stats: ScanStats::default(),
            mode,
            rename_candidate_generation: None,
            existing_index_entries: BTreeMap::new(),
            observed_index_entries: BTreeMap::new(),
            committed_manifest: manifest
                .into_iter()
                .map(|entry| (entry.relative_path.clone(), entry))
                .collect(),
            committed_manifest_revision: manifest_revision,
            last_committed_revision: None,
            manifest_audit: None,
            targeted_manifest_scope: None,
            source_tree_incomplete: false,
            uncertain_prefixes: BTreeSet::new(),
            traversal_policy: SourceTraversalPolicy::default(),
        }
    }

    pub(in crate::sample_sources::scanner) fn set_traversal_policy(
        &mut self,
        policy: SourceTraversalPolicy,
    ) {
        self.traversal_policy = policy;
    }

    pub(in crate::sample_sources::scanner) fn traversal_policy(&self) -> SourceTraversalPolicy {
        self.traversal_policy
    }

    pub(in crate::sample_sources::scanner) fn set_targeted_manifest_scope(
        &mut self,
        paths: Vec<PathBuf>,
    ) {
        self.targeted_manifest_scope = Some(
            paths
                .into_iter()
                .filter(|path| path.to_str().is_some())
                .collect(),
        );
    }

    pub(in crate::sample_sources::scanner) fn refresh_targeted_manifest(
        &mut self,
        db: &SourceDatabase,
        extra_paths: impl IntoIterator<Item = PathBuf>,
        expected_revision: Option<u64>,
    ) -> Result<(), ScanError> {
        let Some(scope) = self.targeted_manifest_scope.as_ref() else {
            return Ok(());
        };
        let mut paths = scope.clone();
        paths.extend(extra_paths);
        let (revision, entries) = db.manifest_snapshot_with_revision_under_paths(&paths)?;
        if let Some(expected) = expected_revision
            && revision != expected
        {
            return Err(ScanError::StaleRevision {
                expected,
                actual: revision,
            });
        }
        self.stats.targeted_manifest_rows_read = self
            .stats
            .targeted_manifest_rows_read
            .saturating_add(entries.len());
        self.stats.targeted_manifest_query_count =
            self.stats.targeted_manifest_query_count.saturating_add(1);
        self.committed_manifest = entries
            .into_iter()
            .map(|entry| (entry.relative_path.clone(), entry))
            .collect();
        self.committed_manifest_revision = revision;
        Ok(())
    }

    pub(in crate::sample_sources::scanner) fn set_targeted_index_entries(
        &mut self,
        existing: impl IntoIterator<Item = SourceIndexEntry>,
        observed: impl IntoIterator<Item = SourceIndexEntry>,
    ) {
        self.existing_index_entries = existing
            .into_iter()
            .map(|entry| (entry.relative_path.clone(), entry))
            .collect();
        self.observe_index_entries(observed);
    }

    pub(in crate::sample_sources::scanner) fn observe_index_entries(
        &mut self,
        entries: impl IntoIterator<Item = SourceIndexEntry>,
    ) {
        self.observed_index_entries.extend(
            entries
                .into_iter()
                .map(|entry| (entry.relative_path.clone(), entry)),
        );
    }

    pub(in crate::sample_sources::scanner) fn take_index_reconciliation(
        &mut self,
    ) -> (
        BTreeMap<PathBuf, SourceIndexEntry>,
        BTreeMap<PathBuf, SourceIndexEntry>,
    ) {
        (
            std::mem::take(&mut self.existing_index_entries),
            std::mem::take(&mut self.observed_index_entries),
        )
    }

    pub(in crate::sample_sources::scanner) fn mark_source_tree_incomplete(&mut self) {
        self.source_tree_incomplete = true;
    }

    pub(in crate::sample_sources::scanner) fn source_tree_incomplete(&self) -> bool {
        self.source_tree_incomplete
    }

    /// Record a relative prefix whose descendants could not be observed.
    /// Missing-row reconciliation must leave every existing row below this
    /// boundary untouched until a later authoritative traversal succeeds.
    pub(in crate::sample_sources::scanner) fn mark_uncertain_prefixes(
        &mut self,
        prefixes: impl IntoIterator<Item = PathBuf>,
    ) {
        let mut recorded = false;
        for prefix in prefixes {
            if self.uncertain_prefixes.insert(prefix) {
                recorded = true;
            }
        }
        if recorded {
            self.mark_source_tree_incomplete();
        }
    }

    pub(in crate::sample_sources::scanner) fn preserves_missing_row(
        &self,
        path: &std::path::Path,
    ) -> bool {
        self.uncertain_prefixes
            .iter()
            .any(|prefix| path.starts_with(prefix))
    }

    pub(in crate::sample_sources::scanner) fn has_uncertain_prefixes(&self) -> bool {
        !self.uncertain_prefixes.is_empty()
    }

    pub(in crate::sample_sources::scanner) fn uncertainty_error(&self) -> String {
        format!(
            "source traversal could not enumerate {} subtree(s); retry required",
            self.uncertain_prefixes.len()
        )
    }

    pub(in crate::sample_sources::scanner) fn resume_manifest_audit(
        &mut self,
        db: &SourceDatabase,
        started_at: i64,
    ) -> Result<(), ScanError> {
        let (paths, checked_files) =
            db.begin_or_resume_manifest_audit_batch(started_at, MANIFEST_AUDIT_CHECKPOINT_SIZE)?;
        let revalidation_pending = paths.into_iter().collect::<HashSet<_>>();
        self.stats.total_files = checked_files;
        self.manifest_audit = Some(ManifestAuditCheckpoint {
            expected_total: self.committed_manifest.len().max(checked_files),
            revalidation_remaining: checked_files,
            revalidation_pending,
            revalidated_pending: Vec::new(),
            pending: Vec::new(),
        });
        Ok(())
    }

    pub(in crate::sample_sources::scanner) fn manifest_audit_progress(
        &self,
    ) -> Option<(usize, usize)> {
        self.manifest_audit.as_ref().map(|audit| {
            (
                self.stats.total_files,
                audit.expected_total.max(self.stats.total_files),
            )
        })
    }

    pub(in crate::sample_sources::scanner) fn manifest_audit_revalidates_path(
        &self,
        relative_path: &std::path::Path,
    ) -> bool {
        self.manifest_audit
            .as_ref()
            .is_some_and(|audit| audit.revalidation_pending.contains(relative_path))
    }

    pub(in crate::sample_sources::scanner) fn record_manifest_audit_paths(
        &mut self,
        paths: impl IntoIterator<Item = PathBuf>,
    ) {
        let Some(audit) = self.manifest_audit.as_mut() else {
            return;
        };
        for path in paths {
            if audit.revalidation_pending.remove(&path) {
                audit.revalidated_pending.push(path);
            } else {
                audit.pending.push(path);
            }
        }
    }

    pub(in crate::sample_sources::scanner) fn manifest_audit_checkpoint_due(&self) -> bool {
        self.manifest_audit
            .as_ref()
            .is_some_and(|audit| audit.pending.len() >= MANIFEST_AUDIT_CHECKPOINT_SIZE)
    }

    pub(in crate::sample_sources::scanner) fn discard_manifest_audit_paths(
        &mut self,
        paths: impl IntoIterator<Item = PathBuf>,
    ) {
        let Some(audit) = self.manifest_audit.as_mut() else {
            return;
        };
        let paths = paths.into_iter().collect::<HashSet<_>>();
        audit
            .pending
            .retain(|path| if paths.contains(path) { false } else { true });
        let revalidated = std::mem::take(&mut audit.revalidated_pending);
        for path in revalidated {
            if paths.contains(&path) {
                audit.revalidation_pending.insert(path);
            } else {
                audit.revalidated_pending.push(path);
            }
        }
    }

    pub(in crate::sample_sources::scanner) fn flush_manifest_audit_checkpoint(
        &mut self,
        db: &SourceDatabase,
    ) -> Result<(), ScanError> {
        let Some(audit) = self.manifest_audit.as_mut() else {
            return Ok(());
        };
        let pending = std::mem::take(&mut audit.pending);
        let revalidated = std::mem::take(&mut audit.revalidated_pending);
        let inserted = db.checkpoint_manifest_audit_paths_with_count(&pending)?;
        db.clear_manifest_audit_paths(&revalidated)?;
        audit.revalidation_remaining = audit
            .revalidation_remaining
            .saturating_sub(revalidated.len());
        self.stats.total_files = self.stats.total_files.saturating_add(inserted);
        Ok(())
    }

    pub(in crate::sample_sources::scanner) fn complete_missing_manifest_audit_paths(&mut self) {
        let Some(missing) = self.manifest_audit.as_ref().map(|audit| {
            audit
                .revalidation_pending
                .iter()
                .filter(|path| self.existing.contains_key(*path))
                .cloned()
                .collect::<Vec<_>>()
        }) else {
            return;
        };
        let audit = self.manifest_audit.as_mut().expect("audit exists");
        for path in missing {
            audit.revalidation_pending.remove(&path);
            audit.revalidated_pending.push(path);
        }
    }

    pub(in crate::sample_sources::scanner) fn manifest_audit_revalidation_pending(&self) -> bool {
        self.manifest_audit.as_ref().is_some_and(|audit| {
            audit.revalidation_remaining > 0 || !audit.revalidation_pending.is_empty()
        })
    }

    pub(in crate::sample_sources::scanner) fn resumable_manifest_audit_active(&self) -> bool {
        self.manifest_audit.is_some()
    }

    pub(in crate::sample_sources::scanner) fn ensure_rename_candidate_generation(
        &mut self,
        batch: &mut SourceWriteBatch<'_>,
    ) -> Result<(), ScanError> {
        if self.rename_candidate_generation.is_some() {
            return Ok(());
        }
        let generation = match self.mode {
            ScanMode::Targeted => batch.begin_targeted_scan_generation()?,
            ScanMode::Quick => batch.begin_quick_scan_rename_candidates()?,
            // Hard rescans clean transient candidates at completion. A
            // placeholder generation lets newly discovered paths be staged
            // without changing scan metadata on otherwise unchanged scans.
            ScanMode::Hard => 0,
        };
        self.rename_candidate_generation = Some(generation);
        Ok(())
    }

    pub(in crate::sample_sources::scanner) fn committed_file_identity(
        &self,
        relative_path: &std::path::Path,
    ) -> Option<&str> {
        self.committed_manifest
            .get(relative_path)
            .and_then(|entry| entry.file_identity.as_deref())
    }

    pub(in crate::sample_sources::scanner) fn has_committed_manifest_path(
        &self,
        relative_path: &std::path::Path,
    ) -> bool {
        self.committed_manifest.contains_key(relative_path)
    }

    pub(in crate::sample_sources::scanner) fn commit_batch(
        &mut self,
        db: &SourceDatabase,
        batch: SourceWriteBatch<'_>,
    ) -> Result<u64, ScanError> {
        self.commit_batch_with_post_commit_hook(db, batch, || {})
    }

    fn commit_batch_with_post_commit_hook(
        &mut self,
        db: &SourceDatabase,
        batch: SourceWriteBatch<'_>,
        post_commit_hook: impl FnOnce(),
    ) -> Result<u64, ScanError> {
        if !batch.matches_source_traversal_policy(self.traversal_policy)? {
            return Err(ScanError::TraversalPolicyChanged);
        }
        let expected_revision = self.committed_manifest_revision;
        let result = if self.mode == ScanMode::Targeted {
            if !batch.matches_revision(expected_revision)? {
                drop(batch);
                return Err(ScanError::StaleRevision {
                    expected: expected_revision,
                    actual: db.get_revision()?,
                });
            }
            batch.commit_with_bounded_manifest_changes(expected_revision)?
        } else {
            batch.commit_with_manifest_changes(expected_revision)?
        };
        post_commit_hook();
        if let Some(snapshot) = result.authoritative_snapshot {
            self.committed_manifest = snapshot
                .into_iter()
                .map(|entry| (entry.relative_path.clone(), entry))
                .collect();
        } else {
            for (path, entry) in result.touched_path_changes {
                if let Some(entry) = entry {
                    self.committed_manifest.insert(path, entry);
                } else {
                    self.committed_manifest.remove(&path);
                }
            }
        }
        self.committed_manifest_revision = result.revision;
        self.last_committed_revision = Some(result.revision);
        Ok(result.revision)
    }

    pub(in crate::sample_sources::scanner) fn committed_snapshot(
        &self,
        revision: u64,
    ) -> (u64, Vec<SourceManifestEntry>) {
        (
            revision,
            self.committed_manifest.values().cloned().collect(),
        )
    }

    pub(in crate::sample_sources::scanner) fn latest_committed_snapshot(
        &self,
    ) -> (u64, Vec<SourceManifestEntry>) {
        self.committed_snapshot(self.committed_manifest_revision)
    }
}

fn index_existing(db: &SourceDatabase) -> Result<HashMap<PathBuf, WavEntry>, ScanError> {
    let entries = db.list_files()?;
    Ok(entries
        .into_iter()
        .map(|entry| (entry.relative_path.clone(), entry))
        .collect())
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    #[test]
    fn interleaved_manifest_writer_forces_exact_committed_resnapshot() {
        let directory = tempfile::tempdir().expect("source root");
        let database = SourceDatabase::open_for_scan(directory.path()).expect("source database");
        let mut initial_batch = database.write_batch().expect("initial batch");
        initial_batch
            .upsert_file_with_hash(Path::new("initial.wav"), 1, 1, "initial")
            .expect("initial row");
        initial_batch.commit().expect("commit initial row");
        let (manifest_revision, manifest) = database
            .manifest_snapshot_with_revision()
            .expect("initial manifest");
        let existing = index_existing(&database).expect("existing rows");
        let mut context =
            ScanContext::from_existing(existing, ScanMode::Quick, manifest_revision, manifest);

        let mut external_batch = database.write_batch().expect("external batch");
        external_batch
            .upsert_file_with_hash(Path::new("external.wav"), 2, 2, "external")
            .expect("interleaved writer");
        external_batch.commit().expect("commit interleaved writer");
        let mut scan_batch = database.write_batch().expect("scan batch");
        scan_batch
            .upsert_file_with_hash(Path::new("scan.wav"), 3, 3, "scan")
            .expect("scan row");
        let revision = context
            .commit_batch_with_post_commit_hook(&database, scan_batch, || {
                let mut later_batch = database.write_batch().expect("later batch");
                later_batch
                    .upsert_file_with_hash(Path::new("later.wav"), 4, 4, "later")
                    .expect("later row");
                later_batch.commit().expect("commit later writer");
            })
            .expect("commit scan batch");
        let (_revision, snapshot) = context.committed_snapshot(revision);
        let paths = snapshot
            .into_iter()
            .map(|entry| entry.relative_path)
            .collect::<Vec<_>>();

        assert!(revision < database.get_revision().expect("current revision"));
        assert_eq!(
            paths,
            vec![
                PathBuf::from("external.wav"),
                PathBuf::from("initial.wav"),
                PathBuf::from("scan.wav"),
            ]
        );
    }

    #[test]
    fn targeted_commit_reports_stale_revision_after_an_interleaved_writer() {
        let directory = tempfile::tempdir().expect("source root");
        let database = SourceDatabase::open_for_scan(directory.path()).expect("source database");
        let mut initial_batch = database.write_batch().expect("initial batch");
        initial_batch
            .upsert_file_with_hash(Path::new("initial.wav"), 1, 1, "initial")
            .expect("initial row");
        initial_batch.commit().expect("commit initial row");
        let (manifest_revision, manifest) = database
            .manifest_snapshot_with_revision_under_paths(&[PathBuf::from("initial.wav")])
            .expect("initial manifest");
        let existing = index_existing(&database).expect("existing rows");
        let mut context =
            ScanContext::from_existing(existing, ScanMode::Targeted, manifest_revision, manifest);

        let mut first_batch = database.write_batch().expect("first scan batch");
        first_batch
            .upsert_file_with_hash(Path::new("scan.wav"), 2, 2, "scan")
            .expect("scan row");
        context
            .commit_batch_with_post_commit_hook(&database, first_batch, || {
                database
                    .upsert_file(Path::new("external.wav"), 3, 3)
                    .expect("interleaved writer");
            })
            .expect("first scan commit");

        let mut second_batch = database.write_batch().expect("second scan batch");
        second_batch
            .upsert_file_with_hash(Path::new("later.wav"), 4, 4, "later")
            .expect("later scan row");
        let result = context.commit_batch(&database, second_batch);

        assert!(matches!(result, Err(ScanError::StaleRevision { .. })));
    }
}
