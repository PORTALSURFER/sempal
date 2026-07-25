use super::{
    ArtifactPublishOutcome, AtomicBool, DatabasePhase, DatabaseWriterGate, DatabaseWriterGuard,
    ExecutionOutcome, Ordering, PREREQUISITE_INVALIDATION_RETRY_SECONDS, READINESS_LEASE_SECONDS,
    READINESS_MAX_ATTEMPTS, ReadinessExecutionOutcome, ReadinessFailureClassification,
    ReadinessFailureOutcome, ReadinessRetryPolicy, ReadinessStore, ReadinessTarget, SampleSource,
    SourceDatabase, SourceDatabaseConnectionRole, cancel_claim,
    cleanup_unpublished_readiness_output, execution_outcome_for_failure,
    invalidate_persisted_waveform_cache_ref, now_epoch_seconds, run_readiness_stage,
    run_with_readiness_lease_heartbeat,
};

pub(super) fn execute_readiness_target(
    source: &SampleSource,
    target: &ReadinessTarget,
    cancel: &AtomicBool,
    database_writer: &DatabaseWriterGate,
) -> Result<ExecutionOutcome, String> {
    let (mut connection, claim, now) = {
        let _writer = database_writer.lock(DatabasePhase::Claim);
        let database_root = source.database_root().map_err(|error| error.to_string())?;
        let mut connection = SourceDatabase::open_connection_with_role_and_database_root(
            &source.root,
            &database_root,
            SourceDatabaseConnectionRole::JobWorker,
        )
        .map_err(|error| error.to_string())?;
        let now = now_epoch_seconds();
        let Some(claim) = ReadinessStore::new(&mut connection)
            .claim(target, now, READINESS_LEASE_SECONDS)
            .map_err(|error| error.to_string())?
        else {
            return Ok(ExecutionOutcome::NotClaimed);
        };
        (connection, claim, now)
    };
    tracing::info!(
        target: "wavecrate::source_processing",
        event = "source_processing.readiness.claimed",
        source_id = source.id.as_str(),
        stage = ?target.stage,
        scope_id = target.scope_id.as_str(),
        claim_generation = claim.claim_generation(),
        claim_origin = claim.origin().as_str(),
        lease_expires_at = claim.lease_expires_at(),
        "Readiness work claimed"
    );
    if cancel.load(Ordering::Acquire) {
        let _writer = database_writer.lock(DatabasePhase::Publish);
        return cancel_claim(&mut connection, &claim, "runtime cancellation", now);
    }
    let (outcome, lease_stale) = match run_with_readiness_lease_heartbeat(
        source,
        &claim,
        cancel,
        READINESS_LEASE_SECONDS,
        database_writer,
        |lease_cancel| {
            run_readiness_stage(
                source,
                &mut connection,
                database_writer,
                &claim,
                lease_cancel,
            )
        },
    ) {
        Ok(result) => result,
        Err(error) => {
            let _writer = database_writer.lock(DatabasePhase::Publish);
            let _ = ReadinessStore::new(&mut connection).cancel(
                &claim,
                "readiness lease heartbeat failure",
                now_epoch_seconds(),
            );
            return Err(error);
        }
    };
    if lease_stale {
        cleanup_unpublished_readiness_output(&outcome);
        return Ok(ExecutionOutcome::Stale);
    }
    if cancel.load(Ordering::Acquire) {
        cleanup_unpublished_readiness_output(&outcome);
        let _writer = database_writer.lock(DatabasePhase::Publish);
        return cancel_claim(
            &mut connection,
            &claim,
            "runtime cancellation before readiness publication",
            now_epoch_seconds(),
        );
    }
    let (_writer, cancelled_after_wait) = lock_readiness_publication(database_writer, cancel);
    if cancelled_after_wait {
        cleanup_unpublished_readiness_output(&outcome);
        return cancel_claim(
            &mut connection,
            &claim,
            "runtime cancellation while waiting for readiness publication",
            now_epoch_seconds(),
        );
    }
    match outcome {
        Ok(ReadinessExecutionOutcome::Complete(artifact_ref)) => {
            let completed = match artifact_ref.as_deref() {
                Some(artifact_ref) => ReadinessStore::new(&mut connection)
                    .complete_with_artifact_ref(
                        &claim,
                        now_epoch_seconds(),
                        &artifact_ref.to_string_lossy(),
                    ),
                None => ReadinessStore::new(&mut connection).complete(&claim, now_epoch_seconds()),
            };
            let completed = match completed {
                Ok(completed) => completed,
                Err(error) => {
                    if let Some(artifact_ref) = artifact_ref.as_deref() {
                        invalidate_persisted_waveform_cache_ref(artifact_ref);
                    }
                    return Err(error.to_string());
                }
            };
            match completed {
                ArtifactPublishOutcome::Recorded => Ok(ExecutionOutcome::Completed),
                ArtifactPublishOutcome::RejectedStale => {
                    if let Some(artifact_ref) = artifact_ref.as_deref() {
                        invalidate_persisted_waveform_cache_ref(artifact_ref);
                    }
                    Ok(ExecutionOutcome::Stale)
                }
            }
        }
        Ok(ReadinessExecutionOutcome::Retry(reason)) => {
            let policy = ReadinessRetryPolicy::new(5, 5 * 60, READINESS_MAX_ATTEMPTS)
                .expect("valid readiness retry policy");
            let outcome = ReadinessStore::new(&mut connection)
                .fail(
                    &claim,
                    ReadinessFailureClassification::Retryable,
                    "readiness_retry",
                    reason,
                    now_epoch_seconds(),
                    policy,
                )
                .map_err(|error| error.to_string())?;
            Ok(execution_outcome_for_failure(outcome))
        }
        Err(ref failure) if failure.is_cancelled() => {
            cleanup_unpublished_readiness_output(&outcome);
            cancel_claim(
                &mut connection,
                &claim,
                &failure.context,
                now_epoch_seconds(),
            )
        }
        Err(failure) => {
            let policy = ReadinessRetryPolicy::new(5, 5 * 60, READINESS_MAX_ATTEMPTS)
                .expect("valid readiness retry policy");
            let outcome = ReadinessStore::new(&mut connection)
                .fail(
                    &claim,
                    failure.readiness_failure_classification(),
                    failure.code.as_str(),
                    &failure.context,
                    now_epoch_seconds(),
                    policy,
                )
                .map_err(|error| error.to_string())?;
            tracing::warn!(
                target: "wavecrate::source_processing",
                source_id = source.id.as_str(),
                failure_code = failure.code.as_str(),
                source_error = ?failure.source_error,
                "Readiness execution failed"
            );
            Ok(execution_outcome_for_failure(outcome))
        }
        Ok(ReadinessExecutionOutcome::Permanent(reason)) => {
            let policy =
                ReadinessRetryPolicy::new(5, 5 * 60, 1).expect("valid readiness terminal policy");
            let outcome = ReadinessStore::new(&mut connection)
                .fail(
                    &claim,
                    ReadinessFailureClassification::Permanent,
                    "readiness_permanent",
                    reason,
                    now_epoch_seconds(),
                    policy,
                )
                .map_err(|error| error.to_string())?;
            Ok(execution_outcome_for_failure(outcome))
        }
        Ok(ReadinessExecutionOutcome::Unsupported(reason)) => {
            let policy =
                ReadinessRetryPolicy::new(5, 5 * 60, 1).expect("valid readiness terminal policy");
            let outcome = ReadinessStore::new(&mut connection)
                .fail(
                    &claim,
                    ReadinessFailureClassification::Unsupported,
                    "readiness_unsupported",
                    reason,
                    now_epoch_seconds(),
                    policy,
                )
                .map_err(|error| error.to_string())?;
            Ok(execution_outcome_for_failure(outcome))
        }
        Ok(ReadinessExecutionOutcome::PrerequisiteInvalidated(reason)) => {
            let policy = ReadinessRetryPolicy::new(
                PREREQUISITE_INVALIDATION_RETRY_SECONDS,
                5 * 60,
                READINESS_MAX_ATTEMPTS,
            )
            .expect("valid prerequisite invalidation retry policy");
            match ReadinessStore::new(&mut connection)
                .fail(
                    &claim,
                    ReadinessFailureClassification::Retryable,
                    "prerequisite_invalidated",
                    reason,
                    now_epoch_seconds(),
                    policy,
                )
                .map_err(|error| error.to_string())?
            {
                ReadinessFailureOutcome::RetryScheduled { retry_at } => {
                    Ok(ExecutionOutcome::PrerequisiteInvalidated { retry_at, reason })
                }
                ReadinessFailureOutcome::RejectedStale => Ok(ExecutionOutcome::Stale),
                ReadinessFailureOutcome::Permanent
                | ReadinessFailureOutcome::Unsupported
                | ReadinessFailureOutcome::AttemptsExhausted => Ok(ExecutionOutcome::Failed),
            }
        }
    }
}

