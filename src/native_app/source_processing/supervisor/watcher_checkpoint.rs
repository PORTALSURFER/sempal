use super::{
    Arc, DatabasePhase, SampleSource, Shared, commands::request_source_manifest_audit,
    source_descriptors_match,
};
use crate::native_app::sample_library::source_watcher::{
    CheckpointAdvanceOutcome, RevisionBoundCheckpoint, write_revision_bound_checkpoint,
};
use wavecrate_library::filesystem_identity::stable_filesystem_identity;

/// Drain and execute checkpoint requests on the source-processing coordinator thread.
pub(super) fn process_pending_watcher_checkpoints(shared: &Arc<Shared>) {
    let requests = {
        let mut control = shared.control();
        std::mem::take(&mut control.pending_watcher_checkpoints)
    };
    for request in requests {
        process_watcher_checkpoint(shared, request);
    }
}

fn process_watcher_checkpoint(shared: &Arc<Shared>, request: RevisionBoundCheckpoint) {
    let Some(source) = configured_source_for_request(shared, &request) else {
        tracing::debug!(
            source_id = request.source_id.as_str(),
            lifecycle_generation = request.lifecycle_generation,
            "Dropping stale watcher checkpoint request before source DB open"
        );
        return;
    };
    let outcome = {
        let _writer = shared.database_writer.lock(DatabasePhase::Publish);
        let Some(current_source) = configured_source_for_request(shared, &request) else {
            tracing::debug!(
                source_id = request.source_id.as_str(),
                lifecycle_generation = request.lifecycle_generation,
                "Dropping watcher checkpoint after source lifecycle changed"
            );
            return;
        };
        if !source_descriptors_match(&source, &current_source) {
            tracing::debug!(
                source_id = request.source_id.as_str(),
                lifecycle_generation = request.lifecycle_generation,
                "Dropping watcher checkpoint after source descriptor replacement"
            );
            return;
        }
        let Some(root_identity) = live_root_identity(&current_source) else {
            tracing::debug!(
                source_id = current_source.id.as_str(),
                outcome = ?CheckpointAdvanceOutcome::Retryable,
                "Could not capture live source root identity for watcher checkpoint"
            );
            request_source_manifest_audit(
                shared.as_ref(),
                request.source_id.as_str(),
                "watcher_checkpoint_root_identity_unavailable",
            );
            return;
        };
        if root_identity != request.root_identity {
            tracing::debug!(
                source_id = current_source.id.as_str(),
                requested_root_identity = request.root_identity.as_str(),
                live_root_identity = root_identity.as_str(),
                outcome = ?CheckpointAdvanceOutcome::AuditRequired,
                "Dropping watcher checkpoint with a stale source root identity"
            );
            request_source_manifest_audit(
                shared.as_ref(),
                request.source_id.as_str(),
                "watcher_checkpoint_root_identity_changed",
            );
            return;
        }
        write_revision_bound_checkpoint(
            &current_source,
            &request,
            request.lifecycle_generation,
            &root_identity,
        )
    };
    match outcome {
        CheckpointAdvanceOutcome::Applied => tracing::debug!(
            source_id = request.source_id.as_str(),
            event_id = request.event_id,
            "Committed revision-bound watcher checkpoint"
        ),
        CheckpointAdvanceOutcome::AlreadyApplied | CheckpointAdvanceOutcome::Superseded => {
            tracing::debug!(
                source_id = request.source_id.as_str(),
                event_id = request.event_id,
                ?outcome,
                "Watcher checkpoint did not advance"
            )
        }
        CheckpointAdvanceOutcome::AuditRequired => {
            tracing::debug!(
                source_id = request.source_id.as_str(),
                event_id = request.event_id,
                ?outcome,
                "Watcher checkpoint requires a source manifest audit"
            );
            request_source_manifest_audit(
                shared.as_ref(),
                request.source_id.as_str(),
                "watcher_checkpoint_audit_required",
            );
        }
        CheckpointAdvanceOutcome::Retryable => {
            tracing::warn!(
                source_id = request.source_id.as_str(),
                event_id = request.event_id,
                ?outcome,
                "Watcher checkpoint publication is retryable"
            );
            request_source_manifest_audit(
                shared.as_ref(),
                request.source_id.as_str(),
                "watcher_checkpoint_retryable",
            );
        }
    }
}

