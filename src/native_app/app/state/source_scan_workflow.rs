use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    path::PathBuf,
    time::Instant,
};

use super::source_refresh::{
    PendingSourceRefresh, PendingTargetedSourceSync, QueuedSourceRefresh, QueuedTargetedSourceSync,
    SourceRefreshCause,
};

use crate::native_app::sample_library::folder_browser::{
    FolderBrowserState,
    scan::{
        FolderScanDiscoveryBatch, FolderScanLifecycle, FolderScanProgress, FolderScanRequest,
        FolderScanResult,
    },
};
use crate::native_app::sample_library::source_watcher::{
    WatcherContinuityProof, watcher_replay_evidence_is_well_formed,
};
use wavecrate::sample_sources::scanner::CommittedSourceDelta;
use wavecrate_library::sample_sources::reconciliation::{
    ReconciliationScope, ReconciliationScopeKind, coalesce_scopes,
};

const MAX_TYPED_SCOPE_COUNT: usize = 4096;
const MAX_TYPED_SCOPE_PATH_BYTES: usize = 256 * 1024;

#[cfg(test)]
#[path = "source_scan_workflow/tests.rs"]
/// Source-scan workflow state tests split out from the runtime module.
mod tests;

pub(in crate::native_app) struct SourceScanWorkflow {
    progress: Option<FolderScanProgress>,
    pending_refreshes: VecDeque<QueuedSourceRefresh>,
    pending_targeted_syncs: BTreeMap<String, QueuedTargetedSourceSync>,
    active_targeted_syncs: BTreeMap<String, u64>,
    retry_counts: BTreeMap<String, u32>,
    last_discovery_sequences: BTreeMap<String, (u64, u64)>,
}

pub(in crate::native_app) enum SourceFilesystemChangePlan {
    IgnoredSourceMissing {
        source_id: String,
    },
    SyncPaths {
        source_id: String,
        changed_count: usize,
    },
    SyncScopes {
        source_id: String,
        scopes: Vec<ReconciliationScope>,
        changed_count: usize,
        source_root_identity: Option<String>,
        lifecycle_generation: Option<u64>,
        journal_checkpoint_event_id: Option<u64>,
        watcher_continuity_proof: Option<WatcherContinuityProof>,
    },
    SourceAuditRequired {
        source_id: String,
        reason: &'static str,
    },
    DeferredAlreadyRunning {
        source_id: String,
    },
    QueueRefresh {
        source_id: String,
        cause: SourceRefreshCause,
    },
}

pub(in crate::native_app) enum SourceRefreshRequest {
    Queued(FolderScanRequest),
    Deferred {
        source_id: String,
    },
    Covered {
        source_id: String,
        accepted_revision: u64,
    },
    IgnoredMissing {
        source_id: String,
    },
}

pub(in crate::native_app) enum SourceSelectionRequest {
    Queued(FolderScanRequest),
    Deferred,
    Settled,
}

pub(in crate::native_app) enum SourceScanFinish {
    Applied {
        source_id: String,
        label: String,
        file_count: usize,
        folder_count: usize,
        source_db_error: Option<String>,
        metadata_hydration_error: Option<String>,
        committed_delta: Option<CommittedSourceDelta>,
        source_root_available: bool,
    },
    Stale {
        label: String,
    },
    Cancelled {
        source_id: String,
        label: String,
    },
}

impl SourceScanWorkflow {
    pub(in crate::native_app) fn new() -> Self {
        Self {
            progress: None,
            pending_refreshes: VecDeque::new(),
            pending_targeted_syncs: BTreeMap::new(),
            active_targeted_syncs: BTreeMap::new(),
            retry_counts: BTreeMap::new(),
            last_discovery_sequences: BTreeMap::new(),
        }
    }

    pub(in crate::native_app) fn progress(&self) -> Option<&FolderScanProgress> {
        self.progress.as_ref()
    }

    pub(in crate::native_app) fn active(&self) -> bool {
        self.progress.is_some()
    }

    pub(in crate::native_app) fn begin_add_source_path(
        &mut self,
        browser: &mut FolderBrowserState,
        root: PathBuf,
        task_id: u64,
    ) -> Option<FolderScanRequest> {
        if self.active() {
            if let Some(source_id) = browser.source_id_for_root_path(&root) {
                let _ = self.begin_select_source(browser, source_id, task_id);
            } else if let Some(source_id) = browser.defer_add_source_path(root, true) {
                self.queue_selected_required_refresh_with_cause(
                    source_id,
                    SourceRefreshCause::DeferredSourceAdd,
                );
            }
            return None;
        }
        browser.begin_add_source_path(root, task_id)
    }

    pub(in crate::native_app) fn begin_add_source_path_preserving_selection(
        &mut self,
        browser: &mut FolderBrowserState,
        root: PathBuf,
        task_id: u64,
    ) -> Option<FolderScanRequest> {
        if self.active() {
            if let Some(source_id) = browser.source_id_for_root_path(&root)
                && !browser.source_is_missing(&source_id)
            {
                self.queue_required_refresh_with_context(
                    source_id,
                    SourceRefreshCause::DeferredSourceAdd,
                    None,
                );
            } else if let Some(source_id) = browser.defer_add_source_path(root, false) {
                self.queue_required_refresh_with_context(
                    source_id,
                    SourceRefreshCause::DeferredSourceAdd,
                    None,
                );
            }
            return None;
        }
        browser.begin_add_source_path_preserving_selection(root, task_id)
    }

