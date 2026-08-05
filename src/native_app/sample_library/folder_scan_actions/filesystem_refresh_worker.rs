use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
    thread,
    time::Duration,
};

use wavecrate::sample_sources::{Rating, SourceDatabase, SourceIndexClassification};
use wavecrate_library::filesystem_identity::stable_filesystem_identity;
use wavecrate_library::sample_sources::reconciliation::{
    ReconciliationScope, ReconciliationScopeKind, RootRelativePath,
};
use wavecrate_scan::sample_sources::scanner::{
    self, ScanWritePhase, ScanWriter, UncoordinatedScanWriter,
};

use crate::native_app::{
    app::{
        BrowserProjectionDelta, SourceFilesystemSyncAuditReason, SourceFilesystemSyncResult,
        SourceFilesystemSyncSuccess,
    },
    sample_library::folder_browser::model::file_entry_with_snapshot_metadata,
    sample_library::source_watcher::WatcherContinuityProof,
};

const MAX_SYNC_ATTEMPTS: usize = 3;
const SYNC_RETRY_DELAYS: [Duration; MAX_SYNC_ATTEMPTS - 1] =
    [Duration::from_millis(50), Duration::from_millis(200)];

pub(in crate::native_app) fn recover_source_filesystem_sync(
    source_id: String,
    lifecycle_generation: u64,
    changed_count: usize,
    work: impl FnOnce() -> SourceFilesystemSyncResult,
) -> SourceFilesystemSyncResult {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(work)) {
        Ok(mut result) => {
            result.lifecycle_generation = lifecycle_generation;
            result
        }
        Err(_) => SourceFilesystemSyncResult {
            source_id,
            lifecycle_generation,
            changed_count,
            root_identity: None,
            journal_checkpoint_event_id: None,
            watcher_continuity_proof: None,
            cancelled: false,
            audit_required: Some(SourceFilesystemSyncAuditReason::WorkerPanic),
            result: Err(String::from(
                "Source filesystem sync worker stopped unexpectedly",
            )),
        },
    }
}

pub(in crate::native_app) fn sync_source_database_paths(
    source_id: String,
    root: PathBuf,
    database_root: PathBuf,
    paths: Vec<PathBuf>,
    changed_count: usize,
    cancel: &AtomicBool,
) -> SourceFilesystemSyncResult {
    sync_source_database_paths_with_writer(
        source_id,
        root,
        database_root,
        paths,
        changed_count,
        cancel,
        None,
        &UncoordinatedScanWriter,
    )
}

pub(in crate::native_app) fn capture_source_root_identity(root: &Path) -> Option<String> {
    std::fs::metadata(root)
        .ok()
        .and_then(|metadata| stable_filesystem_identity(root, &metadata))
}

pub(in crate::native_app) fn root_identity_matches_watcher_proof(
    root_identity: Option<&str>,
    watcher_continuity_proof: Option<&WatcherContinuityProof>,
) -> bool {
    root_identity.is_some_and(|root_identity| {
        watcher_continuity_proof.is_some_and(|proof| root_identity == proof.root_identity)
    })
}

/// Run targeted database work only after the worker has established that the captured source root
/// is the root named by the watcher replay evidence. The database worker performs a second live
/// identity check at its own boundary before opening the source database.
pub(in crate::native_app) fn run_targeted_sync_after_root_identity_gate(
    source_id: String,
    lifecycle_generation: u64,
    changed_count: usize,
    root_identity: Option<String>,
    journal_checkpoint_event_id: Option<u64>,
    watcher_continuity_proof: Option<WatcherContinuityProof>,
    work: impl FnOnce() -> SourceFilesystemSyncResult,
) -> SourceFilesystemSyncResult {
    if !root_identity_matches_watcher_proof(
        root_identity.as_deref(),
        watcher_continuity_proof.as_ref(),
    ) {
        return SourceFilesystemSyncResult {
            source_id,
            lifecycle_generation,
            changed_count,
            root_identity,
            journal_checkpoint_event_id,
            watcher_continuity_proof,
            cancelled: false,
            audit_required: Some(SourceFilesystemSyncAuditReason::RootIdentityUncertain),
            result: Err(String::from(
                "Targeted source sync rejected because the captured source root identity is unavailable or does not match watcher replay evidence",
            )),
        };
    }

    let mut result = work();
    result.lifecycle_generation = lifecycle_generation;
    result.journal_checkpoint_event_id = journal_checkpoint_event_id;
    result.watcher_continuity_proof = watcher_continuity_proof;
    result
}

pub(in crate::native_app) fn sync_source_database_paths_with_writer(
    source_id: String,
    root: PathBuf,
    database_root: PathBuf,
    paths: Vec<PathBuf>,
    changed_count: usize,
    cancel: &AtomicBool,
    watcher_continuity_proof: Option<WatcherContinuityProof>,
    writer: &impl ScanWriter,
) -> SourceFilesystemSyncResult {
    let root_identity = capture_source_root_identity(&root);
    if watcher_continuity_proof.is_some()
        && !root_identity_matches_watcher_proof(
            root_identity.as_deref(),
            watcher_continuity_proof.as_ref(),
        )
    {
        return SourceFilesystemSyncResult {
            source_id,
            lifecycle_generation: 0,
            changed_count,
            root_identity,
            journal_checkpoint_event_id: None,
            watcher_continuity_proof,
            cancelled: cancel.load(Ordering::Acquire),
            audit_required: Some(SourceFilesystemSyncAuditReason::RootIdentityUncertain),
            result: Err(String::from(
                "Targeted source sync rejected because the live source root identity is unavailable or does not match watcher replay evidence",
            )),
        };
    }
    let mut result = Err(String::from("Source filesystem sync did not run"));
    let mut observed_root_identity = root_identity;
    for attempt in 0..MAX_SYNC_ATTEMPTS {
        let sync_attempt = sync_source_database_paths_once(
            &source_id,
            &root,
            &database_root,
            &paths,
            cancel,
            watcher_continuity_proof.as_ref(),
            writer,
        );
        observed_root_identity = sync_attempt.root_identity;
        result = sync_attempt.result;
        if result.is_ok() || cancel.load(Ordering::Acquire) || !sync_attempt.retryable {
            break;
        }
        let Some(delay) = SYNC_RETRY_DELAYS.get(attempt).copied() else {
            break;
        };
        tracing::warn!(
            source_id,
            attempt = attempt + 1,
            max_attempts = MAX_SYNC_ATTEMPTS,
            delay_ms = delay.as_millis(),
            error = %result.as_ref().expect_err("failed attempt"),
            "Retrying targeted source sync"
        );
        if !wait_for_retry(cancel, delay) {
            break;
        }
    }
    let cancelled = cancel.load(Ordering::Acquire);
    SourceFilesystemSyncResult {
        source_id,
        lifecycle_generation: 0,
        changed_count,
        root_identity: observed_root_identity,
        journal_checkpoint_event_id: None,
        watcher_continuity_proof,
        cancelled,
        audit_required: cancelled.then_some(SourceFilesystemSyncAuditReason::Cancelled),
        result,
    }
}

