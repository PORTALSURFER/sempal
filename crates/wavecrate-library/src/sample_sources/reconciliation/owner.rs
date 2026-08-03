//! Source-scoped ownership over the pure reconciliation admission supervisor.

use super::admission::{
    AdmissionError, AdmissionLaneKey, ReconciliationAdmissionSupervisor, ReconciliationLifecycle,
};
use super::model::{RootIdentity, WatcherGeneration};
use crate::sample_sources::SourceId;

/// A source-scoped error returned by the admission owner.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AdmissionOwnerError {
    /// The requested source has no registered admission lane.
    UnknownSource,
    /// The requested source is registered to a different physical root identity.
    SourceRootMismatch,
    /// The requested source is already registered to the supplied physical root identity.
    SourceRootUnchanged,
    /// The underlying supervisor rejected an owner operation.
    Supervisor(AdmissionError),
}

impl From<AdmissionError> for AdmissionOwnerError {
    fn from(error: AdmissionError) -> Self {
        Self::Supervisor(error)
    }
}

/// An owned snapshot of one source's registered admission lane.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OwnedAdmissionLane {
    lane: AdmissionLaneKey,
    generation: WatcherGeneration,
    lifecycle: ReconciliationLifecycle,
}

impl OwnedAdmissionLane {
    fn new(
        lane: AdmissionLaneKey,
        generation: WatcherGeneration,
        lifecycle: ReconciliationLifecycle,
    ) -> Self {
        Self {
            lane,
            generation,
            lifecycle,
        }
    }

    /// Borrow the exact source/root lane key represented by this snapshot.
    pub fn lane(&self) -> &AdmissionLaneKey {
        &self.lane
    }

    /// Borrow the source identifier represented by this snapshot.
    pub fn source_id(&self) -> &SourceId {
        self.lane.source_id()
    }

    /// Borrow the physical root identity represented by this snapshot.
    pub fn root_identity(&self) -> &RootIdentity {
        self.lane.root_identity()
    }

    /// Return the watcher generation represented by this snapshot.
    pub const fn generation(&self) -> WatcherGeneration {
        self.generation
    }

    /// Return the supervisor lifecycle represented by this snapshot.
    pub const fn lifecycle(&self) -> ReconciliationLifecycle {
        self.lifecycle
    }
}

/// Owns a caller-supplied reconciliation admission supervisor for source-scoped lifecycle work.
///
/// The owner keeps no parallel lane registry. Every source lookup and snapshot is resolved from
/// the supervisor's authoritative source index and lane queries.
pub struct ReconciliationAdmissionOwner {
    supervisor: ReconciliationAdmissionSupervisor,
}

impl ReconciliationAdmissionOwner {
    /// Wrap a reconciliation admission supervisor in source-scoped ownership.
    pub fn new(supervisor: ReconciliationAdmissionSupervisor) -> Self {
        Self { supervisor }
    }

    /// Borrow the owned supervisor without exposing mutable supervisor state.
    pub fn supervisor(&self) -> &ReconciliationAdmissionSupervisor {
        &self.supervisor
    }

    /// Consume the owner and return its admission supervisor.
    pub fn into_supervisor(self) -> ReconciliationAdmissionSupervisor {
        self.supervisor
    }

    /// Snapshot the registered lane for a source, or return `None` when it is unknown.
    pub fn lane(&self, source_id: &SourceId) -> Option<OwnedAdmissionLane> {
        let lane = self.supervisor.lane_for_source(source_id)?;
        self.snapshot(lane).ok()
    }

    /// Iterate over source identifiers registered in the owned supervisor.
    pub fn source_ids(&self) -> impl Iterator<Item = &SourceId> {
        self.supervisor.source_ids()
    }

