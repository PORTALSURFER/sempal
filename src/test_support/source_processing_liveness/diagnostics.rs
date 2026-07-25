use super::*;

#[derive(Debug, Serialize)]
pub(in crate::native_app::source_processing::supervisor) struct RuntimeObservation {
    pub(in crate::native_app::source_processing::supervisor) coordinator_running: bool,
    pub(in crate::native_app::source_processing::supervisor) source_configured: bool,
    pub(in crate::native_app::source_processing::supervisor) source_active: bool,
    pub(in crate::native_app::source_processing::supervisor) source_dirty: bool,
    pub(in crate::native_app::source_processing::supervisor) source_quarantined: bool,
    pub(in crate::native_app::source_processing::supervisor) wake_generation: u64,
    pub(in crate::native_app::source_processing::supervisor) settled_wake_generation: u64,
    pub(in crate::native_app::source_processing::supervisor) wake_reason: &'static str,
    pub(in crate::native_app::source_processing::supervisor) lifecycle_generation: Option<u64>,
    pub(in crate::native_app::source_processing::supervisor) in_flight: usize,
    pub(in crate::native_app::source_processing::supervisor) active_budget: bool,
    pub(in crate::native_app::source_processing::supervisor) queue_depth: usize,
    pub(in crate::native_app::source_processing::supervisor) readiness_queue_depth: usize,
    pub(in crate::native_app::source_processing::supervisor) retries_due: usize,
    pub(in crate::native_app::source_processing::supervisor) retry_at: Option<i64>,
    pub(in crate::native_app::source_processing::supervisor) sweeps: u64,
    pub(in crate::native_app::source_processing::supervisor) claimed: u64,
    pub(in crate::native_app::source_processing::supervisor) completed: u64,
    pub(in crate::native_app::source_processing::supervisor) failed: u64,
    pub(in crate::native_app::source_processing::supervisor) retried: u64,
    pub(in crate::native_app::source_processing::supervisor) stale: u64,
    pub(in crate::native_app::source_processing::supervisor) cancelled: u64,
    pub(in crate::native_app::source_processing::supervisor) contention: u64,
    pub(in crate::native_app::source_processing::supervisor) oldest_job_age_seconds: u64,
}

#[derive(Debug, Serialize)]
struct DurableJobDiagnostics {
    by_status: BTreeMap<String, usize>,
    earliest_retry_at: Option<i64>,
    earliest_lease_expiry: Option<i64>,
}

#[derive(Debug, Serialize)]
struct LivenessDiagnostic {
    source_id: String,
    source_root_available: bool,
    watcher_stimulus: WatcherStimulus,
    watcher_events_committed: u64,
    source_generation: Option<i64>,
    readiness_revision: Option<i64>,
    availability: Option<String>,
    activity: Option<String>,
    deficits_by_stage: BTreeMap<String, usize>,
    classifications_by_stage: BTreeMap<String, BTreeMap<String, usize>>,
    durable_jobs: Option<DurableJobDiagnostics>,
    runtime: RuntimeObservation,
}

pub(in crate::native_app::source_processing::supervisor) fn silently_idle(
    snapshot: &ReadinessSnapshot,
    runtime: &RuntimeObservation,
) -> bool {
    if snapshot.availability != SourceAvailability::Active || snapshot.deficits.is_empty() {
        return false;
    }
    let now = now_epoch_seconds();
    let has_schedulable_deficit = snapshot
        .deficits
        .iter()
        .any(|deficit| snapshot.prerequisites_are_current(&deficit.target));
    let waiting_for_retry = !has_schedulable_deficit
        && (snapshot.activity == ReadinessActivity::WaitingForRetry
            || snapshot.entries.iter().any(|entry| {
                matches!(
                    entry.classification,
                    ReadinessClassification::RetryableFailure { retry_at, .. } if retry_at > now
                )
            }));
    let waiting_for_prerequisite = !has_schedulable_deficit
        && snapshot.entries.iter().any(|entry| {
            entry.target.stage == ReadinessStage::SimilarityLayout
                && entry.classification != ReadinessClassification::Current
                && !snapshot.prerequisites_are_current(&entry.target)
        });
    let observable_work = runtime.source_dirty
        || runtime.wake_generation > runtime.settled_wake_generation
        || runtime.queue_depth > 0
        || runtime.readiness_queue_depth > 0
        || runtime.in_flight > 0
        || runtime.active_budget
        // Retry deadlines are tracked with second precision. Keep a deadline that is due in
        // the current second observable until the coordinator gets a chance to dispatch its
        // reconciliation sweep; otherwise the liveness oracle can report a false idle window
        // between the timer expiring and the next coordinator pass.
        || runtime.retry_at.is_some_and(|retry_at| retry_at >= now)
        || waiting_for_retry
        || waiting_for_prerequisite;
    !runtime.coordinator_running || !runtime.source_active || !observable_work
}