pub(in crate::native_app) fn sync_source_database_scopes_with_writer(
    source_id: String,
    root: PathBuf,
    database_root: PathBuf,
    scopes: Vec<ReconciliationScope>,
    changed_count: usize,
    source_root_identity: Option<String>,
    cancel: &AtomicBool,
    watcher_continuity_proof: Option<WatcherContinuityProof>,
    writer: &impl ScanWriter,
) -> SourceFilesystemSyncResult {
    let root_identity = capture_source_root_identity(&root);
    let cancelled = cancel.load(Ordering::Acquire);
    let audit_required = if cancelled {
        Some(SourceFilesystemSyncAuditReason::Cancelled)
    } else if scopes.is_empty() {
        Some(SourceFilesystemSyncAuditReason::ScopeLost)
    } else if scopes
        .iter()
        .any(|scope| scope.kind() == ReconciliationScopeKind::SourceAudit)
    {
        Some(SourceFilesystemSyncAuditReason::SourceAuditScope)
    } else if !root_identity_matches_watcher_proof(
        root_identity.as_deref(),
        watcher_continuity_proof.as_ref(),
    ) || source_root_identity.as_deref()
        != watcher_continuity_proof
            .as_ref()
            .map(|proof| proof.root_identity.as_str())
    {
        Some(SourceFilesystemSyncAuditReason::RootIdentityUncertain)
    } else if !scopes
        .iter()
        .all(|scope| scope.kind() == ReconciliationScopeKind::ExactEntry && scope.path().is_some())
    {
        Some(SourceFilesystemSyncAuditReason::TypedScopeDispatchUnavailable)
    } else {
        None
    };
    if let Some(audit_required) = audit_required {
        return SourceFilesystemSyncResult {
            source_id,
            lifecycle_generation: 0,
            changed_count,
            root_identity,
            journal_checkpoint_event_id: None,
            watcher_continuity_proof,
            cancelled,
            audit_required: Some(audit_required),
            result: Err(format!(
                "Typed reconciliation scope dispatch requires source audit: {}",
                audit_required.label()
            )),
        };
    }

    let exact_paths = scopes
        .iter()
        .map(|scope| {
            scope
                .path()
                .expect("validated exact scope must carry a path")
                .clone()
        })
        .collect::<Vec<_>>();
    if let Err(error) = scanner::validate_exact_regular_paths(&root, &exact_paths, Some(cancel)) {
        let audit_required = exact_scope_preflight_audit_reason(&root, &error, cancel);
        return SourceFilesystemSyncResult {
            source_id,
            lifecycle_generation: 0,
            changed_count,
            root_identity: capture_source_root_identity(&root),
            journal_checkpoint_event_id: None,
            watcher_continuity_proof,
            cancelled: cancel.load(Ordering::Acquire),
            audit_required: Some(audit_required),
            result: Err(format!("Exact scope preflight failed: {error}")),
        };
    }

    let attempt = sync_source_database_target_once(
        &source_id,
        &root,
        &database_root,
        SourceDatabaseSyncTarget::ExactEntries(&exact_paths),
        cancel,
        watcher_continuity_proof.as_ref(),
        writer,
    );
    let cancelled = cancel.load(Ordering::Acquire);
    SourceFilesystemSyncResult {
        source_id,
        lifecycle_generation: 0,
        changed_count,
        root_identity: attempt.root_identity,
        journal_checkpoint_event_id: None,
        watcher_continuity_proof,
        cancelled,
        audit_required: cancelled.then_some(SourceFilesystemSyncAuditReason::Cancelled),
        result: attempt.result,
    }
}

struct SourceDatabaseSyncAttempt {
    root_identity: Option<String>,
    result: Result<SourceFilesystemSyncSuccess, String>,
    retryable: bool,
}

fn sync_source_database_paths_once(
    source_id: &str,
    root: &std::path::Path,
    database_root: &std::path::Path,
    paths: &[PathBuf],
    cancel: &AtomicBool,
    watcher_continuity_proof: Option<&WatcherContinuityProof>,
    writer: &impl ScanWriter,
) -> SourceDatabaseSyncAttempt {
    sync_source_database_target_once(
        source_id,
        root,
        database_root,
        SourceDatabaseSyncTarget::CompatibilityPaths(paths),
        cancel,
        watcher_continuity_proof,
        writer,
    )
}

