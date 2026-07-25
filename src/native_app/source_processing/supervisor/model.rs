use super::{
    Arc, AtomicBool, BTreeSet, CommittedSourceDelta, ReadinessTarget, SampleSource, WorkCandidate,
};

#[derive(Clone)]
pub(super) struct PendingSourceRetirement {
    pub(super) source: SampleSource,
    pub(super) lifecycle_generation: u64,
    pub(super) cancel: Arc<AtomicBool>,
    pub(super) retry_at: i64,
    pub(super) attempts: u32,
    pub(super) terminal_offline: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct PendingReadinessDelta {
    pub(super) scope_ids: BTreeSet<String>,
    #[cfg(test)]
    pub(super) state_machine_inputs: BTreeSet<(u64, &'static str)>,
}

impl PendingReadinessDelta {
    pub(super) fn merge(&mut self, delta: &CommittedSourceDelta, reason: &'static str) {
        self.scope_ids.extend(
            delta
                .created
                .iter()
                .chain(&delta.changed)
                .chain(&delta.deleted)
                .map(|entry| entry.identity.clone()),
        );
        self.scope_ids
            .extend(delta.moved.iter().map(|entry| entry.identity.clone()));
        #[cfg(test)]
        self.state_machine_inputs.insert((delta.revision, reason));
        #[cfg(not(test))]
        let _ = reason;
    }

    pub(super) fn is_empty(&self) -> bool {
        self.scope_ids.is_empty()
    }
}

#[cfg(test)]
#[derive(Clone, Debug)]
pub(super) struct StateMachinePublicationObservation {
    pub(super) source_id: String,
    pub(super) lifecycle_generation: u64,
    pub(super) source_generation: i64,
    pub(super) readiness_revision: i64,
    pub(super) inputs: BTreeSet<(u64, &'static str)>,
    pub(super) outcome: super::SourceHealthPublicationOutcome,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum SourceHealthPublicationOutcome {
    Published,
    AlreadyPublished,
    Rejected,
    NoSink,
    Superseded,
}

#[derive(Clone, Debug)]
pub(super) enum RuntimeTask {
    ManifestAudit { accelerated: bool },
    Readiness(ReadinessTarget),
}

pub(super) struct RuntimeCandidate {
    pub(super) schedule: WorkCandidate,
    pub(super) source: SampleSource,
    pub(super) task: RuntimeTask,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct SourceDiscoveryStats {
    pub(super) readiness_queue_depth: usize,
    pub(super) prerequisites_blocked: usize,
    pub(super) prerequisite_retry_at: Option<i64>,
    pub(super) retries_due: usize,
    pub(super) earliest_retry_at: Option<i64>,
    pub(super) progress_completed: usize,
    pub(super) progress_total: usize,
    pub(super) cheap_noop_sweep: bool,
    pub(super) delta_reconciled: bool,
}

pub(super) enum Cancellable<T> {
    Completed(T),
    Cancelled,
}

pub(super) enum ReadinessExecutionOutcome {
    Complete(Option<std::path::PathBuf>),
    Retry(&'static str),
    Permanent(&'static str),
    Unsupported(&'static str),
    PrerequisiteInvalidated(&'static str),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ExecutionOutcome {
    Completed,
    CompletedAwaitingForegroundRefresh,
    Retried { retry_at: i64 },
    Failed,
    FailedAwaitingForegroundRefresh,
    PrerequisiteInvalidated { retry_at: i64, reason: &'static str },
    Stale,
    Cancelled,
    Parked,
    NotClaimed,
}

impl ExecutionOutcome {
    pub(super) fn was_claimed(self) -> bool {
        !matches!(self, Self::Parked | Self::NotClaimed)
    }
}

pub(super) fn should_requeue_cancelled(
    outcome: Option<ExecutionOutcome>,
    source_active: bool,
    source_dirty: bool,
) -> bool {
    matches!(outcome, Some(ExecutionOutcome::Cancelled)) && source_active && !source_dirty
}