pub(in crate::native_app::source_processing::supervisor) fn runtime_observation(
    supervisor: &SourceProcessingSupervisor,
    source_id: &str,
) -> RuntimeObservation {
    let control = supervisor.shared.control();
    let lifecycle_generation = control.source_lifecycle_generations.get(source_id).copied();
    let source_configured = control.sources.contains_key(source_id);
    let source_active = control.source_is_active(source_id);
    let source_dirty = control.dirty_sources.contains(source_id);
    let source_quarantined = control.quarantined_sources.contains(source_id);
    let wake_generation = control.wake_generation;
    let wake_reason = control.wake_reason;
    drop(control);

    let in_flight = supervisor
        .shared
        .in_flight_work
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
        .iter()
        .filter(|((candidate_source_id, _), _)| candidate_source_id == source_id)
        .map(|(_, count)| *count)
        .sum();
    let active_budget = supervisor
        .shared
        .budgets()
        .active_sources()
        .iter()
        .any(|active| active == source_id);
    let telemetry = supervisor.shared.telemetry();
    RuntimeObservation {
        coordinator_running: supervisor
            .coordinator
            .as_ref()
            .is_some_and(|handle| !handle.is_finished()),
        source_configured,
        source_active,
        source_dirty,
        source_quarantined,
        wake_generation,
        settled_wake_generation: telemetry.settled_wake_generation,
        wake_reason,
        lifecycle_generation,
        in_flight,
        active_budget,
        queue_depth: telemetry
            .queue_depth_by_source
            .get(source_id)
            .copied()
            .unwrap_or_default(),
        readiness_queue_depth: telemetry
            .readiness_queue_depth_by_source
            .get(source_id)
            .copied()
            .unwrap_or_default(),
        retries_due: telemetry
            .retries_due_by_source
            .get(source_id)
            .copied()
            .unwrap_or_default(),
        retry_at: telemetry.retry_at_by_source.get(source_id).copied(),
        sweeps: telemetry.sweeps,
        claimed: telemetry.claimed,
        completed: telemetry.completed,
        failed: telemetry.failed,
        retried: telemetry.retried,
        stale: telemetry.stale,
        cancelled: telemetry.cancelled,
        contention: telemetry.contention,
        oldest_job_age_seconds: telemetry.oldest_job_age_seconds,
    }
}

pub(super) fn diagnostic_json(
    source: &SampleSource,
    supervisor: &SourceProcessingSupervisor,
    watcher_stimulus: WatcherStimulus,
    watcher_events_committed: u64,
) -> String {
    let snapshot = readiness_snapshot(source);
    let mut deficits_by_stage = BTreeMap::new();
    let mut classifications_by_stage = BTreeMap::<String, BTreeMap<String, usize>>::new();
    if let Some(snapshot) = snapshot.as_ref() {
        for deficit in &snapshot.deficits {
            *deficits_by_stage
                .entry(format!("{:?}", deficit.target.stage))
                .or_default() += 1;
        }
        for entry in &snapshot.entries {
            *classifications_by_stage
                .entry(format!("{:?}", entry.target.stage))
                .or_default()
                .entry(format!("{:?}", entry.classification))
                .or_default() += 1;
        }
    }
    let durable_jobs = open_connection(source)
        .ok()
        .and_then(|connection| durable_job_diagnostics(&connection).ok());
    let diagnostic = LivenessDiagnostic {
        source_id: source.id.as_str().to_string(),
        source_root_available: source.root.is_dir(),
        watcher_stimulus,
        watcher_events_committed,
        source_generation: snapshot.as_ref().map(|snapshot| snapshot.source_generation),
        readiness_revision: snapshot
            .as_ref()
            .map(|snapshot| snapshot.readiness_revision),
        availability: snapshot
            .as_ref()
            .map(|snapshot| format!("{:?}", snapshot.availability)),
        activity: snapshot
            .as_ref()
            .map(|snapshot| format!("{:?}", snapshot.activity)),
        deficits_by_stage,
        classifications_by_stage,
        durable_jobs,
        runtime: runtime_observation(supervisor, source.id.as_str()),
    };
    serde_json::to_string_pretty(&diagnostic)
        .unwrap_or_else(|error| format!("failed to serialize liveness diagnostic: {error}"))
}

fn durable_job_diagnostics(connection: &Connection) -> rusqlite::Result<DurableJobDiagnostics> {
    let mut by_status = BTreeMap::new();
    let mut statement = connection.prepare(
        "SELECT status, COUNT(*)
         FROM analysis_jobs
         WHERE readiness_managed = 1
         GROUP BY status
         ORDER BY status",
    )?;
    for row in statement.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, usize>(1)?))
    })? {
        let (status, count) = row?;
        by_status.insert(status, count);
    }
    let (earliest_retry_at, earliest_lease_expiry) = connection.query_row(
        "SELECT MIN(retry_at), MIN(lease_expires_at)
         FROM analysis_jobs
         WHERE readiness_managed = 1",
        [],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    Ok(DurableJobDiagnostics {
        by_status,
        earliest_retry_at,
        earliest_lease_expiry,
    })
}

pub(in crate::native_app::source_processing::supervisor) fn readiness_snapshot(
    source: &SampleSource,
) -> Option<ReadinessSnapshot> {
    let mut connection = open_connection(source).ok()?;
    ReadinessStore::new(&mut connection)
        .reconcile(source.id.as_str(), now_epoch_seconds())
        .ok()
}

pub(super) fn open_connection(source: &SampleSource) -> Result<Connection, String> {
    let database_root = source.database_root().map_err(|error| error.to_string())?;
    if source.root.is_dir() {
        SourceDatabase::open_connection_with_role_and_database_root(
            &source.root,
            &database_root,
            SourceDatabaseConnectionRole::JobWorker,
        )
        .map_err(|error| error.to_string())
    } else {
        SourceDatabase::open_unavailable_source_metadata_connection(
            &database_root,
            SourceDatabaseConnectionRole::JobWorker,
        )
        .map_err(|error| error.to_string())
    }
}