fn configured_source_for_request(
    shared: &Shared,
    request: &RevisionBoundCheckpoint,
) -> Option<SampleSource> {
    let control = shared.control();
    let source = control.sources.get(&request.source_id)?;
    if !control.source_is_active(&request.source_id)
        || control.source_lifecycle_generations.get(&request.source_id)
            != Some(&request.lifecycle_generation)
    {
        return None;
    }
    Some(source.clone())
}

fn live_root_identity(source: &SampleSource) -> Option<String> {
    std::fs::metadata(&source.root)
        .ok()
        .and_then(|metadata| stable_filesystem_identity(&source.root, &metadata))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::native_app::sample_library::source_watcher::CheckpointCause;
    use crate::native_app::source_processing::SourceProcessingSupervisor;
    use std::{
        sync::atomic::Ordering,
        thread,
        time::{Duration, Instant},
    };
    use wavecrate::sample_sources::SourceId;
    use wavecrate_library::sample_sources::{SourceDatabase, db::META_SOURCE_WATCHER_CHECKPOINT};

    fn source(root: &std::path::Path, id: &str) -> SampleSource {
        SampleSource::new_with_id(SourceId::from_string(id), root.to_path_buf())
    }

    fn root_identity(source: &SampleSource) -> String {
        live_root_identity(source).expect("source root identity")
    }

    fn request(
        source: &SampleSource,
        lifecycle_generation: u64,
        root_identity: String,
    ) -> RevisionBoundCheckpoint {
        RevisionBoundCheckpoint {
            source_id: source.id.as_str().to_string(),
            lifecycle_generation,
            source_revision: 0,
            root_identity,
            event_id: 8,
            cause: CheckpointCause::TargetedReplay,
        }
    }

    fn seed_checkpoint(source: &SampleSource, lifecycle_generation: u64, root_identity: &str) {
        seed_checkpoint_value(
            source,
            &serde_json::json!({
                "root_identity": root_identity,
                "event_id": 7,
                "format_version": 2,
                "source_id": source.id.as_str(),
                "lifecycle_generation": lifecycle_generation,
                "source_revision": 0,
                "cause": "targeted_replay"
            })
            .to_string(),
        );
    }

    fn seed_checkpoint_value(source: &SampleSource, value: &str) {
        let database = SourceDatabase::open_for_test_fixture_source_write(&source.root)
            .expect("source database");
        let mut batch = database.write_batch().expect("seed transaction");
        batch
            .set_metadata(META_SOURCE_WATCHER_CHECKPOINT, value)
            .expect("seed watcher checkpoint");
        batch
            .commit_auxiliary_state()
            .expect("seed watcher checkpoint");
    }

    fn checkpoint_bytes(source: &SampleSource) -> Option<String> {
        SourceDatabase::open_for_test_fixture_source_write(&source.root)
            .expect("source database")
            .get_metadata(META_SOURCE_WATCHER_CHECKPOINT)
            .expect("read watcher checkpoint")
    }

    #[test]
    fn submission_only_enqueues_and_wakes_without_database_gate_or_open() {
        let supervisor = SourceProcessingSupervisor::dormant();
        let handle = supervisor.budget_handle();
        let before = handle.shared.database_writer.snapshot();
        let request = RevisionBoundCheckpoint {
            source_id: "source-a".to_string(),
            lifecycle_generation: 1,
            source_revision: 0,
            root_identity: "root-a".to_string(),
            event_id: 1,
            cause: CheckpointCause::TargetedReplay,
        };

        handle.submit_watcher_checkpoint(request);

        assert_eq!(
            handle.shared.database_writer.snapshot().publish.count,
            before.publish.count
        );
        assert_eq!(handle.shared.control().pending_watcher_checkpoints.len(), 1);
        assert!(handle.shared.control().wake_generation > 1);
    }

    #[test]
    fn queue_commits_valid_checkpoint_and_preserves_root_mismatch_bytes() {
        let directory = tempfile::tempdir().expect("source directory");
        let source = source(directory.path(), "source-a");
        let shared = Arc::new(Shared::new(vec![source.clone()], None));
        let generation = shared.control().source_lifecycle_generations["source-a"];
        let root_identity = root_identity(&source);
        seed_checkpoint(&source, generation, &root_identity);
        let handle = super::super::SourceProcessingBudgetHandle {
            shared: Arc::clone(&shared),
        };
        handle.submit_watcher_checkpoint(request(&source, generation, root_identity.clone()));
        process_pending_watcher_checkpoints(&shared);
        let committed = checkpoint_bytes(&source).expect("committed checkpoint");
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&committed).expect("checkpoint JSON")["event_id"],
            8
        );

        handle.submit_watcher_checkpoint(request(
            &source,
            generation,
            "replacement-root".to_string(),
        ));
        process_pending_watcher_checkpoints(&shared);
        assert_eq!(
            checkpoint_bytes(&source).expect("checkpoint bytes"),
            committed
        );
    }

    #[test]
    fn audit_required_checkpoint_requests_source_scoped_manifest_audit_and_preserves_bytes() {
        let directory = tempfile::tempdir().expect("source directory");
        let source = source(directory.path(), "source-a");
        let shared = Arc::new(Shared::new(vec![source.clone()], None));
        let generation = shared.control().source_lifecycle_generations["source-a"];
        let root_identity = root_identity(&source);
        let unknown_bytes = format!(
            r#"{{"root_identity":"{root_identity}","event_id":7,"format_version":2,"source_id":"source-a","lifecycle_generation":{generation},"source_revision":0,"cause":"targeted_replay","unexpected":"evidence"}}"#
        );
        seed_checkpoint_value(&source, &unknown_bytes);
        {
            let mut control = shared.control();
            control.dirty_sources.clear();
            control.safety_probe_sources.clear();
            control.force_manifest_audit_sources.clear();
        }
        let handle = super::super::SourceProcessingBudgetHandle {
            shared: Arc::clone(&shared),
        };
        handle.submit_watcher_checkpoint(request(&source, generation, root_identity));

        process_pending_watcher_checkpoints(&shared);

        let control = shared.control();
        assert!(
            control
                .force_manifest_audit_sources
                .contains(source.id.as_str())
        );
        assert!(control.dirty_sources.contains(source.id.as_str()));
        drop(control);
        assert_eq!(
            checkpoint_bytes(&source).as_deref(),
            Some(unknown_bytes.as_str())
        );
    }

    #[test]
    fn replaced_source_is_fenced_before_writable_open() {
        let old_directory = tempfile::tempdir().expect("old source directory");
        let new_directory = tempfile::tempdir().expect("new source directory");
        let old_source = source(old_directory.path(), "source-a");
        let replacement = source(new_directory.path(), "source-a");
        let shared = Arc::new(Shared::new(vec![old_source.clone()], None));
        let old_generation = shared.control().source_lifecycle_generations["source-a"];
        let root_identity = root_identity(&old_source);
        seed_checkpoint(&old_source, old_generation, &root_identity);
        let old_bytes = checkpoint_bytes(&old_source).expect("old checkpoint bytes");
        let handle = super::super::SourceProcessingBudgetHandle {
            shared: Arc::clone(&shared),
        };
        handle.submit_watcher_checkpoint(request(&old_source, old_generation, root_identity));
        let supervisor = SourceProcessingSupervisor {
            shared: Arc::clone(&shared),
            coordinator: None,
            retirement_worker: None,
        };
        supervisor
            .replace_sources(vec![replacement])
            .expect("replace source");
        process_pending_watcher_checkpoints(&shared);
        assert_eq!(
            checkpoint_bytes(&old_source).expect("old checkpoint bytes"),
            old_bytes
        );
    }

    #[test]
    fn queued_checkpoint_waits_for_publish_gate_then_serializes() {
        let directory = tempfile::tempdir().expect("source directory");
        let source = source(directory.path(), "source-a");
        let shared = Arc::new(Shared::new(vec![source.clone()], None));
        let generation = shared.control().source_lifecycle_generations["source-a"];
        let root_identity = root_identity(&source);
        seed_checkpoint(&source, generation, &root_identity);
        let handle = super::super::SourceProcessingBudgetHandle {
            shared: Arc::clone(&shared),
        };
        handle.submit_watcher_checkpoint(request(&source, generation, root_identity));
        let held = shared.database_writer.lock(DatabasePhase::Publish);
        let worker_shared = Arc::clone(&shared);
        let worker = thread::spawn(move || process_pending_watcher_checkpoints(&worker_shared));
        let deadline = Instant::now() + Duration::from_secs(2);
        while shared.database_writer.waiting_count() == 0 {
            assert!(
                Instant::now() < deadline,
                "checkpoint did not wait on publish gate"
            );
            thread::sleep(Duration::from_millis(5));
        }
        drop(held);
        worker.join().expect("checkpoint worker");
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(
                &checkpoint_bytes(&source).expect("checkpoint bytes")
            )
            .expect("checkpoint JSON")["event_id"],
            8
        );
        assert_eq!(shared.database_writer.snapshot().publish.count, 2);
        assert!(!shared.cancel.load(Ordering::Acquire));
    }
}
