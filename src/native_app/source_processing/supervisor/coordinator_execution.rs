#[cfg(test)]
use super::ExecutionResult;
use super::coordinator_completion::handle_completion;
#[cfg(test)]
use super::execute_synthetic_candidate_for_profile;
use super::{
    Arc, AtomicBool, BTreeMap, BTreeSet, ExecutionPool, ExecutionRequest, FairScheduler, Instant,
    Ordering, PROGRESS_REFRESH_INTERVAL, RuntimeCandidate, RuntimeTask, Shared,
    SourceDiscoveryStats, SourceProcessingLifecycle, publish_source_processing_progress,
    scheduler_candidate_indices,
};

pub(super) struct CoordinatorExecutionState {
    pub(super) next_retry_at: Option<i64>,
    pub(super) pending_similarity_refresh_lifecycles: BTreeSet<SourceProcessingLifecycle>,
    pub(super) last_similarity_refresh_publish_at: Option<Instant>,
    pub(super) active_progress_source: Option<String>,
    pub(super) last_progress_publish_at: Option<Instant>,
    pub(super) progress_visible: bool,
}

pub(super) fn execute_candidates(
    shared: &Arc<Shared>,
    pool: &mut ExecutionPool,
    candidates: &mut Vec<RuntimeCandidate>,
    scheduler: &mut FairScheduler,
    source_stats: &mut BTreeMap<String, SourceDiscoveryStats>,
    source_work_cancels: &BTreeMap<String, Arc<AtomicBool>>,
    mut state: CoordinatorExecutionState,
    #[cfg(test)] synthetic_connections: &mut BTreeMap<String, rusqlite::Connection>,
) -> CoordinatorExecutionState {
    while let Some(completion) = pool.try_result() {
        handle_completion(shared, candidates, source_stats, &mut state, completion);
    }

    #[cfg(test)]
    if shared.synthetic_test_execution.load(Ordering::Acquire) {
        execute_synthetic_candidates(
            shared,
            candidates,
            scheduler,
            source_stats,
            source_work_cancels,
            &mut state,
            synthetic_connections,
        );
        return state;
    }

    while !candidates.is_empty()
        && pool.in_flight_count() < pool.capacity()
        && !shared.cancel.load(Ordering::Acquire)
    {
        let control = shared.control();
        let interrupted = !control.dirty_sources.is_empty();
        let priority = control.priority.clone();
        drop(control);
        if interrupted {
            break;
        }
        let external_scan_admitted = shared.has_external_scan_admission();
        let eligible_indices = scheduler_candidate_indices(candidates, external_scan_admitted);
        if eligible_indices.is_empty() {
            let mut telemetry = shared.telemetry();
            telemetry.contention = telemetry.contention.saturating_add(1);
            break;
        }
        let schedules = eligible_indices
            .iter()
            .map(|index| candidates[*index].schedule.clone())
            .collect::<Vec<_>>();
        let previously_active_source = scheduler.active_source().map(str::to_string);
        let Some(schedule_index) = scheduler.choose(&schedules, &priority, &shared.budgets())
        else {
            let mut telemetry = shared.telemetry();
            telemetry.contention = telemetry.contention.saturating_add(1);
            break;
        };
        let index = eligible_indices[schedule_index];
        let candidate = candidates.swap_remove(index);
        if scheduler.active_source() != previously_active_source.as_deref() {
            tracing::info!(
                target: "wavecrate::source_processing",
                event = "source_processing.source_selected",
                previous_source_id = previously_active_source.as_deref().unwrap_or(""),
                source_id = candidate.source.id.as_str(),
                queued_total = candidates.len().saturating_add(1),
                "Selected source for processing"
            );
        }
        let Some(permit) = shared
            .budgets()
            .try_acquire(&candidate.schedule.source_id, candidate.schedule.lane)
        else {
            candidates.push(candidate);
            break;
        };
        let Some(candidate_cancel) = source_work_cancels
            .get(candidate.source.id.as_str())
            .cloned()
        else {
            shared.budgets().release(permit);
            shared.budget_wake.notify_all();
            continue;
        };
        let Some(in_flight) =
            shared.begin_in_flight_work(candidate.source.id.as_str(), &candidate_cancel)
        else {
            shared.budgets().release(permit);
            shared.budget_wake.notify_all();
            candidates.push(candidate);
            break;
        };
        publish_candidate_start(
            shared,
            &candidate,
            in_flight.lifecycle_generation,
            source_stats,
            &mut state,
        );
        let request = ExecutionRequest {
            candidate,
            permit,
            cancel: candidate_cancel,
            in_flight,
        };
        if let Err(request) = pool.try_dispatch(request) {
            shared.budgets().release(request.permit);
            shared.budget_wake.notify_all();
            candidates.push(request.candidate);
            break;
        }
        let mut telemetry = shared.telemetry();
        telemetry.execution_queue_depth = pool.in_flight_count();
        telemetry.max_execution_queue_depth = telemetry
            .max_execution_queue_depth
            .max(telemetry.execution_queue_depth);
    }
    state
}

