use super::{
    Arc, BTreeMap, BTreeSet, CoordinatorExecutionState, Duration, ExecutionPool, FairScheduler,
    Instant, RuntimeCandidate, RuntimeTask, SAFETY_SWEEP_INTERVAL, Shared, SourceDiscoveryStats,
    SourceProcessingLifecycle, aggregate_source_stats, coordinator_wait_duration,
    discover_candidates, earliest_deadline, execute_candidates, now_epoch_seconds,
    oldest_job_age_seconds, publish_similarity_readiness_refreshes,
    publish_source_processing_finished, publish_source_processing_wait, queue_depths_by_source,
    release_converged_source_owner, select_source_for_discovery,
};

pub(super) fn run_coordinator(shared: Arc<Shared>) {
    let worker_limit = shared.budgets().execution_worker_limit();
    let mut execution_pool = ExecutionPool::new(&shared, worker_limit);
    let mut observed_generation = 0;
    let mut next_retry_at = None;
    let mut next_safety_sweep_at = Instant::now() + SAFETY_SWEEP_INTERVAL;
    let mut scheduler = FairScheduler::default();
    let mut candidates = Vec::<RuntimeCandidate>::new();
    let mut source_stats = BTreeMap::<String, SourceDiscoveryStats>::new();
    let mut active_progress_source = None::<String>;
    let mut last_progress_publish_at = None::<Instant>;
    let mut pending_similarity_refresh_lifecycles = BTreeSet::<SourceProcessingLifecycle>::new();
    let mut pending_discovery_sources = BTreeSet::<String>::new();
    let mut pending_safety_probe_sources = BTreeSet::<String>::new();
    let mut last_similarity_refresh_publish_at = None::<Instant>;
    let mut progress_visible = false;
    #[cfg(test)]
    let mut synthetic_connections = BTreeMap::<String, rusqlite::Connection>::new();
    loop {
        let (
            sources,
            dirty_sources,
            safety_probe_sources,
            awaiting_foreground_refresh_sources,
            force_manifest_audit_sources,
            force_reanalysis_sources,
            pending_readiness_deltas,
            source_work_cancels,
            source_lifecycle_generations,
            priority,
            generation,
            reason,
        ) = {
            let mut control = shared.control();
            while !control.shutdown && control.wake_generation == observed_generation {
                let pending_retirement_retry_at = control
                    .pending_retirements
                    .values()
                    .filter_map(|retirement| {
                        (retirement.retry_at > 0).then_some(retirement.retry_at)
                    })
                    .min();
                let wait_duration = coordinator_wait_duration(
                    earliest_deadline(next_retry_at, pending_retirement_retry_at),
                    now_epoch_seconds(),
                    next_safety_sweep_at.saturating_duration_since(Instant::now()),
                );
                let active_source_waiting = scheduler.active_source().is_some_and(|source_id| {
                    source_stats.get(source_id).is_some_and(|stats| {
                        stats.prerequisites_blocked > 0 || stats.earliest_retry_at.is_some()
                    })
                });
                if publish_finished_if_idle(
                    &shared,
                    progress_visible,
                    wait_duration,
                    active_source_waiting,
                    execution_pool.in_flight_count() > 0,
                ) {
                    // Keep feedback stable across immediate coordinator handoffs. Only clear it
                    // when the coordinator is genuinely about to sleep with no newly published
                    // work or prerequisite retry waiting to be handled.
                    progress_visible = false;
                    active_progress_source = None;
                    last_progress_publish_at = None;
                }
                let (next, _) = shared
                    .wake
                    .wait_timeout(control, wait_duration)
                    .unwrap_or_else(|poison| poison.into_inner());
                control = next;
                if control.wake_generation == observed_generation {
                    let now = now_epoch_seconds();
                    if Instant::now() >= next_safety_sweep_at {
                        control.mark_all_sources_for_safety_probe();
                        next_safety_sweep_at = Instant::now() + SAFETY_SWEEP_INTERVAL;
                    } else {
                        let due_sources = source_stats
                            .iter()
                            .filter_map(|(source_id, stats)| {
                                stats
                                    .earliest_retry_at
                                    .is_some_and(|deadline| deadline <= now)
                                    .then(|| source_id.clone())
                            })
                            .collect::<Vec<_>>();
                        for source_id in due_sources {
                            control.dirty_sources.insert(source_id);
                        }
                        control.notify("retry_deadline");
                    }
                }
            }
            if control.shutdown {
                break;
            }
            let awaiting_foreground_refresh_sources =
                control.awaiting_foreground_refresh_sources.clone();
            let lifecycle_audits_deferred = control.lifecycle_audits_deferred_until_watcher_ready;
            let deferred_lifecycle_audit_sources = control.deferred_lifecycle_audit_sources.clone();
            let pending_safety_probe_sources = std::mem::take(&mut control.safety_probe_sources);
            let pending_dirty_sources = std::mem::take(&mut control.dirty_sources);
            let deferred_sources = pending_dirty_sources
                .iter()
                .filter(|source_id| {
                    (lifecycle_audits_deferred && pending_safety_probe_sources.contains(*source_id))
                        || deferred_lifecycle_audit_sources.contains(*source_id)
                })
                .cloned()
                .collect::<BTreeSet<_>>();
            let dirty_sources = pending_dirty_sources
                .into_iter()
                .filter(|source_id| {
                    !awaiting_foreground_refresh_sources.contains(source_id)
                        && !deferred_sources.contains(source_id)
                })
                .collect::<BTreeSet<_>>();
            if !deferred_sources.is_empty() {
                control
                    .dirty_sources
                    .extend(deferred_sources.iter().cloned());
                control.safety_probe_sources.extend(
                    deferred_sources
                        .intersection(&pending_safety_probe_sources)
                        .cloned(),
                );
            }
            let safety_probe_sources = pending_safety_probe_sources
                .into_iter()
                .filter(|source_id| dirty_sources.contains(source_id))
                .collect::<BTreeSet<_>>();
            let force_manifest_audit_sources = control.force_manifest_audit_sources.clone();
            let force_reanalysis_sources = control.force_reanalysis_sources.clone();
            let pending_readiness_deltas = control.pending_readiness_deltas.clone();
            (
                control
                    .sources
                    .iter()
                    .filter(|(source_id, _)| control.source_is_active(source_id))
                    .map(|(_, source)| source.clone())
                    .collect::<Vec<_>>(),
                dirty_sources,
                safety_probe_sources,
                awaiting_foreground_refresh_sources,
                force_manifest_audit_sources,
                force_reanalysis_sources,
                pending_readiness_deltas,
                control.source_work_cancels.clone(),
                control.source_lifecycle_generations.clone(),
                control.priority.clone(),
                control.wake_generation,
                control.wake_reason,
            )
        };
        observed_generation = generation;
        let configured_source_ids = sources
            .iter()
            .map(|source| source.id.as_str().to_string())
            .collect::<BTreeSet<_>>();
        pending_discovery_sources.extend(dirty_sources.iter().cloned());
        for source_id in &dirty_sources {
            if safety_probe_sources.contains(source_id) {
                pending_safety_probe_sources.insert(source_id.clone());
            } else {
                pending_safety_probe_sources.remove(source_id);
            }
        }
        pending_discovery_sources
            .retain(|source_id| !awaiting_foreground_refresh_sources.contains(source_id));
        pending_discovery_sources.retain(|source_id| configured_source_ids.contains(source_id));
        pending_safety_probe_sources
            .retain(|source_id| pending_discovery_sources.contains(source_id));
        let discovery_source_id = select_source_for_discovery(
            &sources,
            &pending_discovery_sources,
            scheduler.active_source(),
            &priority,
        );
        let discovery_is_safety_probe = discovery_source_id
            .as_ref()
            .is_some_and(|source_id| pending_safety_probe_sources.contains(source_id));
        if let Some(source_id) = discovery_source_id.as_ref() {
            pending_discovery_sources.remove(source_id);
            pending_safety_probe_sources.remove(source_id);
        }
        let sources_to_discover = sources
            .iter()
            .filter(|source| discovery_source_id.as_deref() == Some(source.id.as_str()))
            .cloned()
            .collect::<Vec<_>>();
        candidates.retain(|candidate| {
            let source_id = candidate.source.id.as_str();
            if !configured_source_ids.contains(source_id) {
                return false;
            }
            if !dirty_sources.contains(source_id) {
                return true;
            }
            pending_readiness_deltas
                .get(source_id)
                .is_some_and(|delta| {
                    candidate.schedule.scope_id != source_id
                        && !delta.scope_ids.contains(&candidate.schedule.scope_id)
                })
        });
        source_stats.retain(|source_id, _| configured_source_ids.contains(source_id));
        let sweep_started = Instant::now();
        for source in &sources_to_discover {
            if !discovery_is_safety_probe
                && !pending_readiness_deltas.contains_key(source.id.as_str())
            {
                source_stats.remove(source.id.as_str());
            }
        }
        let (
            mut discovered,
            mut discovered_source_stats,
            deferred_discoveries,
            consumed_readiness_delta_sources,
            discovery_progress_published,
        ) = discover_candidates(
            &shared,
            &sources_to_discover,
            &force_manifest_audit_sources,
            &force_reanalysis_sources,
            &pending_readiness_deltas,
            discovery_is_safety_probe,
            &source_work_cancels,
        );
        if !consumed_readiness_delta_sources.is_empty() {
            let mut control = shared.control();
            for source_id in consumed_readiness_delta_sources {
                if control.pending_readiness_deltas.get(&source_id)
                    == pending_readiness_deltas.get(&source_id)
                {
                    control.pending_readiness_deltas.remove(&source_id);
                }
            }
        }
        let discovery_deferred_for_capacity = !deferred_discoveries.is_empty();
        if discovery_progress_published {
            progress_visible = true;
            active_progress_source = sources_to_discover
                .first()
                .map(|source| source.id.as_str().to_string());
            last_progress_publish_at = Some(Instant::now());
        }
        pending_discovery_sources.extend(deferred_discoveries);
        if discovery_is_safety_probe {
            pending_safety_probe_sources.extend(
                pending_discovery_sources
                    .iter()
                    .filter(|source_id| {
                        sources_to_discover
                            .iter()
                            .any(|source| source.id.as_str() == source_id.as_str())
                    })
                    .cloned(),
            );
        }
        for (source_id, delta_stats) in &mut discovered_source_stats {
            if !pending_readiness_deltas.contains_key(source_id) {
                continue;
            }
            let Some(previous) = source_stats.get(source_id).copied() else {
                continue;
            };
            let retained_readiness = candidates
                .iter()
                .filter(|candidate| {
                    candidate.source.id.as_str() == source_id
                        && matches!(candidate.task, RuntimeTask::Readiness(_))
                })
                .count();
            delta_stats.readiness_queue_depth = delta_stats
                .readiness_queue_depth
                .saturating_add(retained_readiness);
            delta_stats.progress_total = previous.progress_total.max(delta_stats.progress_total);
            delta_stats.progress_completed = previous
                .progress_completed
                .saturating_sub(delta_stats.readiness_queue_depth);
            delta_stats.earliest_retry_at =
                earliest_deadline(previous.earliest_retry_at, delta_stats.earliest_retry_at);
            delta_stats.retries_due = previous.retries_due.saturating_add(delta_stats.retries_due);
        }
        candidates.append(&mut discovered);
        source_stats.extend(discovered_source_stats);
        let discovered_stats = aggregate_source_stats(source_stats.values().copied());
        next_retry_at = discovered_stats.earliest_retry_at;
        {
            let mut telemetry = shared.telemetry();
            telemetry.sweeps = telemetry.sweeps.saturating_add(1);
            telemetry.queue_depth = candidates.len();
            telemetry.max_queue_depth = telemetry.max_queue_depth.max(telemetry.queue_depth);
            telemetry.oldest_job_age_seconds =
                oldest_job_age_seconds(&candidates, now_epoch_seconds());
            telemetry.retries_due = discovered_stats.retries_due;
            telemetry.readiness_queue_depth = discovered_stats.readiness_queue_depth;
            telemetry.queue_depth_by_source = queue_depths_by_source(&candidates);
            telemetry.readiness_queue_depth_by_source = source_stats
                .iter()
                .map(|(source_id, stats)| (source_id.clone(), stats.readiness_queue_depth))
                .collect();
            telemetry.retries_due_by_source = source_stats
                .iter()
                .map(|(source_id, stats)| (source_id.clone(), stats.retries_due))
                .collect();
            telemetry.retry_at_by_source = source_stats
                .iter()
                .filter_map(|(source_id, stats)| {
                    stats
                        .earliest_retry_at
                        .map(|retry_at| (source_id.clone(), retry_at))
                })
                .collect();
        }
        let active_source_in_flight = scheduler
            .active_source()
            .is_some_and(|source_id| execution_pool.source_is_in_flight(source_id));
        release_converged_source_owner(
            &mut scheduler,
            &configured_source_ids,
            &source_stats,
            &candidates,
            active_source_in_flight,
        );
        let execution_state = execute_candidates(
            &shared,
            &mut execution_pool,
            &mut candidates,
            &mut scheduler,
            &mut source_stats,
            &source_work_cancels,
            CoordinatorExecutionState {
                next_retry_at,
                pending_similarity_refresh_lifecycles,
                last_similarity_refresh_publish_at,
                active_progress_source,
                last_progress_publish_at,
                progress_visible,
            },
            #[cfg(test)]
            &mut synthetic_connections,
        );
        next_retry_at = execution_state.next_retry_at;
        pending_similarity_refresh_lifecycles =
            execution_state.pending_similarity_refresh_lifecycles;
        last_similarity_refresh_publish_at = execution_state.last_similarity_refresh_publish_at;
        active_progress_source = execution_state.active_progress_source;
        last_progress_publish_at = execution_state.last_progress_publish_at;
        progress_visible = execution_state.progress_visible;
        if publish_similarity_readiness_refreshes(
            &shared,
            &mut pending_similarity_refresh_lifecycles,
        ) {
            last_similarity_refresh_publish_at = Some(Instant::now());
        }
        let active_source_has_runnable_work = scheduler.active_source().is_some_and(|source_id| {
            execution_pool.source_is_in_flight(source_id)
                || candidates
                    .iter()
                    .any(|candidate| candidate.source.id.as_str() == source_id)
        });
        if !active_source_has_runnable_work
            && publish_source_processing_wait(
                &shared,
                scheduler.active_source(),
                &source_lifecycle_generations,
                &source_stats,
            )
        {
            progress_visible = true;
            active_progress_source = None;
            last_progress_publish_at = Some(Instant::now());
        }
        let mut telemetry = shared.telemetry();
        telemetry.queue_depth = candidates.len();
        telemetry.oldest_job_age_seconds = oldest_job_age_seconds(&candidates, now_epoch_seconds());
        telemetry.queue_depth_by_source = queue_depths_by_source(&candidates);
        telemetry.settled_wake_generation = observed_generation;
        tracing::info!(
            target: "wavecrate::source_processing",
            event = "source_processing.sweep",
            reason,
            active_source_id = scheduler.active_source().unwrap_or(""),
            source_count = sources.len(),
            queued = telemetry.queue_depth,
            queue_depth_by_source = ?telemetry.queue_depth_by_source,
            readiness_queue_depth_by_source = ?telemetry.readiness_queue_depth_by_source,
            retries_due_by_source = ?telemetry.retries_due_by_source,
            retry_at_by_source = ?telemetry.retry_at_by_source,
            oldest_job_age_seconds = telemetry.oldest_job_age_seconds,
            retries_due = telemetry.retries_due,
            claimed = telemetry.claimed,
            completed = telemetry.completed,
            failed = telemetry.failed,
            retried = telemetry.retried,
            stale = telemetry.stale,
            cancelled = telemetry.cancelled,
            contention = telemetry.contention,
            elapsed_ms = sweep_started.elapsed().as_secs_f64() * 1_000.0,
            "Source processing sweep complete"
        );
        drop(telemetry);
        if scheduler.active_source().is_none()
            && !pending_discovery_sources.is_empty()
            && !discovery_deferred_for_capacity
        {
            shared.control().notify("next_source_discovery");
            shared.wake.notify_one();
        }
    }
    if !execution_pool.shutdown() {
        tracing::error!(
            target: "wavecrate::source_processing",
            "A source execution worker panicked during shutdown"
        );
    }
}

pub(super) fn publish_finished_if_idle(
    shared: &Shared,
    progress_visible: bool,
    wait_duration: Duration,
    active_source_waiting: bool,
    execution_in_flight: bool,
) -> bool {
    if !progress_visible || wait_duration.is_zero() || active_source_waiting || execution_in_flight
    {
        return false;
    }
    publish_source_processing_finished(shared);
    true
}