    pub(in crate::native_app) fn begin_select_source(
        &mut self,
        browser: &mut FolderBrowserState,
        id: String,
        task_id: u64,
    ) -> SourceSelectionRequest {
        if self.active() {
            let active_source_id = self
                .progress
                .as_ref()
                .map(|progress| progress.source_id.as_str());
            if active_source_id == Some(id.as_str()) {
                let _ = browser.select_source_without_scan(id.clone());
                self.clear_pending_selection(&id);
                return SourceSelectionRequest::Settled;
            }
            if !browser.select_source_without_scan(id.clone()) {
                return SourceSelectionRequest::Settled;
            }
            if browser.source_is_missing(&id) {
                self.remove_pending_refresh(&id);
                return SourceSelectionRequest::Settled;
            }
            if browser.selected_source_loaded() {
                // The cached tree is already authoritative enough to satisfy
                // this selection. Do not turn a click made during another
                // source's scan into a later full refresh.
                self.clear_pending_selection(&id);
                return SourceSelectionRequest::Settled;
            }
            self.queue_selected_required_refresh_with_cause(
                id,
                SourceRefreshCause::DeferredSelection,
            );
            return SourceSelectionRequest::Deferred;
        }
        if !browser.select_source_without_scan(id.clone()) {
            return SourceSelectionRequest::Settled;
        }
        if browser.source_is_missing(&id) || browser.selected_source_loaded() {
            self.clear_pending_selection(&id);
            return SourceSelectionRequest::Settled;
        }
        if self.active_targeted_syncs.contains_key(&id) {
            self.queue_selected_required_refresh_with_cause(
                id,
                SourceRefreshCause::DeferredSelection,
            );
            return SourceSelectionRequest::Deferred;
        }
        browser
            .begin_select_source(id, task_id)
            .map(SourceSelectionRequest::Queued)
            .unwrap_or(SourceSelectionRequest::Settled)
    }

    pub(in crate::native_app) fn begin_source_scan(
        &mut self,
        browser: &mut FolderBrowserState,
        id: String,
        task_id: u64,
    ) -> Option<FolderScanRequest> {
        if self.active_targeted_syncs.contains_key(&id) {
            self.queue_required_refresh_with_context(
                id,
                SourceRefreshCause::DeferredSelection,
                None,
            );
            return None;
        }
        browser.begin_source_scan(id, task_id)
    }

    pub(in crate::native_app) fn begin_selected_source_scan(
        &mut self,
        browser: &mut FolderBrowserState,
        task_id: u64,
    ) -> Option<FolderScanRequest> {
        let source_id = browser.selected_source_id().to_string();
        if self.active_targeted_syncs.contains_key(&source_id) {
            self.queue_required_refresh_with_context(
                source_id,
                SourceRefreshCause::DeferredSelection,
                None,
            );
            return None;
        }
        browser.begin_selected_source_scan(task_id)
    }

    pub(in crate::native_app) fn start_scan(&mut self, request: &FolderScanRequest) {
        self.remove_pending_targeted_sync(&request.source_id);
        self.last_discovery_sequences.remove(&request.source_id);
        let mut progress = FolderScanProgress::transition(
            request.task_id,
            request.source_id.clone(),
            request.label.clone(),
            FolderScanLifecycle::Queued,
            "Queued — preparing source scan",
        );
        progress.retry_count = self
            .retry_counts
            .get(&request.source_id)
            .copied()
            .unwrap_or(0);
        self.progress = Some(progress);
    }

    pub(in crate::native_app) fn apply_progress(
        &mut self,
        browser: &FolderBrowserState,
        mut progress: FolderScanProgress,
    ) -> bool {
        if !browser.scan_is_active(&progress.source_id, progress.task_id) {
            return false;
        }
        if let Some(previous) = self.progress.as_ref() {
            progress.reconcile_timing_from(previous);
        }
        self.progress = Some(progress);
        true
    }

    pub(in crate::native_app) fn transition_current_scan(
        &mut self,
        task_id: u64,
        source_id: &str,
        lifecycle_generation: Option<u64>,
        lifecycle: FolderScanLifecycle,
        detail: impl Into<String>,
    ) -> bool {
        let Some(progress) = self.progress.as_mut() else {
            return false;
        };
        if progress.task_id != task_id || progress.source_id != source_id {
            return false;
        }
        if lifecycle_generation.is_some()
            && progress.lifecycle_generation.is_some()
            && lifecycle_generation != progress.lifecycle_generation
        {
            return false;
        }
        let now = Instant::now();
        progress.lifecycle = lifecycle;
        progress.detail = detail.into();
        progress.state_changed_at = now;
        progress.last_progress_at = now;
        progress.lifecycle_generation = lifecycle_generation.or(progress.lifecycle_generation);
        true
    }

