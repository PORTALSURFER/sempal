use std::{path::PathBuf, time::Instant};

use radiant::prelude as ui;

use crate::native_app::app::{
    GuiMessage, NativeAppState, SourceFilesystemChangePlan, SourceFilesystemSyncResult,
    SourceRefreshCause, SourceRefreshRequest, emit_gui_action,
};
use crate::native_app::sample_library::folder_scan_actions::filesystem_refresh_worker::{
    capture_source_root_identity, recover_source_filesystem_sync,
    sync_source_database_paths_with_writer,
};
use crate::native_app::sample_library::source_prep::{
    CacheWarmIntent, MetadataRefreshIntent, ReadinessIntent, SourceFeedbackIntent,
    SourcePrepIntents, SourcePriorityIntent,
};
use crate::native_app::sample_library::source_watcher::{
    CheckpointCause, RevisionBoundCheckpoint, WatcherContinuityProof,
};
use crate::native_app::source_processing::{
    ExternalScanHandoff, manifest_delta_requires_browser_refresh,
};

pub(in crate::native_app) const FILESYSTEM_SYNC_PREP_INTENTS: SourcePrepIntents =
    SourcePrepIntents {
        readiness: ReadinessIntent::InvalidateAndRequestConvergence,
        priority: SourcePriorityIntent::PromoteIfSelected,
        metadata_refresh: MetadataRefreshIntent::Force,
        refresh_waveform_cache_projection_if_selected: true,
        cache_warm: CacheWarmIntent::Preserve,
        feedback: SourceFeedbackIntent::Preserve,
    };
pub(in crate::native_app) const FILESYSTEM_SYNC_PREP_REASON: &str = "filesystem_changed";

impl NativeAppState {
    pub(in crate::native_app) fn queue_full_source_reconciliation_after_committed_mutation(
        &mut self,
        source_id: String,
        committed_revision: u64,
        lifecycle_generation: u64,
        context: &mut ui::UiUpdateContext<GuiMessage>,
    ) {
        self.queue_filesystem_source_refresh(
            source_id,
            SourceRefreshCause::ProjectionRevisionGap { committed_revision },
            Some(lifecycle_generation),
            Instant::now(),
            context,
        );
    }

    pub(in crate::native_app) fn refresh_source_after_filesystem_change(
        &mut self,
        source_id: String,
        paths: Vec<PathBuf>,
        overflowed: bool,
        source_root_available: bool,
        journal_checkpoint_event_id: Option<u64>,
        watcher_continuity_proof: Option<WatcherContinuityProof>,
        context: &mut ui::UiUpdateContext<GuiMessage>,
    ) {
        let started_at = Instant::now();
        let lifecycle_generation = self
            .background
            .source_lifecycle_generations
            .get(&source_id)
            .copied();
        match self.library.plan_filesystem_change(
            source_id,
            &paths,
            overflowed,
            source_root_available,
            lifecycle_generation,
        ) {
            SourceFilesystemChangePlan::IgnoredSourceMissing { source_id } => {
                self.background
                    .source_processing
                    .wake_source(&source_id, "source_root_availability_changed");
                if source_id == self.library.folder_browser.selected_source_id() {
                    self.ui.status.sample = String::from("Source missing");
                }
                self.persist_user_configuration(
                    "folder_browser.source.availability_changed",
                    started_at,
                );
                emit_gui_action(
                    "folder_browser.source.filesystem_change",
                    Some("sources"),
                    Some(&source_id),
                    "ignored",
                    started_at,
                    Some("source_not_found"),
                );
            }
            SourceFilesystemChangePlan::SyncPaths {
                source_id,
                changed_count,
            } => {
                self.queue_source_filesystem_sync(
                    source_id.clone(),
                    paths,
                    changed_count,
                    journal_checkpoint_event_id,
                    watcher_continuity_proof,
                    context,
                );
                emit_gui_action(
                    "folder_browser.source.filesystem_change",
                    Some("sources"),
                    Some(&source_id),
                    "sync_queued",
                    started_at,
                    None,
                );
            }
            SourceFilesystemChangePlan::DeferredAlreadyRunning { source_id } => {
                emit_gui_action(
                    "folder_browser.source.filesystem_change",
                    Some("sources"),
                    Some(&source_id),
                    "deferred",
                    started_at,
                    Some("scan_already_running"),
                );
            }
            SourceFilesystemChangePlan::QueueRefresh { source_id } => {
                self.queue_filesystem_source_refresh(
                    source_id,
                    SourceRefreshCause::WatcherOverflow,
                    lifecycle_generation,
                    started_at,
                    context,
                );
            }
        }
    }

