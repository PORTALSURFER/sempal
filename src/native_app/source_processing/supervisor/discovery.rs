use super::{
    Arc, AtomicBool, BTreeMap, BTreeSet, Cancellable, DiscoveryProgressPublisher,
    DiscoveryProgressUpdate, Instant, PendingReadinessDelta, ProcessingLane, ReadinessStore,
    RuntimeCandidate, SOURCE_DISCOVERY_RETRY_SECONDS, SampleSource, Shared, SourceDatabase,
    SourceDatabaseConnectionRole, SourceDiscoveryStats, SourceHealthSummary,
    SourceProcessingPresentation, cancelled,
    discover_source_candidates_with_connection_and_progress_presented, now_epoch_seconds,
    readiness_safety_probe, source_health_summary, source_processing_schema_available,
};

const DISCOVERY_RETRY_PENDING_CODE: &str = "discovery_retry_pending";
const DISCOVERY_RECONCILIATION_FAILED_CODE: &str = "reconciliation_failed";

/// Keep transient discovery outages out of the repair-needed state. The discovery boundary
/// currently carries errors as strings, so classify only exact, known SQLite/OS leaf messages;
/// unknown/schema-integrity failures remain durable reconciliation failures.
fn discovery_error_is_retryable(error: &str) -> bool {
    let error = error.trim().to_ascii_lowercase();
    let leaf = error
        .rsplit_once(": ")
        .map_or(error.as_str(), |(_, leaf)| leaf);
    let known_sqlite_wrapper = error == leaf
        || error
            .strip_prefix("database query failed: ")
            .is_some_and(|cause| cause == leaf);
    (known_sqlite_wrapper
        && (matches!(
            leaf,
            "database is busy, please retry" | "database is locked"
        ) || leaf
            .strip_prefix("database is locked (")
            .and_then(|suffix| suffix.strip_suffix(')'))
            .is_some_and(|code| {
                !code.is_empty() && code.chars().all(|character| character.is_ascii_digit())
            })))
        || [
            "operation timed out",
            "resource temporarily unavailable",
            "stale file handle",
            "input/output error",
            "i/o error",
            "interrupted system call",
        ]
        .iter()
        .any(|marker| os_error_leaf_matches(leaf, marker))
}

fn os_error_leaf_matches(leaf: &str, marker: &str) -> bool {
    leaf.strip_prefix(marker)
        .and_then(|suffix| suffix.strip_prefix(" (os error "))
        .and_then(|suffix| suffix.strip_suffix(')'))
        .is_some_and(|code| {
            !code.is_empty() && code.chars().all(|character| character.is_ascii_digit())
        })
}

fn discovery_failure_health(error: &str, retry_at: i64) -> SourceHealthSummary {
    if discovery_error_is_retryable(error) {
        SourceHealthSummary::waiting_for_retry(DISCOVERY_RETRY_PENDING_CODE, retry_at)
    } else {
        SourceHealthSummary::reconciliation_failed(DISCOVERY_RECONCILIATION_FAILED_CODE)
    }
}

#[cfg(test)]
mod tests {
    use super::{discovery_error_is_retryable, discovery_failure_health};
    use crate::native_app::source_processing::SourceProcessingHealthState;

    #[test]
    fn discovery_error_classification_is_conservative() {
        assert!(discovery_error_is_retryable(
            "Database is busy, please retry"
        ));
        assert!(discovery_error_is_retryable(
            "Database query failed: database is locked"
        ));
        assert!(discovery_error_is_retryable(
            "Could not read source: Input/output error (os error 5)"
        ));
        assert!(discovery_error_is_retryable(
            "Could not inspect source database path /tmp/source: Operation timed out (os error 60)"
        ));
        assert!(!discovery_error_is_retryable(
            "Database query failed: no such table: metadata"
        ));
        assert!(!discovery_error_is_retryable(
            "SQLite returned an unexpected result"
        ));
        assert!(!discovery_error_is_retryable(
            "Database query failed: database disk image is malformed"
        ));
        assert!(!discovery_error_is_retryable(
            "Database query failed: malformed schema timeout while validating"
        ));
        assert!(!discovery_error_is_retryable(
            "Database query failed: no such table: metadata (database is locked)"
        ));
        assert!(!discovery_error_is_retryable(
            "SQLite returned an unexpected result: input/output error recorded in metadata"
        ));
        assert!(!discovery_error_is_retryable("operation timed out"));
        assert!(!discovery_error_is_retryable("input/output error"));
        assert!(!discovery_error_is_retryable("i/o error"));
        assert!(!discovery_error_is_retryable(
            "schema validation: database is locked"
        ));
    }