fn lock_readiness_publication(
    database_writer: &DatabaseWriterGate,
    cancel: &AtomicBool,
) -> (DatabaseWriterGuard, bool) {
    let writer = database_writer.lock(DatabasePhase::Publish);
    let cancelled_after_wait = cancel.load(Ordering::Acquire);
    (writer, cancelled_after_wait)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        sync::Arc,
        thread,
        time::{Duration, Instant},
    };

    #[test]
    fn publication_lock_observes_cancellation_that_arrives_while_waiting() {
        let gate = DatabaseWriterGate::default();
        let held_writer = gate.lock(DatabasePhase::Publish);
        let cancel = Arc::new(AtomicBool::new(false));
        let worker_gate = gate.clone();
        let worker_cancel = Arc::clone(&cancel);
        let worker = thread::spawn(move || {
            let (_writer, cancelled) =
                lock_readiness_publication(&worker_gate, worker_cancel.as_ref());
            cancelled
        });

        let deadline = Instant::now() + Duration::from_secs(1);
        while gate.waiting_count() == 0 {
            assert!(
                Instant::now() < deadline,
                "publication did not wait for gate"
            );
            thread::sleep(Duration::from_millis(1));
        }
        cancel.store(true, Ordering::Release);
        drop(held_writer);

        assert!(worker.join().expect("publication waiter joins"));
    }
}