    pub(in crate::native_app) fn finish_current_scan_terminal(
        &mut self,
        task_id: u64,
        source_id: &str,
        lifecycle_generation: Option<u64>,
        lifecycle: FolderScanLifecycle,
    ) -> Option<FolderScanProgress> {
        if !lifecycle.is_terminal() {
            return None;
        }
        if !self.transition_current_scan(
            task_id,
            source_id,
            lifecycle_generation,
            lifecycle,
            "Source scan reached a terminal outcome",
        ) {
            return None;
        }
        self.progress.take()
    }

    pub(in crate::native_app) fn apply_discovery_batch(
        &mut self,
        browser: &mut FolderBrowserState,
        batch: FolderScanDiscoveryBatch,
    ) -> bool {
        if batch.events.is_empty() || batch.events.len() > 64 {
            return false;
        }
        let Some(progress) = self.progress.as_ref() else {
            return false;
        };
        if progress.task_id != batch.task_id || progress.source_id != batch.source_id {
            return false;
        }
        if progress.lifecycle_generation != batch.lifecycle_generation {
            return false;
        }
        if batch.events.iter().any(|event| {
            event.task_id != batch.task_id
                || event.source_id != batch.source_id
                || event.committed_revision != batch.committed_revision
        }) {
            return false;
        }
        match self.last_discovery_sequences.get(&batch.source_id) {
            Some((task_id, sequence))
                if *task_id != batch.task_id || batch.sequence != sequence.saturating_add(1) =>
            {
                return false;
            }
            None if batch.sequence != 0 => return false,
            _ => {}
        }
        if !browser.apply_scan_discovered_batch(batch.clone()) {
            return false;
        }
        self.last_discovery_sequences
            .insert(batch.source_id, (batch.task_id, batch.sequence));
        true
    }

    pub(in crate::native_app) fn plan_filesystem_change_for_generation(
        &mut self,
        browser: &mut FolderBrowserState,
        source_id: String,
        paths: &[PathBuf],
        overflowed: bool,
        source_root_available: bool,
        lifecycle_generation: Option<u64>,
        journal_checkpoint_event_id: Option<u64>,
        watcher_continuity_proof: Option<WatcherContinuityProof>,
    ) -> SourceFilesystemChangePlan {
        self.plan_filesystem_change_with_scopes_for_generation(
            browser,
            source_id,
            None,
            paths,
            overflowed,
            source_root_available,
            None,
            lifecycle_generation,
            journal_checkpoint_event_id,
            watcher_continuity_proof,
        )
    }