#[derive(Clone, Copy)]
enum SourceDatabaseSyncTarget<'a> {
    CompatibilityPaths(&'a [PathBuf]),
    ExactEntries(&'a [RootRelativePath]),
}

impl SourceDatabaseSyncTarget<'_> {
    fn browser_delta_eligible(self) -> bool {
        match self {
            Self::CompatibilityPaths(paths) => paths.iter().all(|path| path.to_str().is_some()),
            Self::ExactEntries(paths) => paths.iter().all(|path| path.as_path().to_str().is_some()),
        }
    }
}

fn sync_source_database_target_once(
    source_id: &str,
    root: &std::path::Path,
    database_root: &std::path::Path,
    target: SourceDatabaseSyncTarget<'_>,
    cancel: &AtomicBool,
    watcher_continuity_proof: Option<&WatcherContinuityProof>,
    writer: &impl ScanWriter,
) -> SourceDatabaseSyncAttempt {
    // Browser IDs are UTF-8 paths. A raw path is still reconciled in the database, but it must
    // take the existing full-recovery path rather than enter a lossy incremental projection.
    let browser_delta_eligible = target.browser_delta_eligible();
    let _writer = writer.lock(ScanWritePhase::Open);
    let root_identity = capture_source_root_identity(root);
    if cancel.load(Ordering::Acquire) {
        return SourceDatabaseSyncAttempt {
            root_identity,
            result: Err(String::from(
                "Source filesystem sync canceled before database open",
            )),
            retryable: false,
        };
    }
    if watcher_continuity_proof.is_some()
        && !root_identity_matches_watcher_proof(root_identity.as_deref(), watcher_continuity_proof)
    {
        return SourceDatabaseSyncAttempt {
            root_identity,
            result: Err(String::from(
                "Targeted source sync rejected because the live source root identity is unavailable or does not match watcher replay evidence",
            )),
            retryable: false,
        };
    }
    let database = SourceDatabase::open_for_background_job_with_database_root(root, database_root);
    drop(_writer);
    let result = database
        .map_err(|err| format!("open source index: {err}"))
        .and_then(|db| {
            let scan_result = match target {
                SourceDatabaseSyncTarget::CompatibilityPaths(paths) => {
                    scanner::sync_paths_with_progress_and_writer(
                        &db,
                        paths,
                        Some(cancel),
                        &mut |_, _| {},
                        writer,
                    )
                }
                SourceDatabaseSyncTarget::ExactEntries(paths) => {
                    scanner::sync_exact_paths_with_progress_and_writer(
                        &db,
                        paths,
                        Some(cancel),
                        &mut |_, _| {},
                        writer,
                    )
                }
            };
            let (stats, mut incomplete_error) = match scan_result {
                Ok(stats) => (stats, None),
                Err(scanner::ScanError::Incomplete { committed, error }) => {
                    (*committed, Some(error))
                }
                Err(error) => return Err(format!("sync source index: {error}")),
            };
            let committed = stats.clone();
            let completed = if incomplete_error.is_some()
                || matches!(target, SourceDatabaseSyncTarget::ExactEntries(_))
            {
                committed
            } else {
                match scanner::complete_deferred_rename_candidates_with_cancel_and_writer(
                    &db,
                    stats,
                    Some(cancel),
                    writer,
                ) {
                    Ok(completed) => completed,
                    Err(error) => {
                        incomplete_error = Some(error.to_string());
                        tracing::warn!(
                            source_id,
                            error = %error,
                            "Deferred rename reconciliation failed after filesystem sync committed"
                        );
                        committed
                    }
                }
            };
            let browser_projection_delta = if browser_delta_eligible && incomplete_error.is_none() {
                match build_browser_projection_delta(
                    root,
                    &db,
                    &completed.committed_delta,
                    &completed.committed_source_index_delta,
                ) {
                    Ok(Some(projection)) => Some(projection),
                    Ok(None) => {
                        incomplete_error = Some(format!(
                            "browser projection was not available at committed revision {}",
                            completed.committed_delta.revision
                        ));
                        tracing::warn!(
                            source_id,
                            revision = completed.committed_delta.revision,
                            "Falling back to a full browser projection after exact revision hydration failed"
                        );
                        None
                    }
                    Err(error) => {
                        incomplete_error = Some(format!(
                            "browser projection hydration failed: {error}"
                        ));
                        tracing::warn!(
                            source_id,
                            error,
                            "Falling back to a full browser projection after delta hydration failed"
                        );
                        None
                    }
                }
            } else {
                None
            };
            Ok(SourceFilesystemSyncSuccess {
                renames_reconciled: completed.renames_reconciled,
                incomplete_error,
                committed_delta: completed.committed_delta,
                committed_source_index_delta: completed.committed_source_index_delta,
                browser_projection_delta,
                projection_handoff_ticket: None,
            })
        });
    SourceDatabaseSyncAttempt {
        root_identity,
        result,
        retryable: true,
    }
}

fn exact_scope_preflight_audit_reason(
    root: &Path,
    error: &scanner::ScanError,
    cancel: &AtomicBool,
) -> SourceFilesystemSyncAuditReason {
    if cancel.load(Ordering::Acquire) || matches!(error, scanner::ScanError::Canceled) {
        return SourceFilesystemSyncAuditReason::Cancelled;
    }
    let root_loss = matches!(
        error,
        scanner::ScanError::InvalidRoot(_) | scanner::ScanError::StaleRootGeneration { .. }
    ) || matches!(error, scanner::ScanError::Io { path, .. } if path == root);
    if root_loss {
        SourceFilesystemSyncAuditReason::RootIdentityUncertain
    } else {
        SourceFilesystemSyncAuditReason::ScopeLost
    }
}