    #[test]
    fn ambiguous_discovery_errors_stay_repair_needed_without_retry_deadline() {
        for error in [
            "Database query failed: malformed schema timeout while validating",
            "operation timed out",
            "input/output error",
            "i/o error",
            "schema validation: database is locked",
        ] {
            let health = discovery_failure_health(error, 123);
            assert_eq!(
                health.state_for_test(),
                SourceProcessingHealthState::ReconciliationFailed,
                "error: {error}"
            );
            assert_eq!(health.retry_at_for_test(), None, "error: {error}");
            assert_eq!(
                health.failure_codes_for_test(),
                ["reconciliation_failed"],
                "error: {error}"
            );
        }
    }
}

pub(super) fn scheduler_candidate_indices(
    candidates: &[RuntimeCandidate],
    external_scan_admitted: bool,
) -> Vec<usize> {
    candidates
        .iter()
        .enumerate()
        .filter_map(|(index, _candidate)| (!external_scan_admitted).then_some(index))
        .collect()
}

pub(super) fn discover_candidates(
    shared: &Arc<Shared>,
    sources: &[SampleSource],
    force_manifest_audit_sources: &BTreeSet<String>,
    force_reanalysis_sources: &BTreeSet<String>,
    pending_readiness_deltas: &BTreeMap<String, PendingReadinessDelta>,
    safety_probe_only: bool,
    presentation: SourceProcessingPresentation,
    source_work_cancels: &BTreeMap<String, Arc<AtomicBool>>,
) -> (
    Vec<RuntimeCandidate>,
    BTreeMap<String, SourceDiscoveryStats>,
    BTreeSet<String>,
    BTreeSet<String>,
    bool,
) {
    let now = now_epoch_seconds();
    let mut candidates = Vec::new();
    let mut source_stats = BTreeMap::new();
    let mut deferred = BTreeSet::new();
    let mut consumed_readiness_deltas = BTreeSet::new();
    let mut progress_published = false;
    for source in sources {
        let Some(permit) = shared
            .budgets()
            .try_acquire(source.id.as_str(), ProcessingLane::Cleanup)
        else {
            deferred.insert(source.id.as_str().to_string());
            continue;
        };
        let Some(source_cancel) = source_work_cancels.get(source.id.as_str()) else {
            shared.budgets().release(permit);
            shared.budget_wake.notify_all();
            continue;
        };
        let Some(in_flight_work) = shared.begin_in_flight_work(source.id.as_str(), source_cancel)
        else {
            shared.budgets().release(permit);
            shared.budget_wake.notify_all();
            continue;
        };
        {
            let mut telemetry = shared.telemetry();
            telemetry.source_discoveries = telemetry.source_discoveries.saturating_add(1);
        }
        let mut progress = DiscoveryProgressPublisher {
            shared,
            source_id: source.id.as_str(),
            lifecycle_generation: in_flight_work.lifecycle_generation,
            started_at: Instant::now(),
            last_progress: None,
            last_event_publish_at: None,
            last_log_publish_at: None,
            event_published: false,
            work_units: 0,
            presentation,
        };
        let discovery_result = {
            let _writer = shared
                .database_writer
                .lock(super::DatabasePhase::SerialCompatibility);
            discover_source_candidates_with_progress_presented(
                source,
                now,
                force_manifest_audit_sources.contains(source.id.as_str()),
                force_reanalysis_sources.contains(source.id.as_str()),
                pending_readiness_deltas.get(source.id.as_str()),
                safety_probe_only,
                source_cancel,
                &mut |update| progress.advance(update),
            )
        };
        match discovery_result {
            Ok(Cancellable::Completed((mut source_candidates, stats, health))) => {
                if stats.cheap_noop_sweep {
                    let mut telemetry = shared.telemetry();
                    telemetry.cheap_noop_sweeps = telemetry.cheap_noop_sweeps.saturating_add(1);
                }
                if stats.delta_reconciled {
                    let mut telemetry = shared.telemetry();
                    telemetry.delta_reconciliations =
                        telemetry.delta_reconciliations.saturating_add(1);
                }
                let pending_revision = pending_readiness_deltas
                    .get(source.id.as_str())
                    .and_then(|pending| pending.latest_manifest_revision);
                let mut control = shared.control();
                if let Some(revision) = manifest_revision_to_accept(&stats, pending_revision) {
                    control.accept_reconciled_manifest_revision(
                        source.id.as_str(),
                        in_flight_work.lifecycle_generation,
                        revision,
                    );
                }
                drop(control);
                candidates.append(&mut source_candidates);
                if !stats.cheap_noop_sweep {
                    source_stats.insert(source.id.as_str().to_string(), stats);
                }
                let pending_readiness_delta = pending_readiness_deltas.get(source.id.as_str());
                let mut consume_readiness_delta = pending_readiness_delta.is_some();
                if let Some(health) = health {
                    let health = health.into_event(super::SourceProcessingLifecycle::new(
                        source.id.as_str(),
                        in_flight_work.lifecycle_generation,
                    ));
                    let publication_outcome = shared.publish_source_health_outcome(health.clone());
                    consume_readiness_delta = matches!(
                        publication_outcome,
                        super::SourceHealthPublicationOutcome::Published
                            | super::SourceHealthPublicationOutcome::AlreadyPublished
                            | super::SourceHealthPublicationOutcome::NoSink
                    );
                    #[cfg(test)]
                    if let Some(pending) = pending_readiness_delta
                        .filter(|pending| !pending.state_machine_inputs.is_empty())
                        .filter(|_| {
                            publication_outcome != super::SourceHealthPublicationOutcome::NoSink
                                && publication_outcome
                                    != super::SourceHealthPublicationOutcome::Superseded
                        })
                    {
                        shared
                            .state_machine_publications
                            .lock()
                            .unwrap_or_else(|poison| poison.into_inner())
                            .push(super::StateMachinePublicationObservation {
                                source_id: source.id.as_str().to_string(),
                                lifecycle_generation: in_flight_work.lifecycle_generation,
                                source_generation: health.source_generation,
                                readiness_revision: health.readiness_revision,
                                inputs: pending.state_machine_inputs.clone(),
                                outcome: publication_outcome,
                            });
                    }
                }
                if consume_readiness_delta {
                    consumed_readiness_deltas.insert(source.id.as_str().to_string());
                }
                shared
                    .control()
                    .force_reanalysis_sources
                    .remove(source.id.as_str());
            }
            Ok(Cancellable::Cancelled) => {
                deferred.insert(source.id.as_str().to_string());
            }
            Err(error) => {
                record_discovery_error(shared, source, &error);
                let retry_at = now_epoch_seconds().saturating_add(SOURCE_DISCOVERY_RETRY_SECONDS);
                let health = discovery_failure_health(&error, retry_at);
                shared.publish_source_health(health.into_event(
                    super::SourceProcessingLifecycle::new(
                        source.id.as_str(),
                        in_flight_work.lifecycle_generation,
                    ),
                ));
                source_stats.insert(
                    source.id.as_str().to_string(),
                    SourceDiscoveryStats {
                        earliest_retry_at: Some(retry_at),
                        ..SourceDiscoveryStats::default()
                    },
                );
            }
        }
        progress_published |= progress.event_published;
        drop(in_flight_work);
        shared.budgets().release(permit);
        shared.budget_wake.notify_all();
    }
    (
        candidates,
        source_stats,
        deferred,
        consumed_readiness_deltas,
        progress_published,
    )
}