    pub(in crate::native_app) fn plan_filesystem_change_with_scopes_for_generation(
        &mut self,
        browser: &mut FolderBrowserState,
        source_id: String,
        scopes: Option<&[ReconciliationScope]>,
        paths: &[PathBuf],
        overflowed: bool,
        source_root_available: bool,
        source_root_identity: Option<String>,
        lifecycle_generation: Option<u64>,
        journal_checkpoint_event_id: Option<u64>,
        watcher_continuity_proof: Option<WatcherContinuityProof>,
    ) -> SourceFilesystemChangePlan {
        let Some(source_missing) =
            browser.apply_observed_source_availability(&source_id, source_root_available)
        else {
            self.remove_pending_refresh(&source_id);
            self.remove_targeted_sync(&source_id);
            return SourceFilesystemChangePlan::IgnoredSourceMissing { source_id };
        };
        if source_missing {
            self.remove_pending_refresh(&source_id);
            self.remove_targeted_sync(&source_id);
            return SourceFilesystemChangePlan::IgnoredSourceMissing { source_id };
        }
        if let Some(scopes) = scopes {
            let scopes = coalesce_scopes(scopes.iter().cloned());
            if scopes.is_empty() {
                return SourceFilesystemChangePlan::SourceAuditRequired {
                    source_id,
                    reason: "reconciliation_scope_lost",
                };
            }
            if scopes
                .iter()
                .any(|scope| scope.kind() == ReconciliationScopeKind::SourceAudit)
            {
                return SourceFilesystemChangePlan::SourceAuditRequired {
                    source_id,
                    reason: "reconciliation_source_audit_scope",
                };
            }
            let scope_path_bytes = scopes
                .iter()
                .filter_map(|scope| scope.path())
                .map(|path| path.as_path().to_string_lossy().len())
                .sum::<usize>();
            if scopes.len() > MAX_TYPED_SCOPE_COUNT
                || scope_path_bytes > MAX_TYPED_SCOPE_PATH_BYTES
                || overflowed
            {
                return SourceFilesystemChangePlan::SourceAuditRequired {
                    source_id,
                    reason: "reconciliation_scope_budget_exceeded",
                };
            }
            if lifecycle_generation.is_none() {
                return SourceFilesystemChangePlan::SourceAuditRequired {
                    source_id,
                    reason: "targeted_sync_lifecycle_uncertain",
                };
            }
            let authority_is_valid = watcher_replay_evidence_is_well_formed(
                journal_checkpoint_event_id,
                watcher_continuity_proof.as_ref(),
            ) && source_root_identity.as_deref()
                == watcher_continuity_proof
                    .as_ref()
                    .map(|proof| proof.root_identity.as_str());
            if !authority_is_valid {
                return SourceFilesystemChangePlan::SourceAuditRequired {
                    source_id,
                    reason: "targeted_sync_authority_unproven",
                };
            }
            let full_scan_active_for_source = self
                .progress
                .as_ref()
                .is_some_and(|progress| progress.source_id == source_id);
            let full_scan_pending_for_source = self
                .pending_refreshes
                .iter()
                .any(|pending| pending.source_id == source_id);
            if full_scan_pending_for_source && !full_scan_active_for_source {
                self.queue_targeted_sync(
                    source_id.clone(),
                    Some(&scopes),
                    &[],
                    source_root_identity,
                    lifecycle_generation,
                    journal_checkpoint_event_id,
                    watcher_continuity_proof,
                );
                return SourceFilesystemChangePlan::DeferredAlreadyRunning { source_id };
            }
            if full_scan_active_for_source || self.active_targeted_syncs.contains_key(&source_id) {
                self.queue_targeted_sync(
                    source_id.clone(),
                    Some(&scopes),
                    &[],
                    source_root_identity,
                    lifecycle_generation,
                    journal_checkpoint_event_id,
                    watcher_continuity_proof,
                );
                return SourceFilesystemChangePlan::DeferredAlreadyRunning { source_id };
            }
            return SourceFilesystemChangePlan::SyncScopes {
                source_id,
                changed_count: scopes.len(),
                scopes,
                source_root_identity,
                lifecycle_generation,
                journal_checkpoint_event_id,
                watcher_continuity_proof,
            };
        }
        if !overflowed && !paths.is_empty() {
            let watcher_replay_proven = watcher_replay_evidence_is_well_formed(
                journal_checkpoint_event_id,
                watcher_continuity_proof.as_ref(),
            );
            let full_scan_active_for_source = self
                .progress
                .as_ref()
                .is_some_and(|progress| progress.source_id == source_id);
            let full_scan_pending_for_source = self
                .pending_refreshes
                .iter()
                .any(|pending| pending.source_id == source_id);
            if full_scan_pending_for_source && !full_scan_active_for_source {
                if watcher_replay_proven {
                    self.queue_targeted_sync(
                        source_id.clone(),
                        None,
                        paths,
                        None,
                        lifecycle_generation,
                        journal_checkpoint_event_id,
                        watcher_continuity_proof,
                    );
                } else {
                    self.queue_required_refresh_with_context(
                        source_id.clone(),
                        SourceRefreshCause::WatcherAuthorityUnproven,
                        lifecycle_generation,
                    );
                }
                tracing::info!(
                    target: "wavecrate::source_processing",
                    source_id,
                    path_count = paths.len(),
                    outcome = "subsumed_by_pending_full_scan",
                    "Source discovery causal plan merged watcher paths"
                );
                return SourceFilesystemChangePlan::DeferredAlreadyRunning { source_id };
            }
            if full_scan_active_for_source || self.active_targeted_syncs.contains_key(&source_id) {
                self.queue_targeted_sync(
                    source_id.clone(),
                    None,
                    paths,
                    None,
                    lifecycle_generation,
                    journal_checkpoint_event_id,
                    watcher_continuity_proof,
                );
                return SourceFilesystemChangePlan::DeferredAlreadyRunning { source_id };
            }
            if !watcher_replay_proven {
                return SourceFilesystemChangePlan::QueueRefresh {
                    source_id,
                    cause: SourceRefreshCause::WatcherAuthorityUnproven,
                };
            }
            return SourceFilesystemChangePlan::SyncPaths {
                source_id,
                changed_count: paths.len(),
            };
        }
        if self.active() {
            self.remove_pending_targeted_sync(&source_id);
            self.queue_required_refresh_with_context(
                source_id.clone(),
                SourceRefreshCause::WatcherOverflow,
                lifecycle_generation,
            );
            return SourceFilesystemChangePlan::DeferredAlreadyRunning { source_id };
        }
        SourceFilesystemChangePlan::QueueRefresh {
            source_id,
            cause: SourceRefreshCause::WatcherOverflow,
        }
    }

    pub(in crate::native_app) fn mark_targeted_sync_started(
        &mut self,
        source_id: &str,
        lifecycle_generation: u64,
    ) -> bool {
        if self.active_targeted_syncs.contains_key(source_id) {
            return false;
        }
        self.active_targeted_syncs
            .insert(source_id.to_string(), lifecycle_generation);
        true
    }

    pub(in crate::native_app) fn mark_targeted_sync_finished(
        &mut self,
        source_id: &str,
        lifecycle_generation: u64,
    ) {
        if self.active_targeted_syncs.get(source_id) == Some(&lifecycle_generation) {
            self.active_targeted_syncs.remove(source_id);
        }
    }

    #[cfg(test)]
    pub(in crate::native_app) fn targeted_sync_active_for_tests(&self, source_id: &str) -> bool {
        self.active_targeted_syncs.contains_key(source_id)
    }

