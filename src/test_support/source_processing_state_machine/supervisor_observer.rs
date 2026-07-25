use std::collections::BTreeSet;

use wavecrate_scan::CommittedSourceDelta;

use super::super::{
    SourceHealthPublicationOutcome, SourceProcessingSupervisor, StateMachinePublicationObservation,
};
use super::{ScanCause, StateMachineHarness};

const DUPLICATE_ADMISSION_COUNT: usize = 3;

impl StateMachineHarness {
    pub(super) fn admit_supervisor_delta(
        &mut self,
        delta: &CommittedSourceDelta,
        cause: ScanCause,
        reject_delivery_once: bool,
        reject_publication_once: bool,
    ) -> Result<(), String> {
        if delta.is_empty() || self.supervisor.is_none() {
            return Ok(());
        }
        let reason = cause.publication_reason();
        let Some(observation) = self.supervisor.as_ref().and_then(|supervisor| {
            supervisor.request_repeated_source_delta_for_state_machine(
                self.source.id.as_str(),
                delta,
                reason,
                DUPLICATE_ADMISSION_COUNT,
                reject_delivery_once,
                reject_publication_once,
            )
        }) else {
            self.pending_publication_retries
                .push((delta.clone(), cause));
            return Ok(());
        };
        let delta_scope_ids = delta_scope_ids(delta);
        if observation.delivery_rejected != reject_delivery_once {
            return Err(format!(
                "watcher delivery boundary mismatch: expected rejection={reject_delivery_once}, actual={}",
                observation.delivery_rejected
            ));
        }
        if reject_delivery_once
            && observation.scope_ids_after_rejection != observation.before_scope_ids
        {
            return Err(format!(
                "rejected watcher delivery mutated pending work: before={:?}, after={:?}",
                observation.before_scope_ids, observation.scope_ids_after_rejection
            ));
        }
        let mut expected_scopes = observation.before_scope_ids.clone();
        expected_scopes.extend(delta_scope_ids);
        if observation.pending_scope_ids != expected_scopes {
            return Err(format!(
                "actual supervisor delta queue did not coalesce exact identities: expected={expected_scopes:?}, actual={:?}",
                observation.pending_scope_ids
            ));
        }
        if observation.dirty_source_slots != 1 || observation.pending_delta_slots != 1 {
            return Err(format!(
                "actual supervisor source queue was not bounded to one coalesced slot: dirty={} delta={}",
                observation.dirty_source_slots, observation.pending_delta_slots
            ));
        }
        if !observation
            .pending_inputs
            .contains(&(delta.revision, reason))
        {
            return Err(format!(
                "actual supervisor queue lost revision/cause input {}:{reason}",
                delta.revision
            ));
        }
        self.actual_queue_admissions = self
            .actual_queue_admissions
            .saturating_add(DUPLICATE_ADMISSION_COUNT as u64);
        self.max_actual_pending_scopes = self
            .max_actual_pending_scopes
            .max(observation.pending_scope_ids.len());
        self.expected_supervisor_publications.insert((
            observation.lifecycle_generation,
            delta.revision,
            cause,
        ));
        if reject_publication_once {
            self.expected_rejected_supervisor_publications.insert((
                observation.lifecycle_generation,
                delta.revision,
                cause,
            ));
        }
        Ok(())
    }

    pub(super) fn admit_pending_publication_retries(&mut self) -> Result<(), String> {
        let pending = std::mem::take(&mut self.pending_publication_retries);
        for (delta, _original_cause) in pending {
            self.admit_supervisor_delta(&delta, ScanCause::Retry, false, false)?;
        }
        Ok(())
    }

    pub(super) fn collect_actual_publications(&mut self) -> Result<(), String> {
        let observations = self
            .supervisor
            .as_ref()
            .map(take_publications)
            .unwrap_or_default();
        self.accept_publication_batch(observations)
    }

    pub(super) fn collect_publications_from(
        &mut self,
        supervisor: &SourceProcessingSupervisor,
    ) -> Result<(), String> {
        self.accept_publication_batch(take_publications(supervisor))
    }

    fn accept_publication_batch(
        &mut self,
        observations: Vec<StateMachinePublicationObservation>,
    ) -> Result<(), String> {
        for observation in observations {
            self.accept_actual_publication(observation)?;
        }
        Ok(())
    }

    pub(super) fn retire_outstanding_publications(
        &mut self,
        lifecycle_generation: u64,
    ) -> Result<(), String> {
        self.collect_actual_publications()?;
        self.mark_outstanding_publications_stale(lifecycle_generation);
        Ok(())
    }

    pub(super) fn mark_outstanding_publications_stale(&mut self, lifecycle_generation: u64) {
        self.stale_supervisor_publications.extend(
            self.expected_supervisor_publications
                .iter()
                .filter(|(lifecycle, _, _)| *lifecycle == lifecycle_generation)
                .filter(|key| !self.observed_supervisor_publications.contains(key))
                .copied(),
        );
        self.pending_publication_retries.clear();
    }

