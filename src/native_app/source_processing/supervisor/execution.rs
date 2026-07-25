#[cfg(test)]
use super::{
    ArtifactPublishOutcome, BTreeMap, READINESS_LEASE_SECONDS, ReadinessStore,
    SourceDatabaseConnectionRole, cancel_claim,
};
use super::{
    AtomicBool, ContentAuditActivity, ContentAuditBudget, ContentAuditStorage, DatabaseWriterGate,
    Duration, ExecutionOutcome, Instant, Ordering, RuntimeCandidate, RuntimeTask, ScanError,
    SourceDatabase, SourceProcessingActivity, SourceProcessingEvent, SourceProcessingLifecycle,
    SourceProcessingProgressEvent, audit_source_and_record_with_budget_and_progress_and_writer,
    execute_readiness_target, manifest_audit_source_row_active, now_epoch_seconds,
};

pub(super) fn execute_candidate(
    candidate: &RuntimeCandidate,
    lifecycle_generation: u64,
    cancel: &AtomicBool,
    database_writer: &DatabaseWriterGate,
    content_audit_activity: ContentAuditActivity,
    publish_event: &mut dyn FnMut(SourceProcessingEvent) -> bool,
) -> Result<ExecutionOutcome, String> {
    let result = match &candidate.task {
        RuntimeTask::ManifestAudit { accelerated } => {
            let database_root = candidate
                .source
                .database_root()
                .map_err(|error| error.to_string())?;
            if !candidate.source.root.is_dir() {
                tracing::info!(
                    target: "wavecrate::source_processing",
                    source_id = candidate.source.id.as_str(),
                    root = %candidate.source.root.display(),
                    "Skipping manifest audit because the source root became unavailable"
                );
                return Ok(ExecutionOutcome::Parked);
            }
            let database = SourceDatabase::open_for_background_job_with_database_root(
                &candidate.source.root,
                database_root,
            )
            .map_err(|error| error.to_string())?;
            let completed_at = now_epoch_seconds();
            let expected_files = database
                .list_manifest_entries()
                .map_err(|error| error.to_string())?
                .len();
            let content_budget = ContentAuditBudget::adaptive(
                expected_files,
                content_audit_activity,
                ContentAuditStorage::classify(&candidate.source.root),
                *accelerated,
            );
            let source_id = candidate.source.id.as_str().to_string();
            let source_root = candidate.source.root.clone();
            let audit_started_at = Instant::now();
            let mut last_progress_publish_at = None::<Instant>;
            let mut publish_progress = |checked: usize, path: &std::path::Path| {
                let publish_due = last_progress_publish_at.is_none_or(|published_at| {
                    published_at.elapsed() >= Duration::from_millis(250)
                });
                if !publish_due {
                    return;
                }
                let relative = path.strip_prefix(&source_root).unwrap_or(path);
                let total = expected_files.max(checked);
                publish_event(SourceProcessingEvent::Progress(
                    SourceProcessingProgressEvent {
                        lifecycle: SourceProcessingLifecycle::new(
                            source_id.clone(),
                            lifecycle_generation,
                        ),
                        source_row_active: manifest_audit_source_row_active(audit_started_at),
                        completed: checked.min(total),
                        total,
                        activity: SourceProcessingActivity::ManifestAudit {
                            checked: Some(checked),
                            relative_path: Some(relative.to_path_buf()),
                        },
                    },
                ));
                last_progress_publish_at = Some(Instant::now());
            };
            let (outcome, content_incomplete_error) =
                match audit_source_and_record_with_budget_and_progress_and_writer(
                    &database,
                    Some(cancel),
                    content_budget,
                    completed_at,
                    &mut publish_progress,
                    database_writer,
                ) {
                    Ok(outcome) => (outcome, None),
                    Err(ScanError::Incomplete { committed, error }) => (*committed, Some(error)),
                    Err(error) => {
                        publish_event(SourceProcessingEvent::ManifestAuditFinished {
                            lifecycle: SourceProcessingLifecycle::new(
                                candidate.source.id.as_str(),
                                lifecycle_generation,
                            ),
                            complete: false,
                        });
                        return Err(error.to_string());
                    }
                };
            if let Some(report) = outcome.content_audit.as_ref() {
                tracing::info!(
                    target: "wavecrate::source_processing",
                    event = "source_processing.content_audit_coverage",
                    source_id = candidate.source.id.as_str(),
                    rotation_id = report.rotation_id,
                    checkpoint_revision = report.checkpoint_revision,
                    retry_cursor = report.retry_cursor.as_str(),
                    retry_next = report.retry_next,
                    verified_entries = report.verified_entries,
                    total_entries = report.total_entries,
                    remaining_entries = report.remaining_entries,
                    verified_bytes = report.verified_bytes,
                    remaining_bytes = report.remaining_bytes,
                    bytes_read = report.bytes_read,
                    skipped_retry_entries = report.skipped_retry_entries,
                    oldest_unverified_age_seconds = report.oldest_unverified_age_seconds,
                    estimated_rotation_seconds = report.estimated_rotation_seconds,
                    last_rotation_seconds = report.last_rotation_seconds,
                    "Content verification coverage checkpoint committed"
                );
            }
            tracing::debug!(
                target: "wavecrate::source_processing",
                source_id = candidate.source.id.as_str(),
                revision = outcome.committed_delta.revision,
                created = outcome.committed_delta.created.len(),
                created_paths = ?outcome
                    .committed_delta
                    .created
                    .iter()
                    .map(|identity| identity.relative_path.as_path())
                    .collect::<Vec<_>>(),
                changed = outcome.committed_delta.changed.len(),
                moved = outcome.committed_delta.moved.len(),
                deleted = outcome.committed_delta.deleted.len(),
                deleted_paths = ?outcome
                    .committed_delta
                    .deleted
                    .iter()
                    .map(|identity| identity.relative_path.as_path())
                    .collect::<Vec<_>>(),
                "Periodic source manifest audit committed"
            );
            let browser_refresh_required = !outcome.committed_delta.is_empty()
                && crate::native_app::source_processing::manifest_delta_requires_browser_refresh(
                    &outcome.committed_delta,
                );
            let audit_published = publish_event(SourceProcessingEvent::ManifestAuditCommitted {
                lifecycle: SourceProcessingLifecycle::new(
                    candidate.source.id.as_str(),
                    lifecycle_generation,
                ),
                committed_delta: outcome.committed_delta,
            });
            let foreground_refresh_owns_reconciliation =
                browser_refresh_required && audit_published;
            publish_event(SourceProcessingEvent::ManifestAuditFinished {
                lifecycle: SourceProcessingLifecycle::new(
                    candidate.source.id.as_str(),
                    lifecycle_generation,
                ),
                complete: true,
            });
            if let Some(error) = content_incomplete_error {
                tracing::warn!(
                    target: "wavecrate::source_processing",
                    source_id = candidate.source.id.as_str(),
                    error,
                    "Manifest audit completed; content verification paused at its durable checkpoint"
                );
            }
            Ok(manifest_audit_execution_outcome(
                foreground_refresh_owns_reconciliation,
                false,
                cancel.load(Ordering::Acquire),
            ))
        }
        RuntimeTask::Readiness(target) => {
            execute_readiness_target(&candidate.source, target, cancel, database_writer)
        }
    };
    if matches!(
        result,
        Ok(ExecutionOutcome::CompletedAwaitingForegroundRefresh
            | ExecutionOutcome::FailedAwaitingForegroundRefresh)
    ) {
        result
    } else if cancel.load(Ordering::Acquire) {
        Ok(ExecutionOutcome::Cancelled)
    } else {
        result
    }
}