    pub(in crate::native_app) fn finish_source_filesystem_sync(
        &mut self,
        result: SourceFilesystemSyncResult,
        context: &mut ui::UiUpdateContext<GuiMessage>,
    ) {
        let source_id = result.source_id;
        let root_identity = result.root_identity;
        let journal_checkpoint_event_id = result.journal_checkpoint_event_id;
        let watcher_continuity_proof = result.watcher_continuity_proof;
        self.library
            .mark_targeted_source_sync_finished(&source_id, result.lifecycle_generation);
        if self.background.source_lifecycle_generations.get(&source_id)
            != Some(&result.lifecycle_generation)
        {
            tracing::debug!(
                source_id = %source_id,
                lifecycle_generation = result.lifecycle_generation,
                "Ignoring filesystem sync completion from an inactive source generation"
            );
            self.maybe_run_pending_source_refresh(context);
            return;
        }
        let changed_count = result.changed_count;
        if !self.library.folder_browser.source_exists(&source_id) {
            tracing::debug!(
                source_id = %source_id,
                "Ignoring stale filesystem sync completion for removed source"
            );
            self.maybe_run_pending_source_refresh(context);
            return;
        }
        match result.result {
            Ok(success) => {
                let renames_reconciled = success.renames_reconciled;
                let mut incomplete_error = success.incomplete_error;
                let mut incomplete_reconciliation_reason =
                    "filesystem_sync_incomplete_after_commit";
                let delta = success.committed_delta;
                let browser_delta_applied = if incomplete_error.is_none() {
                    match success.browser_projection_delta {
                        Some(projection) => self
                            .library
                            .folder_browser
                            .apply_committed_projection_delta(&source_id, projection),
                        None => false,
                    }
                } else {
                    false
                };
                let projection_accepted = if incomplete_error.is_none() && browser_delta_applied {
                    success
                        .projection_handoff_ticket
                        .as_ref()
                        .is_some_and(|ticket| ticket.accept())
                } else {
                    if let Some(ticket) = success.projection_handoff_ticket.as_ref() {
                        ticket.reject("projection_handoff_projection_rejected");
                    }
                    false
                };
                if incomplete_error.is_none() && !projection_accepted {
                    incomplete_error = Some(String::from(
                        "committed filesystem sync did not apply an exact browser projection",
                    ));
                    tracing::warn!(
                        source_id = %source_id,
                        revision = delta.revision,
                        "Falling back to a full browser projection after committed projection completion failed"
                    );
                }
                if !result.cancelled && projection_accepted && incomplete_error.is_none() {
                    match (
                        root_identity,
                        journal_checkpoint_event_id,
                        watcher_continuity_proof,
                    ) {
                        (Some(root_identity), Some(event_id), Some(proof)) => self
                            .background
                            .source_processing
                            .budget_handle()
                            .submit_watcher_checkpoint(RevisionBoundCheckpoint {
                                source_id: source_id.clone(),
                                lifecycle_generation: result.lifecycle_generation,
                                source_revision: delta.revision,
                                root_identity,
                                event_id,
                                cause: CheckpointCause::TargetedReplay,
                                continuity_proof: Some(proof),
                            }),
                        (Some(root_identity), Some(event_id), None) => self
                            .background
                            .source_processing
                            .budget_handle()
                            .submit_watcher_checkpoint(RevisionBoundCheckpoint {
                                source_id: source_id.clone(),
                                lifecycle_generation: result.lifecycle_generation,
                                source_revision: delta.revision,
                                root_identity,
                                event_id,
                                cause: CheckpointCause::TargetedReplay,
                                continuity_proof: None,
                            }),
                        (None, _, _) => {
                            incomplete_error = Some(String::from(
                                "targeted filesystem sync completed without worker-captured source root identity",
                            ));
                            incomplete_reconciliation_reason =
                                "targeted_sync_root_identity_missing";
                            tracing::warn!(
                                source_id = %source_id,
                                revision = delta.revision,
                                "Refusing targeted watcher checkpoint without worker-captured root identity"
                            );
                        }
                        (Some(_), None, _) => tracing::debug!(
                            source_id = %source_id,
                            revision = delta.revision,
                            "Skipping targeted watcher checkpoint because replay event identity is unavailable"
                        ),
                    }
                }
                self.reapply_desired_rating_overlay();
                tracing::info!(
                    source_id = %source_id,
                    revision = delta.revision,
                    created = delta.created.len(),
                    changed = delta.changed.len(),
                    moved = delta.moved.len(),
                    deleted = delta.deleted.len(),
                    renames_reconciled,
                    "Committed filesystem source delta"
                );
                if !result.cancelled
                    && !delta.is_empty()
                    && projection_accepted
                    && incomplete_error.is_none()
                {
                    self.ui.status.sample = format!("Synced {changed_count} filesystem change(s)");
                    self.queue_source_prep(
                        source_id.clone(),
                        FILESYSTEM_SYNC_PREP_INTENTS,
                        FILESYSTEM_SYNC_PREP_REASON,
                        context,
                    );
                }
                if result.cancelled || incomplete_error.is_some() {
                    self.background
                        .source_processing
                        .wake_source_for_full_reconciliation(
                            &source_id,
                            incomplete_reconciliation_reason,
                        );
                }
                if result.cancelled || incomplete_error.is_some() {
                    self.queue_filesystem_source_refresh(
                        source_id,
                        SourceRefreshCause::FilesystemSyncIncomplete,
                        Some(result.lifecycle_generation),
                        Instant::now(),
                        context,
                    );
                }
            }
            Err(error) => {
                tracing::warn!(
                    source_id = %source_id,
                    changed_count,
                    error = %error,
                    "Failed to sync source database after filesystem change"
                );
                if source_id == self.library.folder_browser.selected_source_id() {
                    self.ui.status.sample = format!("Source sync failed: {error}");
                }
                self.queue_filesystem_source_refresh(
                    source_id,
                    SourceRefreshCause::FilesystemSyncFailed,
                    Some(result.lifecycle_generation),
                    Instant::now(),
                    context,
                );
            }
        }
        self.maybe_run_pending_source_refresh(context);
    }