pub(super) fn manifest_revision_to_accept(
    stats: &SourceDiscoveryStats,
    pending_revision: Option<u64>,
) -> Option<u64> {
    if stats.delta_reconciled {
        pending_revision
    } else {
        stats.reconciled_manifest_revision
    }
}

#[cfg(test)]
pub(super) fn discover_source_candidates(
    source: &SampleSource,
    now: i64,
    force_manifest_audit: bool,
    cancel: &AtomicBool,
) -> Result<Cancellable<(Vec<RuntimeCandidate>, SourceDiscoveryStats)>, String> {
    discover_source_candidates_with_progress(
        source,
        now,
        force_manifest_audit,
        false,
        None,
        false,
        cancel,
        &mut |_| {},
    )
    .map(|result| match result {
        Cancellable::Completed((candidates, stats, _)) => {
            Cancellable::Completed((candidates, stats))
        }
        Cancellable::Cancelled => Cancellable::Cancelled,
    })
}

#[cfg(test)]
pub(super) fn discover_source_candidates_with_progress(
    source: &SampleSource,
    now: i64,
    force_manifest_audit: bool,
    force_reanalysis: bool,
    pending_readiness_delta: Option<&PendingReadinessDelta>,
    safety_probe_only: bool,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(DiscoveryProgressUpdate),
) -> Result<
    Cancellable<(
        Vec<RuntimeCandidate>,
        SourceDiscoveryStats,
        Option<SourceHealthSummary>,
    )>,
    String,
