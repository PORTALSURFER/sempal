use super::{
    BTreeMap, BTreeSet, DISCOVERY_PROGRESS_EVENT_GRACE_INTERVAL, DISCOVERY_PROGRESS_LOG_INTERVAL,
    DISCOVERY_PROGRESS_REFRESH_INTERVAL, Instant, PROGRESS_REFRESH_INTERVAL,
    ReadinessClassification, ReadinessEligibility, ReadinessScopeKind, ReadinessSnapshot,
    ReadinessStage, RuntimeCandidate, RuntimeTask, Shared, SourceDiscoveryPhase,
    SourceDiscoveryStats, SourceProcessingActivity, SourceProcessingEvent,
    SourceProcessingLifecycle, SourceProcessingProgressEvent, earliest_deadline,
};

pub(super) struct DiscoveryProgressPublisher<'a> {
    pub(super) shared: &'a Shared,
    pub(super) source_id: &'a str,
    pub(super) lifecycle_generation: u64,
    pub(super) started_at: Instant,
    pub(super) last_progress: Option<DiscoveryProgressUpdate>,
    pub(super) last_event_publish_at: Option<Instant>,
    pub(super) last_log_publish_at: Option<Instant>,
    pub(super) event_published: bool,
    pub(super) work_units: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct DiscoveryProgressUpdate {
    pub(super) phase: SourceDiscoveryPhase,
    pub(super) completed: usize,
    pub(super) total: usize,
}

impl DiscoveryProgressUpdate {
    pub(super) const fn indeterminate(phase: SourceDiscoveryPhase) -> Self {
        Self {
            phase,
            completed: 0,
            total: 0,
        }
    }

    pub(super) const fn determinate(
        phase: SourceDiscoveryPhase,
        completed: usize,
        total: usize,
    ) -> Self {
        Self {
            phase,
            completed,
            total,
        }
    }
}

impl DiscoveryProgressPublisher<'_> {
    pub(super) fn advance(&mut self, progress: DiscoveryProgressUpdate) {
        self.work_units = self.work_units.saturating_add(1);
        let phase_changed = self
            .last_progress
            .is_none_or(|previous| previous.phase != progress.phase);
        let regressed = self.last_progress.is_some_and(|previous| {
            discovery_phase_rank(progress.phase) < discovery_phase_rank(previous.phase)
                || (previous.phase == progress.phase
                    && previous.total > 0
                    && (progress.total != previous.total
                        || progress.completed < previous.completed))
        });
        let event_due = self.started_at.elapsed() >= DISCOVERY_PROGRESS_EVENT_GRACE_INTERVAL
            && !regressed
            && (phase_changed
                || self.last_event_publish_at.is_none_or(|published_at| {
                    published_at.elapsed() >= DISCOVERY_PROGRESS_REFRESH_INTERVAL
                }));
        if event_due {
            self.event_published |= self.shared.publish_event(SourceProcessingEvent::Progress(
                SourceProcessingProgressEvent {
                    lifecycle: SourceProcessingLifecycle::new(
                        self.source_id,
                        self.lifecycle_generation,
                    ),
                    source_row_active: true,
                    completed: progress.completed.min(progress.total),
                    total: progress.total,
                    activity: SourceProcessingActivity::Discovering {
                        phase: progress.phase,
                    },
                },
            ));
            self.last_event_publish_at = Some(Instant::now());
        }
        let log_due = phase_changed
            || self.last_log_publish_at.is_none_or(|published_at| {
                published_at.elapsed() >= DISCOVERY_PROGRESS_LOG_INTERVAL
            });
        if log_due {
            tracing::debug!(
                target: "wavecrate::source_processing",
                event = "source_processing.discovery_progress",
                source_id = self.source_id,
                lifecycle_generation = self.lifecycle_generation,
                phase = ?progress.phase,
                work_units = self.work_units,
                completed = progress.completed,
                total = progress.total,
                "Source discovery reconciliation advanced"
            );
            self.last_log_publish_at = Some(Instant::now());
        }
        if !regressed {
            self.last_progress = Some(progress);
        }
    }
}

