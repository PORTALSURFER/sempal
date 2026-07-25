use super::{
    BTreeMap, BTreeSet, ReadinessActivity, ReadinessClassification, ReadinessSnapshot,
    ReadinessStage, ReadinessStageCounts, SourceAvailability, SourceDiscoveryStats,
    SourceProcessingHealthEvent, SourceProcessingHealthState, SourceProcessingLifecycle,
};

const MAX_FAILURE_CODES: usize = 8;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct SourceHealthSummary {
    state: SourceProcessingHealthState,
    source_generation: i64,
    readiness_revision: i64,
    stage_counts: BTreeMap<ReadinessStage, ReadinessStageCounts>,
    retry_at: Option<i64>,
    failure_codes: Vec<String>,
}

impl SourceHealthSummary {
    pub(super) fn offline() -> Self {
        Self {
            state: SourceProcessingHealthState::Offline,
            source_generation: 0,
            readiness_revision: 0,
            stage_counts: BTreeMap::new(),
            retry_at: None,
            failure_codes: Vec::new(),
        }
    }

    pub(super) fn reconciliation_failed(code: impl Into<String>) -> Self {
        Self::reconciliation_failed_at(code, None)
    }

    pub(super) fn reconciliation_failed_at(code: impl Into<String>, retry_at: Option<i64>) -> Self {
        Self {
            state: SourceProcessingHealthState::ReconciliationFailed,
            source_generation: 0,
            readiness_revision: 0,
            stage_counts: BTreeMap::new(),
            retry_at,
            failure_codes: vec![code.into()],
        }
    }

    pub(super) fn into_event(
        self,
        lifecycle: SourceProcessingLifecycle,
    ) -> SourceProcessingHealthEvent {
        SourceProcessingHealthEvent {
            lifecycle,
            state: self.state,
            source_generation: self.source_generation,
            readiness_revision: self.readiness_revision,
            stage_counts: self.stage_counts,
            retry_at: self.retry_at,
            failure_codes: self.failure_codes,
        }
    }

    #[cfg(test)]
    pub(crate) fn retry_at_for_test(&self) -> Option<i64> {
        self.retry_at
    }

    #[cfg(test)]
    pub(crate) fn failure_codes_for_test(&self) -> &[String] {
        &self.failure_codes
    }
}

pub(super) fn source_health_summary(
    snapshot: &ReadinessSnapshot,
    stats: &SourceDiscoveryStats,
) -> SourceHealthSummary {
    let failure_codes = bounded_failure_codes(snapshot);
    let terminal_count = snapshot
        .stage_counts
        .values()
        .fold(0_usize, |total, counts| {
            total
                .saturating_add(counts.permanent)
                .saturating_add(counts.unsupported)
                .saturating_add(counts.deleted)
        });
    let state = match snapshot.availability {
        SourceAvailability::Offline => SourceProcessingHealthState::Offline,
        SourceAvailability::Disabled => SourceProcessingHealthState::Disabled,
        SourceAvailability::Active
            if stats.readiness_queue_depth > 0
                || stats.retries_due > 0
                || matches!(
                    snapshot.activity,
                    ReadinessActivity::Actionable | ReadinessActivity::Running
                ) =>
        {
            SourceProcessingHealthState::Processing
        }
        SourceAvailability::Active if stats.prerequisites_blocked > 0 => {
            SourceProcessingHealthState::BlockedByPrerequisites
        }
        SourceAvailability::Active
            if stats.earliest_retry_at.is_some()
                || snapshot.activity == ReadinessActivity::WaitingForRetry =>
        {
            SourceProcessingHealthState::WaitingForRetry
        }
        SourceAvailability::Active if terminal_count > 0 => {
            SourceProcessingHealthState::DegradedTerminal
        }
        SourceAvailability::Active if snapshot.is_fully_ready() => {
            SourceProcessingHealthState::Ready
        }
        SourceAvailability::Active => SourceProcessingHealthState::Processing,
    };
    SourceHealthSummary {
        state,
        source_generation: snapshot.source_generation,
        readiness_revision: snapshot.readiness_revision,
        stage_counts: snapshot.stage_counts.clone(),
        retry_at: stats.prerequisite_retry_at.or(stats.earliest_retry_at),
        failure_codes,
    }
}

