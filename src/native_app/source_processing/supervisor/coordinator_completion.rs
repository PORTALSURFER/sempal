use super::{
    Arc, BTreeMap, CandidateInvalidationScope, ExecutionOutcome, ExecutionResult, Instant,
    Ordering, ReadinessStage, ReadinessTarget, RuntimeCandidate, RuntimeTask,
    SIMILARITY_SCORE_REFRESH_INTERVAL, Shared, SourceDiscoveryStats, SourceProcessingLifecycle,
    advance_source_progress, aggregate_source_stats, candidate_invalidation_scope,
    clear_satisfied_manifest_audit_request, earliest_deadline, progress_refresh_due,
    publish_similarity_readiness_refreshes, publish_source_processing_progress,
    should_requeue_cancelled,
};

use super::coordinator_execution::CoordinatorExecutionState;

pub(super) fn handle_completion(
    shared: &Arc<Shared>,
    candidates: &mut Vec<RuntimeCandidate>,
    source_stats: &mut BTreeMap<String, SourceDiscoveryStats>,
    state: &mut CoordinatorExecutionState,
    completion: ExecutionResult,
) {
    let ExecutionResult {
        candidate,
        permit,
        lifecycle_generation,
        result,
        elapsed_ms,
        in_flight,
    } = completion;
    tracing::info!(
        target: "wavecrate::source_processing",
        event = "source_processing.candidate.finished",
        source_id = candidate.source.id.as_str(),
        lifecycle_generation,
        task = ?candidate.task,
        outcome = ?result,
        elapsed_ms,
        "Source processing candidate finished"
    );
    drop(in_flight);
    shared.budgets().release(permit);
    shared.budget_wake.notify_all();
    {
        let mut telemetry = shared.telemetry();
        telemetry.execution_queue_depth = telemetry.execution_queue_depth.saturating_sub(1);
    }
    let stale_lifecycle = {
        let mut control = shared.control();
        let current = control.source_is_active(candidate.source.id.as_str())
            && control
                .source_lifecycle_generations
                .get(candidate.source.id.as_str())
                == Some(&lifecycle_generation);
        if current
            && matches!(
                result,
                Ok(ExecutionOutcome::CompletedAwaitingForegroundRefresh
                    | ExecutionOutcome::FailedAwaitingForegroundRefresh)
            )
        {
            control
                .awaiting_foreground_refresh_sources
                .insert(candidate.source.id.as_str().to_string());
        }
        !current
    };
    if stale_lifecycle {
        let mut telemetry = shared.telemetry();
        telemetry.stale = telemetry.stale.saturating_add(1);
        drop(telemetry);
        tracing::debug!(
            target: "wavecrate::source_processing",
            source_id = candidate.source.id.as_str(),
            lifecycle_generation,
            task = ?candidate.task,
            "Discarded source work completion from a retired lifecycle"
        );
        return;
    }
    if matches!(&candidate.task, RuntimeTask::ManifestAudit { .. }) {
        let complete = matches!(
            &result,
            Ok(ExecutionOutcome::Completed | ExecutionOutcome::CompletedAwaitingForegroundRefresh)
        );
        shared
            .control()
            .finish_source_audit_request(candidate.source.id.as_str(), complete);
    }
    if matches!(
        &result,
        Ok(ExecutionOutcome::Completed | ExecutionOutcome::CompletedAwaitingForegroundRefresh)
    ) && matches!(&candidate.task, RuntimeTask::ManifestAudit { .. })
    {
        clear_satisfied_manifest_audit_request(shared.as_ref(), candidate.source.id.as_str());
    }
    let mut execution_outcome = None;
    {
        let mut telemetry = shared.telemetry();
        match &result {
            Ok(outcome) => {
                execution_outcome = Some(*outcome);
                if *outcome == ExecutionOutcome::Completed
                    && let RuntimeTask::Readiness(target) = &candidate.task
                    && target.stage == ReadinessStage::EmbeddingAspects
                {
                    state.pending_similarity_refresh_lifecycles.insert(
                        SourceProcessingLifecycle::new(
                            target.source_id.clone(),
                            lifecycle_generation,
                        ),
                    );
                }
                if outcome.was_claimed() {
                    telemetry.claimed = telemetry.claimed.saturating_add(1);
                }
                match *outcome {
                    ExecutionOutcome::Completed
                    | ExecutionOutcome::CompletedAwaitingForegroundRefresh => {
                        telemetry.completed = telemetry.completed.saturating_add(1);
                        if matches!(&candidate.task, RuntimeTask::Readiness(_))
                            && let Some(progress) =
                                advance_source_progress(source_stats, candidate.source.id.as_str())
                            && progress_refresh_due(state.last_progress_publish_at)
                        {
                            publish_source_processing_progress(
                                shared,
                                &candidate,
                                lifecycle_generation,
                                progress,
                            );
                            state.last_progress_publish_at = Some(Instant::now());
                            state.progress_visible = true;
                        }
                    }
                    ExecutionOutcome::Retried { retry_at }
                    | ExecutionOutcome::PrerequisiteInvalidated { retry_at, .. } => {
                        if matches!(outcome, ExecutionOutcome::Retried { .. }) {
                            telemetry.retried = telemetry.retried.saturating_add(1);
                        } else {
                            telemetry.stale = telemetry.stale.saturating_add(1);
                        }
                        if let Some(stats) = source_stats.get_mut(candidate.source.id.as_str()) {
                            stats.earliest_retry_at =
                                earliest_deadline(stats.earliest_retry_at, Some(retry_at));
                        }
                        state.next_retry_at =
                            aggregate_source_stats(source_stats.values().copied())
                                .earliest_retry_at;
                    }
                    ExecutionOutcome::Failed
                    | ExecutionOutcome::FailedAwaitingForegroundRefresh => {
                        telemetry.failed = telemetry.failed.saturating_add(1);
                    }
                    ExecutionOutcome::Stale => telemetry.stale = telemetry.stale.saturating_add(1),
                    ExecutionOutcome::Cancelled => {
                        telemetry.cancelled = telemetry.cancelled.saturating_add(1)
                    }
                    ExecutionOutcome::Parked | ExecutionOutcome::NotClaimed => {}
                }
            }
            Err(error) if shared.cancel.load(Ordering::Acquire) => {
                telemetry.cancelled = telemetry.cancelled.saturating_add(1);
                tracing::debug!(
                    target: "wavecrate::source_processing",
                    source_id = candidate.source.id.as_str(),
                    task = ?candidate.task,
                    error,
                    "Source work cancelled"
                );
            }
            Err(error) => {
                telemetry.failed = telemetry.failed.saturating_add(1);
                tracing::warn!(
                    target: "wavecrate::source_processing",
                    source_id = candidate.source.id.as_str(),
                    task = ?candidate.task,
                    error,
                    "Source work failed"
                );
            }
        }
        let aggregate = aggregate_source_stats(source_stats.values().copied());
        telemetry.readiness_queue_depth = aggregate.readiness_queue_depth;
        telemetry.readiness_queue_depth_by_source = source_stats
            .iter()
            .map(|(source_id, stats)| (source_id.clone(), stats.readiness_queue_depth))
            .collect();
    }
    if state
        .last_similarity_refresh_publish_at
        .is_none_or(|published_at| published_at.elapsed() >= SIMILARITY_SCORE_REFRESH_INTERVAL)
        && publish_similarity_readiness_refreshes(
            shared,
            &mut state.pending_similarity_refresh_lifecycles,
        )
    {
        state.last_similarity_refresh_publish_at = Some(Instant::now());
    }
    let requeue_cancelled = {
        let control = shared.control();
        should_requeue_cancelled(
            execution_outcome,
            control.source_is_active(candidate.source.id.as_str()),
            control.dirty_sources.contains(candidate.source.id.as_str()),
        )
    };
    if requeue_cancelled {
        candidates.push(candidate);
        return;
    }
    match candidate_invalidation_scope(&candidate.task, execution_outcome) {
        CandidateInvalidationScope::None => {}
        CandidateInvalidationScope::TargetScope => candidates.retain(|queued| {
            queued.source.id != candidate.source.id
                || queued.schedule.scope_id != candidate.schedule.scope_id
        }),
        CandidateInvalidationScope::Source => {
            candidates.retain(|queued| queued.source.id != candidate.source.id)
        }
    }
    if candidates
        .iter()
        .any(|queued| queued.source.id == candidate.source.id)
    {
        return;
    }
    let should_refresh = matches!(
        (&candidate.task, execution_outcome),
        (
            RuntimeTask::ManifestAudit { .. },
            Some(ExecutionOutcome::Completed | ExecutionOutcome::Failed)
        ) | (
            RuntimeTask::Readiness(ReadinessTarget {
                stage: ReadinessStage::IndexedIdentity
                    | ReadinessStage::AnalysisFeatures
                    | ReadinessStage::EmbeddingAspects
                    | ReadinessStage::SimilarityLayout,
                ..
            }),
            Some(ExecutionOutcome::Completed),
        ) | (
            RuntimeTask::Readiness(_),
            Some(
                ExecutionOutcome::Retried { .. }
                    | ExecutionOutcome::Failed
                    | ExecutionOutcome::PrerequisiteInvalidated { .. }
                    | ExecutionOutcome::NotClaimed
                    | ExecutionOutcome::Stale
            )
        )
    );
    if should_refresh {
        shared
            .control()
            .mark_source_dirty(candidate.source.id.as_str(), "source_stage_progress");
    }
}