> {
    discover_source_candidates_with_progress_presented(
        source,
        now,
        force_manifest_audit,
        force_reanalysis,
        pending_readiness_delta,
        safety_probe_only,
        cancel,
        progress,
    )
}

pub(super) fn discover_source_candidates_with_progress_presented(
    source: &SampleSource,
    now: i64,
    force_manifest_audit: bool,
    force_reanalysis: bool,
    pending_readiness_delta: Option<&PendingReadinessDelta>,
    safety_probe_only: bool,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(DiscoveryProgressUpdate),
) -> Result<
    Cancellable<(
        Vec<RuntimeCandidate>,
        SourceDiscoveryStats,
        Option<SourceHealthSummary>,
    )>,
    String,
> {
    if cancelled(cancel) {
        return Ok(Cancellable::Cancelled);
    }
    let database_root = source.database_root().map_err(|error| error.to_string())?;
    if !source.root.is_dir() {
        let health = if database_root != source.root && database_root.is_dir() {
            let mut connection = SourceDatabase::open_unavailable_source_metadata_connection(
                &database_root,
                SourceDatabaseConnectionRole::JobWorker,
            )
            .map_err(|error| error.to_string())?;
            if source_processing_schema_available(&mut connection)? {
                ReadinessStore::new(&mut connection)
                    .mark_temporarily_unavailable(source.id.as_str(), now)
                    .map_err(|error| error.to_string())?;
                ReadinessStore::new(&mut connection)
                    .reconcile(source.id.as_str(), now)
                    .ok()
                    .map(|snapshot| {
                        source_health_summary(&snapshot, &SourceDiscoveryStats::default())
                    })
                    .unwrap_or_else(SourceHealthSummary::offline)
            } else {
                SourceHealthSummary::offline()
            }
        } else {
            SourceHealthSummary::offline()
        };
        return Ok(Cancellable::Completed((
            Vec::new(),
            SourceDiscoveryStats::default(),
            Some(health),
        )));
    }
    if safety_probe_only {
        match SourceDatabase::open_connection_with_role_and_database_root(
            &source.root,
            &database_root,
            SourceDatabaseConnectionRole::BackgroundRead,
        ) {
            Ok(mut probe_connection) => {
                let probe = readiness_safety_probe(
                    &mut probe_connection,
                    source,
                    now,
                    force_manifest_audit,
                )?;
                if probe.current && probe.earliest_deadline.is_none() {
                    tracing::debug!(
                        target: "wavecrate::source_processing",
                        event = "source_processing.safety_sweep_noop",
                        source_id = source.id.as_str(),
                        "Periodic readiness safety probe found no durable delta"
                    );
                    return Ok(Cancellable::Completed((
                        Vec::new(),
                        SourceDiscoveryStats {
                            cheap_noop_sweep: true,
                            ..SourceDiscoveryStats::default()
                        },
                        None,
                    )));
                } else if probe.current {
                    return Ok(Cancellable::Completed((
                        Vec::new(),
                        SourceDiscoveryStats {
                            earliest_retry_at: probe.earliest_deadline,
                            ..SourceDiscoveryStats::default()
                        },
                        None,
                    )));
                }
            }
            Err(error) => {
                tracing::debug!(
                    target: "wavecrate::source_processing",
                    source_id = source.id.as_str(),
                    %error,
                    "Read-only readiness safety probe unavailable; retrying with worker connection"
                );
            }
        }
    }
    let mut connection = SourceDatabase::open_connection_with_role_and_database_root(
        &source.root,
        &database_root,
        SourceDatabaseConnectionRole::JobWorker,
    )
    .map_err(|error| error.to_string())?;
    discover_source_candidates_with_connection_and_progress_presented(
        source,
        &mut connection,
        now,
        force_manifest_audit,
        force_reanalysis,
        pending_readiness_delta,
        safety_probe_only,
        cancel,
        progress,
    )
}

pub(super) fn record_discovery_error(shared: &Shared, source: &SampleSource, error: &str) {
    let mut telemetry = shared.telemetry();
    telemetry.failed = telemetry.failed.saturating_add(1);
    drop(telemetry);
    tracing::warn!(
        target: "wavecrate::source_processing",
        source_id = source.id.as_str(),
        error,
        "Source processing discovery failed"
    );
}