    pub(in crate::native_app) fn next_pending_targeted_sync(
        &mut self,
    ) -> Option<PendingTargetedSourceSync> {
        let source_id = self
            .pending_targeted_syncs
            .keys()
            .find(|source_id| {
                !self.active_targeted_syncs.contains_key(*source_id)
                    && !self
                        .progress
                        .as_ref()
                        .is_some_and(|progress| &progress.source_id == *source_id)
                    && !self
                        .pending_refreshes
                        .iter()
                        .any(|pending| &pending.source_id == *source_id)
            })
            .cloned()?;
        let pending = self.pending_targeted_syncs.remove(&source_id)?;
        Some(PendingTargetedSourceSync {
            source_id: pending.source_id,
            scopes: pending.scopes,
            paths: pending.paths.into_iter().collect(),
            source_root_identity: pending.source_root_identity,
            lifecycle_generation: pending.lifecycle_generation,
            journal_checkpoint_event_id: pending.journal_checkpoint_event_id,
            watcher_continuity_proof: pending.watcher_continuity_proof,
            proofless_evidence_seen: pending.proofless_evidence_seen,
            audit_required: pending.audit_required,
            enqueued_at: pending.enqueued_at,
        })
    }

    #[cfg(test)]
    pub(in crate::native_app) fn plan_filesystem_change(
        &mut self,
        browser: &mut FolderBrowserState,
        source_id: String,
        paths: &[PathBuf],
        overflowed: bool,
        source_root_available: bool,
    ) -> SourceFilesystemChangePlan {
        self.plan_filesystem_change_for_generation(
            browser,
            source_id,
            paths,
            overflowed,
            source_root_available,
            None,
            None,
            None,
        )
    }

    pub(in crate::native_app) fn next_pending_refresh_context_if_idle(
        &mut self,
    ) -> Option<PendingSourceRefresh> {
        if self.active() {
            return None;
        }
        let pending_selection = self.pending_refreshes.iter().rposition(|pending| {
            pending.selection_requested
                && !self.active_targeted_syncs.contains_key(&pending.source_id)
        });
        pending_selection
            .and_then(|index| self.pending_refreshes.remove(index))
            .or_else(|| {
                let runnable = self.pending_refreshes.iter().rposition(|pending| {
                    !self.active_targeted_syncs.contains_key(&pending.source_id)
                })?;
                self.pending_refreshes.remove(runnable)
            })
            .map(|pending| PendingSourceRefresh {
                source_id: pending.source_id,
                cause: pending.cause,
                lifecycle_generation: pending.lifecycle_generation,
                enqueued_at: pending.enqueued_at,
            })
    }

    #[cfg(test)]
    pub(in crate::native_app) fn next_pending_refresh_if_idle(&mut self) -> Option<String> {
        self.next_pending_refresh_context_if_idle()
            .map(|pending| pending.source_id)
    }

    /// Retire every queued or visible scan projection for a removed source immediately.
    ///
    /// The worker is cancelled through the source-processing lifecycle token. Clearing the local
    /// owner here prevents a late completion from leaving the one-at-a-time global scan lane
    /// permanently active while the removed source no longer exists in the browser model.
    pub(in crate::native_app) fn retire_source(&mut self, source_id: &str) -> bool {
        self.remove_pending_refresh(source_id);
        self.remove_targeted_sync(source_id);
        self.retry_counts.remove(source_id);
        let active = self
            .progress
            .as_ref()
            .is_some_and(|progress| progress.source_id == source_id);
        if active {
            self.progress = None;
        }
        active
    }

    pub(in crate::native_app) fn cancel_active_scan_by_user(
        &mut self,
        browser: &mut FolderBrowserState,
    ) -> Option<(String, String)> {
        let progress = self.progress.take()?;
        let browser_owner_released = browser.cancel_scan(&progress.source_id, progress.task_id);
        let workflow_only_owner =
            matches!(progress.lifecycle, FolderScanLifecycle::PersistingResults);
        if !browser_owner_released && !workflow_only_owner {
            self.progress = Some(progress);
            return None;
        }
        self.remove_pending_refresh(&progress.source_id);
        self.retry_counts.remove(&progress.source_id);
        Some((progress.source_id, progress.label))
    }

    pub(in crate::native_app) fn fail_active_scan(
        &mut self,
        browser: &mut FolderBrowserState,
        task_id: u64,
        source_id: &str,
        lifecycle_generation: Option<u64>,
    ) -> Option<FolderScanProgress> {
        if !self.transition_current_scan(
            task_id,
            source_id,
            lifecycle_generation,
            FolderScanLifecycle::Failed,
            "Source scan stopped unexpectedly",
        ) || !browser.cancel_scan(source_id, task_id)
        {
            return None;
        }
        self.remove_pending_refresh(source_id);
        self.retry_counts.remove(source_id);
        self.progress.take()
    }