fn publish_candidate_start(
    shared: &Arc<Shared>,
    candidate: &RuntimeCandidate,
    lifecycle_generation: u64,
    source_stats: &BTreeMap<String, SourceDiscoveryStats>,
    state: &mut CoordinatorExecutionState,
) {
    let progress_due = state
        .last_progress_publish_at
        .is_none_or(|published_at| published_at.elapsed() >= PROGRESS_REFRESH_INTERVAL);
    if (state.active_progress_source.as_deref() != Some(candidate.source.id.as_str())
        || !matches!(&candidate.task, RuntimeTask::Readiness(_))
        || progress_due)
        && let Some(progress) = source_stats.get(candidate.source.id.as_str()).copied()
    {
        publish_source_processing_progress(shared, candidate, lifecycle_generation, progress);
        state.active_progress_source = Some(candidate.source.id.as_str().to_string());
        state.last_progress_publish_at = Some(Instant::now());
        state.progress_visible = true;
    }
    if matches!(&candidate.task, RuntimeTask::ManifestAudit { .. }) {
        let mut telemetry = shared.telemetry();
        telemetry.full_audits = telemetry.full_audits.saturating_add(1);
    }
    tracing::info!(
        target: "wavecrate::source_processing",
        event = "source_processing.candidate.started",
        source_id = candidate.source.id.as_str(),
        lifecycle_generation,
        task = ?candidate.task,
        lane = ?candidate.schedule.lane,
        "Source processing candidate dispatched"
    );
}

#[cfg(test)]
fn execute_synthetic_candidates(
    shared: &Arc<Shared>,
    candidates: &mut Vec<RuntimeCandidate>,
    scheduler: &mut FairScheduler,
    source_stats: &mut BTreeMap<String, SourceDiscoveryStats>,
    source_work_cancels: &BTreeMap<String, Arc<AtomicBool>>,
    state: &mut CoordinatorExecutionState,
    connections: &mut BTreeMap<String, rusqlite::Connection>,
) {
    while !candidates.is_empty() && !shared.cancel.load(Ordering::Acquire) {
        let priority = shared.control().priority.clone();
        let schedules = candidates
            .iter()
            .map(|candidate| candidate.schedule.clone())
            .collect::<Vec<_>>();
        let Some(index) = scheduler.choose(&schedules, &priority, &shared.budgets()) else {
            break;
        };
        let candidate = candidates.swap_remove(index);
        let Some(permit) = shared
            .budgets()
            .try_acquire(&candidate.schedule.source_id, candidate.schedule.lane)
        else {
            candidates.push(candidate);
            break;
        };
        let Some(cancel) = source_work_cancels
            .get(candidate.source.id.as_str())
            .cloned()
        else {
            shared.budgets().release(permit);
            continue;
        };
        let Some(in_flight) = shared.begin_in_flight_work(candidate.source.id.as_str(), &cancel)
        else {
            shared.budgets().release(permit);
            candidates.push(candidate);
            break;
        };
        let lifecycle_generation = in_flight.lifecycle_generation;
        publish_candidate_start(
            shared,
            &candidate,
            lifecycle_generation,
            source_stats,
            state,
        );
        let started = Instant::now();
        let result =
            execute_synthetic_candidate_for_profile(&candidate, cancel.as_ref(), connections);
        handle_completion(
            shared,
            candidates,
            source_stats,
            state,
            ExecutionResult {
                candidate,
                permit,
                lifecycle_generation,
                result,
                elapsed_ms: started.elapsed().as_secs_f64() * 1_000.0,
                in_flight,
            },
        );
    }
}
