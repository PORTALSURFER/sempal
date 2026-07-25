use super::commands::queue_source_delta;
use super::{BTreeSet, CommittedSourceDelta, SourceProcessingSupervisor};

pub(super) struct StateMachineQueueObservation {
    pub(super) lifecycle_generation: u64,
    pub(super) before_scope_ids: BTreeSet<String>,
    pub(super) scope_ids_after_rejection: BTreeSet<String>,
    pub(super) pending_scope_ids: BTreeSet<String>,
    pub(super) pending_inputs: BTreeSet<(u64, &'static str)>,
    pub(super) dirty_source_slots: usize,
    pub(super) pending_delta_slots: usize,
    pub(super) delivery_rejected: bool,
}

impl SourceProcessingSupervisor {
    pub(super) fn request_repeated_source_delta_for_state_machine(
        &self,
        source_id: &str,
        delta: &CommittedSourceDelta,
        reason: &'static str,
        repetitions: usize,
        reject_delivery_once: bool,
        reject_publication_once: bool,
    ) -> Option<StateMachineQueueObservation> {
        let mut control = self.shared.control();
        if !control.source_is_active(source_id) {
            return None;
        }
        let before_scope_ids = control
            .pending_readiness_deltas
            .get(source_id)
            .map(|pending| pending.scope_ids.clone())
            .unwrap_or_default();
        let delivery_rejected = if reject_delivery_once {
            control.reject_next_delta_delivery = true;
            !queue_source_delta(&mut control, source_id, delta, reason)
        } else {
            false
        };
        let scope_ids_after_rejection = control
            .pending_readiness_deltas
            .get(source_id)
            .map(|pending| pending.scope_ids.clone())
            .unwrap_or_default();
        for _ in 0..repetitions {
            queue_source_delta(&mut control, source_id, delta, reason);
        }
        let lifecycle_generation = control
            .source_lifecycle_generations
            .get(source_id)
            .copied()?;
        let pending = control.pending_readiness_deltas.get(source_id)?;
        let observation = StateMachineQueueObservation {
            lifecycle_generation,
            before_scope_ids,
            scope_ids_after_rejection,
            pending_scope_ids: pending.scope_ids.clone(),
            pending_inputs: pending.state_machine_inputs.clone(),
            dirty_source_slots: usize::from(control.dirty_sources.contains(source_id)),
            pending_delta_slots: usize::from(
                control.pending_readiness_deltas.contains_key(source_id),
            ),
            delivery_rejected,
        };
        if reject_publication_once {
            self.shared
                .state_machine_reject_next_health_publication
                .store(true, std::sync::atomic::Ordering::Release);
        }
        drop(control);
        self.shared.wake.notify_one();
        Some(observation)
    }

    pub(super) fn reject_next_source_replacement_for_state_machine(&self) {
        self.shared.control().reject_next_source_replacement = true;
    }
}