    /// Register and begin a source, or begin/restart its existing same-root lane.
    ///
    /// A newly registered lane is left in `Starting` if the final `begin_capture` call fails;
    /// the caller can observe and retry that state through [`Self::lane`].
    pub fn begin_source(
        &mut self,
        source_id: SourceId,
        root_identity: RootIdentity,
    ) -> Result<OwnedAdmissionLane, AdmissionOwnerError> {
        let Some(existing_lane) = self.supervisor.lane_for_source(&source_id).cloned() else {
            let (lane, generation) = self
                .supervisor
                .register_lane(source_id, root_identity)
                .map_err(AdmissionOwnerError::Supervisor)?;
            self.supervisor
                .begin_capture(&lane, generation)
                .map_err(AdmissionOwnerError::Supervisor)?;
            return self.snapshot(&lane);
        };

        if existing_lane.root_identity() != &root_identity {
            return Err(AdmissionOwnerError::SourceRootMismatch);
        }

        let generation = self
            .supervisor
            .generation(&existing_lane)
            .map_err(AdmissionOwnerError::Supervisor)?;
        let generation = match self
            .supervisor
            .lifecycle(&existing_lane)
            .map_err(AdmissionOwnerError::Supervisor)?
        {
            ReconciliationLifecycle::Starting => generation,
            ReconciliationLifecycle::Stopped => self
                .supervisor
                .restart_lane(&existing_lane)
                .map_err(AdmissionOwnerError::Supervisor)?,
            ReconciliationLifecycle::Capturing => return self.snapshot(&existing_lane),
        };

        self.supervisor
            .begin_capture(&existing_lane, generation)
            .map_err(AdmissionOwnerError::Supervisor)?;
        self.snapshot(&existing_lane)
    }

    /// Stop the current lane for a source and return its stopped snapshot.
    pub fn stop_source(
        &mut self,
        source_id: &SourceId,
    ) -> Result<OwnedAdmissionLane, AdmissionOwnerError> {
        let lane = self.current_lane(source_id)?.clone();
        let generation = self
            .supervisor
            .generation(&lane)
            .map_err(AdmissionOwnerError::Supervisor)?;
        self.supervisor
            .stop_lane(&lane, generation)
            .map_err(AdmissionOwnerError::Supervisor)?;
        self.snapshot(&lane)
    }

    /// Remove a source's stopped lane without clearing retained uncertainty or counters.
    pub fn remove_source(&mut self, source_id: &SourceId) -> Result<(), AdmissionOwnerError> {
        let lane = self.current_lane(source_id)?.clone();
        self.supervisor
            .remove_stopped_lane(&lane)
            .map_err(AdmissionOwnerError::Supervisor)
    }

    /// Rebind an existing source to a different root and return its replacement `Starting` lane.
    pub fn rebind_source(
        &mut self,
        source_id: &SourceId,
        root_identity: RootIdentity,
    ) -> Result<OwnedAdmissionLane, AdmissionOwnerError> {
        let lane = self.current_lane(source_id)?.clone();
        if lane.root_identity() == &root_identity {
            return Err(AdmissionOwnerError::SourceRootUnchanged);
        }
        let generation = self
            .supervisor
            .generation(&lane)
            .map_err(AdmissionOwnerError::Supervisor)?;
        let (replacement_lane, _) = self
            .supervisor
            .rebind_lane(&lane, generation, root_identity)
            .map_err(AdmissionOwnerError::Supervisor)?;
        self.snapshot(&replacement_lane)
    }

    fn current_lane(&self, source_id: &SourceId) -> Result<&AdmissionLaneKey, AdmissionOwnerError> {
        self.supervisor
            .lane_for_source(source_id)
            .ok_or(AdmissionOwnerError::UnknownSource)
    }