fn build_browser_projection_delta(
    root: &std::path::Path,
    db: &SourceDatabase,
    delta: &scanner::CommittedSourceDelta,
    index_delta: &scanner::CommittedSourceIndexDelta,
) -> Result<Option<BrowserProjectionDelta>, String> {
    if !index_delta.is_empty()
        && (index_delta.revision != delta.revision || index_delta.index_revision == 0)
    {
        tracing::info!(
            source_revision = delta.revision,
            index_source_revision = index_delta.revision,
            index_revision = index_delta.index_revision,
            "Source-index projection facts are not bound to the final committed source revision"
        );
        return Ok(None);
    }
    let projection_paths = delta
        .created
        .iter()
        .map(|entry| entry.relative_path.clone())
        .chain(
            delta
                .changed
                .iter()
                .map(|entry| entry.relative_path.clone()),
        )
        .chain(
            delta
                .moved
                .iter()
                .map(|entry| entry.new_relative_path.clone()),
        )
        .collect::<Vec<_>>();
    let mut manifest_upsert_paths = BTreeSet::new();
    for path in &projection_paths {
        if !manifest_upsert_paths.insert(path.clone()) {
            return Err(format!(
                "duplicate supported browser upsert path {}",
                path.display()
            ));
        }
        ensure_unicode_projection_path(root, path)?;
    }
    let index_paths = index_delta
        .upserted_entries
        .iter()
        .map(|entry| entry.relative_path.clone())
        .chain(index_delta.removed_paths.iter().cloned())
        .collect::<BTreeSet<_>>();
    let index_paths = index_paths.into_iter().collect::<Vec<_>>();
    for path in &index_paths {
        ensure_unicode_projection_path(root, path)?;
    }
    for entry in &index_delta.upserted_entries {
        if !manifest_upsert_paths.insert(entry.relative_path.clone()) {
            return Err(format!(
                "supported and source-index browser upserts overlap at {}",
                entry.relative_path.display()
            ));
        }
    }

    let snapshot = db
        .browser_metadata_for_paths_at_revision(delta.revision, &projection_paths)
        .map_err(|error| format!("read committed browser projection delta: {error}"))?;
    let revision = snapshot.revision;
    let files = snapshot.files;
    if revision != delta.revision {
        tracing::info!(
            committed_revision = delta.revision,
            snapshot_revision = revision,
            "Browser delta snapshot was not the exact committed revision"
        );
        return Ok(None);
    }
    let mut folders = std::collections::BTreeSet::new();
    let mut upserted_files = Vec::new();
    let mut hydrated_manifest_paths = BTreeSet::new();
    for entry in files {
        if !entry.missing && manifest_upsert_paths.contains(&entry.relative_path) {
            let absolute = root.join(&entry.relative_path);
            if let Some(parent) = absolute.parent() {
                folders.insert(parent.to_path_buf());
            }
            hydrated_manifest_paths.insert(entry.relative_path.clone());
            upserted_files.push(file_entry_with_snapshot_metadata(
                &absolute,
                entry.file_size,
                entry.rating,
                entry.locked,
                entry.collections,
                entry.last_played_at,
                entry.last_curated_at,
            ));
        }
    }
    if hydrated_manifest_paths.len() != projection_paths.len() {
        return Err(String::from(
            "committed supported browser metadata did not hydrate every upsert",
        ));
    }

    if !index_delta.is_empty() {
        let index_snapshot = db
            .source_index_entries_for_paths_at_revision(index_delta.index_revision, &index_paths)
            .map_err(|error| format!("read committed source-index projection delta: {error}"))?;
        let actual = index_snapshot
            .entries
            .into_iter()
            .map(|entry| (entry.relative_path.clone(), entry))
            .collect::<BTreeMap<_, _>>();
        let expected = index_delta
            .upserted_entries
            .iter()
            .map(|entry| (entry.relative_path.clone(), entry))
            .collect::<BTreeMap<_, _>>();
        if actual.len() != expected.len()
            || expected
                .iter()
                .any(|(path, expected)| actual.get(path) != Some(expected))
            || index_delta
                .removed_paths
                .iter()
                .any(|path| actual.contains_key(path))
        {
            return Err(String::from(
                "committed source-index projection hydration did not match its write evidence",
            ));
        }
        for entry in &index_delta.upserted_entries {
            let Some(file_size) = entry.file_size else {
                return Err(format!(
                    "source-index upsert has no file size: {}",
                    entry.relative_path.display()
                ));
            };
            if matches!(
                entry.classification,
                SourceIndexClassification::Inaccessible
            ) {
                return Err(format!(
                    "inaccessible source-index row cannot be projected: {}",
                    entry.relative_path.display()
                ));
            }
            let absolute = root.join(&entry.relative_path);
            if let Some(parent) = absolute.parent() {
                folders.insert(parent.to_path_buf());
            }
            upserted_files.push(file_entry_with_snapshot_metadata(
                &absolute,
                file_size,
                Rating::NEUTRAL,
                false,
                Vec::new(),
                None,
                None,
            ));
        }
    }
    upserted_files.sort_by(|left, right| left.id.cmp(&right.id));

    let mut removed_file_ids = BTreeSet::new();
    for path in delta
        .deleted
        .iter()
        .map(|entry| &entry.relative_path)
        .chain(delta.moved.iter().map(|entry| &entry.old_relative_path))
        .chain(index_delta.removed_paths.iter())
    {
        removed_file_ids.insert(projected_path_id(root, path)?);
    }
    Ok(Some(BrowserProjectionDelta {
        manifest_revision: delta.revision,
        snapshot_revision: revision,
        folders: folders.into_iter().collect(),
        removed_file_ids: removed_file_ids.into_iter().collect(),
        upserted_files,
    }))
}

fn ensure_unicode_projection_path(root: &Path, relative_path: &Path) -> Result<(), String> {
    let absolute = root.join(relative_path);
    if absolute.to_str().is_none() {
        return Err(format!(
            "non-Unicode source path cannot enter browser projection: {}",
            relative_path.display()
        ));
    }
    Ok(())
}

fn projected_path_id(root: &Path, relative_path: &Path) -> Result<String, String> {
    let absolute = root.join(relative_path);
    absolute.to_str().map(str::to_owned).ok_or_else(|| {
        format!(
            "non-Unicode source path cannot enter browser removal projection: {}",
            relative_path.display()
        )
    })
}

