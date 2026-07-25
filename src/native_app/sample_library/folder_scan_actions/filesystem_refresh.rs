use std::{path::PathBuf, time::Instant};

use radiant::prelude as ui;

use crate::native_app::app::{
    GuiMessage, NativeAppState, SourceFilesystemChangePlan, SourceFilesystemSyncResult,
    SourceRefreshCause, SourceRefreshRequest, emit_gui_action,
};
use crate::native_app::sample_library::folder_scan_actions::filesystem_refresh_worker::{
    recover_source_filesystem_sync, sync_source_database_paths_with_writer,
};
use crate::native_app::sample_library::source_prep::{
    CacheWarmIntent, MetadataRefreshIntent, ReadinessIntent, SourceFeedbackIntent,
    SourcePrepIntents, SourcePriorityIntent,
};
use crate::native_app::source_processing::manifest_delta_requires_browser_refresh;

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
    pub(in crate::native_app) fn refresh_source_after_filesystem_change(
        &mut self,
        source_id: String,
        paths: Vec<PathBuf>,
        overflowed: bool,
        source_root_available: bool,
        journal_checkpoint_event_id: Option<u64>,
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
        let journal_checkpoint_event_id = result.journal_checkpoint_event_id;
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
                let incomplete_error = success.incomplete_error;
                let delta = success.committed_delta;
                let browser_delta_applied = success
                    .browser_projection_delta
                    .map(|projection| {
                        self.library
                            .folder_browser
                            .apply_committed_projection_delta(&source_id, projection)
                    })
                    .unwrap_or(false);
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
                self.background.source_processing.request_source_delta(
                    &source_id,
                    &delta,
                    "filesystem_sync_committed_delta",
                );
                if !delta.is_empty() && incomplete_error.is_none() {
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
                            "filesystem_sync_incomplete_after_commit",
                        );
                }
                if !result.cancelled
                    && incomplete_error.is_none()
                    && let (Some(watcher), Some(event_id)) = (
                        self.library.source_watcher.as_ref(),
                        journal_checkpoint_event_id,
                    )
                {
                    watcher.advance_journal_checkpoint(source_id.clone(), event_id);
                }
                if result.cancelled || incomplete_error.is_some() {
                    self.queue_filesystem_source_refresh(
                        source_id,
                        SourceRefreshCause::FilesystemSyncIncomplete,
                        Some(result.lifecycle_generation),
                        Instant::now(),
                        context,
                    );
                } else if !browser_delta_applied {
                    self.queue_filesystem_source_refresh(
                        source_id,
                        SourceRefreshCause::ProjectionRevisionGap {
                            committed_revision: delta.revision,
                        },
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
        context: &mut ui::UiUpdateContext<GuiMessage>,
    ) {
        if self.background.source_lifecycle_generations.get(&source_id)
            != Some(&lifecycle_generation)
            || committed_delta.is_empty()
            || !self.library.folder_browser.source_exists(&source_id)
        {
            return;
        }
        self.background.source_processing.request_source_delta(
            &source_id,
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
                        journal_checkpoint_event_id,
                        cancelled: true,
                        result: Err(String::from("Source filesystem sync canceled")),
                    };
                };
                let lifecycle_generation = permit.lifecycle_generation();
                let cancel = permit.cancel_token();
                let scan_writer = permit.scan_writer();
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
                result.journal_checkpoint_event_id = journal_checkpoint_event_id;
                drop(permit);
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
    use std::path::PathBuf;

    use super::{ManifestAuditFollowup, manifest_audit_followup};
    use wavecrate::sample_sources::scanner::{CommittedSourceDelta, ManifestIdentityDelta};

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
}
