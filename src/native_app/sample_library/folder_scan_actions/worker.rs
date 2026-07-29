use std::{cell::Cell, time::Instant};

use radiant::prelude as ui;

use crate::native_app::{
    app::{
        FolderScanWorkerEvent, GuiMessage, NativeAppState, SourceScanFinish, emit_gui_action,
        run_folder_scan_worker,
    },
    sample_library::folder_browser::scan::{
        FolderScanLifecycle, FolderScanProgress, FolderScanRequest, PreparedFolderScanResult,
        reserve_source_scan_cache_revision,
    },
    sample_library::source_prep::{
        CacheWarmIntent, MetadataRefreshIntent, ReadinessIntent, SourceFeedbackIntent,
        SourcePrepIntents, SourcePriorityIntent,
    },
    source_processing::{ExternalScanHandoff, SourceScanAdmissionState},
};
use wavecrate::sample_sources::config::{AppConfig, reserve_save_revision};
use wavecrate_scan::sample_sources::scanner::UncoordinatedScanWriter;

use super::maintenance::{
    FolderScanCompletionContext, FolderScanMaintenanceRequest, FolderScanMaintenanceResult,
    persist_folder_scan_maintenance_recovering,
};

pub(in crate::native_app) const SOURCE_SCAN_COMPLETION_PREP_INTENTS: SourcePrepIntents =
    SourcePrepIntents {
        readiness: ReadinessIntent::Preserve,
        priority: SourcePriorityIntent::PromoteIfSelected,
        metadata_refresh: MetadataRefreshIntent::Force,
        refresh_waveform_cache_projection_if_selected: true,
        cache_warm: CacheWarmIntent::Preserve,
        feedback: SourceFeedbackIntent::Preserve,
    };
pub(in crate::native_app) const SOURCE_SCAN_COMPLETION_PREP_REASON: &str = "source_scan_finished";

impl NativeAppState {
    #[cfg(test)]
    pub(in crate::native_app) fn launch_folder_scan(
        &mut self,
        request: FolderScanRequest,
        context: &mut ui::UiUpdateContext<GuiMessage>,
    ) {
        self.launch_folder_scan_with_cause(request, "test", context);
    }

