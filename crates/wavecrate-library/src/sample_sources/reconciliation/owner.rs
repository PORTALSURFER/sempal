//! Source-scoped ownership over the pure reconciliation admission supervisor.

use super::adapter::{
    AdapterError, LiveAuditAdmission, ReconciliationAdapter, SyntheticObservationBatch,
};
use super::admission::{
    AdmissionError, AdmissionLaneKey, DispatchTicket, DispatchedObservation,
    ReconciliationAcknowledgementOutcome, ReconciliationAdmissionSupervisor,
    ReconciliationLifecycle, SourceAuditReceipt, SourceAuditRequest,
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

    /// Build the bounded source-audit request for the currently registered lane.
    ///
    /// The supervisor derives the request from the lane's current source/root/generation and
    /// issued uncertainty watermark without allocating another marker or admission ticket.
    pub fn source_audit_request_for_current_lane(
        &self,
        source_id: &SourceId,
    ) -> Option<SourceAuditRequest> {
        self.supervisor
            .source_audit_request_for_current_lane(source_id)
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

    /// Admit one live batch through the owner-held adapter without replacing the supervisor.
    pub fn admit_live_with_correlation(
        &mut self,
        batch: SyntheticObservationBatch,
    ) -> Result<LiveAuditAdmission, AdapterError> {
        ReconciliationAdapter::new(&mut self.supervisor).admit_live_with_correlation(batch)
    }

    /// Select the next admitted envelope using the supervisor's fair lane scheduler.
    pub fn dispatch_next(&mut self) -> Option<DispatchedObservation> {
        self.supervisor.dispatch_next()
    }

    /// Advance one dispatched envelope to the downstream handoff phase.
    pub fn mark_dispatched(&mut self, ticket: DispatchTicket) -> Result<(), AdmissionError> {
        self.supervisor.mark_dispatched(ticket)
    }

    /// Advance one handed-off envelope to the applied phase.
    pub fn mark_applied(&mut self, ticket: DispatchTicket) -> Result<(), AdmissionError> {
        self.supervisor.mark_applied(ticket)
    }

    /// Retire one proofless live envelope after its conservative audit handoff.
    pub fn mark_unproven_audit_handed_off(
        &mut self,
        ticket: DispatchTicket,
    ) -> Result<(), AdmissionError> {
        self.supervisor.mark_unproven_audit_handed_off(ticket)
    }

    /// Apply a complete source-audit receipt only while its lane identity is still active.
    ///
    /// This is the only native-owner entry point for the existing typed acknowledgement model.
    /// Stopped lanes and replaced roots/generations reject old receipts without clearing anything.
    pub fn acknowledge_source_audit_receipt(
        &mut self,
        receipt: &SourceAuditReceipt,
    ) -> ReconciliationAcknowledgementOutcome {
        let Some(acknowledgement) = receipt.authoritative_acknowledgement() else {
            return ReconciliationAcknowledgementOutcome::unchanged(
                self.supervisor.retained_uncertainties().len(),
            );
        };
        let Some(lane) = self
            .supervisor
            .lane_for_source(acknowledgement.identity().source_id())
            .cloned()
        else {
            return ReconciliationAcknowledgementOutcome::unchanged(
                self.supervisor.retained_uncertainties().len(),
            );
        };
        let lane_generation = self.supervisor.generation(&lane).ok();
        let lane_lifecycle = self.supervisor.lifecycle(&lane).ok();
        if lane.root_identity() != acknowledgement.identity().root_identity()
            || lane_generation != Some(acknowledgement.identity().generation())
            || lane_lifecycle != Some(ReconciliationLifecycle::Capturing)
        {
            return ReconciliationAcknowledgementOutcome::unchanged(
                self.supervisor.retained_uncertainties().len(),
            );
        }
        self.supervisor
            .acknowledge_committed_authoritative_reconciliation(&acknowledgement)
    }

    /// Return the bounded number of live envelopes the owner can retain.
    pub fn max_in_flight(&self) -> usize {
        self.supervisor.limits().max_in_flight()
    }

    /// Return the bounded number of exact source-audit request entries the watcher may retain.
    ///
    /// The request transport mirrors both ordinary and emergency retained-uncertainty capacity;
    /// it is not an independent source-count or context limit.
    pub fn max_source_audit_request_entries(&self) -> usize {
        let limits = self.supervisor.limits();
        limits
            .max_retained_uncertainties()
            .saturating_add(limits.max_emergency_uncertainties())
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
        AdmissionOutcome, AdmissionRejectReason, BackendStreamIdentity, CaptureBoundary,
        RawEventKind, RawObservation, RawObservationEnvelope, RawObservationLimits,
        RawObservationProvenance, RawObservedPath, RawPathRole,
        ReconciliationAcknowledgementIdentity, ReconciliationAdmissionLimits,
        RetainedUncertaintyBoundary, SourceAuditCommit, SourceAuditRequest,
        SyntheticObservationBatch, UncertaintyReason,
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

    fn live_batch(
        source: &SourceId,
        root_identity: &RootIdentity,
        generation: WatcherGeneration,
        captured_at: u64,
    ) -> SyntheticObservationBatch {
        live_batch_with_kind(
            source,
            root_identity,
            generation,
            captured_at,
            RawEventKind::Create,
        )
    }

    fn committed_audit(
        request: &SourceAuditRequest,
        revision: u64,
        root_identity: RootIdentity,
    ) -> SourceAuditCommit {
        SourceAuditCommit::new(revision, root_identity, Some(request.clone()))
    }

    fn live_batch_with_kind(
        source: &SourceId,
        root_identity: &RootIdentity,
        generation: WatcherGeneration,
        captured_at: u64,
        kind: RawEventKind,
    ) -> SyntheticObservationBatch {
        SyntheticObservationBatch::new(
            RawObservationProvenance::new(
                source.clone(),
                Some(root_identity.clone()),
                Some(BackendStreamIdentity::from_bytes(b"stream".to_vec())),
                generation,
                CaptureBoundary::try_new(captured_at, None, None).expect("capture boundary"),
            ),
            vec![RawObservation::new(
                kind,
                vec![RawObservedPath::new(
                    "sample.wav".into(),
                    RawPathRole::Subject,
                )],
            )],
            RawObservationLimits::new(8, usize::MAX, usize::MAX).expect("batch limits"),
        )
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
    fn live_dispatch_seam_preserves_correlation_and_releases_only_proofless_work() {
        let source = SourceId::from_string("live-seam");
        let root_identity = root(b"live-root");
        let mut owner = owner(1, 1, 8);
        let lane = owner
            .begin_source(source.clone(), root_identity.clone())
            .expect("capturing lane");
        let boundary = CaptureBoundary::try_new(4, None, None).expect("capture boundary");
        let batch = SyntheticObservationBatch::new(
            RawObservationProvenance::new(
                source.clone(),
                Some(root_identity.clone()),
                Some(BackendStreamIdentity::from_bytes(b"live-stream".to_vec())),
                lane.generation(),
                boundary,
            ),
            vec![RawObservation::new(
                RawEventKind::Create,
                vec![RawObservedPath::new(
                    "sample.wav".into(),
                    RawPathRole::Subject,
                )],
            )],
            RawObservationLimits::new(8, usize::MAX, usize::MAX).expect("batch limits"),
        );

        let live = owner
            .admit_live_with_correlation(batch)
            .expect("live admission");
        let ticket = match live.admission().outcome() {
            AdmissionOutcome::Accepted(ticket) => *ticket,
            outcome => panic!("expected accepted live admission, got {outcome:?}"),
        };
        assert_eq!(
            live.correlation().map(|correlation| correlation.ticket()),
            Some(ticket)
        );
        let dispatched = owner.dispatch_next().expect("fair dispatch");
        assert_eq!(dispatched.ticket(), ticket);
        assert!(dispatched.normalized().proof().is_unproven());
        owner.mark_dispatched(ticket).expect("mark dispatched");
        owner.mark_applied(ticket).expect("mark applied");
        owner
            .mark_unproven_audit_handed_off(ticket)
            .expect("retire proofless handoff");

        assert_eq!(owner.supervisor().in_flight(), 0);
        let marker = owner
            .supervisor()
            .retained_uncertainties()
            .iter()
            .find(|marker| marker.source_id() == Some(&source))
            .expect("retained live uncertainty");
        assert_eq!(marker.root_identity(), Some(&root_identity));
        assert_eq!(marker.generation(), Some(lane.generation()));
        assert_eq!(marker.capture_boundary(), Some(boundary));
        assert!(marker.reasons().contains(&UncertaintyReason::LiveUnproven));
    }

    #[test]
    fn full_retention_uses_bounded_emergency_marker_and_monotonic_audit_correlation() {
        let first_source = SourceId::from_string("emergency-first");
        let first_root = root(b"emergency-first-root");
        let second_source = SourceId::from_string("emergency-second");
        let second_root = root(b"emergency-second-root");
        let mut owner = owner(2, 2, 1);
        let first_lane = owner
            .begin_source(first_source.clone(), first_root.clone())
            .expect("first capturing lane");
        let second_lane = owner
            .begin_source(second_source.clone(), second_root.clone())
            .expect("second capturing lane");

        let first = owner
            .admit_live_with_correlation(live_batch(
                &first_source,
                &first_root,
                first_lane.generation(),
                1,
            ))
            .expect("ordinary live admission");
        let first_ticket = match first.admission().outcome() {
            AdmissionOutcome::Accepted(ticket) => *ticket,
            outcome => panic!("expected first accepted admission, got {outcome:?}"),
        };
        owner.dispatch_next().expect("dispatch first admission");
        owner
            .mark_dispatched(first_ticket)
            .expect("mark first dispatched");
        owner
            .mark_applied(first_ticket)
            .expect("mark first applied");
        owner
            .mark_unproven_audit_handed_off(first_ticket)
            .expect("handoff first admission");

        let emergency = owner
            .admit_live_with_correlation(live_batch_with_kind(
                &second_source,
                &second_root,
                second_lane.generation(),
                2,
                RawEventKind::Overflow,
            ))
            .expect("overflow admission");
        assert!(matches!(
            emergency.admission().outcome(),
            AdmissionOutcome::Rejected(AdmissionRejectReason::UncertaintyMarkerRetained)
        ));
        assert!(emergency.correlation().is_none());
        let emergency_request = emergency
            .audit_request()
            .cloned()
            .expect("emergency source audit request");
        let emergency_marker = owner
            .supervisor()
            .retained_uncertainties()
            .iter()
            .find(|marker| marker.source_id() == Some(&second_source))
            .expect("emergency marker");
        assert!(emergency_marker.is_emergency());
        assert_eq!(emergency_marker.root_identity(), Some(&second_root));
        assert_eq!(
            emergency_marker.generation(),
            Some(second_lane.generation())
        );
        assert_eq!(
            emergency_marker.backend_stream_identity(),
            Some(&BackendStreamIdentity::from_bytes(b"stream".to_vec()))
        );
        assert_eq!(emergency_request.identity().source_id(), &second_source);
        assert_eq!(emergency_request.root_identity(), &second_root);
        assert_eq!(emergency_request.generation(), second_lane.generation());
        assert_eq!(emergency_request.boundary(), emergency_marker.boundary());
        assert_eq!(emergency_request.boundary().first(), 2);
        assert_eq!(emergency_request.boundary().through(), 2);

        let later = owner
            .admit_live_with_correlation(live_batch(
                &second_source,
                &second_root,
                second_lane.generation(),
                3,
            ))
            .expect("later capture admission");
        let later_ticket = match later.admission().outcome() {
            AdmissionOutcome::Accepted(ticket) => *ticket,
            outcome => panic!("later capture must remain deferred, got {outcome:?}"),
        };
        let later_request = later
            .audit_request()
            .cloned()
            .expect("later capture audit request");
        assert!(later.correlation().is_some());
        assert_eq!(later_request.boundary().first(), 2);
        assert_eq!(later_request.boundary().through(), 3);
        owner.dispatch_next().expect("dispatch later capture");
        owner
            .mark_dispatched(later_ticket)
            .expect("mark later dispatched");
        owner
            .mark_applied(later_ticket)
            .expect("mark later applied");
        owner
            .mark_unproven_audit_handed_off(later_ticket)
            .expect("handoff later capture");

        let premature = emergency_request
            .complete_from_committed_audit(committed_audit(
                &emergency_request,
                7,
                second_root.clone(),
            ))
            .expect("matching root commit token");
        assert_eq!(
            owner.acknowledge_source_audit_receipt(&premature),
            ReconciliationAcknowledgementOutcome::unchanged(2),
            "the earlier boundary cannot clear later retained evidence"
        );
        let committed = later_request
            .complete_from_committed_audit(committed_audit(&later_request, 8, second_root))
            .expect("matching later root commit token");
        let acknowledgement = owner.acknowledge_source_audit_receipt(&committed);
        assert_eq!(acknowledgement.cleared_markers(), 1);
        assert_eq!(acknowledgement.remaining_markers(), 1);
        assert!(
            owner
                .supervisor()
                .retained_uncertainties()
                .iter()
                .any(|marker| marker.source_id() == Some(&first_source))
        );
    }

    #[test]
    fn source_audit_receipts_require_complete_matching_identity_and_boundary() {
        let source = SourceId::from_string("receipt-source");
        let root_identity = root(b"receipt-root");
        let mut owner = owner(1, 1, 8);
        let lane = owner
            .begin_source(source.clone(), root_identity.clone())
            .expect("capturing lane");
        let live = owner
            .admit_live_with_correlation(live_batch(&source, &root_identity, lane.generation(), 11))
            .expect("live admission");
        let ticket = match live.admission().outcome() {
            AdmissionOutcome::Accepted(ticket) => *ticket,
            outcome => panic!("expected accepted live admission, got {outcome:?}"),
        };
        let request = live
            .correlation()
            .expect("live audit request correlation")
            .audit_request();
        owner.dispatch_next().expect("dispatch live admission");
        owner.mark_dispatched(ticket).expect("mark dispatched");
        owner.mark_applied(ticket).expect("mark applied");
        owner
            .mark_unproven_audit_handed_off(ticket)
            .expect("handoff proofless live admission");

        let incomplete = request.incomplete();
        assert_eq!(
            owner.acknowledge_source_audit_receipt(&incomplete),
            ReconciliationAcknowledgementOutcome::unchanged(1)
        );
        assert!(
            request
                .complete_from_committed_audit(SourceAuditCommit::new(
                    0,
                    root_identity.clone(),
                    Some(request.clone()),
                ))
                .is_none(),
            "a zero revision cannot mint a clearing receipt"
        );
        assert!(
            request
                .complete_from_committed_audit(SourceAuditCommit::new(
                    12,
                    RootIdentity::from_bytes(b"other-root".to_vec()),
                    Some(request.clone()),
                ))
                .is_none(),
            "a commit for another physical root cannot mint a clearing receipt"
        );
        assert!(
            request
                .complete_from_committed_audit(SourceAuditCommit::new(
                    12,
                    root_identity.clone(),
                    None,
                ))
                .is_none(),
            "an unbound scanner completion cannot mint a clearing receipt"
        );
        let insufficient_boundary_request = SourceAuditRequest::new(
            ReconciliationAcknowledgementIdentity::new(
                source.clone(),
                root_identity.clone(),
                lane.generation(),
            ),
            RetainedUncertaintyBoundary::new(0, 0),
        );
        let insufficient_boundary = insufficient_boundary_request
            .complete_from_committed_audit(committed_audit(
                &insufficient_boundary_request,
                12,
                root_identity.clone(),
            ))
            .expect("matching committed audit authority");
        assert_eq!(
            owner.acknowledge_source_audit_receipt(&insufficient_boundary),
            ReconciliationAcknowledgementOutcome::unchanged(1)
        );
        let wrong_identity_request = SourceAuditRequest::new(
            ReconciliationAcknowledgementIdentity::new(
                SourceId::from_string("wrong-source"),
                root_identity.clone(),
                lane.generation(),
            ),
            request.boundary(),
        );
        let wrong_identity = wrong_identity_request
            .complete_from_committed_audit(committed_audit(
                &wrong_identity_request,
                12,
                root_identity.clone(),
            ))
            .expect("matching committed audit authority");
        assert_eq!(
            owner.acknowledge_source_audit_receipt(&wrong_identity),
            ReconciliationAcknowledgementOutcome::unchanged(1)
        );

        let newer_request = SourceAuditRequest::new(
            request.identity().clone(),
            RetainedUncertaintyBoundary::new(
                request.boundary().first(),
                request.boundary().through() + 1,
            ),
        );
        assert!(
            newer_request
                .complete_from_committed_audit(
                    committed_audit(&request, 12, root_identity.clone(),)
                )
                .is_none(),
            "an old valid commit cannot mint a receipt for a newer request"
        );
        let complete = request
            .complete_from_committed_audit(committed_audit(&request, 12, root_identity.clone()))
            .expect("matching committed audit authority");
        let acknowledgement = owner.acknowledge_source_audit_receipt(&complete);
        assert_eq!(acknowledgement.cleared_markers(), 1);
        assert_eq!(acknowledgement.remaining_markers(), 0);
        assert_eq!(
            owner.acknowledge_source_audit_receipt(&complete),
            ReconciliationAcknowledgementOutcome::unchanged(0),
            "duplicate receipt must be idempotent"
        );
    }

    #[test]
    fn audit_receipt_before_capture_and_after_root_rebind_clears_nothing() {
        let source = SourceId::from_string("receipt-fence");
        let old_root = root(b"old-root");
        let new_root = root(b"new-root");
        let mut owner = owner(1, 1, 8);
        let pre_capture_request = SourceAuditRequest::new(
            ReconciliationAcknowledgementIdentity::new(
                source.clone(),
                old_root.clone(),
                WatcherGeneration::new(1),
            ),
            RetainedUncertaintyBoundary::new(1, 1),
        );
        let pre_capture_receipt = pre_capture_request
            .complete_from_committed_audit(committed_audit(
                &pre_capture_request,
                1,
                old_root.clone(),
            ))
            .expect("matching committed audit authority");
        assert_eq!(
            owner.acknowledge_source_audit_receipt(&pre_capture_receipt),
            ReconciliationAcknowledgementOutcome::unchanged(0)
        );

        let lane = owner
            .begin_source(source.clone(), old_root.clone())
            .expect("old capturing lane");
        let live = owner
            .admit_live_with_correlation(live_batch(&source, &old_root, lane.generation(), 21))
            .expect("live admission");
        let ticket = match live.admission().outcome() {
            AdmissionOutcome::Accepted(ticket) => *ticket,
            outcome => panic!("expected accepted live admission, got {outcome:?}"),
        };
        let audit_request = live
            .correlation()
            .expect("live audit correlation")
            .audit_request();
        let receipt = audit_request
            .complete_from_committed_audit(committed_audit(&audit_request, 2, old_root.clone()))
            .expect("matching committed audit authority");
        owner.dispatch_next().expect("dispatch live admission");
        owner.mark_dispatched(ticket).expect("mark dispatched");
        owner.mark_applied(ticket).expect("mark applied");
        owner
            .mark_unproven_audit_handed_off(ticket)
            .expect("handoff live admission");

        owner
            .rebind_source(&source, new_root.clone())
            .expect("rebind source root");
        let fenced_before_restart = owner.acknowledge_source_audit_receipt(&receipt);
        assert_eq!(fenced_before_restart.cleared_markers(), 0);
        assert!(fenced_before_restart.remaining_markers() >= 1);
        owner
            .begin_source(source, new_root)
            .expect("restart replacement lane");
        let fenced_after_restart = owner.acknowledge_source_audit_receipt(&receipt);
        assert_eq!(fenced_after_restart.cleared_markers(), 0);
        assert!(
            fenced_after_restart.remaining_markers() >= 1,
            "old root/generation receipt must remain fenced after rebind"
        );
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