    pub(in crate::native_app) fn finish_source_manifest_audit(
        &mut self,
        source_id: String,
        lifecycle_generation: u64,
        committed_delta: wavecrate::sample_sources::scanner::CommittedSourceDelta,
        complete: bool,
        context: &mut ui::UiUpdateContext<GuiMessage>,
    ) {
        if self.background.source_lifecycle_generations.get(&source_id)
            != Some(&lifecycle_generation)
            || committed_delta.is_empty()
            || !self.library.folder_browser.source_exists(&source_id)
        {
            return;
        }
        if !complete {
            self.background
                .source_processing
                .wake_source_for_full_reconciliation(
                    &source_id,
                    "manifest_audit_incomplete_after_commit",
                );
            return;
        }
        self.background.source_processing.request_source_delta(
            &source_id,
            lifecycle_generation,
            &committed_delta,
            "manifest_audit_committed_delta",
        );
        match manifest_audit_followup(&committed_delta) {
            ManifestAuditFollowup::ReconcileImmediately => {
                tracing::debug!(
                    source_id = %source_id,
                    revision = committed_delta.revision,
                    "Skipping filesystem rescan for content-generation-only audit delta"
                );
            }
            ManifestAuditFollowup::RefreshBrowserThenReconcile => {
                tracing::info!(
                    source_id = %source_id,
                    revision = committed_delta.revision,
                    created = committed_delta.created.len(),
                    changed = committed_delta.changed.len(),
                    moved = committed_delta.moved.len(),
                    deleted = committed_delta.deleted.len(),
                    "Refreshing browser projection after periodic source audit"
                );
                // The folder refresh writes the same source database that
                // discovery reads. Its completion queues SourceScanFinished
                // reconciliation, so wait until the refresh releases the DB.
                self.queue_filesystem_source_refresh(
                    source_id,
                    SourceRefreshCause::ManifestAudit {
                        committed_revision: committed_delta.revision,
                    },
                    Some(lifecycle_generation),
                    Instant::now(),
                    context,
                );
            }
        }
    }