    pub(in crate::native_app) fn launch_folder_scan_with_cause(
        &mut self,
        mut request: FolderScanRequest,
        enqueue_cause: &str,
        context: &mut ui::UiUpdateContext<GuiMessage>,
    ) {
        request.rating_decay_weeks = self.ui.settings.persisted.controls.rating_decay_weeks;
        let started_at = Instant::now();
        let label = request.label.clone();
        let root = request.root.display().to_string();
        let source = self
            .library
            .folder_browser
            .configured_sample_sources()
            .into_iter()
            .find(|source| source.id.as_str() == request.source_id);
        let admission_generation = source.as_ref().and_then(|source| {
            match self
                .background
                .source_processing
                .register_source_for_scan(source.clone())
            {
                Ok(generation) => Some(generation),
                Err(error) => {
                    tracing::info!(
                        target: "wavecrate::source_processing",
                        source_id = request.source_id.as_str(),
                        error,
                        "Folder scan admission will wait on the background worker"
                    );
                    None
                }
            }
        });
        if let Some(generation) = admission_generation {
            self.background
                .source_lifecycle_generations
                .insert(request.source_id.clone(), generation);
        }
        if source.is_none() {
            tracing::error!(
                target: "wavecrate::source_processing",
                source_id = request.source_id.as_str(),
                "Folder scan will be cancelled because the source is not configured"
            );
        }
        self.library.start_folder_scan(&request);
        self.ui.status.sample = format!("Queued source scan for {}", request.label);
        tracing::info!(
            source = label,
            root = root,
            task_id = request.task_id,
            enqueue_cause,
            "default gui: folder scan queued"
        );
        emit_gui_action(
            "folder_browser.scan.queue",
            Some("folder_browser"),
            Some(&label),
            "queued",
            started_at,
            None,
        );
        let budget = self.background.source_processing.budget_handle();
        let source_id = request.source_id.clone();
        // Keep this stream fully ordered: discovery batches must not be
        // replaced by progress.
        context.business().background("gui-folder-scan").stream(
            move |_context, events| {
                let task_id = request.task_id;
                let progress_source_id = request.source_id.clone();
                let progress_label = request.label.clone();
                let recovery_request = request.clone();
                let recovery_events = events.clone();
                let recovery_generation = Cell::new(admission_generation);
                let emit_lifecycle =
                    |events: &ui::BusinessEventSink<FolderScanWorkerEvent>,
                     lifecycle: FolderScanLifecycle,
                     detail: &str,
                     generation: Option<u64>| {
                        let mut progress = FolderScanProgress::transition(
                            task_id,
                            progress_source_id.clone(),
                            progress_label.clone(),
                            lifecycle,
                            detail,
                        );
                        progress.lifecycle_generation = generation;
                        let _ = events.emit(FolderScanWorkerEvent::Progress(progress));
                    };
                let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    let Some(source) = source else {
                        emit_lifecycle(
                            &events,
                            FolderScanLifecycle::Failed,
                            "Source is no longer configured",
                            None,
                        );
                        let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
                        return run_folder_scan_worker(
                            request,
                            events,
                            cancel,
                            None,
                            &UncoordinatedScanWriter,
                        );
                    };
                    if admission_generation.is_none() {
                        emit_lifecycle(
                            &events,
                            FolderScanLifecycle::WaitingForSourceRegistration,
                            "Waiting for source replacement to finish",
                            None,
                        );
                    }
                    let generation = match match admission_generation {
                        Some(generation) => Ok(generation),
                        None => budget.register_source_for_scan_waiting(source),
                    } {
                        Ok(generation) => generation,
                        Err(error) => {
                            tracing::error!(
                                target: "wavecrate::source_processing",
                                source_id,
                                error,
                                "Folder scan was cancelled because source admission failed"
                            );
                            emit_lifecycle(
                                &events,
                                FolderScanLifecycle::Failed,
                                "Source registration failed",
                                None,
                            );
                            let cancel =
                                std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
                            return run_folder_scan_worker(
                                request,
                                events,
                                cancel,
                                None,
                                &UncoordinatedScanWriter,
                            );
                        }
                    };
                    recovery_generation.set(Some(generation));
                    let Some(permit) = budget.acquire_scan_for_generation_with_state(
                        &source_id,
                        generation,
                        |state| {
                            let (lifecycle, detail) = match state {
                                SourceScanAdmissionState::WaitingForSourceActivation => (
                                    FolderScanLifecycle::WaitingForSourceRegistration,
                                    "Waiting for source replacement to finish",
                                ),
                                SourceScanAdmissionState::WaitingForCapacity { current_owner } => (
                                    FolderScanLifecycle::WaitingForScanCapacity { current_owner },
                                    "Queued behind another source reconciliation",
                                ),
                                SourceScanAdmissionState::Admitted => {
                                    (FolderScanLifecycle::Scanning, "Source access acquired")
                                }
                            };
                            emit_lifecycle(&events, lifecycle, detail, Some(generation));
                        },
                    ) else {
                        emit_lifecycle(
                            &events,
                            FolderScanLifecycle::Canceled,
                            "Source scan canceled before admission",
                            Some(generation),
                        );
                        let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
                        return run_folder_scan_worker(
                            request,
                            events,
                            cancel,
                            Some(generation),
                            &UncoordinatedScanWriter,
                        );
                    };
                    let cancel = permit.cancel_token();
                    let scan_writer = permit.scan_writer();
                    let completion_events = events.clone();
                    let mut result = run_folder_scan_worker(
                        request,
                        events,
                        cancel,
                        Some(generation),
                        &scan_writer,
                    );
                    result.lifecycle_generation = Some(generation);
                    if result.scan.cancelled {
                        emit_lifecycle(
                            &completion_events,
                            FolderScanLifecycle::RetryScheduled,
                            "The interrupted source scan will retry safely",
                            Some(generation),
                        );
                    }
                    let handoff = foreground_scan_handoff(&result.scan);
                    permit.release_after_handoff(handoff);
                    result
                }));
                match outcome {
                    Ok(result) => result,
                    Err(payload) => {
                        let panic_detail = payload
                            .downcast_ref::<&str>()
                            .copied()
                            .or_else(|| payload.downcast_ref::<String>().map(String::as_str))
                            .unwrap_or("unknown panic");
                        tracing::error!(
                            target: "wavecrate::source_processing",
                            task_id,
                            source_id = progress_source_id,
                            panic_detail,
                            terminal_outcome = "worker_panicked",
                            "Source scan worker panic converted into a failed terminal result"
                        );
                        emit_lifecycle(
                            &recovery_events,
                            FolderScanLifecycle::Failed,
                            "Source scan stopped unexpectedly",
                            recovery_generation.get(),
                        );
                        let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
                        let mut recovery = run_folder_scan_worker(
                            recovery_request,
                            recovery_events,
                            cancel,
                            recovery_generation.get(),
                            &UncoordinatedScanWriter,
                        );
                        recovery.lifecycle_generation = recovery_generation.get();
                        recovery.terminal_failure =
                            Some(String::from("Source scan worker stopped unexpectedly"));
                        recovery
                    }
                }
            },
            folder_scan_worker_event_message,
            GuiMessage::FolderScanFinished,
        );
    }

    pub(in crate::native_app) fn finish_folder_scan(
        &mut self,
        prepared: impl Into<PreparedFolderScanResult>,
        context: &mut ui::UiUpdateContext<GuiMessage>,
    ) {
        let mut prepared = prepared.into();
        let started_at = Instant::now();
        let lifecycle_generation = prepared.lifecycle_generation;
        let terminal_failure = prepared.terminal_failure.take();
        let task_id = prepared.scan.task_id;
        let source_id = prepared.scan.source_id.clone();
        let mut lifecycle_is_current = true;
        if let Some(generation) = lifecycle_generation {
            if self
                .background
                .source_processing
                .lifecycle_generations()
                .get(&source_id)
                == Some(&generation)
            {
                self.background
                    .source_lifecycle_generations
                    .insert(source_id.clone(), generation);
            } else {
                lifecycle_is_current = false;
                tracing::debug!(
                    source_id,
                    lifecycle_generation = generation,
                    "Rejecting folder projection from an inactive source generation"
                );
                prepared.scan.cancelled = true;
            }
        }
        if let Some(failure) = terminal_failure {
            if lifecycle_is_current {
                if let Some(progress) =
                    self.library
                        .fail_active_folder_scan(task_id, &source_id, lifecycle_generation)
                {
                    self.ui.chrome.job_details_open = false;
                    self.background.progress_tick = 0.0;
                    self.background
                        .source_processing
                        .finish_foreground_source_refresh(&source_id, "source_scan_worker_failed");
                    self.maybe_run_pending_source_refresh(context);
                    self.ui.status.sample = failure.clone();
                    tracing::error!(
                        target: "wavecrate::source_processing",
                        task_id,
                        source_id,
                        lifecycle_generation = ?lifecycle_generation,
                        queue_age_ms = progress.queued_at.elapsed().as_millis(),
                        last_progress_age_ms = progress.last_progress_at.elapsed().as_millis(),
                        retry_count = progress.retry_count,
                        terminal_outcome = "failed",
                        error = failure,
                        "Source scan worker reached a failed terminal outcome"
                    );
                }
                return;
            }
        }
        if lifecycle_is_current {
            self.library.transition_folder_scan(
                task_id,
                &source_id,
                lifecycle_generation,
                FolderScanLifecycle::ApplyingResults,
                "Applying the refreshed source view",
            );
        }
        let applying_progress = self.library.folder_progress().cloned();
        let scan_cache_update = prepared.scan_cache_update;
        let rating_decay_maintenance = prepared.rating_decay_maintenance;
        match self.library.finish_folder_scan(
            prepared.scan,
            lifecycle_generation,
            lifecycle_is_current,
        ) {
            SourceScanFinish::Applied {
                source_id,
                label,
                file_count,
                folder_count,
                source_db_error,
                metadata_hydration_error,
                committed_delta,
                source_root_available,
            } => {
                let Some(lifecycle_generation) = lifecycle_generation else {
                    tracing::debug!(
                        source_id,
                        "Discarding folder scan completion without a captured source lifecycle"
                    );
                    return;
                };
                if let Some(progress) = applying_progress {
                    self.library
                        .resume_folder_scan_progress_after_projection(progress);
                }
                let completion = FolderScanCompletionContext {
                    task_id,
                    source_id: source_id.clone(),
                    label: label.clone(),
                    lifecycle_generation: Some(lifecycle_generation),
                    source_root_available,
                    source_db_error: source_db_error.clone(),
                    metadata_hydration_error: metadata_hydration_error.clone(),
                };
                self.queue_folder_scan_maintenance(
                    completion,
                    source_root_available
                        .then_some(prepared.audio_file_paths)
                        .unwrap_or_default(),
                    scan_cache_update,
                    rating_decay_maintenance.filter(|_| source_root_available),
                    context,
                );
                self.apply_finished_folder_scan(
                    AppliedFolderScan {
                        source_id,
                        label,
                        lifecycle_generation,
                        file_count,
                        folder_count,
                        source_db_error,
                        metadata_hydration_error,
                        committed_delta,
                        source_root_available,
                    },
                    started_at,
                    context,
                );
            }
            SourceScanFinish::Stale { label } => {
                emit_gui_action(
                    "folder_browser.scan.finish",
                    Some("folder_browser"),
                    Some(&label),
                    "stale",
                    started_at,
                    None,
                );
            }
            SourceScanFinish::Cancelled { source_id, label } => {
                self.ui.status.sample = format!("Paused source scan for {label}");
                emit_gui_action(
                    "folder_browser.scan.finish",
                    Some("folder_browser"),
                    Some(&label),
                    "cancelled",
                    started_at,
                    Some("source_processing_cancelled"),
                );
                self.background
                    .source_processing
                    .wake_source(&source_id, "external_scan_cancelled");
            }
        }
    }

    fn queue_folder_scan_maintenance(
        &mut self,
        completion: FolderScanCompletionContext,
        audio_file_paths: Vec<std::path::PathBuf>,
        scan_cache_update: crate::native_app::sample_library::folder_browser::scan::FolderScanCacheUpdate,
        rating_decay: Option<
            crate::native_app::sample_library::folder_browser::scan::RatingDecayMaintenanceRequest,
        >,
        context: &mut ui::UiUpdateContext<GuiMessage>,
    ) {
        self.library.transition_folder_scan(
            completion.task_id,
            &completion.source_id,
            completion.lifecycle_generation,
            FolderScanLifecycle::PersistingResults,
            "Saving the refreshed source state",
        );
        let sources = self.library.folder_browser.configured_sample_sources();
        let request = FolderScanMaintenanceRequest {
            completion,
            config: AppConfig {
                sources: sources.clone(),
                core: self.current_settings_core(),
            },
            config_revision: reserve_save_revision().map_err(|error| error.to_string()),
            sources,
            audio_file_paths,
            scan_cache_update,
            scan_cache_revision: reserve_source_scan_cache_revision(),
            rating_decay,
        };
        #[cfg(test)]
        {
            let result = persist_folder_scan_maintenance_recovering(request.clone());
            self.finish_folder_scan_maintenance(result, context);
        }
        #[cfg(not(test))]
        context
            .business()
            .background("gui-folder-scan-maintenance")
            .run(
                move |_| persist_folder_scan_maintenance_recovering(request),
                GuiMessage::FolderScanMaintenanceFinished,
            );
    }

    pub(in crate::native_app) fn finish_folder_scan_maintenance(
        &mut self,
        result: FolderScanMaintenanceResult,
        context: &mut ui::UiUpdateContext<GuiMessage>,
    ) {
        let completion = result.completion.clone();
        let persistence_error = result.persistence_error();
        let has_optional_errors = persistence_error.is_some()
            || !result.harvest_errors.is_empty()
            || result.rating_decay_error.is_some();
        let rating_decay_refresh = (result.rating_decay_updated_count > 0)
            .then(|| result.rating_decay_source_id.clone())
            .flatten();
        if let Some(error) = persistence_error.as_deref() {
            self.ui.status.sample = format!("Settings not saved: {error}");
            emit_gui_action(
                "folder_browser.sources.persist",
                Some("settings"),
                None,
                "persist_error",
                Instant::now(),
                Some(&error),
            );
        }
        if let Some(error) = result.config_error.as_deref() {
            tracing::warn!("failed to persist source configuration after scan: {error}");
        }
        if let Some(error) = result.scan_cache_error.as_deref() {
            tracing::warn!("failed to persist source scan cache after scan: {error}");
        }
        for error in &result.harvest_errors {
            tracing::warn!("{error}");
        }
        if let Some(error) = result.rating_decay_error.as_deref() {
            tracing::warn!("rating decay maintenance failed: {error}");
        }
        if let Some(completion) = completion {
            let (terminal, terminal_outcome) =
                classify_terminal_outcome(&completion, has_optional_errors);
            if has_optional_errors
                && persistence_error.is_none()
                && completion.metadata_hydration_error.is_none()
            {
                self.ui.status.sample = format!(
                    "Loaded source {} with maintenance warnings",
                    completion.label
                );
            }
            if let Some(progress) = self.library.finish_folder_scan_terminal(
                completion.task_id,
                &completion.source_id,
                completion.lifecycle_generation,
                terminal,
            ) {
                self.ui.chrome.job_details_open = false;
                self.background.progress_tick = 0.0;
                self.background
                    .source_processing
                    .finish_foreground_source_refresh(
                        &completion.source_id,
                        "source_scan_terminal",
                    );
                self.maybe_run_pending_source_refresh(context);
                tracing::info!(
                    target: "wavecrate::source_processing",
                    task_id = completion.task_id,
                    source_id = completion.source_id,
                    lifecycle_generation = ?completion.lifecycle_generation,
                    queue_age_ms = progress.queued_at.elapsed().as_millis(),
                    last_progress_age_ms = progress.last_progress_at.elapsed().as_millis(),
                    retry_count = progress.retry_count,
                    terminal_outcome,
                    "Source scan maintenance reached its terminal outcome"
                );
            } else {
                tracing::info!(
                    target: "wavecrate::source_processing",
                    task_id = completion.task_id,
                    source_id = completion.source_id,
                    lifecycle_generation = ?completion.lifecycle_generation,
                    terminal_outcome = "stale",
                    "Discarding maintenance completion for a retired scan owner"
                );
            }
        }
        if let Some(source_id) = rating_decay_refresh {
            self.refresh_source(source_id, context);
        }
        self.reapply_desired_rating_overlay();
    }

    fn apply_finished_folder_scan(
        &mut self,
        scan: AppliedFolderScan,
        started_at: Instant,
        context: &mut ui::UiUpdateContext<GuiMessage>,
    ) {
        self.reapply_desired_rating_overlay();
        if !scan.source_root_available {
            self.ui.status.sample = format!("Source missing: {}", scan.label);
            emit_gui_action(
                "folder_browser.scan.finish",
                Some("folder_browser"),
                Some(&scan.label),
                "missing",
                started_at,
                Some("source_root_missing"),
            );
            self.sync_source_watcher();
            return;
        }
        if let Some(error) = scan.source_db_error {
            self.background
                .source_processing
                .wake_source_for_full_reconciliation(
                    &scan.source_id,
                    "folder_scan_index_incomplete",
                );
            self.ui.status.sample = format!(
                "Loaded source {}: {} files in {} folders, but indexing failed: {error}",
                scan.label, scan.file_count, scan.folder_count
            );
            emit_gui_action(
                "folder_browser.scan.source_db_sync",
                Some("folder_browser"),
                Some(&scan.label),
                "error",
                started_at,
                Some(&error),
            );
        } else if let Some(error) = scan.metadata_hydration_error {
            self.background
                .source_processing
                .wake_source_for_full_reconciliation(
                    &scan.source_id,
                    "folder_scan_metadata_hydration_failed",
                );
            self.ui.status.sample = format!(
                "Loaded source {}: {} files in {} folders; metadata refresh failed and the previous metadata was preserved: {error}",
                scan.label, scan.file_count, scan.folder_count
            );
            tracing::warn!(
                source_id = scan.source_id,
                "browser metadata hydration failed: {error}"
            );
            emit_gui_action(
                "folder_browser.scan.metadata_hydration",
                Some("folder_browser"),
                Some(&scan.label),
                "error",
                started_at,
                Some(&error),
            );
        } else {
            if let Some(committed_delta) = scan.committed_delta.as_ref() {
                self.background.source_processing.request_source_delta(
                    &scan.source_id,
                    scan.lifecycle_generation,
                    committed_delta,
                    "foreground_scan_committed_delta",
                );
            } else {
                self.background
                    .source_processing
                    .wake_source_for_full_reconciliation(
                        &scan.source_id,
                        "folder_scan_committed_delta_missing",
                    );
                tracing::warn!(
                    source_id = scan.source_id,
                    "Foreground scan completed without an authoritative committed delta"
                );
            }
            self.ui.status.sample = format!(
                "Loaded source {}: {} files in {} folders",
                scan.label, scan.file_count, scan.folder_count
            );
            self.queue_source_prep(
                scan.source_id.clone(),
                SOURCE_SCAN_COMPLETION_PREP_INTENTS,
                SOURCE_SCAN_COMPLETION_PREP_REASON,
                context,
            );
        }
        tracing::info!(
            source = scan.label,
            file_count = scan.file_count,
            folder_count = scan.folder_count,
            "default gui: folder scan finished"
        );
        emit_gui_action(
            "folder_browser.scan.finish",
            Some("folder_browser"),
            Some(&scan.label),
            "success",
            started_at,
            None,
        );
        self.sync_source_watcher();
        self.open_ready_audio_documents(context, started_at);
    }
}

