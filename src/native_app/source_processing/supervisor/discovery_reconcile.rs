use super::{
    AtomicBool, Cancellable, DiscoveryProgressUpdate, MANIFEST_AUDIT_INTERVAL_SECONDS,
    META_LAST_MANIFEST_AUDIT_AT, META_WAV_IDENTITIES_REVISION, META_WAV_PATHS_REVISION,
    PendingReadinessDelta, ProcessingLane, ReadinessClassification, ReadinessProgress,
    ReadinessStore, ReadinessView, RuntimeCandidate, RuntimeTask, SampleSource, SourceAvailability,
    SourceDiscoveryPhase, SourceDiscoveryStats, SourceHealthSummary, WorkCandidate,
    active_recording_deferrals, cancelled, clear_duplicate_identity_diagnostic,
    duplicate_identity_diagnostic_for_revision, earliest_deadline,
    find_duplicate_identity_diagnostics, legacy_unsupported_decode_failure_text,
    persist_duplicate_identity_diagnostic, publish_current_readiness_delta_with_cancel,
    publish_current_readiness_targets_with_cancel_and_checkpoint, readiness_contract_version,
    retire_legacy_playback_readiness, similarity_prerequisite_blocker_stats,
    source_processing_schema_available,
};
use rusqlite::OptionalExtension;
#[cfg(target_os = "macos")]
use wavecrate_library::{
    filesystem_identity::stable_filesystem_identity,
    sample_sources::db::META_SOURCE_WATCHER_CHECKPOINT,
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct ReadinessSafetyProbe {
    pub(super) current: bool,
    pub(super) earliest_deadline: Option<i64>,
}

pub(super) fn readiness_safety_probe(
    connection: &mut rusqlite::Connection,
    source: &SampleSource,
    now: i64,
    force_manifest_audit: bool,
) -> Result<ReadinessSafetyProbe, String> {
    if force_manifest_audit || !source_processing_schema_available(connection)? {
        return Ok(ReadinessSafetyProbe::default());
    }
    let source_id = source.id.as_str();
    let last_manifest_audit_at = connection
        .query_row(
            "SELECT value FROM metadata WHERE key = ?1",
            [META_LAST_MANIFEST_AUDIT_AT],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(|error| error.to_string())?
        .and_then(|value| value.parse::<i64>().ok())
        .unwrap_or_default();
    if now.saturating_sub(last_manifest_audit_at) >= MANIFEST_AUDIT_INTERVAL_SECONDS {
        return Ok(ReadinessSafetyProbe::default());
    }
    let source_generation = connection
        .query_row(
            "SELECT COALESCE(
                (SELECT CAST(value AS INTEGER) FROM metadata WHERE key = ?1),
                0
             )",
            [META_WAV_PATHS_REVISION],
            |row| row.get::<_, i64>(0),
        )
        .map_err(|error| error.to_string())?;
    let contract_version = readiness_contract_version();
    let readiness_is_current = ReadinessStore::new(connection)
        .source_state(source_id)
        .map_err(|error| error.to_string())?
        .is_some_and(|state| {
            state.source_generation == source_generation
                && state.availability == SourceAvailability::Active
                && state.contract_version == contract_version
        });
    if !readiness_is_current || !durable_watcher_coverage_is_current(connection, source)? {
        return Ok(ReadinessSafetyProbe::default());
    }
    let work = ReadinessView::new(connection)
        .source_work_stats(source_id, now)
        .map_err(|error| error.to_string())?;
    Ok(ReadinessSafetyProbe {
        current: !work.has_actionable_work(),
        earliest_deadline: work.earliest_future_deadline(),
    })
}

/// On macOS a current readiness revision is only safe to skip when its root still matches the
/// durable FSEvents replay cursor. The watcher advances work from that cursor after it is live;
/// a missing or replaced cursor deliberately leaves admission eligible for the bounded audit.
#[cfg(target_os = "macos")]
fn durable_watcher_coverage_is_current(
    connection: &rusqlite::Connection,
    source: &SampleSource,
) -> Result<bool, String> {
    let Some(root_identity) = std::fs::metadata(&source.root)
        .ok()
        .and_then(|metadata| stable_filesystem_identity(&source.root, &metadata))
    else {
        return Ok(false);
    };
    let checkpoint = connection
        .query_row(
            "SELECT value FROM metadata WHERE key = ?1",
            [META_SOURCE_WATCHER_CHECKPOINT],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(|error| error.to_string())?;
    let Some(checkpoint) = checkpoint else {
        return Ok(false);
    };
    let value: serde_json::Value = match serde_json::from_str(&checkpoint) {
        Ok(value) => value,
        Err(_) => return Ok(false),
    };
    Ok(value
        .get("root_identity")
        .and_then(serde_json::Value::as_str)
        .is_some_and(|identity| identity == root_identity)
        && value
            .get("event_id")
            .and_then(serde_json::Value::as_u64)
            .is_some_and(|event_id| event_id > 0))
}

#[cfg(not(target_os = "macos"))]
fn durable_watcher_coverage_is_current(
    _connection: &rusqlite::Connection,
    _source: &SampleSource,
) -> Result<bool, String> {
    // Other platforms keep their established bounded audit behavior until a comparable durable
    // journal is available. They must not treat a process-local notify stream as a durable cursor.
    Ok(true)
}

#[cfg(test)]
pub(super) fn discover_source_candidates_with_connection(
    source: &SampleSource,
    connection: &mut rusqlite::Connection,
    now: i64,
    force_manifest_audit: bool,
    cancel: &AtomicBool,
) -> Result<Cancellable<(Vec<RuntimeCandidate>, SourceDiscoveryStats)>, String> {
    discover_source_candidates_with_connection_and_progress(
        source,
        connection,
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

pub(super) fn discover_source_candidates_with_connection_and_progress(
    source: &SampleSource,
    connection: &mut rusqlite::Connection,
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
    let source_id = source.id.as_str();
    let mut candidates = Vec::new();
    let mut stats = SourceDiscoveryStats::default();
    if connection
        .is_readonly(rusqlite::MAIN_DB)
        .map_err(|error| error.to_string())?
    {
        tracing::debug!(
            target: "wavecrate::source_processing",
            source_id,
            "Source processing is disabled for a read-only source database"
        );
        return Ok(Cancellable::Completed((
            candidates,
            stats,
            Some(SourceHealthSummary::reconciliation_failed(
                "read_only_source",
            )),
        )));
    }
    if !source_processing_schema_available(connection)? {
        tracing::debug!(
            target: "wavecrate::source_processing",
            source_id,
            "Source processing is unavailable until the read-only source database is migrated"
        );
        return Ok(Cancellable::Completed((
            candidates,
            stats,
            Some(SourceHealthSummary::reconciliation_failed(
                "readiness_schema_unavailable",
            )),
        )));
    }
    let identity_revision = connection
        .query_row(
            "SELECT COALESCE(
                (SELECT CAST(value AS INTEGER) FROM metadata WHERE key = ?1),
                0
             )",
            [META_WAV_IDENTITIES_REVISION],
            |row| row.get::<_, i64>(0),
        )
        .map_err(|error| error.to_string())?;
    if !force_manifest_audit
        && duplicate_identity_diagnostic_for_revision(connection, identity_revision)?.is_some()
    {
        tracing::warn!(
            target: "wavecrate::source_processing",
            event = "source_processing.duplicate_identity_terminal",
            source_id,
            identity_revision,
            "Skipped unchanged readiness reconciliation after a durable duplicate-identity diagnosis"
        );
        return Ok(Cancellable::Completed((
            candidates,
            stats,
            Some(SourceHealthSummary::reconciliation_failed(
                "duplicate_manifest_identity",
            )),
        )));
    }
    clear_duplicate_identity_diagnostic(connection)?;
    let duplicate_identities = find_duplicate_identity_diagnostics(connection)?;
    if !duplicate_identities.is_empty() {
        persist_duplicate_identity_diagnostic(
            connection,
            identity_revision,
            &duplicate_identities,
        )?;
        for diagnostic in &duplicate_identities {
            tracing::warn!(
                target: "wavecrate::source_processing",
                event = "source_processing.duplicate_identity_terminal",
                source_id,
                identity_revision,
                identity = diagnostic.identity.as_str(),
                alias_count = diagnostic.paths.len(),
                paths = ?diagnostic.paths,
                "Readiness reconciliation is parked because multiple manifest paths share one stable filesystem identity"
            );
        }
        return Ok(Cancellable::Completed((
            candidates,
            stats,
            Some(SourceHealthSummary::reconciliation_failed(
                "duplicate_manifest_identity",
            )),
        )));
    }
    let durable_watcher_coverage_current = durable_watcher_coverage_is_current(connection, source)?;
    let last_manifest_audit_at = connection
        .query_row(
            "SELECT value FROM metadata WHERE key = ?1",
            [META_LAST_MANIFEST_AUDIT_AT],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(|error| error.to_string())?
        .and_then(|value| value.parse::<i64>().ok())
        .unwrap_or_default();
    if safety_probe_only {
        let probe = readiness_safety_probe(connection, source, now, force_manifest_audit)?;
        if probe.current {
            if probe.earliest_deadline.is_none() {
                stats.cheap_noop_sweep = true;
                tracing::debug!(
                    target: "wavecrate::source_processing",
                    event = "source_processing.safety_sweep_noop",
                    source_id,
                    "Periodic readiness safety probe found no durable delta"
                );
            } else {
                stats.earliest_retry_at = probe.earliest_deadline;
                tracing::debug!(
                    target: "wavecrate::source_processing",
                    event = "source_processing.safety_sweep_deadline",
                    source_id,
                    deadline = probe.earliest_deadline,
                    "Readiness safety probe parked until durable work is actionable"
                );
            }
            return Ok(Cancellable::Completed((candidates, stats, None)));
        }
    }
    if force_reanalysis {
        let changed = ReadinessStore::new(connection)
            .requeue_source_analysis(source_id, now)
            .map_err(|error| format!("Requeue source analysis failed: {error}"))?;
        tracing::info!(
            target: "wavecrate::source_processing",
            source_id,
            changed,
            "Requeued source analysis through readiness"
        );
    }
    let manifest_identity_repair_due: bool = connection
        .query_row(
            "SELECT EXISTS(
                SELECT 1 FROM wav_files
                WHERE missing = 0
                  AND (file_identity IS NULL OR TRIM(file_identity) = '')
                  AND path NOT GLOB '._*'
                  AND path NOT GLOB '*/._*'
             )",
            [],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;
    if force_manifest_audit
        || manifest_identity_repair_due
        || (safety_probe_only && !durable_watcher_coverage_current)
        || now.saturating_sub(last_manifest_audit_at) >= MANIFEST_AUDIT_INTERVAL_SECONDS
    {
        candidates.push(RuntimeCandidate {
            schedule: WorkCandidate::source(source_id, ProcessingLane::Scan, 0, now),
            source: source.clone(),
            task: RuntimeTask::ManifestAudit {
                accelerated: force_manifest_audit,
            },
        });
    }
    progress(DiscoveryProgressUpdate::indeterminate(
        SourceDiscoveryPhase::Preparing,
    ));
    if matches!(
        retire_legacy_playback_readiness(source, connection, cancel)?,
        Cancellable::Cancelled
    ) {
        return Ok(Cancellable::Cancelled);
    }
    let mut delta_applied = false;
    if let Some(delta) = pending_readiness_delta.filter(|delta| !delta.is_empty()) {
        match publish_current_readiness_delta_with_cancel(
            connection, source_id, delta, now, cancel,
        )? {
            Cancellable::Completed(Some(_changed)) => {
                stats.delta_reconciled = true;
                delta_applied = true;
            }
            Cancellable::Completed(None) => {}
            Cancellable::Cancelled => return Ok(Cancellable::Cancelled),
        }
    }
    if !delta_applied {
        let target_publication = publish_current_readiness_targets_with_cancel_and_checkpoint(
            connection, source_id, now, cancel, true, progress,
        )?;
        if matches!(target_publication, Cancellable::Cancelled) {
            return Ok(Cancellable::Cancelled);
        }
    }
    if cancelled(cancel) {
        return Ok(Cancellable::Cancelled);
    }
    let readiness_source_exists = ReadinessStore::new(connection)
        .source_exists(source_id)
        .map_err(|error| error.to_string())?;
    let mut health = None;
    if readiness_source_exists {
        let reclassified = ReadinessStore::new(connection)
            .reclassify_known_unsupported_failures(legacy_unsupported_decode_failure_text)
            .map_err(|error| error.to_string())?;
        if reclassified > 0 {
            tracing::info!(
                target: "wavecrate::source_processing",
                source_id,
                reclassified,
                "Reclassified deterministic audio decode failures as unsupported"
            );
        }
        let reconciliation = if delta_applied {
            ReadinessStore::new(connection).reconcile_scopes_with_cancel_and_progress(
                source_id,
                &pending_readiness_delta
                    .expect("a published readiness delta has affected scopes")
                    .scope_ids,
                now,
                cancel,
                &mut |update| {
                    progress(match update {
                        ReadinessProgress::Inspecting => DiscoveryProgressUpdate::indeterminate(
                            SourceDiscoveryPhase::ComparingChangedReadiness,
                        ),
                        ReadinessProgress::ComparingTargets { completed, total } => {
                            DiscoveryProgressUpdate::determinate(
                                SourceDiscoveryPhase::ComparingChangedReadiness,
                                completed,
                                total,
                            )
                        }
                        ReadinessProgress::QueueingTargets { .. } => unreachable!(),
                    });
                },
            )
        } else {
            ReadinessStore::new(connection).reconcile_with_cancel_and_progress(
                source_id,
                now,
                cancel,
                &mut |update| {
                    progress(match update {
                        ReadinessProgress::Inspecting => DiscoveryProgressUpdate::indeterminate(
                            SourceDiscoveryPhase::ComparingReadiness,
                        ),
                        ReadinessProgress::ComparingTargets { completed, total } => {
                            DiscoveryProgressUpdate::determinate(
                                SourceDiscoveryPhase::ComparingReadiness,
                                completed,
                                total,
                            )
                        }
                        ReadinessProgress::QueueingTargets { .. } => unreachable!(),
                    });
                },
            )
        };
        let snapshot = match reconciliation {
            Ok(snapshot) => snapshot,
            Err(wavecrate::sample_sources::readiness::ReadinessError::Cancelled) => {
                return Ok(Cancellable::Cancelled);
            }
            Err(error) => return Err(error.to_string()),
        };
        let active_recordings = active_recording_deferrals(connection, now)?;
        let persistable_deficits = snapshot
            .deficits
            .iter()
            .filter(|deficit| {
                !active_recordings
                    .scope_ids
                    .contains(&deficit.target.scope_id)
            })
            .cloned()
            .collect::<Vec<_>>();
        match ReadinessStore::new(connection).persist_deficits_with_cancel_and_progress(
            &persistable_deficits,
            now,
            cancel,
            &mut |update| {
                let ReadinessProgress::QueueingTargets { completed, total } = update else {
                    unreachable!();
                };
                progress(DiscoveryProgressUpdate::determinate(
                    SourceDiscoveryPhase::QueueingWork,
                    completed,
                    total,
                ));
            },
        ) {
            Ok(_) => {}
            Err(wavecrate::sample_sources::readiness::ReadinessError::Cancelled) => {
                return Ok(Cancellable::Cancelled);
            }
            Err(error) => return Err(error.to_string()),
        }
        ReadinessStore::new(connection)
            .defer_active_recordings(&active_recordings.scope_ids)
            .map_err(|error| error.to_string())?;
        let schedulable_deficits = persistable_deficits
            .iter()
            .filter(|deficit| delta_applied || snapshot.prerequisites_are_current(&deficit.target))
            .collect::<Vec<_>>();
        stats.readiness_queue_depth = schedulable_deficits.len();
        (stats.prerequisites_blocked, stats.prerequisite_retry_at) =
            similarity_prerequisite_blocker_stats(&snapshot);
        candidates.extend(schedulable_deficits.iter().map(|deficit| RuntimeCandidate {
            schedule: WorkCandidate::readiness(&deficit.target, deficit.enqueued_at.unwrap_or(now)),
            source: source.clone(),
            task: RuntimeTask::Readiness(deficit.target.clone()),
        }));
        let work_stats = if delta_applied {
            None
        } else {
            Some(
                ReadinessStore::new(connection)
                    .work_stats(now)
                    .map_err(|error| error.to_string())?,
            )
        };
        if let Some(work_stats) = work_stats {
            stats.progress_total = work_stats.total;
            stats.progress_completed = work_stats
                .completed
                .saturating_add(work_stats.permanent_failures)
                .saturating_add(work_stats.unsupported)
                .min(stats.progress_total);
            stats.retries_due = work_stats.retries_due;
            stats.earliest_retry_at = earliest_deadline(
                earliest_deadline(
                    work_stats.earliest_retry_at,
                    work_stats.earliest_lease_expiry_at,
                ),
                active_recordings.retry_at,
            );
            tracing::debug!(
                target: "wavecrate::source_processing",
                source_id,
                pending = work_stats.pending,
                running = work_stats.running,
                retries_due = work_stats.retries_due,
                retries_waiting = work_stats.retries_waiting,
                expired_leases = work_stats.expired_leases,
                prerequisites_blocked = stats.prerequisites_blocked,
                "Readiness work reconciled"
            );
        } else {
            stats.progress_total = snapshot.entries.len();
            stats.progress_completed = snapshot
                .entries
                .iter()
                .filter(|entry| {
                    matches!(
                        entry.classification,
                        ReadinessClassification::Current
                            | ReadinessClassification::PermanentFailure { .. }
                            | ReadinessClassification::Unsupported { .. }
                    )
                })
                .count();
            for entry in &snapshot.entries {
                match entry.classification {
                    ReadinessClassification::RetryableFailure { retry_at, .. } => {
                        stats.earliest_retry_at =
                            earliest_deadline(stats.earliest_retry_at, Some(retry_at));
                        if retry_at <= now {
                            stats.retries_due = stats.retries_due.saturating_add(1);
                        }
                    }
                    ReadinessClassification::Running { lease_expires_at } => {
                        stats.earliest_retry_at =
                            earliest_deadline(stats.earliest_retry_at, Some(lease_expires_at));
                    }
                    _ => {}
                }
            }
            stats.earliest_retry_at =
                earliest_deadline(stats.earliest_retry_at, active_recordings.retry_at);
        }
        if !active_recordings.scope_ids.is_empty() {
            tracing::info!(
                target: "wavecrate::source_processing",
                event = "source_processing.active_recordings_deferred",
                source_id,
                file_count = active_recordings.scope_ids.len(),
                retry_at = active_recordings.retry_at.unwrap_or_default(),
                "Deferred files that are still being actively written"
            );
        }
        health = Some(super::source_health_summary(&snapshot, &stats));
    }

    let health = Some(health.unwrap_or_else(|| {
        SourceHealthSummary::reconciliation_failed("readiness_source_unavailable")
    }));
    if cancelled(cancel) {
        Ok(Cancellable::Cancelled)
    } else {
        Ok(Cancellable::Completed((candidates, stats, health)))
    }
}