fn wait_for_retry(cancel: &AtomicBool, delay: Duration) -> bool {
    let deadline = std::time::Instant::now() + delay;
    while std::time::Instant::now() < deadline {
        if cancel.load(Ordering::Acquire) {
            return false;
        }
        thread::sleep(Duration::from_millis(10));
    }
    true
}

#[cfg(test)]
mod tests {
    use std::{
        path::{Path, PathBuf},
        sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        },
    };

    use wavecrate::sample_sources::{Rating, SourceDatabase, scanner};
    use wavecrate_library::sample_sources::reconciliation::{
        BackendStreamIdentity, CaptureBoundary, RawEventKind, RawObservation,
        RawObservationEnvelope, RawObservationLimits, RawObservationProvenance, RawObservedPath,
        RawPathRole, RootIdentity, WatcherGeneration, normalize_observation,
    };
    use wavecrate_scan::sample_sources::scanner::{
        ScanWritePhase, ScanWriter, UncoordinatedScanWriter,
    };

    use crate::native_app::sample_library::source_watcher::{
        WatcherBackend, WatcherContinuityProof,
    };

    use super::{
        capture_source_root_identity, recover_source_filesystem_sync,
        run_targeted_sync_after_root_identity_gate, sync_source_database_paths,
        sync_source_database_paths_with_writer, sync_source_database_scopes_with_writer,
    };

    #[derive(Clone)]
    struct RevisionBumpingWriter {
        database_root: PathBuf,
        manifest_locks: Arc<std::sync::atomic::AtomicUsize>,
    }

    struct RevisionBumpGuard {
        database_root: PathBuf,
        bump_revision: bool,
    }

    impl Drop for RevisionBumpGuard {
        fn drop(&mut self) {
            if !self.bump_revision {
                return;
            }
            let db = SourceDatabase::open_for_test_fixture_source_write(&self.database_root)
                .expect("open revision-bump database");
            db.set_tag(Path::new("fresh.wav"), Rating::KEEP_1)
                .expect("bump committed revision after scan");
        }
    }

    impl ScanWriter for RevisionBumpingWriter {
        type Guard = RevisionBumpGuard;

        fn lock(&self, phase: ScanWritePhase) -> Self::Guard {
            RevisionBumpGuard {
                database_root: self.database_root.clone(),
                bump_revision: phase == ScanWritePhase::Manifest
                    && self.manifest_locks.fetch_add(1, Ordering::AcqRel) == 1,
            }
        }
    }

    #[derive(Clone)]
    struct CountingWriter {
        database_open_started: Arc<AtomicBool>,
    }

    struct CountingGuard;

    impl ScanWriter for CountingWriter {
        type Guard = CountingGuard;

        fn lock(&self, _phase: ScanWritePhase) -> Self::Guard {
            self.database_open_started.store(true, Ordering::Release);
            CountingGuard
        }
    }

    fn watcher_proof(root_identity: &str, event_id: u64) -> WatcherContinuityProof {
        WatcherContinuityProof {
            root_identity: root_identity.to_string(),
            backend: WatcherBackend::Fsevents,
            backend_device: 10,
            watcher_generation: 4,
            replay_coverage_start_event_id: event_id.saturating_sub(1),
            replay_coverage_end_event_id: event_id,
            acknowledged_end_event_id: event_id,
        }
    }

    fn exact_scope() -> wavecrate_library::sample_sources::reconciliation::ReconciliationScope {
        let provenance = RawObservationProvenance::new(
            wavecrate::sample_sources::SourceId::from_string("source-a"),
            Some(RootIdentity::from_bytes(vec![1])),
            Some(BackendStreamIdentity::from_bytes(vec![2])),
            WatcherGeneration::new(4),
            CaptureBoundary::try_new(9, Some(8), Some(9)).expect("capture boundary"),
        );
        let envelope = RawObservationEnvelope::try_new(
            provenance,
            vec![RawObservation::new(
                RawEventKind::Modify,
                vec![RawObservedPath::new(
                    PathBuf::from("exact.wav"),
                    RawPathRole::Subject,
                )],
            )],
            RawObservationLimits::new(8, usize::MAX, usize::MAX).expect("observation limits"),
        )
        .expect("observation envelope");
        normalize_observation(envelope)
            .scopes()
            .first()
            .cloned()
            .expect("exact scope")
    }

    #[test]
    fn typed_scope_preflight_falls_back_without_opening_database() {
        let root = tempfile::tempdir().expect("source root");
        let root_identity = capture_source_root_identity(root.path()).expect("root identity");
        let proof = watcher_proof(&root_identity, 73);
        let writer = CountingWriter {
            database_open_started: Arc::new(AtomicBool::new(false)),
        };

        let result = sync_source_database_scopes_with_writer(
            String::from("source-a"),
            root.path().to_path_buf(),
            root.path().to_path_buf(),
            vec![exact_scope()],
            1,
            Some(root_identity.clone()),
            &AtomicBool::new(false),
            Some(proof),
            &writer,
        );

        assert_eq!(
            result.audit_required,
            Some(crate::native_app::app::SourceFilesystemSyncAuditReason::ScopeLost)
        );
        assert!(result.result.is_err());
        assert!(!writer.database_open_started.load(Ordering::Acquire));
    }

    #[test]
    fn typed_scope_dispatch_uses_exact_scanner_and_committed_projection() {
        let root = tempfile::tempdir().expect("source root");
        std::fs::write(root.path().join("exact.wav"), b"exact").expect("exact file");
        let database_parent = tempfile::tempdir().expect("database parent");
        let database_root = database_parent.path().join("source-db");
        let database_path = database_root.join(".wavecrate.db");
        let root_identity = capture_source_root_identity(root.path()).expect("root identity");
        let proof = watcher_proof(&root_identity, 73);

        let result = sync_source_database_scopes_with_writer(
            String::from("source-a"),
            root.path().to_path_buf(),
            database_root,
            vec![exact_scope()],
            1,
            Some(root_identity),
            &AtomicBool::new(false),
            Some(proof),
            &UncoordinatedScanWriter,
        );

        assert!(result.audit_required.is_none());
        let success = result.result.expect("exact scope sync");
        assert_eq!(success.renames_reconciled, 0);
        assert_eq!(success.committed_delta.created.len(), 1);
        assert!(success.incomplete_error.is_none());
        assert!(success.browser_projection_delta.is_some());
        assert!(success.projection_handoff_ticket.is_none());
        assert!(database_path.is_file());
        assert!(!root.path().join(".wavecrate.db").exists());
    }

    #[test]
    fn mismatched_targeted_sync_root_is_rejected_before_database_work() {
        let root = tempfile::tempdir().expect("source root");
        let captured_root_identity = capture_source_root_identity(root.path());
        let database_work_started = AtomicBool::new(false);
        let proof = watcher_proof("replaced-root", 73);

        let result = run_targeted_sync_after_root_identity_gate(
            String::from("source-a"),
            7,
            1,
            captured_root_identity.clone(),
            Some(73),
            Some(proof.clone()),
            || {
                database_work_started.store(true, Ordering::Release);
                panic!("mismatched watcher root must not reach database work");
            },
        );

        assert!(!database_work_started.load(Ordering::Acquire));
        assert_eq!(result.root_identity, captured_root_identity);
        assert_eq!(result.journal_checkpoint_event_id, Some(73));
        assert_eq!(result.watcher_continuity_proof, Some(proof));
        assert!(
            result
                .result
                .expect_err("mismatched watcher root must be terminal")
                .contains("root identity")
        );
    }

    #[test]
    fn replacement_between_authority_gate_and_database_sync_is_rejected_before_open() {
        let parent = tempfile::tempdir().expect("source parent");
        let root = parent.path().join("source");
        let replacement = parent.path().join("replacement");
        let displaced = parent.path().join("displaced");
        std::fs::create_dir(&root).expect("source root");
        std::fs::create_dir(&replacement).expect("replacement root");

        let captured_root_identity = capture_source_root_identity(&root);
        let proof = watcher_proof(
            captured_root_identity
                .as_deref()
                .expect("source root identity"),
            73,
        );
        let database_open_started = Arc::new(AtomicBool::new(false));
        let writer = CountingWriter {
            database_open_started: database_open_started.clone(),
        };
        let result = run_targeted_sync_after_root_identity_gate(
            String::from("source-a"),
            7,
            1,
            captured_root_identity.clone(),
            Some(73),
            Some(proof.clone()),
            || {
                std::fs::rename(&root, &displaced).expect("displace source root");
                std::fs::rename(&replacement, &root).expect("install replacement root");
                sync_source_database_paths_with_writer(
                    String::from("source-a"),
                    root.clone(),
                    root.clone(),
                    vec![PathBuf::from("fresh.wav")],
                    1,
                    &AtomicBool::new(false),
                    Some(proof.clone()),
                    &writer,
                )
            },
        );

        let observed_root_identity = capture_source_root_identity(&root);
        assert_ne!(observed_root_identity, captured_root_identity);
        assert_eq!(result.root_identity, observed_root_identity);
        assert_eq!(result.watcher_continuity_proof, Some(proof));
        assert!(!database_open_started.load(Ordering::Acquire));
        assert!(
            result
                .result
                .expect_err("replacement must reject targeted sync")
                .contains("live source root identity")
        );
    }

    #[test]
    fn filesystem_sync_panic_returns_a_terminal_result() {
        let result = recover_source_filesystem_sync(String::from("source"), 17, 2, || {
            panic!("simulated targeted sync panic")
        });

        assert_eq!(result.source_id, "source");
        assert_eq!(result.lifecycle_generation, 17);
        assert_eq!(result.changed_count, 2);
        assert!(!result.cancelled);
        assert!(
            result
                .result
                .expect_err("panic must become an error")
                .contains("stopped unexpectedly")
        );
    }

    #[test]
    fn filesystem_sync_returns_deferred_rename_results_for_refresh() {
        let root = tempfile::tempdir().expect("source root");
        let old = root.path().join("old.wav");
        let new = root.path().join("new.wav");
        std::fs::write(&old, vec![5_u8; 9 * 1024 * 1024]).expect("large wav");
        let db =
            SourceDatabase::open_for_test_fixture_source_write(root.path()).expect("source db");
        scanner::hard_rescan(&db).expect("initial scan");
        db.set_tag(Path::new("old.wav"), Rating::KEEP_1)
            .expect("tag old path");
        std::fs::rename(&old, &new).expect("rename wav");

        let result = sync_source_database_paths(
            String::from("source-a"),
            root.path().to_path_buf(),
            root.path().to_path_buf(),
            vec![PathBuf::from("old.wav"), PathBuf::from("new.wav")],
            2,
            &AtomicBool::new(false),
        );

        let success = result.result.expect("sync result");
        assert!(result.root_identity.is_some());
        assert_eq!(success.renames_reconciled, 1);
        assert_eq!(success.committed_delta.moved.len(), 1);
        let projection = success
            .browser_projection_delta
            .expect("exact browser projection delta");
        assert_eq!(projection.removed_file_ids.len(), 1);
        assert_eq!(projection.upserted_files.len(), 1);
        assert_eq!(
            db.entry_for_path(Path::new("new.wav"))
                .unwrap()
                .unwrap()
                .tag,
            Rating::KEEP_1
        );
    }

    #[test]
    fn filesystem_sync_leaves_non_rename_hashing_for_the_supervisor() {
        let root = tempfile::tempdir().expect("source root");
        let fresh = root.path().join("fresh.wav");
        std::fs::write(&fresh, vec![7_u8; 9 * 1024 * 1024]).expect("large wav");

        let result = sync_source_database_paths(
            String::from("source-a"),
            root.path().to_path_buf(),
            root.path().to_path_buf(),
            vec![PathBuf::from("fresh.wav")],
            1,
            &AtomicBool::new(false),
        );

        let success = result.result.expect("sync result");
        assert_eq!(success.renames_reconciled, 0);
        assert_eq!(success.committed_delta.created.len(), 1);
        assert_eq!(
            success
                .browser_projection_delta
                .expect("exact browser projection delta")
                .upserted_files
                .len(),
            1
        );
        let db =
            SourceDatabase::open_for_test_fixture_source_write(root.path()).expect("source db");
        assert!(
            db.entry_for_path(Path::new("fresh.wav"))
                .expect("read entry")
                .expect("fresh entry")
                .content_hash
                .is_none(),
            "ordinary deep hashing must remain queued for the supervisor"
        );
    }

    #[test]
    fn filesystem_sync_publishes_exact_revision_for_an_empty_duplicate_sync() {
        let root = tempfile::tempdir().expect("source root");
        let unchanged = root.path().join("unchanged.wav");
        std::fs::write(&unchanged, vec![3_u8; 128]).expect("unchanged wav");
        let db =
            SourceDatabase::open_for_test_fixture_source_write(root.path()).expect("source db");
        scanner::hard_rescan(&db).expect("initial scan");

        let result = sync_source_database_paths(
            String::from("source-a"),
            root.path().to_path_buf(),
            root.path().to_path_buf(),
            vec![PathBuf::from("unchanged.wav")],
            1,
            &AtomicBool::new(false),
        );

        let success = result.result.expect("sync result");
        assert!(success.committed_delta.is_empty());
        let projection = success
            .browser_projection_delta
            .expect("empty sync still carries the authoritative revision");
        assert_eq!(
            projection.manifest_revision,
            success.committed_delta.revision
        );
        assert_eq!(
            projection.snapshot_revision,
            success.committed_delta.revision
        );
        assert!(projection.upserted_files.is_empty());
        assert!(projection.removed_file_ids.is_empty());
    }

    #[test]
    fn filesystem_sync_projects_visible_unsupported_create_update_and_delete() {
        let root = tempfile::tempdir().expect("source root");
        let relative = PathBuf::from("visible.flac");
        let absolute = root.path().join(&relative);
        std::fs::write(&absolute, b"one").expect("unsupported file");

        let created = sync_source_database_paths(
            String::from("source-a"),
            root.path().to_path_buf(),
            root.path().to_path_buf(),
            vec![relative.clone()],
            1,
            &AtomicBool::new(false),
        )
        .result
        .expect("unsupported create sync");
        assert!(created.committed_delta.is_empty());
        assert_eq!(
            created.committed_source_index_delta.upserted_entries.len(),
            1
        );
        let created_projection = created
            .browser_projection_delta
            .expect("unsupported create projection");
        assert_eq!(created_projection.upserted_files.len(), 1);
        assert_eq!(
            created_projection.upserted_files[0].id,
            absolute.display().to_string()
        );
        assert_eq!(
            created_projection.upserted_files[0].kind,
            "Unsupported audio"
        );
        assert_eq!(created_projection.upserted_files[0].size_bytes, 3);

        std::fs::write(&absolute, b"updated").expect("unsupported update");
        let updated = sync_source_database_paths(
            String::from("source-a"),
            root.path().to_path_buf(),
            root.path().to_path_buf(),
            vec![relative.clone()],
            1,
            &AtomicBool::new(false),
        )
        .result
        .expect("unsupported update sync");
        assert!(updated.committed_delta.is_empty());
        assert_eq!(
            updated.committed_source_index_delta.upserted_entries.len(),
            1
        );
        assert_eq!(
            updated
                .browser_projection_delta
                .as_ref()
                .expect("unsupported update projection")
                .upserted_files[0]
                .size_bytes,
            7
        );

        std::fs::remove_file(&absolute).expect("unsupported delete");
        let deleted = sync_source_database_paths(
            String::from("source-a"),
            root.path().to_path_buf(),
            root.path().to_path_buf(),
            vec![relative.clone()],
            1,
            &AtomicBool::new(false),
        )
        .result
        .expect("unsupported delete sync");
        assert!(deleted.committed_delta.is_empty());
        assert_eq!(
            deleted.committed_source_index_delta.removed_paths,
            vec![relative]
        );
        let deleted_projection = deleted
            .browser_projection_delta
            .expect("unsupported delete projection");
        assert_eq!(
            deleted_projection.removed_file_ids,
            vec![absolute.display().to_string()]
        );
        assert!(deleted_projection.upserted_files.is_empty());
    }

    #[test]
    fn filesystem_sync_projects_supported_and_unsupported_transitions_once() {
        let root = tempfile::tempdir().expect("source root");
        let supported = root.path().join("transition.wav");
        let unsupported = root.path().join("transition.flac");
        std::fs::write(&supported, b"supported").expect("supported file");
        sync_source_database_paths(
            String::from("source-a"),
            root.path().to_path_buf(),
            root.path().to_path_buf(),
            vec![PathBuf::from("transition.wav")],
            1,
            &AtomicBool::new(false),
        )
        .result
        .expect("initial supported sync");

        std::fs::rename(&supported, &unsupported).expect("supported to unsupported rename");
        let to_unsupported = sync_source_database_paths(
            String::from("source-a"),
            root.path().to_path_buf(),
            root.path().to_path_buf(),
            vec![
                PathBuf::from("transition.wav"),
                PathBuf::from("transition.flac"),
            ],
            2,
            &AtomicBool::new(false),
        )
        .result
        .expect("supported to unsupported sync");
        let projection = to_unsupported
            .browser_projection_delta
            .expect("supported to unsupported projection");
        assert_eq!(
            projection.removed_file_ids,
            vec![supported.display().to_string()]
        );
        assert_eq!(projection.upserted_files.len(), 1);
        assert_eq!(
            projection.upserted_files[0].id,
            unsupported.display().to_string()
        );
        assert_eq!(projection.upserted_files[0].kind, "Unsupported audio");

        std::fs::rename(&unsupported, &supported).expect("unsupported to supported rename");
        let to_supported = sync_source_database_paths(
            String::from("source-a"),
            root.path().to_path_buf(),
            root.path().to_path_buf(),
            vec![
                PathBuf::from("transition.flac"),
                PathBuf::from("transition.wav"),
            ],
            2,
            &AtomicBool::new(false),
        )
        .result
        .expect("unsupported to supported sync");
        let projection = to_supported
            .browser_projection_delta
            .expect("unsupported to supported projection");
        assert_eq!(
            projection.removed_file_ids,
            vec![unsupported.display().to_string()]
        );
        assert_eq!(projection.upserted_files.len(), 1);
        assert_eq!(
            projection.upserted_files[0].id,
            supported.display().to_string()
        );
        assert_eq!(projection.upserted_files[0].kind, "Audio");
    }

    #[test]
    fn filesystem_sync_requires_full_recovery_when_projection_revision_changes() {
        let root = tempfile::tempdir().expect("source root");
        std::fs::write(root.path().join("fresh.wav"), vec![7_u8; 9 * 1024 * 1024])
            .expect("large wav");
        let writer = RevisionBumpingWriter {
            database_root: root.path().to_path_buf(),
            manifest_locks: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
        };

        let result = sync_source_database_paths_with_writer(
            String::from("source-a"),
            root.path().to_path_buf(),
            root.path().to_path_buf(),
            vec![PathBuf::from("fresh.wav")],
            1,
            &AtomicBool::new(false),
            None,
            &writer,
        );

        let success = result
            .result
            .expect("revision mismatch should retain the committed scan");
        assert!(success.browser_projection_delta.is_none());
        assert!(!success.committed_delta.is_empty());
        assert!(
            success
                .incomplete_error
                .as_deref()
                .is_some_and(|error| error.contains("browser projection hydration failed"))
        );
    }

    #[cfg(unix)]
    #[test]
    fn filesystem_sync_retires_a_symlinked_file_from_the_browser_projection() {
        use std::os::unix::fs as unix_fs;

        let root = tempfile::tempdir().expect("source root");
        let outside = tempfile::tempdir().expect("outside source root");
        let tracked = root.path().join("tracked.wav");
        std::fs::write(&tracked, b"tracked").expect("tracked wav");
        std::fs::write(outside.path().join("outside.wav"), b"outside").expect("outside wav");
        let db =
            SourceDatabase::open_for_test_fixture_source_write(root.path()).expect("source db");
        scanner::hard_rescan(&db).expect("initial scan");
        std::fs::remove_file(&tracked).expect("replace tracked wav");
        unix_fs::symlink(outside.path().join("outside.wav"), &tracked).expect("file link");

        let result = sync_source_database_paths(
            String::from("source-a"),
            root.path().to_path_buf(),
            root.path().to_path_buf(),
            vec![PathBuf::from("tracked.wav")],
            1,
            &AtomicBool::new(false),
        );

        let success = result.result.expect("sync result");
        assert!(
            db.entry_for_path(Path::new("tracked.wav"))
                .expect("read tracked entry")
                .is_none()
        );
        let projection = success
            .browser_projection_delta
            .expect("browser projection delta");
        assert_eq!(
            projection.removed_file_ids,
            vec![tracked.display().to_string()]
        );
        assert!(projection.upserted_files.is_empty());
    }

    #[test]
    fn filesystem_sync_reports_lifecycle_cancellation_for_requeue() {
        let root = tempfile::tempdir().expect("source root");
        let fresh = root.path().join("fresh.wav");
        std::fs::write(&fresh, b"fresh").expect("wav");
        let cancel = AtomicBool::new(true);

        let result = sync_source_database_paths(
            String::from("source-a"),
            root.path().to_path_buf(),
            root.path().to_path_buf(),
            vec![PathBuf::from("fresh.wav")],
            1,
            &cancel,
        );

        assert!(result.cancelled);
        assert_eq!(
            result.audit_required,
            Some(crate::native_app::app::SourceFilesystemSyncAuditReason::Cancelled)
        );
        assert!(result.result.is_err());
        let db =
            SourceDatabase::open_for_test_fixture_source_write(root.path()).expect("source db");
        assert!(
            db.entry_for_path(Path::new("fresh.wav"))
                .expect("read entry")
                .is_none()
        );
    }

    #[test]
    fn filesystem_sync_retries_a_transient_database_root_failure() {
        let root = tempfile::tempdir().expect("source root");
        std::fs::write(root.path().join("fresh.wav"), b"fresh").expect("wav");
        let database_parent = tempfile::tempdir().expect("database parent");
        let database_root = database_parent.path().join("source-db");
        std::fs::write(&database_root, b"temporarily blocked").expect("block database root");
        let repaired_root = database_root.clone();
        let repair = std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(75));
            std::fs::remove_file(&repaired_root).expect("remove transient blocker");
            std::fs::create_dir(&repaired_root).expect("repair database root");
        });

        let result = sync_source_database_paths(
            String::from("source-a"),
            root.path().to_path_buf(),
            database_root,
            vec![PathBuf::from("fresh.wav")],
            1,
            &AtomicBool::new(false),
        );
        repair.join().expect("repair worker");

        let success = result.result.expect("transient sync should converge");
        assert_eq!(success.committed_delta.created.len(), 1);
    }
}