pub(super) fn manifest_audit_execution_outcome(
    foreground_refresh_owns_reconciliation: bool,
    incomplete: bool,
    cancelled: bool,
) -> ExecutionOutcome {
    match (
        foreground_refresh_owns_reconciliation,
        incomplete,
        cancelled,
    ) {
        (true, false, _) => ExecutionOutcome::CompletedAwaitingForegroundRefresh,
        (true, true, _) => ExecutionOutcome::FailedAwaitingForegroundRefresh,
        (false, _, true) => ExecutionOutcome::Cancelled,
        (false, false, false) => ExecutionOutcome::Completed,
        (false, true, false) => ExecutionOutcome::Failed,
    }
}

#[cfg(test)]
pub(super) fn execute_synthetic_candidate_for_profile(
    candidate: &RuntimeCandidate,
    cancel: &AtomicBool,
    connections: &mut BTreeMap<String, rusqlite::Connection>,
) -> Result<ExecutionOutcome, String> {
    let RuntimeTask::Readiness(target) = &candidate.task else {
        return Ok(ExecutionOutcome::Completed);
    };
    let source_id = candidate.source.id.as_str();
    if !connections.contains_key(source_id) {
        let database_root = candidate
            .source
            .database_root()
            .map_err(|error| error.to_string())?;
        let connection = SourceDatabase::open_connection_with_role_and_database_root(
            &candidate.source.root,
            &database_root,
            SourceDatabaseConnectionRole::JobWorker,
        )
        .map_err(|error| error.to_string())?;
        connections.insert(source_id.to_string(), connection);
    }
    let connection = connections
        .get_mut(source_id)
        .expect("synthetic source connection was inserted");
    let now = now_epoch_seconds();
    let Some(claim) = ReadinessStore::new(connection)
        .claim(target, now, READINESS_LEASE_SECONDS)
        .map_err(|error| error.to_string())?
    else {
        return Ok(ExecutionOutcome::NotClaimed);
    };
    if cancel.load(Ordering::Acquire) {
        return cancel_claim(connection, &claim, "profile cancellation", now);
    }
    match ReadinessStore::new(connection)
        .complete(&claim, now_epoch_seconds())
        .map_err(|error| error.to_string())?
    {
        ArtifactPublishOutcome::Recorded => Ok(ExecutionOutcome::Completed),
        ArtifactPublishOutcome::RejectedStale => Ok(ExecutionOutcome::Stale),
    }
}