    fn snapshot(&self, lane: &AdmissionLaneKey) -> Result<OwnedAdmissionLane, AdmissionOwnerError> {
        let generation = self
            .supervisor
            .generation(lane)
            .map_err(AdmissionOwnerError::Supervisor)?;
        let lifecycle = self
            .supervisor
            .lifecycle(lane)
            .map_err(AdmissionOwnerError::Supervisor)?;
        Ok(OwnedAdmissionLane::new(lane.clone(), generation, lifecycle))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sample_sources::reconciliation::{
        AdmissionOutcome, BackendStreamIdentity, CaptureBoundary, RawEventKind, RawObservation,
        RawObservationEnvelope, RawObservationLimits, RawObservationProvenance, RawObservedPath,
        RawPathRole, ReconciliationAdmissionLimits,
    };

    fn root(value: &[u8]) -> RootIdentity {
        RootIdentity::from_bytes(value.to_vec())
    }

    fn limits(
        max_lanes: usize,
        max_in_flight: usize,
        max_retained_uncertainties: usize,
    ) -> ReconciliationAdmissionLimits {
        let lane = RawObservationLimits::new(8, usize::MAX, usize::MAX).expect("lane limits");
        let global = RawObservationLimits::new(32, usize::MAX, usize::MAX).expect("global limits");
        ReconciliationAdmissionLimits::new(
            max_lanes,
            lane,
            global,
            max_in_flight,
            8,
            max_retained_uncertainties,
        )
        .expect("admission limits")
    }

    fn owner(
        max_lanes: usize,
        max_in_flight: usize,
        max_retained_uncertainties: usize,
    ) -> ReconciliationAdmissionOwner {
        ReconciliationAdmissionOwner::new(ReconciliationAdmissionSupervisor::new(limits(
            max_lanes,
            max_in_flight,
            max_retained_uncertainties,
        )))
    }

    fn envelope(
        source_id: &SourceId,
        root_identity: &RootIdentity,
        generation: WatcherGeneration,
    ) -> RawObservationEnvelope {
        RawObservationEnvelope::try_new(
            RawObservationProvenance::new(
                source_id.clone(),
                Some(root_identity.clone()),
                Some(BackendStreamIdentity::from_bytes(b"stream".to_vec())),
                generation,
                CaptureBoundary::try_new(1, None, None).expect("capture boundary"),
            ),
            vec![RawObservation::new(
                RawEventKind::Create,
                vec![RawObservedPath::new(
                    "sample.wav".into(),
                    RawPathRole::Subject,
                )],
            )],
            RawObservationLimits::new(8, usize::MAX, usize::MAX).expect("envelope limits"),
        )
        .expect("envelope")
    }

    #[test]
    fn wrapping_existing_supervisor_and_lookup_use_authoritative_lane() {
        let source = SourceId::from_string("source-a");
        let root_identity = root(b"root-a");
        let mut supervisor = ReconciliationAdmissionSupervisor::new(limits(1, 2, 8));
        let (lane, generation) = supervisor
            .register_lane(source.clone(), root_identity.clone())
            .expect("register lane");
        supervisor
            .begin_capture(&lane, generation)
            .expect("begin capture");
        let owner = ReconciliationAdmissionOwner::new(supervisor);

        let snapshot = owner.lane(&source).expect("known source lane");
        assert_eq!(snapshot.lane(), &lane);
        assert_eq!(snapshot.source_id(), &source);
        assert_eq!(snapshot.root_identity(), &root_identity);
        assert_eq!(snapshot.generation(), generation);
        assert_eq!(snapshot.lifecycle(), ReconciliationLifecycle::Capturing);
        assert_eq!(owner.supervisor().lane_for_source(&source), Some(&lane));

        let supervisor = owner.into_supervisor();
        assert_eq!(supervisor.lane_for_source(&source), Some(&lane));
    }

    #[test]
    fn unknown_source_lookup_and_lifecycle_operations_are_typed() {
        let unknown = SourceId::from_string("unknown");
        let mut owner = owner(1, 2, 8);

        assert_eq!(owner.lane(&unknown), None);
        assert_eq!(
            owner.stop_source(&unknown),
            Err(AdmissionOwnerError::UnknownSource)
        );
        assert_eq!(
            owner.remove_source(&unknown),
            Err(AdmissionOwnerError::UnknownSource)
        );
        assert_eq!(
            owner.rebind_source(&unknown, root(b"root-a")),
            Err(AdmissionOwnerError::UnknownSource)
        );
    }

    #[test]
    fn begin_source_registers_new_lanes_and_preserves_supervisor_errors() {
        let source = SourceId::from_string("source-a");
        let root_identity = root(b"root-a");
        let mut owner = owner(1, 2, 8);

        let snapshot = owner
            .begin_source(source.clone(), root_identity.clone())
            .expect("new source capture");
        assert_eq!(snapshot.lifecycle(), ReconciliationLifecycle::Capturing);
        assert_eq!(snapshot.root_identity(), &root_identity);

        let other_source = SourceId::from_string("source-b");
        assert_eq!(
            owner.begin_source(other_source.clone(), root(b"root-b")),
            Err(AdmissionOwnerError::Supervisor(
                AdmissionError::LaneLimitReached
            ))
        );
    }

    #[test]
    fn begin_source_is_idempotent_for_capturing_same_root() {
        let source = SourceId::from_string("source-a");
        let root_identity = root(b"root-a");
        let mut owner = owner(1, 2, 8);

        let first = owner
            .begin_source(source.clone(), root_identity.clone())
            .expect("begin source");
        let second = owner
            .begin_source(source.clone(), root_identity)
            .expect("repeat begin source");

        assert_eq!(second, first);
        assert_eq!(owner.supervisor().in_flight(), 0);
    }

    #[test]
    fn stop_and_restart_use_newer_generation_and_repeated_stop_is_wrapped() {
        let source = SourceId::from_string("source-a");
        let mut owner = owner(1, 2, 8);
        let capturing = owner
            .begin_source(source.clone(), root(b"root-a"))
            .expect("begin source");
        let stopped = owner.stop_source(&source).expect("stop source");
        assert_eq!(stopped.lifecycle(), ReconciliationLifecycle::Stopped);
        assert_eq!(stopped.generation(), capturing.generation());
        assert_eq!(
            owner.stop_source(&source),
            Err(AdmissionOwnerError::Supervisor(
                AdmissionError::InvalidLifecycleTransition
            ))
        );

        let restarted = owner
            .begin_source(source.clone(), root(b"root-a"))
            .expect("restart source");
        assert_eq!(restarted.lifecycle(), ReconciliationLifecycle::Capturing);
        assert!(restarted.generation() > stopped.generation());
        assert_eq!(restarted.lane(), stopped.lane());
    }

    #[test]
    fn rebind_requires_a_changed_root_and_needs_an_explicit_begin() {
        let source = SourceId::from_string("source-a");
        let old_root = root(b"root-a");
        let new_root = root(b"root-b");
        let mut owner = owner(1, 2, 8);
        let original = owner
            .begin_source(source.clone(), old_root.clone())
            .expect("begin source");

        assert_eq!(
            owner.rebind_source(&source, old_root.clone()),
            Err(AdmissionOwnerError::SourceRootUnchanged)
        );
        let replacement = owner
            .rebind_source(&source, new_root.clone())
            .expect("rebind source");
        assert_eq!(replacement.lifecycle(), ReconciliationLifecycle::Starting);
        assert_eq!(replacement.root_identity(), &new_root);
        assert!(replacement.generation() > original.generation());
        assert_ne!(replacement.lane(), original.lane());

        let capturing = owner
            .begin_source(source.clone(), new_root)
            .expect("explicitly begin rebound source");
        assert_eq!(capturing.lifecycle(), ReconciliationLifecycle::Capturing);
        assert_eq!(capturing.lane(), replacement.lane());
        assert_eq!(
            owner.begin_source(source.clone(), old_root),
            Err(AdmissionOwnerError::SourceRootMismatch)
        );
    }

    #[test]
    fn remove_requires_explicit_stop_reclaims_capacity_without_clearing_state() {
        let source = SourceId::from_string("source-a");
        let replacement_source = SourceId::from_string("source-b");
        let root_identity = root(b"root-a");
        let replacement_root = root(b"root-b");
        let mut supervisor = ReconciliationAdmissionSupervisor::new(limits(1, 2, 8));
        let (lane, generation) = supervisor
            .register_lane(source.clone(), root_identity.clone())
            .expect("register source");
        supervisor
            .begin_capture(&lane, generation)
            .expect("begin source");
        let ticket = match supervisor.admit(envelope(&source, &root_identity, generation)) {
            AdmissionOutcome::Accepted(ticket) => ticket,
            outcome => panic!("unexpected admission outcome: {outcome:?}"),
        };
        let mut owner = ReconciliationAdmissionOwner::new(supervisor);

        assert_eq!(
            owner.remove_source(&source),
            Err(AdmissionOwnerError::Supervisor(
                AdmissionError::InvalidLifecycleTransition
            ))
        );
        let stopped = owner.stop_source(&source).expect("stop source");
        let uncertainties = owner.supervisor().retained_uncertainties().to_vec();
        assert_eq!(owner.supervisor().in_flight(), 0);
        owner.remove_source(&source).expect("remove stopped source");
        assert_eq!(
            owner.supervisor().retained_uncertainties(),
            uncertainties.as_slice()
        );
        assert_eq!(owner.lane(&source), None);

        let mut supervisor = owner.into_supervisor();
        let (replacement_lane, replacement_generation) = supervisor
            .register_lane(replacement_source.clone(), replacement_root.clone())
            .expect("reclaim lane capacity");
        supervisor
            .begin_capture(&replacement_lane, replacement_generation)
            .expect("begin replacement source");
        let replacement_ticket = match supervisor.admit(envelope(
            &replacement_source,
            &replacement_root,
            replacement_generation,
        )) {
            AdmissionOutcome::Accepted(ticket) => ticket,
            outcome => panic!("unexpected replacement outcome: {outcome:?}"),
        };
        assert!(replacement_ticket.id() > ticket.id());
        assert_eq!(stopped.lifecycle(), ReconciliationLifecycle::Stopped);
    }

    #[test]
    fn stale_generation_and_retired_key_remain_fenced() {
        let source = SourceId::from_string("source-a");
        let old_root = root(b"root-a");
        let new_root = root(b"root-b");
        let mut owner = owner(1, 2, 16);
        let initial = owner
            .begin_source(source.clone(), old_root.clone())
            .expect("begin source");
        let stopped = owner.stop_source(&source).expect("stop source");
        let restarted = owner
            .begin_source(source.clone(), old_root)
            .expect("restart source");
        assert!(restarted.generation() > stopped.generation());

        let mut supervisor = owner.into_supervisor();
        assert_eq!(
            supervisor.stop_lane(stopped.lane(), stopped.generation()),
            Err(AdmissionError::GenerationMismatch)
        );
        let mut owner = ReconciliationAdmissionOwner::new(supervisor);
        let replacement = owner
            .rebind_source(&source, new_root)
            .expect("rebind source");
        let supervisor = owner.into_supervisor();
        assert_eq!(
            supervisor.lifecycle(initial.lane()),
            Err(AdmissionError::UnknownLane)
        );
        assert_eq!(
            supervisor.generation(replacement.lane()),
            Ok(replacement.generation())
        );
    }

    #[test]
    fn sources_are_independent() {
        let source_a = SourceId::from_string("source-a");
        let source_b = SourceId::from_string("source-b");
        let mut owner = owner(2, 3, 16);
        let lane_a = owner
            .begin_source(source_a.clone(), root(b"root-a"))
            .expect("begin source a");
        let lane_b = owner
            .begin_source(source_b.clone(), root(b"root-b"))
            .expect("begin source b");

        owner.stop_source(&source_a).expect("stop source a");
        assert_eq!(
            owner.lane(&source_b).expect("source b remains").lifecycle(),
            ReconciliationLifecycle::Capturing
        );
        assert_eq!(
            owner.lane(&source_b).expect("source b remains").lane(),
            lane_b.lane()
        );
        assert_eq!(lane_a.source_id(), &source_a);
    }

    #[test]
    fn supervisor_capacity_errors_are_wrapped_without_owner_state_rollback() {
        let source = SourceId::from_string("source-a");
        let mut owner = owner(1, 2, 1);
        owner
            .begin_source(source.clone(), root(b"root-a"))
            .expect("begin source");
        owner
            .stop_source(&source)
            .expect("first stop retains marker");
        assert_eq!(
            owner.rebind_source(&source, root(b"root-b")),
            Err(AdmissionOwnerError::Supervisor(
                AdmissionError::UncertaintyCapacityExhausted
            ))
        );
        assert_eq!(
            owner
                .lane(&source)
                .expect("lane remains after failed rebind")
                .lifecycle(),
            ReconciliationLifecycle::Stopped
        );
    }
}