    pub(in crate::native_app) fn maybe_run_pending_source_refresh(
        &mut self,
        context: &mut ui::UiUpdateContext<GuiMessage>,
    ) {
        while let Some(pending) = self.library.next_pending_source_refresh_if_idle() {
            let current_generation = self
                .background
                .source_lifecycle_generations
                .get(&pending.source_id)
                .copied();
            if pending.lifecycle_generation.is_some()
                && pending.lifecycle_generation != current_generation
            {
                tracing::info!(
                    target: "wavecrate::source_processing",
                    source_id = pending.source_id,
                    cause = pending.cause.label(),
                    queued_generation = ?pending.lifecycle_generation,
                    current_generation = ?current_generation,
                    queue_age_ms = pending.enqueued_at.elapsed().as_millis(),
                    outcome = "stale_lifecycle",
                    "Suppressing stale pending source refresh"
                );
                continue;
            }
            self.queue_filesystem_source_refresh(
                pending.source_id,
                pending.cause,
                pending.lifecycle_generation,
                pending.enqueued_at,
                context,
            );
            break;
        }
        while let Some(pending) = self.library.next_pending_targeted_source_sync() {
            let current_generation = self
                .background
                .source_lifecycle_generations
                .get(&pending.source_id)
                .copied();
            if pending.lifecycle_generation.is_some()
                && pending.lifecycle_generation != current_generation
            {
                tracing::info!(
                    target: "wavecrate::source_processing",
                    source_id = pending.source_id,
                    queued_generation = ?pending.lifecycle_generation,
                    current_generation = ?current_generation,
                    queue_age_ms = pending.enqueued_at.elapsed().as_millis(),
                    outcome = "stale_lifecycle",
                    "Suppressing stale targeted source sync"
                );
                continue;
            }
            let changed_count = pending.paths.len();
            tracing::info!(
                target: "wavecrate::source_processing",
                source_id = pending.source_id,
                path_count = changed_count,
                lifecycle_generation = ?pending.lifecycle_generation,
                queue_age_ms = pending.enqueued_at.elapsed().as_millis(),
                outcome = "targeted_sync_admitted",
                "Source discovery causal plan admitted queued watcher paths"
            );
            self.queue_source_filesystem_sync(
                pending.source_id,
                pending.paths,
                changed_count,
                None,
                None,
                context,
            );
            break;
        }
    }

    fn queue_filesystem_source_refresh(
        &mut self,
        source_id: String,
        cause: SourceRefreshCause,
        lifecycle_generation: Option<u64>,
        started_at: Instant,
        context: &mut ui::UiUpdateContext<GuiMessage>,
    ) {
        let task_id = self.next_folder_task_id();
        match self.library.begin_filesystem_refresh(
            source_id.clone(),
            task_id,
            cause,
            lifecycle_generation,
        ) {
            SourceRefreshRequest::Queued(request) => {
                let label = request.label.clone();
                tracing::info!(
                    target: "wavecrate::source_processing",
                    source_id,
                    cause = cause.label(),
                    covered_revision = ?cause.committed_revision(),
                    lifecycle_generation = ?lifecycle_generation,
                    queue_age_ms = started_at.elapsed().as_millis(),
                    outcome = "scan_queued",
                    "Source refresh convergence transition"
                );
                emit_gui_action(
                    "folder_browser.source.filesystem_change",
                    Some("sources"),
                    Some(&label),
                    "scan_queued",
                    started_at,
                    None,
                );
                self.launch_folder_scan_with_cause(request, cause.label(), context);
            }
            SourceRefreshRequest::Deferred { source_id } => {
                tracing::info!(
                    target: "wavecrate::source_processing",
                    source_id,
                    cause = cause.label(),
                    covered_revision = ?cause.committed_revision(),
                    lifecycle_generation = ?lifecycle_generation,
                    queue_age_ms = started_at.elapsed().as_millis(),
                    outcome = "coalesced_while_active",
                    "Source refresh convergence transition"
                );
                emit_gui_action(
                    "folder_browser.source.filesystem_change",
                    Some("sources"),
                    Some(&source_id),
                    "deferred",
                    started_at,
                    Some("source_not_queued"),
                );
            }
            SourceRefreshRequest::Covered {
                source_id,
                accepted_revision,
            } => {
                tracing::info!(
                    target: "wavecrate::source_processing",
                    source_id,
                    cause = cause.label(),
                    covered_revision = ?cause.committed_revision(),
                    accepted_revision,
                    lifecycle_generation = ?lifecycle_generation,
                    queue_age_ms = started_at.elapsed().as_millis(),
                    outcome = "covered_before_queue",
                    "Suppressing covered source refresh"
                );
            }
            SourceRefreshRequest::IgnoredMissing { source_id } => {
                self.background
                    .source_processing
                    .finish_foreground_source_refresh(
                        &source_id,
                        "source_refresh_root_unavailable",
                    );
                emit_gui_action(
                    "folder_browser.source.filesystem_change",
                    Some("sources"),
                    Some(&source_id),
                    "ignored_missing",
                    started_at,
                    Some("source_root_missing"),
                );
            }
        }
    }