    pub(in crate::native_app) fn begin_filesystem_refresh_with_context(
        &mut self,
        browser: &mut FolderBrowserState,
        source_id: String,
        task_id: u64,
        cause: SourceRefreshCause,
        lifecycle_generation: Option<u64>,
    ) -> SourceRefreshRequest {
        if let (Some(required_revision), Some(accepted_revision)) = (
            cause.committed_revision(),
            browser.source_projection_revision(&source_id),
        ) && accepted_revision >= required_revision
        {
            self.remove_pending_refresh(&source_id);
            return SourceRefreshRequest::Covered {
                source_id,
                accepted_revision,
            };
        }
        if self.active_targeted_syncs.contains_key(&source_id) {
            self.remove_pending_targeted_sync(&source_id);
            self.queue_required_refresh_with_context(
                source_id.clone(),
                cause,
                lifecycle_generation,
            );
            return SourceRefreshRequest::Deferred { source_id };
        }
        if let Some(request) = browser.begin_source_scan(source_id.clone(), task_id) {
            return SourceRefreshRequest::Queued(request);
        }
        if !browser.source_exists(&source_id) || browser.source_is_missing(&source_id) {
            self.remove_pending_refresh(&source_id);
            return SourceRefreshRequest::IgnoredMissing { source_id };
        }
        self.queue_required_refresh_with_context(source_id.clone(), cause, lifecycle_generation);
        SourceRefreshRequest::Deferred { source_id }
    }

    #[cfg(test)]
    pub(in crate::native_app) fn begin_filesystem_refresh(
        &mut self,
        browser: &mut FolderBrowserState,
        source_id: String,
        task_id: u64,
    ) -> SourceRefreshRequest {
        self.begin_filesystem_refresh_with_context(
            browser,
            source_id,
            task_id,
            SourceRefreshCause::WatcherOverflow,
            None,
        )
    }

    fn queue_required_refresh_with_context(
        &mut self,
        source_id: String,
        cause: SourceRefreshCause,
        lifecycle_generation: Option<u64>,
    ) {
        self.queue_pending_refresh(source_id, false, true, cause, lifecycle_generation);
    }

    #[cfg(test)]
    fn queue_required_refresh(&mut self, source_id: String) {
        self.queue_required_refresh_with_context(
            source_id,
            SourceRefreshCause::WatcherOverflow,
            None,
        );
    }

    fn queue_selected_required_refresh_with_cause(
        &mut self,
        source_id: String,
        cause: SourceRefreshCause,
    ) {
        self.queue_pending_refresh(source_id, true, true, cause, None);
    }

    #[cfg(test)]
    fn queue_selected_required_refresh(&mut self, source_id: String) {
        self.queue_selected_required_refresh_with_cause(
            source_id,
            SourceRefreshCause::DeferredSelection,
        );
    }

    fn queue_pending_refresh(
        &mut self,
        source_id: String,
        selection_requested: bool,
        scan_required: bool,
        cause: SourceRefreshCause,
        lifecycle_generation: Option<u64>,
    ) {
        self.remove_pending_targeted_sync(&source_id);
        let previous = self
            .pending_refreshes
            .iter()
            .find(|pending| pending.source_id == source_id)
            .filter(|pending| pending.lifecycle_generation == lifecycle_generation)
            .map(|pending| {
                (
                    pending.selection_requested,
                    pending.scan_required,
                    pending.cause,
                    pending.lifecycle_generation,
                    pending.enqueued_at,
                )
            });
        self.remove_pending_refresh(&source_id);
        self.pending_refreshes.push_back(QueuedSourceRefresh {
            source_id,
            selection_requested: previous.is_some_and(|previous| previous.0) || selection_requested,
            scan_required: previous.is_some_and(|previous| previous.1) || scan_required,
            cause: previous.map_or(cause, |previous| previous.2.merge(cause)),
            lifecycle_generation: lifecycle_generation
                .or_else(|| previous.and_then(|previous| previous.3)),
            enqueued_at: previous.map_or_else(Instant::now, |previous| previous.4),
        });
    }

    fn clear_pending_selection(&mut self, source_id: &str) {
        if let Some(pending) = self
            .pending_refreshes
            .iter_mut()
            .find(|pending| pending.source_id == source_id)
        {
            pending.selection_requested = false;
        }
        self.pending_refreshes
            .retain(|pending| pending.selection_requested || pending.scan_required);
    }

    fn remove_pending_refresh(&mut self, source_id: &str) {
        self.pending_refreshes
            .retain(|pending| pending.source_id != source_id);
    }