fn discovery_phase_rank(phase: SourceDiscoveryPhase) -> u8 {
    match phase {
        SourceDiscoveryPhase::Preparing => 0,
        SourceDiscoveryPhase::InspectingManifest => 1,
        SourceDiscoveryPhase::PreparingTargets => 2,
        SourceDiscoveryPhase::ComparingReadiness
        | SourceDiscoveryPhase::ComparingChangedReadiness => 3,
        SourceDiscoveryPhase::QueueingWork => 4,
    }
}

pub(super) fn progress_refresh_due(last_publish_at: Option<Instant>) -> bool {
    last_publish_at.is_none_or(|published_at| published_at.elapsed() >= PROGRESS_REFRESH_INTERVAL)
}

pub(super) fn manifest_audit_source_row_active(started_at: Instant) -> bool {
    started_at.elapsed() >= DISCOVERY_PROGRESS_EVENT_GRACE_INTERVAL
}

pub(super) fn publish_similarity_readiness_refreshes(
    shared: &Shared,
    pending_lifecycles: &mut BTreeSet<SourceProcessingLifecycle>,
) -> bool {
    if pending_lifecycles.is_empty() {
        return false;
    }
    let mut published = false;
    for lifecycle in std::mem::take(pending_lifecycles) {
        published |=
            shared.publish_event(SourceProcessingEvent::SimilarityReadinessAdvanced { lifecycle });
    }
    published
}

pub(super) fn publish_source_processing_progress(
    shared: &Shared,
    candidate: &RuntimeCandidate,
    lifecycle_generation: u64,
    stats: SourceDiscoveryStats,
) {
    let (completed, total) = match &candidate.task {
        RuntimeTask::Readiness(_) => (stats.progress_completed, stats.progress_total),
        RuntimeTask::ManifestAudit { .. } => (0, 0),
    };
    let (completed, total) = if total > 0 && completed < total {
        (completed, total)
    } else {
        // A claimed candidate is active even when discovery counters have reached their current
        // boundary. Keep showing activity until the coordinator actually becomes idle instead of
        // publishing a false completion while the candidate is still executing.
        (0, 0)
    };
    let activity = match &candidate.task {
        RuntimeTask::Readiness(target) => SourceProcessingActivity::Readiness {
            stage: target.stage,
            relative_path: target.relative_path.clone(),
        },
        RuntimeTask::ManifestAudit { .. } => SourceProcessingActivity::ManifestAudit {
            checked: None,
            relative_path: None,
        },
    };
    shared.publish_event(SourceProcessingEvent::Progress(
        SourceProcessingProgressEvent {
            lifecycle: SourceProcessingLifecycle::new(
                candidate.source.id.as_str(),
                lifecycle_generation,
            ),
            source_row_active: !matches!(candidate.task, RuntimeTask::ManifestAudit { .. }),
            completed,
            total,
            activity,
        },
    ));
}

pub(super) fn publish_source_processing_finished(shared: &Shared) {
    shared.publish_event(SourceProcessingEvent::Completed);
}

#[cfg(test)]
pub(super) fn publish_source_processing_prerequisite_wait(
    shared: &Shared,
    lifecycle_generations: &BTreeMap<String, u64>,
    source_stats: &BTreeMap<String, SourceDiscoveryStats>,
) -> bool {
    let Some((source_id, stats)) = source_stats
        .iter()
        .filter(|(_, stats)| stats.prerequisites_blocked > 0)
        .min_by_key(|(_, stats)| stats.prerequisite_retry_at.unwrap_or(i64::MAX))
    else {
        return false;
    };
    publish_source_processing_wait_for_source(shared, source_id, lifecycle_generations, stats, true)
}

