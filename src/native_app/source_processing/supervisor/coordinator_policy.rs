use super::{ExecutionOutcome, ReadinessStage, RuntimeTask, Shared};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum CandidateInvalidationScope {
    None,
    TargetScope,
    Source,
}

pub(super) fn clear_satisfied_manifest_audit_request(shared: &Shared, source_id: &str) {
    let mut control = shared.control();
    // A watcher-ready boundary or another reconciliation request can arrive
    // while this audit is in flight. That newer request remains in
    // `dirty_sources` until the coordinator snapshots it, so preserve the
    // force flag for the closing audit instead of letting the older completion
    // erase it.
    if !control.dirty_sources.contains(source_id) {
        control.force_manifest_audit_sources.remove(source_id);
    }
}

pub(super) fn candidate_invalidation_scope(
    task: &RuntimeTask,
    outcome: Option<ExecutionOutcome>,
) -> CandidateInvalidationScope {
    match (task, outcome) {
        (
            RuntimeTask::ManifestAudit { .. },
            Some(
                ExecutionOutcome::Completed
                | ExecutionOutcome::CompletedAwaitingForegroundRefresh
                | ExecutionOutcome::FailedAwaitingForegroundRefresh,
            ),
        ) => CandidateInvalidationScope::Source,
        (RuntimeTask::Readiness(target), Some(ExecutionOutcome::Completed))
            if target.stage == ReadinessStage::IndexedIdentity
                && target.content_generation.starts_with("pending-") =>
        {
            CandidateInvalidationScope::TargetScope
        }
        (
            RuntimeTask::Readiness(_),
            Some(
                ExecutionOutcome::Retried { .. }
                | ExecutionOutcome::Failed
                | ExecutionOutcome::PrerequisiteInvalidated { .. }
                | ExecutionOutcome::Stale
                | ExecutionOutcome::NotClaimed,
            ),
        ) => CandidateInvalidationScope::TargetScope,
        _ => CandidateInvalidationScope::None,
    }
}