    fn queue_targeted_sync(
        &mut self,
        source_id: String,
        scopes: Option<&[ReconciliationScope]>,
        paths: &[PathBuf],
        source_root_identity: Option<String>,
        lifecycle_generation: Option<u64>,
        journal_checkpoint_event_id: Option<u64>,
        watcher_continuity_proof: Option<WatcherContinuityProof>,
    ) {
        let pending = self
            .pending_targeted_syncs
            .entry(source_id.clone())
            .or_insert_with(|| QueuedTargetedSourceSync {
                source_id: source_id.clone(),
                scopes: None,
                paths: BTreeSet::new(),
                source_root_identity: None,
                lifecycle_generation,
                journal_checkpoint_event_id: None,
                watcher_continuity_proof: None,
                proofless_evidence_seen: false,
                audit_required: false,
                enqueued_at: Instant::now(),
            });
        if pending.lifecycle_generation != lifecycle_generation {
            pending.scopes = None;
            pending.paths.clear();
            pending.source_root_identity = None;
            pending.lifecycle_generation = lifecycle_generation;
            pending.journal_checkpoint_event_id = None;
            pending.watcher_continuity_proof = None;
            pending.proofless_evidence_seen = false;
            pending.audit_required = false;
            pending.enqueued_at = Instant::now();
        }
        let incoming_scopes = scopes.map(|scopes| coalesce_scopes(scopes.iter().cloned()));
        if let Some(incoming_scopes) = incoming_scopes {
            let incoming_scope_path_bytes = incoming_scopes
                .iter()
                .filter_map(|scope| scope.path())
                .map(|path| path.as_path().to_string_lossy().len())
                .sum::<usize>();
            let incoming_scope_is_safe = !incoming_scopes.is_empty()
                && incoming_scopes.len() <= MAX_TYPED_SCOPE_COUNT
                && incoming_scope_path_bytes <= MAX_TYPED_SCOPE_PATH_BYTES
                && incoming_scopes
                    .iter()
                    .all(|scope| scope.kind() != ReconciliationScopeKind::SourceAudit);
            if pending.scopes.is_none() && !pending.paths.is_empty() {
                pending.audit_required = true;
                pending.paths.clear();
            }
            if !incoming_scope_is_safe {
                pending.audit_required = true;
                pending.scopes = Some(Vec::new());
                pending.paths.clear();
            } else {
                let merged_scopes = coalesce_scopes(
                    pending
                        .scopes
                        .take()
                        .unwrap_or_default()
                        .into_iter()
                        .chain(incoming_scopes),
                );
                let merged_scope_path_bytes = merged_scopes
                    .iter()
                    .filter_map(|scope| scope.path())
                    .map(|path| path.as_path().to_string_lossy().len())
                    .sum::<usize>();
                if merged_scopes.len() > MAX_TYPED_SCOPE_COUNT
                    || merged_scope_path_bytes > MAX_TYPED_SCOPE_PATH_BYTES
                    || merged_scopes
                        .iter()
                        .any(|scope| scope.kind() == ReconciliationScopeKind::SourceAudit)
                {
                    pending.audit_required = true;
                    pending.scopes = Some(Vec::new());
                    pending.paths.clear();
                } else {
                    pending.scopes = Some(merged_scopes);
                    pending.paths.clear();
                    if pending.source_root_identity.is_none() {
                        pending.source_root_identity = source_root_identity.clone();
                    } else if pending.source_root_identity != source_root_identity {
                        pending.audit_required = true;
                        pending.scopes = Some(Vec::new());
                    }
                }
            }
        } else if pending.scopes.is_some() {
            // A legacy path event cannot narrow or supplement an already typed handoff.
            pending.audit_required = true;
            pending.paths.clear();
        } else {
            pending.paths.extend(paths.iter().cloned());
        }
        if journal_checkpoint_event_id.is_none() || watcher_continuity_proof.is_none() {
            pending.proofless_evidence_seen = true;
        }
        if !pending.audit_required {
            let incoming_boundary_is_valid = watcher_replay_evidence_is_well_formed(
                journal_checkpoint_event_id,
                watcher_continuity_proof.as_ref(),
            );
            if !incoming_boundary_is_valid {
                pending.audit_required = true;
                pending.journal_checkpoint_event_id = None;
                pending.watcher_continuity_proof = None;
            } else if let Some(event_id) = journal_checkpoint_event_id {
                let boundary_matches = pending.journal_checkpoint_event_id == Some(event_id)
                    && pending.watcher_continuity_proof.as_ref()
                        == watcher_continuity_proof.as_ref();
                if pending.journal_checkpoint_event_id.is_none() {
                    pending.journal_checkpoint_event_id = Some(event_id);
                    pending.watcher_continuity_proof = watcher_continuity_proof;
                } else if !boundary_matches {
                    pending.audit_required = true;
                    pending.journal_checkpoint_event_id = None;
                    pending.watcher_continuity_proof = None;
                }
            }
        }
        tracing::info!(
            target: "wavecrate::source_processing",
            source_id,
            scope_count = pending.scopes.as_ref().map_or(0, Vec::len),
            path_count = pending.paths.len(),
            lifecycle_generation = ?pending.lifecycle_generation,
            outcome = "coalesced_targeted_scope_or_compatibility_event",
            "Source discovery causal plan merged watcher evidence"
        );
    }

    fn remove_pending_targeted_sync(&mut self, source_id: &str) {
        self.pending_targeted_syncs.remove(source_id);
    }

    fn remove_targeted_sync(&mut self, source_id: &str) {
        self.remove_pending_targeted_sync(source_id);
        self.active_targeted_syncs.remove(source_id);
    }

    #[cfg(test)]
    pub(in crate::native_app) fn finish_scan(
        &mut self,
        browser: &mut FolderBrowserState,
        result: FolderScanResult,
    ) -> SourceScanFinish {
        self.finish_scan_with_lifecycle(browser, result, None, true)
    }