pub(super) fn publish_source_processing_wait(
    shared: &Shared,
    active_source_id: Option<&str>,
    lifecycle_generations: &BTreeMap<String, u64>,
    source_stats: &BTreeMap<String, SourceDiscoveryStats>,
) -> bool {
    let Some(source_id) = active_source_id else {
        return false;
    };
    let Some(stats) = source_stats.get(source_id) else {
        return false;
    };
    if stats.prerequisites_blocked > 0 {
        return publish_source_processing_wait_for_source(
            shared,
            source_id,
            lifecycle_generations,
            stats,
            true,
        );
    }
    if stats.earliest_retry_at.is_none() {
        return false;
    }
    publish_source_processing_wait_for_source(
        shared,
        source_id,
        lifecycle_generations,
        stats,
        false,
    )
}

pub(super) fn publish_source_processing_wait_for_source(
    shared: &Shared,
    source_id: &str,
    lifecycle_generations: &BTreeMap<String, u64>,
    stats: &SourceDiscoveryStats,
    prerequisite_wait: bool,
) -> bool {
    let Some(lifecycle_generation) = lifecycle_generations.get(source_id).copied() else {
        return false;
    };
    let control = shared.control();
    if !control.source_is_active(source_id)
        || control.source_lifecycle_generations.get(source_id) != Some(&lifecycle_generation)
    {
        return false;
    }
    drop(control);
    let retry_at = if prerequisite_wait {
        stats.prerequisite_retry_at
    } else {
        stats.earliest_retry_at
    };
    let activity = match (prerequisite_wait, retry_at) {
        (true, retry_at) => SourceProcessingActivity::WaitingForPrerequisites { retry_at },
        (false, Some(retry_at)) => SourceProcessingActivity::WaitingForRetry { retry_at },
        (false, None) => return false,
    };
    shared.publish_event(SourceProcessingEvent::Progress(
        SourceProcessingProgressEvent {
            lifecycle: SourceProcessingLifecycle::new(source_id, lifecycle_generation),
            source_row_active: true,
            completed: stats.progress_completed,
            total: stats.progress_total,
            activity,
        },
    ))
}

pub(super) fn advance_source_progress(
    source_stats: &mut BTreeMap<String, SourceDiscoveryStats>,
    source_id: &str,
) -> Option<SourceDiscoveryStats> {
    let stats = source_stats.get_mut(source_id)?;
    stats.readiness_queue_depth = stats.readiness_queue_depth.saturating_sub(1);
    stats.progress_completed = stats
        .progress_completed
        .saturating_add(1)
        .min(stats.progress_total);
    Some(*stats)
}

pub(super) fn similarity_prerequisite_blocker_stats(
    snapshot: &ReadinessSnapshot,
) -> (usize, Option<i64>) {
    let Some(layout) = snapshot.entries.iter().find(|entry| {
        entry.target.stage == ReadinessStage::SimilarityLayout
            && entry.target.eligibility == ReadinessEligibility::Eligible
            && entry.classification != ReadinessClassification::Current
            && !snapshot.prerequisites_are_current(&entry.target)
    }) else {
        return (0, None);
    };
    let mut blocked = 0_usize;
    let mut all_retryable = true;
    let mut earliest_retry_at = None;
    for entry in snapshot.entries.iter().filter(|entry| {
        entry.target.source_id == layout.target.source_id
            && entry.target.source_generation == layout.target.source_generation
            && entry.target.scope_kind == ReadinessScopeKind::File
            && entry.target.eligibility == ReadinessEligibility::Eligible
            && matches!(
                entry.target.stage,
                ReadinessStage::IndexedIdentity
                    | ReadinessStage::AnalysisFeatures
                    | ReadinessStage::EmbeddingAspects
            )
            && entry.classification != ReadinessClassification::Current
    }) {
        blocked = blocked.saturating_add(1);
        match entry.classification {
            ReadinessClassification::RetryableFailure { retry_at, .. } => {
                earliest_retry_at = earliest_deadline(earliest_retry_at, Some(retry_at));
            }
            _ => all_retryable = false,
        }
    }
    (
        blocked,
        all_retryable.then_some(earliest_retry_at).flatten(),
    )
}