fn foreground_scan_handoff(
    scan: &crate::native_app::sample_library::folder_browser::scan::FolderScanResult,
) -> ExternalScanHandoff {
    if scan.cancelled
        || !scan.source_root_available
        || scan.source_db_error.is_some()
        || scan.metadata_hydration.error().is_some()
    {
        return ExternalScanHandoff::FullReconciliation {
            reason: "foreground_source_scan_incomplete",
        };
    }
    scan.committed_delta.as_ref().map_or(
        ExternalScanHandoff::FullReconciliation {
            reason: "foreground_source_scan_missing_delta",
        },
        |delta| ExternalScanHandoff::CommittedDelta(delta.clone()),
    )
}

fn classify_terminal_outcome(
    completion: &FolderScanCompletionContext,
    has_optional_errors: bool,
) -> (FolderScanLifecycle, &'static str) {
    if !completion.source_root_available || completion.source_db_error.is_some() {
        (FolderScanLifecycle::Failed, "failed")
    } else if completion.metadata_hydration_error.is_some() || has_optional_errors {
        (
            FolderScanLifecycle::CompleteWithWarnings,
            "complete_with_warnings",
        )
    } else {
        (FolderScanLifecycle::Complete, "complete")
    }
}

#[cfg(test)]
mod lifecycle_tests {
    use super::*;