    fn queue_source_filesystem_sync(
        &mut self,
        source_id: String,
        paths: Vec<PathBuf>,
        changed_count: usize,
        journal_checkpoint_event_id: Option<u64>,
        watcher_continuity_proof: Option<WatcherContinuityProof>,
        context: &mut ui::UiUpdateContext<GuiMessage>,
    ) {
        if paths.is_empty() {
            return;
        }
        let (root, database_root, expected_lifecycle_generation) =
            match self.admit_source_filesystem_sync(&source_id) {
                Ok(admission) => admission,
                Err(error) => {
                    tracing::warn!(
                        target: "wavecrate::source_processing",
                        source_id,
                        error,
                        "Source filesystem sync was not admitted"
                    );
                    let lifecycle_generation = self
                        .background
                        .source_lifecycle_generations
                        .get(&source_id)
                        .copied();
                    self.queue_filesystem_source_refresh(
                        source_id,
                        SourceRefreshCause::FilesystemSyncFailed,
                        lifecycle_generation,
                        Instant::now(),
                        context,
                    );
                    return;
                }
            };
        if !self
            .library
            .mark_targeted_source_sync_started(&source_id, expected_lifecycle_generation)
        {
            self.library.plan_filesystem_change(
                source_id.clone(),
                &paths,
                false,
                true,
                Some(expected_lifecycle_generation),
            );
            return;
        }
        let budget = self.background.source_processing.budget_handle();
        context.business().background("gui-source-db-sync").run(
            move |_| {
                let Some(permit) =
                    budget.acquire_scan_for_generation(&source_id, expected_lifecycle_generation)
                else {
                    return SourceFilesystemSyncResult {
                        source_id,
                        lifecycle_generation: expected_lifecycle_generation,
                        changed_count,
                        root_identity: capture_source_root_identity(&root),
                        journal_checkpoint_event_id,
                        watcher_continuity_proof,
                        cancelled: true,
                        result: Err(String::from("Source filesystem sync canceled")),
                    };
                };
                let lifecycle_generation = permit.lifecycle_generation();
                let cancel = permit.cancel_token();
                let scan_writer = permit.scan_writer();
                let root_identity = capture_source_root_identity(&root);
                let recovery_source_id = source_id.clone();
                let mut result = recover_source_filesystem_sync(
                    recovery_source_id,
                    lifecycle_generation,
                    changed_count,
                    || {
                        sync_source_database_paths_with_writer(
                            source_id,
                            root,
                            database_root,
                            paths,
                            changed_count,
                            cancel.as_ref(),
                            &scan_writer,
                        )
                    },
                );
                result.root_identity = root_identity;
                result.journal_checkpoint_event_id = journal_checkpoint_event_id;
                result.watcher_continuity_proof = watcher_continuity_proof;
                let projection_ticket = match &mut result.result {
                    Ok(success)
                        if !result.cancelled
                            && success.incomplete_error.is_none()
                            && success.browser_projection_delta.is_some() =>
                    {
                        Some(
                            permit
                                .release_after_projection_handoff(success.committed_delta.clone()),
                        )
                    }
                    _ => {
                        permit.release_after_handoff(ExternalScanHandoff::FullReconciliation {
                            reason: "targeted_source_sync_incomplete",
                        });
                        None
                    }
                };
                if let Ok(success) = &mut result.result {
                    success.projection_handoff_ticket = projection_ticket;
                }
                result
            },
            GuiMessage::SourceFilesystemSyncFinished,
        );
    }