    pub(super) fn assert_actual_publications(&mut self) -> Result<(), String> {
        self.collect_actual_publications()?;
        if self.supervisor.is_some()
            && self.actual_queue_admissions > 0
            && self.observed_supervisor_publications.is_empty()
        {
            let readiness = super::super::liveness_tests::readiness_snapshot(&self.source)
                .map(|snapshot| (snapshot.source_generation, snapshot.readiness_revision));
            return Err(format!(
                "integrated lane admitted real supervisor work without observing a durable publication: expected={:?} stale={:?} readiness={readiness:?}",
                self.expected_supervisor_publications, self.stale_supervisor_publications
            ));
        }
        let accounted = self
            .observed_supervisor_publications
            .union(&self.stale_supervisor_publications)
            .copied()
            .collect::<BTreeSet<_>>();
        if accounted != self.expected_supervisor_publications {
            return Err(format!(
                "actual supervisor publications did not account for every admitted revision/cause: expected={:?}, observed={:?}, stale={:?}",
                self.expected_supervisor_publications,
                self.observed_supervisor_publications,
                self.stale_supervisor_publications
            ));
        }
        if self.rejected_supervisor_publications != self.expected_rejected_supervisor_publications {
            return Err(format!(
                "actual supervisor publication rejections differ from injected boundary failures: expected={:?}, actual={:?}",
                self.expected_rejected_supervisor_publications,
                self.rejected_supervisor_publications
            ));
        }
        Ok(())
    }

    fn accept_actual_publication(
        &mut self,
        observation: StateMachinePublicationObservation,
    ) -> Result<(), String> {
        if observation.source_id != self.source.id.as_str() {
            return Ok(());
        }
        if observation.source_generation < 0 || observation.readiness_revision <= 0 {
            return Err(format!(
                "actual readiness publication exposed invalid generations: source={} readiness={}",
                observation.source_generation, observation.readiness_revision
            ));
        }
        if observation.outcome == SourceHealthPublicationOutcome::Rejected {
            for (revision, reason) in observation.inputs {
                let cause = ScanCause::from_publication_reason(reason)
                    .ok_or_else(|| format!("unknown rejected publication cause {reason}"))?;
                let key = (observation.lifecycle_generation, revision, cause);
                if !self
                    .expected_rejected_supervisor_publications
                    .contains(&key)
                {
                    return Err(format!(
                        "supervisor rejected an uninjected publication {key:?}"
                    ));
                }
                if !self.rejected_supervisor_publications.insert(key) {
                    return Err(format!(
                        "supervisor rejected a publication more than once: {key:?}"
                    ));
                }
            }
            return Ok(());
        }
        if !matches!(
            observation.outcome,
            SourceHealthPublicationOutcome::Published
                | SourceHealthPublicationOutcome::AlreadyPublished
        ) {
            return Err(format!(
                "state-machine observation was recorded before a definitive output: {:?}",
                observation.outcome
            ));
        }
        if let Some((last_source, last_readiness)) = self
            .last_actual_output_revisions
            .get(&observation.lifecycle_generation)
        {
            if observation.source_generation < *last_source
                || observation.readiness_revision < *last_readiness
            {
                return Err(format!(
                    "actual readiness publication regressed within lifecycle {}: source {}->{}, readiness {}->{}",
                    observation.lifecycle_generation,
                    last_source,
                    observation.source_generation,
                    last_readiness,
                    observation.readiness_revision
                ));
            }
        }
        self.last_actual_output_revisions.insert(
            observation.lifecycle_generation,
            (
                observation.source_generation,
                observation.readiness_revision,
            ),
        );
        for (revision, reason) in observation.inputs {
            let cause = ScanCause::from_publication_reason(reason)
                .ok_or_else(|| format!("unknown state-machine publication cause {reason}"))?;
            let key = (observation.lifecycle_generation, revision, cause);
            if !self.expected_supervisor_publications.contains(&key) {
                return Err(format!(
                    "supervisor processed an unadmitted revision/cause {key:?}"
                ));
            }
            if !self.observed_supervisor_publications.insert(key) {
                return Err(format!(
                    "supervisor published revision/cause more than once: {key:?}"
                ));
            }
        }
        Ok(())
    }
}

fn take_publications(
    supervisor: &SourceProcessingSupervisor,
) -> Vec<StateMachinePublicationObservation> {
    std::mem::take(
        &mut *supervisor
            .shared
            .state_machine_publications
            .lock()
            .unwrap_or_else(|poison| poison.into_inner()),
    )
}

fn delta_scope_ids(delta: &CommittedSourceDelta) -> BTreeSet<String> {
    delta
        .created
        .iter()
        .chain(&delta.changed)
        .chain(&delta.deleted)
        .map(|entry| entry.identity.clone())
        .chain(delta.moved.iter().map(|entry| entry.identity.clone()))
        .collect()
}