    fn completion() -> FolderScanCompletionContext {
        FolderScanCompletionContext {
            task_id: 7,
            source_id: String::from("source"),
            label: String::from("Samples"),
            lifecycle_generation: Some(11),
            source_root_available: true,
            source_db_error: None,
            metadata_hydration_error: None,
        }
    }

    #[test]
    fn terminal_outcome_is_complete_only_for_clean_applied_and_persisted_results() {
        assert_eq!(
            classify_terminal_outcome(&completion(), false),
            (FolderScanLifecycle::Complete, "complete")
        );

        let mut missing = completion();
        missing.source_root_available = false;
        assert_eq!(
            classify_terminal_outcome(&missing, false),
            (FolderScanLifecycle::Failed, "failed")
        );

        let mut metadata = completion();
        metadata.metadata_hydration_error = Some(String::from("legacy metadata unavailable"));
        assert_eq!(
            classify_terminal_outcome(&metadata, false),
            (
                FolderScanLifecycle::CompleteWithWarnings,
                "complete_with_warnings"
            )
        );

        assert_eq!(
            classify_terminal_outcome(&completion(), true),
            (
                FolderScanLifecycle::CompleteWithWarnings,
                "complete_with_warnings"
            )
        );
    }
}

struct AppliedFolderScan {
    source_id: String,
    label: String,
    lifecycle_generation: u64,
    file_count: usize,
    folder_count: usize,
    source_db_error: Option<String>,
    metadata_hydration_error: Option<String>,
    committed_delta: Option<wavecrate::sample_sources::scanner::CommittedSourceDelta>,
    source_root_available: bool,
}

fn folder_scan_worker_event_message(event: FolderScanWorkerEvent) -> GuiMessage {
    match event {
        FolderScanWorkerEvent::Progress(progress) => GuiMessage::FolderScanProgress(progress),
        FolderScanWorkerEvent::DiscoveryBatch(batch) => GuiMessage::FolderScanDiscoveryBatch(batch),
    }
}