    pub(in crate::native_app) fn admit_source_filesystem_sync(
        &mut self,
        source_id: &str,
    ) -> Result<(PathBuf, PathBuf, u64), String> {
        let source = self
            .library
            .folder_browser
            .configured_sample_sources()
            .into_iter()
            .find(|source| source.id.as_str() == source_id)
            .ok_or_else(|| "Source is not present in the configured source set".to_string())?;
        let root = source.root.clone();
        let database_root = source
            .database_root()
            .map_err(|error| format!("Resolve source metadata location failed: {error}"))?;
        let lifecycle_generation = self
            .background
            .source_processing
            .register_source_for_scan(source)?;
        self.background
            .source_lifecycle_generations
            .insert(source_id.to_string(), lifecycle_generation);
        Ok((root, database_root, lifecycle_generation))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ManifestAuditFollowup {
    ReconcileImmediately,
    RefreshBrowserThenReconcile,
}

fn manifest_audit_followup(
    delta: &wavecrate::sample_sources::scanner::CommittedSourceDelta,
) -> ManifestAuditFollowup {
    if manifest_delta_requires_browser_refresh(delta) {
        ManifestAuditFollowup::RefreshBrowserThenReconcile
    } else {
        ManifestAuditFollowup::ReconcileImmediately
    }
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::{ManifestAuditFollowup, manifest_audit_followup};
    use crate::native_app::{
        app::{BrowserProjectionDelta, SourceFilesystemSyncResult, SourceFilesystemSyncSuccess},
        sample_library::folder_browser::{FolderBrowserState, scan::scan_source_with_progress},
        sample_library::source_watcher::CheckpointCause,
        test_support::state::NativeAppStateFixture,
    };
    use wavecrate::sample_sources::scanner::{CommittedSourceDelta, ManifestIdentityDelta};
    use wavecrate::sample_sources::{SampleSource, SourceId};
    use wavecrate_library::filesystem_identity::stable_filesystem_identity;

    #[test]
    fn content_generation_only_audit_reconciles_without_filesystem_rescan() {
        let delta = CommittedSourceDelta {
            revision: 7,
            changed: vec![ManifestIdentityDelta {
                identity: String::from("file-id"),
                relative_path: PathBuf::from("sample.wav"),
                content_generation: String::from("hash"),
                source_metadata_changed: false,
            }],
            ..CommittedSourceDelta::default()
        };

        assert_eq!(
            manifest_audit_followup(&delta),
            ManifestAuditFollowup::ReconcileImmediately
        );
    }

    #[test]
    fn source_metadata_change_reconciles_after_browser_refresh() {
        let delta = CommittedSourceDelta {
            revision: 8,
            changed: vec![ManifestIdentityDelta {
                identity: String::from("file-id"),
                relative_path: PathBuf::from("sample.wav"),
                content_generation: String::from("new-hash"),
                source_metadata_changed: true,
            }],
            ..CommittedSourceDelta::default()
        };

        assert_eq!(
            manifest_audit_followup(&delta),
            ManifestAuditFollowup::RefreshBrowserThenReconcile
        );
    }

    fn completion_test_state() -> (
        tempfile::TempDir,
        crate::native_app::app::NativeAppState,
        String,
        u64,
    ) {
        let root = tempfile::tempdir().expect("source root");
        std::fs::write(root.path().join("sample.wav"), [0_u8; 8]).expect("write sample");
        let mut browser = FolderBrowserState::from_root(root.path().to_path_buf());
        let request = browser
            .begin_add_source_path(root.path().to_path_buf(), 1)
            .expect("initial scan request");
        let source_id = request.source_id.clone();
        let result = scan_source_with_progress(request, |_| {}, |_| {});
        assert!(browser.apply_scan_finished(result));
        let source = SampleSource::new_with_id(
            SourceId::from_string(source_id.clone()),
            root.path().to_path_buf(),
        );
        let mut state = NativeAppStateFixture::default()
            .with_folder_browser(browser)
            .build();
        let generation = state
            .background
            .source_processing
            .register_source_for_scan(source)
            .expect("register source");
        state
            .background
            .source_lifecycle_generations
            .insert(source_id.clone(), generation);
        (root, state, source_id, generation)
    }

    fn incomplete_result(
        source_id: String,
        generation: u64,
        projection: Option<BrowserProjectionDelta>,
    ) -> SourceFilesystemSyncResult {
        SourceFilesystemSyncResult {
            source_id,
            lifecycle_generation: generation,
            changed_count: 1,
            root_identity: None,
            journal_checkpoint_event_id: Some(73),
            watcher_continuity_proof: None,
            cancelled: false,
            result: Ok(SourceFilesystemSyncSuccess {
                renames_reconciled: 0,
                incomplete_error: None,
                committed_delta: CommittedSourceDelta {
                    revision: 2,
                    created: vec![ManifestIdentityDelta {
                        identity: String::from("new-identity"),
                        relative_path: PathBuf::from("new.wav"),
                        content_generation: String::from("generation"),
                        source_metadata_changed: true,
                    }],
                    ..CommittedSourceDelta::default()
                },
                browser_projection_delta: projection,
                projection_handoff_ticket: None,
            }),
        }
    }

    fn stable_root_identity(root: &Path) -> String {
        let metadata = std::fs::metadata(root).expect("source metadata");
        stable_filesystem_identity(root, &metadata).expect("stable source root identity")
    }

    fn targeted_result_with_projection(
        state: &crate::native_app::app::NativeAppState,
        source_id: String,
        generation: u64,
        root_identity: Option<String>,
        projection_revision: u64,
    ) -> SourceFilesystemSyncResult {
        let committed_delta = CommittedSourceDelta {
            revision: projection_revision,
            ..CommittedSourceDelta::default()
        };
        let ticket = state
            .background
            .source_processing
            .budget_handle()
            .acquire_scan_for_generation(&source_id, generation)
            .expect("targeted replay scan permit")
            .release_after_projection_handoff(committed_delta.clone());
        SourceFilesystemSyncResult {
            source_id,
            lifecycle_generation: generation,
            changed_count: 1,
            root_identity,
            journal_checkpoint_event_id: Some(73),
            watcher_continuity_proof: None,
            cancelled: false,
            result: Ok(SourceFilesystemSyncSuccess {
                renames_reconciled: 0,
                incomplete_error: None,
                committed_delta,
                browser_projection_delta: Some(BrowserProjectionDelta {
                    manifest_revision: projection_revision,
                    snapshot_revision: projection_revision,
                    folders: Vec::new(),
                    removed_file_ids: Vec::new(),
                    upserted_files: Vec::new(),
                }),
                projection_handoff_ticket: Some(ticket),
            }),
        }
    }

    #[test]
    fn accepted_targeted_replay_submits_revision_bound_checkpoint_to_owner_queue() {
        let (root, mut state, source_id, generation) = completion_test_state();
        let current_revision = state
            .library
            .folder_browser
            .source_projection_revision(&source_id)
            .expect("current browser projection revision");
        let result = targeted_result_with_projection(
            &state,
            source_id.clone(),
            generation,
            Some(stable_root_identity(root.path())),
            current_revision.saturating_add(1),
        );
        let mut context = radiant::prelude::UiUpdateContext::default();

        state.finish_source_filesystem_sync(result, &mut context);

        let checkpoint = state
            .background
            .source_processing
            .budget_handle()
            .pending_watcher_checkpoint_for_tests()
            .expect("accepted targeted replay checkpoint");
        assert_eq!(checkpoint.source_id, source_id);
        assert_eq!(checkpoint.lifecycle_generation, generation);
        assert_eq!(checkpoint.source_revision, current_revision + 1);
        assert_eq!(checkpoint.root_identity, stable_root_identity(root.path()));
        assert_eq!(checkpoint.event_id, 73);
        assert_eq!(checkpoint.cause, CheckpointCause::TargetedReplay);
    }

    #[test]
    fn missing_worker_root_identity_requests_reconciliation_without_checkpoint() {
        let (_root, mut state, source_id, generation) = completion_test_state();
        let current_revision = state
            .library
            .folder_browser
            .source_projection_revision(&source_id)
            .expect("current browser projection revision");
        let result = targeted_result_with_projection(
            &state,
            source_id.clone(),
            generation,
            None,
            current_revision.saturating_add(1),
        );
        let mut context = radiant::prelude::UiUpdateContext::default();

        state.finish_source_filesystem_sync(result, &mut context);

        assert!(
            state
                .background
                .source_processing
                .budget_handle()
                .pending_watcher_checkpoint_for_tests()
                .is_none()
        );
        assert!(
            state
                .background
                .source_processing
                .source_dirty_for_tests(&source_id)
        );
    }

    #[test]
    fn stale_targeted_replay_completion_does_not_submit_checkpoint() {
        let (root, mut state, source_id, generation) = completion_test_state();
        let current_revision = state
            .library
            .folder_browser
            .source_projection_revision(&source_id)
            .expect("current browser projection revision");
        let mut result = targeted_result_with_projection(
            &state,
            source_id.clone(),
            generation,
            Some(stable_root_identity(root.path())),
            current_revision.saturating_add(1),
        );
        result.lifecycle_generation = generation.wrapping_sub(1);
        let mut context = radiant::prelude::UiUpdateContext::default();

        state.finish_source_filesystem_sync(result, &mut context);

        assert!(
            state
                .background
                .source_processing
                .budget_handle()
                .pending_watcher_checkpoint_for_tests()
                .is_none()
        );
    }

    #[test]
    fn rejected_projection_ticket_does_not_submit_checkpoint() {
        let (root, mut state, source_id, generation) = completion_test_state();
        let current_revision = state
            .library
            .folder_browser
            .source_projection_revision(&source_id)
            .expect("current browser projection revision");
        let result = targeted_result_with_projection(
            &state,
            source_id.clone(),
            generation,
            Some(stable_root_identity(root.path())),
            current_revision.saturating_add(2),
        );
        let mut context = radiant::prelude::UiUpdateContext::default();

        state.finish_source_filesystem_sync(result, &mut context);

        assert!(
            state
                .background
                .source_processing
                .budget_handle()
                .pending_watcher_checkpoint_for_tests()
                .is_none()
        );
        assert!(
            state
                .background
                .source_processing
                .source_dirty_for_tests(&source_id)
        );
    }

    #[test]
    fn stale_already_surpassed_projection_requests_recovery_without_checkpoint() {
        let (root, mut state, source_id, generation) = completion_test_state();
        let current_revision = state
            .library
            .folder_browser
            .source_projection_revision(&source_id)
            .expect("current browser projection revision");
        assert!(
            state
                .library
                .folder_browser
                .apply_committed_projection_delta(
                    &source_id,
                    BrowserProjectionDelta {
                        manifest_revision: current_revision + 1,
                        snapshot_revision: current_revision + 1,
                        folders: Vec::new(),
                        removed_file_ids: Vec::new(),
                        upserted_files: Vec::new(),
                    },
                )
        );

        let result = targeted_result_with_projection(
            &state,
            source_id.clone(),
            generation,
            Some(stable_root_identity(root.path())),
            current_revision,
        );
        let mut context = radiant::prelude::UiUpdateContext::default();

        state.finish_source_filesystem_sync(result, &mut context);

        assert!(
            state
                .background
                .source_processing
                .budget_handle()
                .pending_watcher_checkpoint_for_tests()
                .is_none(),
            "a stale projection must not acknowledge the watcher cursor"
        );
        assert!(
            state
                .background
                .source_processing
                .source_dirty_for_tests(&source_id),
            "a stale projection must request conservative source recovery"
        );
        assert!(state.library.folder_scan_active());
        assert!(state.library.source_watcher.is_none());
    }

    #[test]
    fn incomplete_completion_without_projection_schedules_full_recovery_without_side_effects() {
        let (_root, mut state, source_id, generation) = completion_test_state();
        let mut context = radiant::prelude::UiUpdateContext::default();

        state.finish_source_filesystem_sync(
            incomplete_result(source_id.clone(), generation, None),
            &mut context,
        );

        assert!(
            !state
                .background
                .source_processing
                .pending_source_delta_contains_identity_for_tests(&source_id, "new-identity")
        );
        assert!(
            state
                .background
                .source_processing
                .source_dirty_for_tests(&source_id)
        );
        assert!(state.library.folder_scan_active());
        assert!(state.library.source_watcher.is_none());
    }

    #[test]
    fn rejected_completion_projection_schedules_full_recovery_without_side_effects() {
        let (_root, mut state, source_id, generation) = completion_test_state();
        let mut context = radiant::prelude::UiUpdateContext::default();
        let rejected = BrowserProjectionDelta {
            manifest_revision: 4,
            snapshot_revision: 4,
            folders: Vec::new(),
            removed_file_ids: Vec::new(),
            upserted_files: Vec::new(),
        };

        state.finish_source_filesystem_sync(
            incomplete_result(source_id.clone(), generation, Some(rejected)),
            &mut context,
        );

        assert!(
            !state
                .background
                .source_processing
                .pending_source_delta_contains_identity_for_tests(&source_id, "new-identity")
        );
        assert!(
            state
                .background
                .source_processing
                .source_dirty_for_tests(&source_id)
        );
        assert!(state.library.folder_scan_active());
        assert!(state.library.source_watcher.is_none());
    }
}