    pub(in crate::native_app) fn finish_scan_with_lifecycle(
        &mut self,
        browser: &mut FolderBrowserState,
        result: FolderScanResult,
        lifecycle_generation: Option<u64>,
        lifecycle_is_current: bool,
    ) -> SourceScanFinish {
        let source_id = result.source_id.clone();
        let task_id = result.task_id;
        let label = result.label.clone();
        let file_count = result.file_count;
        let folder_count = result.folder_count;
        let source_db_error = result.source_db_error.clone();
        let metadata_hydration_error = result.metadata_hydration.error().map(str::to_owned);
        let committed_delta = result.committed_delta.clone();
        let source_root_available = result.source_root_available;
        let queue_age_ms = self
            .progress
            .as_ref()
            .map_or(0, |progress| progress.queued_at.elapsed().as_millis());
        let last_progress_age_ms = self.progress.as_ref().map_or(0, |progress| {
            progress.last_progress_at.elapsed().as_millis()
        });
        let retry_count = self
            .progress
            .as_ref()
            .map_or(0, |progress| progress.retry_count);
        if result.cancelled {
            if browser.cancel_scan(&source_id, result.task_id) {
                self.progress = None;
                if !lifecycle_is_current {
                    tracing::info!(
                        target: "wavecrate::source_processing",
                        task_id,
                        source_id,
                        lifecycle_generation = ?lifecycle_generation,
                        queue_age_ms,
                        last_progress_age_ms,
                        retry_count,
                        outcome = "stale_lifecycle",
                        terminal_outcome = "stale",
                        "Discarding cancelled scan from a retired source generation"
                    );
                    return SourceScanFinish::Stale { label };
                }
                self.queue_required_refresh_with_context(
                    source_id.clone(),
                    SourceRefreshCause::ScanCancelled,
                    lifecycle_generation,
                );
                let next_retry_count = retry_count.saturating_add(1);
                self.retry_counts
                    .insert(source_id.clone(), next_retry_count);
                tracing::info!(
                    target: "wavecrate::source_processing",
                    task_id,
                    source_id,
                    lifecycle_generation = ?lifecycle_generation,
                    queue_age_ms,
                    last_progress_age_ms,
                    retry_count = next_retry_count,
                    terminal_outcome = "cancelled_retry_scheduled",
                    "Source scan reached a terminal outcome"
                );
                return SourceScanFinish::Cancelled { source_id, label };
            }
            tracing::info!(
                target: "wavecrate::source_processing",
                task_id,
                source_id,
                lifecycle_generation = ?lifecycle_generation,
                queue_age_ms,
                last_progress_age_ms,
                retry_count,
                terminal_outcome = "stale",
                "Discarding cancelled completion for a retired scan owner"
            );
            return SourceScanFinish::Stale { label };
        }
        let accepted_revision = result.metadata_hydration.revision();
        if browser.apply_scan_finished(result) {
            self.progress = None;
            self.retry_counts.remove(&source_id);
            if let Some(accepted_revision) = accepted_revision {
                self.discard_refresh_covered_by_revision(&source_id, accepted_revision);
            }
            if self.pending_refreshes.is_empty() {
                tracing::info!(
                    target: "wavecrate::source_processing",
                    source_id,
                    accepted_revision = ?accepted_revision,
                    task_id,
                    lifecycle_generation = ?lifecycle_generation,
                    queue_age_ms,
                    last_progress_age_ms,
                    retry_count,
                    outcome = "terminal_idle",
                    terminal_outcome = "complete",
                    "Source refresh convergence transition"
                );
            }
            SourceScanFinish::Applied {
                source_id,
                label,
                file_count,
                folder_count,
                source_db_error,
                metadata_hydration_error,
                committed_delta,
                source_root_available,
            }
        } else {
            tracing::info!(
                target: "wavecrate::source_processing",
                task_id,
                source_id,
                lifecycle_generation = ?lifecycle_generation,
                queue_age_ms,
                last_progress_age_ms,
                retry_count,
                terminal_outcome = "stale",
                "Discarding completion for a retired scan owner"
            );
            SourceScanFinish::Stale { label }
        }
    }

    pub(in crate::native_app) fn resume_progress_after_projection(
        &mut self,
        progress: FolderScanProgress,
    ) -> bool {
        if self.progress.is_some() || progress.lifecycle != FolderScanLifecycle::ApplyingResults {
            return false;
        }
        self.progress = Some(progress);
        true
    }

    fn discard_refresh_covered_by_revision(&mut self, source_id: &str, accepted_revision: u64) {
        let Some(pending) = self
            .pending_refreshes
            .iter()
            .find(|pending| pending.source_id == source_id)
        else {
            return;
        };
        let Some(required_revision) = pending.cause.committed_revision() else {
            return;
        };
        if accepted_revision < required_revision {
            return;
        }
        let queue_age_ms = pending.enqueued_at.elapsed().as_millis();
        tracing::info!(
            target: "wavecrate::source_processing",
            source_id,
            cause = pending.cause.label(),
            required_revision,
            accepted_revision,
            queue_age_ms,
            outcome = "covered_by_scan",
            "Suppressing covered source refresh"
        );
        self.remove_pending_refresh(source_id);
    }

    #[cfg(test)]
    pub(in crate::native_app) fn set_progress_for_tests(&mut self, progress: FolderScanProgress) {
        self.progress = Some(progress);
    }

    #[cfg(test)]
    pub(in crate::native_app) fn pending_refresh_contains_for_tests(
        &self,
        source_id: &str,
    ) -> bool {
        self.pending_refreshes
            .iter()
            .any(|pending| pending.source_id == source_id)
    }
}