fn bounded_failure_codes(snapshot: &ReadinessSnapshot) -> Vec<String> {
    let mut codes = BTreeSet::new();
    for entry in &snapshot.entries {
        let code = match &entry.classification {
            ReadinessClassification::RetryableFailure { code, .. }
            | ReadinessClassification::PermanentFailure { code, .. } => Some(code.as_str()),
            ReadinessClassification::Unsupported { code } => {
                Some(code.as_deref().unwrap_or("unsupported_input"))
            }
            ReadinessClassification::Deleted => Some("deleted_readiness_target"),
            _ => None,
        };
        if let Some(code) = code {
            codes.insert(code.to_string());
            if codes.len() == MAX_FAILURE_CODES {
                break;
            }
        }
    }
    codes.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, mpsc};

    use wavecrate::sample_sources::{SampleSource, SourceId};

    use super::super::{
        Shared, SourceHealthPublicationOutcome, SourceProcessingEvent, SourceProcessingSupervisor,
    };
    use super::*;

    fn snapshot(
        availability: SourceAvailability,
        activity: ReadinessActivity,
    ) -> ReadinessSnapshot {
        ReadinessSnapshot {
            source_id: String::from("source"),
            source_generation: 7,
            readiness_revision: 9,
            availability,
            entries: Vec::new(),
            deficits: Vec::new(),
            stage_counts: BTreeMap::new(),
            activity,
        }
    }

    #[test]
    fn durable_health_distinguishes_ready_processing_waiting_offline_and_disabled() {
        let ready = source_health_summary(
            &snapshot(SourceAvailability::Active, ReadinessActivity::Idle),
            &SourceDiscoveryStats::default(),
        );
        assert_eq!(ready.state, SourceProcessingHealthState::Ready);

        let processing = source_health_summary(
            &snapshot(SourceAvailability::Active, ReadinessActivity::Actionable),
            &SourceDiscoveryStats::default(),
        );
        assert_eq!(processing.state, SourceProcessingHealthState::Processing);

        let waiting = source_health_summary(
            &snapshot(
                SourceAvailability::Active,
                ReadinessActivity::WaitingForRetry,
            ),
            &SourceDiscoveryStats {
                earliest_retry_at: Some(42),
                ..SourceDiscoveryStats::default()
            },
        );
        assert_eq!(waiting.state, SourceProcessingHealthState::WaitingForRetry);
        assert_eq!(waiting.retry_at, Some(42));

        let offline = source_health_summary(
            &snapshot(SourceAvailability::Offline, ReadinessActivity::Idle),
            &SourceDiscoveryStats::default(),
        );
        assert_eq!(offline.state, SourceProcessingHealthState::Offline);

        let disabled = source_health_summary(
            &snapshot(SourceAvailability::Disabled, ReadinessActivity::Idle),
            &SourceDiscoveryStats::default(),
        );
        assert_eq!(disabled.state, SourceProcessingHealthState::Disabled);

        let reconciliation_failed =
            SourceHealthSummary::reconciliation_failed_at("sqlite_busy", Some(84));
        assert_eq!(
            reconciliation_failed.state,
            SourceProcessingHealthState::ReconciliationFailed
        );
        assert_eq!(reconciliation_failed.retry_at, Some(84));
        assert_eq!(reconciliation_failed.failure_codes, ["sqlite_busy"]);
    }

    #[test]
    fn terminal_counts_and_prerequisite_blocks_never_masquerade_as_ready() {
        let mut degraded_snapshot = snapshot(SourceAvailability::Active, ReadinessActivity::Idle);
        degraded_snapshot.stage_counts.insert(
            ReadinessStage::AnalysisFeatures,
            ReadinessStageCounts {
                permanent: 2,
                unsupported: 3,
                ..ReadinessStageCounts::default()
            },
        );
        let degraded = source_health_summary(&degraded_snapshot, &SourceDiscoveryStats::default());
        assert_eq!(
            degraded.state,
            SourceProcessingHealthState::DegradedTerminal
        );

        let blocked = source_health_summary(
            &snapshot(SourceAvailability::Active, ReadinessActivity::Idle),
            &SourceDiscoveryStats {
                prerequisites_blocked: 1,
                ..SourceDiscoveryStats::default()
            },
        );
        assert_eq!(
            blocked.state,
            SourceProcessingHealthState::BlockedByPrerequisites
        );
    }

    #[test]
    fn health_publication_is_lifecycle_fenced_and_coalesced() {
        let root = tempfile::tempdir().expect("source root");
        let source = SampleSource::new_with_id(
            SourceId::from_string("health-source"),
            root.path().to_path_buf(),
        );
        let (sender, receiver) = mpsc::channel();
        let shared = Arc::new(Shared::new(vec![source], Some(Arc::new(sender))));
        let generation = shared.control().source_lifecycle_generations["health-source"];
        let health = source_health_summary(
            &snapshot(SourceAvailability::Active, ReadinessActivity::Idle),
            &SourceDiscoveryStats::default(),
        )
        .into_event(SourceProcessingLifecycle::new("health-source", generation));

        shared
            .state_machine_reject_next_health_publication
            .store(true, std::sync::atomic::Ordering::Release);
        assert_eq!(
            shared.publish_source_health_outcome(health.clone()),
            SourceHealthPublicationOutcome::Rejected
        );
        assert!(receiver.try_recv().is_err());
        assert!(shared.publish_source_health(health.clone()));
        assert!(!shared.publish_source_health(health));
        assert!(matches!(
            receiver.recv().expect("health event"),
            SourceProcessingEvent::Health(_)
        ));
        assert!(receiver.try_recv().is_err());

        let stale = source_health_summary(
            &snapshot(SourceAvailability::Active, ReadinessActivity::Actionable),
            &SourceDiscoveryStats::default(),
        )
        .into_event(SourceProcessingLifecycle::new(
            "health-source",
            generation.saturating_add(1),
        ));
        assert!(!shared.publish_source_health(stale));
        assert!(receiver.try_recv().is_err());

        let supervisor = SourceProcessingSupervisor {
            shared: Arc::clone(&shared),
            coordinator: None,
            retirement_worker: None,
        };
        supervisor
            .replace_sources(Vec::new())
            .expect("remove health source");
        assert!(
            shared
                .published_source_health
                .lock()
                .expect("published health")
                .is_empty(),
            "health coalescing state must remain bounded to configured lifecycles"
        );
    }
}
