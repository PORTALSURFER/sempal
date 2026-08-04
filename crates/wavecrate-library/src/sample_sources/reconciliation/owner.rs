//! Source-scoped ownership over the pure reconciliation admission supervisor.

use super::adapter::{
    AdapterAdmission, AdapterDisposition, AdapterError, LiveAuditAdmission, ReconciliationAdapter,
    ReplayPriorToken, SyntheticObservationBatch,
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

/// An immutable owner-level replay admission and its conservative audit fallback.
///
/// The adapter admission remains the source of truth for disposition and ticket identity. The
/// optional request is derived from the owner's current lane after invalid, rejected, or
/// capacity-exhausted replay evidence remains unproven. A valid continuity admission and an exact
/// duplicate therefore carry no new audit request.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OwnerReplayAdmission {
    admission: AdapterAdmission,
    audit_request: Option<SourceAuditRequest>,
}

impl OwnerReplayAdmission {
    fn new(admission: AdapterAdmission, audit_request: Option<SourceAuditRequest>) -> Self {
        Self {
            admission,
            audit_request,
        }
    }

    /// Borrow the unchanged adapter-level replay admission.
    pub const fn admission(&self) -> &AdapterAdmission {
        &self.admission
    }

    /// Borrow the current-lane audit request retained for an unproven replay, if any.
    pub const fn audit_request(&self) -> Option<&SourceAuditRequest> {
        self.audit_request.as_ref()
    }

    /// Consume the owner wrapper and return the unchanged adapter admission.
    pub fn into_admission(self) -> AdapterAdmission {
        self.admission
    }

    /// Consume the owner wrapper and return its optional current-lane audit request.
    pub fn into_audit_request(self) -> Option<SourceAuditRequest> {
        self.audit_request
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

    /// Admit replay using optional opaque durable authority through the current source lane.
    ///
    /// The owner consumes, but never constructs, native or database tokens. It independently
    /// forwards the caller's prior only when the batch source has a capturing lane and the prior's
    /// source/root/generation/backend-stream identity matches that current lane and batch stream.
    /// Otherwise it passes no prior so the adapter retains unproven uncertainty. Missing or
    /// ambiguous sequence evidence, a non-contiguous history claim, disqualifying raw evidence,
    /// or any current-lane fence remains at the existing adapter/supervisor boundary. Such
    /// outcomes carry the supervisor's bounded current-lane audit request when one can be derived.
    pub fn admit_replay_with_durable_prior(
        &mut self,
        batch: SyntheticObservationBatch,
        prior: Option<&ReplayPriorToken>,
        contiguous: bool,
    ) -> Result<OwnerReplayAdmission, AdapterError> {
        let source_id = batch.provenance().source_id().clone();
        let prior = self.validated_replay_prior(&batch, prior);
        let admission = ReconciliationAdapter::new(&mut self.supervisor)
            .admit_replay(batch, prior, contiguous)?;
        let audit_request = match admission.disposition() {
            AdapterDisposition::AdmittedWithContinuity
            | AdapterDisposition::DuplicateSuppressed => None,
            AdapterDisposition::AdmittedUnproven
            | AdapterDisposition::SourceAuditRequired
            | AdapterDisposition::UncertaintyCapacityExhausted => self
                .supervisor
                .source_audit_request_for_current_lane(&source_id),
        };

        Ok(OwnerReplayAdmission::new(admission, audit_request))
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

    /// Retire an Applied replay ticket with an opaque, durably committed checkpoint token.
    ///
    /// The supervisor checks that the ticket carries continuity proof, that the committed token
    /// matches its source/root/generation/backend stream, and that it reaches the replay coverage
    /// terminal sequence. This seam consumes authority returned by a future durability owner; it
    /// does not reuse the admission prior or mint a native/database token. Proofless tickets,
    /// mismatches, duplicate terminal calls, and tickets invalidated by stop, rebind, removal, or
    /// generation replacement remain fenced and do not release accounting twice.
    pub fn mark_replay_checkpointed(
        &mut self,
        ticket: DispatchTicket,
        committed_checkpoint: &ReplayPriorToken,
    ) -> Result<(), AdmissionError> {
        self.supervisor
            .mark_replay_checkpointed(ticket, committed_checkpoint)
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

    fn validated_replay_prior<'a>(
        &self,
        batch: &SyntheticObservationBatch,
        prior: Option<&'a ReplayPriorToken>,
    ) -> Option<&'a ReplayPriorToken> {
        let prior = prior?;
        let lane = self
            .supervisor
            .lane_for_source(batch.provenance().source_id())?;
        let generation = self.supervisor.generation(lane).ok()?;
        if self.supervisor.lifecycle(lane).ok()? != ReconciliationLifecycle::Capturing {
            return None;
        }
        let backend_stream_identity = batch.provenance().backend_stream_identity()?.clone();
        if prior.source_id() != lane.source_id()
            || prior.root_identity() != lane.root_identity()
            || prior.watcher_generation() != generation
            || prior.backend_stream_identity() != &backend_stream_identity
        {
            return None;
        }
        Some(prior)
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
        ReconciliationAcknowledgementIdentity, ReconciliationAdmissionLimits, ReplayPriorToken,
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

    fn replay_batch(
        source: &SourceId,
        root_identity: &RootIdentity,
        generation: WatcherGeneration,
        stream: Option<&[u8]>,
        first_sequence: Option<u64>,
        last_sequence: Option<u64>,
        kind: RawEventKind,
    ) -> SyntheticObservationBatch {
        let paths = if matches!(
            kind,
            RawEventKind::Overflow
                | RawEventKind::Error
                | RawEventKind::Unsupported
                | RawEventKind::RootChanged
        ) {
            Vec::new()
        } else {
            vec![RawObservedPath::new(
                "replayed.wav".into(),
                RawPathRole::Subject,
            )]
        };
        SyntheticObservationBatch::new(
            RawObservationProvenance::new(
                source.clone(),
                Some(root_identity.clone()),
                stream.map(|bytes| BackendStreamIdentity::from_bytes(bytes.to_vec())),
                generation,
                CaptureBoundary::try_new(20, first_sequence, last_sequence)
                    .expect("replay boundary"),
            ),
            vec![RawObservation::new(kind, paths)],
            RawObservationLimits::new(8, usize::MAX, usize::MAX).expect("batch limits"),
        )
    }

    fn replay_prior(
        source: &SourceId,
        root_identity: &RootIdentity,
        stream: &BackendStreamIdentity,
        generation: WatcherGeneration,
        acknowledged_sequence: u64,
    ) -> ReplayPriorToken {
        ReplayPriorToken::new(
            source.clone(),
            root_identity.clone(),
            stream.clone(),
            generation,
            acknowledged_sequence,
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
    fn owner_replay_binds_current_lane_and_checkpoint_releases_exactly_once() {
        let source = SourceId::from_string("owner-replay");
        let root_identity = root(b"owner-replay-root");
        let stream = BackendStreamIdentity::from_bytes(b"owner-replay-stream".to_vec());
        let mut owner = owner(1, 1, 8);
        let lane = owner
            .begin_source(source.clone(), root_identity.clone())
            .expect("capturing lane");
        let prior = replay_prior(&source, &root_identity, &stream, lane.generation(), 10);

        let admission = owner
            .admit_replay_with_durable_prior(
                replay_batch(
                    &source,
                    &root_identity,
                    lane.generation(),
                    Some(stream.as_bytes()),
                    Some(11),
                    Some(12),
                    RawEventKind::Create,
                ),
                Some(&prior),
                true,
            )
            .expect("valid owner replay");
        assert_eq!(
            admission.admission().disposition(),
            AdapterDisposition::AdmittedWithContinuity
        );
        assert_eq!(admission.audit_request(), None);
        let ticket = match admission.admission().outcome() {
            AdmissionOutcome::Accepted(ticket) => *ticket,
            outcome => panic!("unexpected owner replay outcome: {outcome:?}"),
        };
        assert!(owner.supervisor().retained_uncertainties().is_empty());

        let dispatched = owner.dispatch_next().expect("dispatch owner replay");
        let proof = dispatched
            .normalized()
            .proof()
            .watcher_continuity()
            .expect("owner replay continuity proof");
        assert_eq!(proof.source_id(), &source);
        assert_eq!(proof.root_identity(), &root_identity);
        assert_eq!(proof.backend_stream_identity(), &stream);
        assert_eq!(proof.watcher_generation(), lane.generation());
        assert_eq!(proof.prior_acknowledgement().sequence(), 10);
        assert_eq!(proof.replay_coverage().after_sequence(), 10);
        assert_eq!(proof.replay_coverage().through_sequence(), 12);
        assert!(proof.replay_coverage().is_contiguous());
        let terminal = replay_prior(&source, &root_identity, &stream, lane.generation(), 12);
        assert_eq!(
            owner.mark_replay_checkpointed(ticket, &terminal),
            Err(AdmissionError::InvalidLifecycleTransition)
        );
        assert_eq!(owner.supervisor().in_flight(), 1);

        owner.mark_dispatched(ticket).expect("replay dispatched");
        owner.mark_applied(ticket).expect("replay applied");
        owner
            .mark_replay_checkpointed(ticket, &terminal)
            .expect("replay checkpointed");
        assert_eq!(owner.supervisor().in_flight(), 0);
        assert_eq!(
            owner.mark_replay_checkpointed(ticket, &terminal),
            Err(AdmissionError::UnknownTicket)
        );
        assert_eq!(owner.supervisor().in_flight(), 0);
    }

    #[test]
    fn into_supervisor_cannot_generic_checkpoint_continuity_proven_replay() {
        let source = SourceId::from_string("owner-supervisor-replay");
        let root_identity = root(b"owner-supervisor-replay-root");
        let stream = BackendStreamIdentity::from_bytes(b"owner-supervisor-replay-stream".to_vec());
        let mut owner = owner(1, 1, 8);
        let lane = owner
            .begin_source(source.clone(), root_identity.clone())
            .expect("capturing lane");
        let prior = replay_prior(&source, &root_identity, &stream, lane.generation(), 10);
        let admission = owner
            .admit_replay_with_durable_prior(
                replay_batch(
                    &source,
                    &root_identity,
                    lane.generation(),
                    Some(stream.as_bytes()),
                    Some(11),
                    Some(11),
                    RawEventKind::Create,
                ),
                Some(&prior),
                true,
            )
            .expect("valid owner replay");
        let ticket = match admission.admission().outcome() {
            AdmissionOutcome::Accepted(ticket) => *ticket,
            outcome => panic!("unexpected owner replay outcome: {outcome:?}"),
        };
        owner.dispatch_next().expect("dispatch owner replay");
        owner.mark_dispatched(ticket).expect("replay dispatched");
        owner.mark_applied(ticket).expect("replay applied");

        let mut supervisor = owner.into_supervisor();
        assert_eq!(
            supervisor.mark_checkpointed(ticket),
            Err(AdmissionError::ReplayCheckpointRequiresDurableAuthority)
        );
        assert_eq!(supervisor.in_flight(), 1);

        let terminal = replay_prior(&source, &root_identity, &stream, lane.generation(), 11);
        supervisor
            .mark_replay_checkpointed(ticket, &terminal)
            .expect("replay checkpoint");
        assert_eq!(supervisor.in_flight(), 0);
    }

    #[test]
    fn owner_invalid_replay_stays_unproven_and_returns_current_lane_audit() {
        let cases = [
            (
                false,
                true,
                Some(11),
                Some(12),
                Some(b"owner-stream".as_slice()),
                RawEventKind::Create,
            ),
            (
                true,
                false,
                Some(11),
                Some(12),
                Some(b"owner-stream".as_slice()),
                RawEventKind::Create,
            ),
            (
                true,
                true,
                None,
                None,
                Some(b"owner-stream".as_slice()),
                RawEventKind::Create,
            ),
            (true, true, Some(11), Some(12), None, RawEventKind::Create),
            (
                true,
                true,
                Some(12),
                Some(13),
                Some(b"owner-stream".as_slice()),
                RawEventKind::Create,
            ),
            (
                true,
                true,
                Some(11),
                Some(12),
                Some(b"owner-stream".as_slice()),
                RawEventKind::Overflow,
            ),
        ];

        for (has_prior, contiguous, first_sequence, last_sequence, stream, kind) in cases {
            let source = SourceId::from_string("owner-invalid-replay");
            let root_identity = root(b"owner-invalid-root");
            let mut owner = owner(1, 2, 8);
            let lane = owner
                .begin_source(source.clone(), root_identity.clone())
                .expect("capturing lane");
            let prior_stream = BackendStreamIdentity::from_bytes(b"owner-stream".to_vec());
            let prior = has_prior.then(|| {
                replay_prior(
                    &source,
                    &root_identity,
                    &prior_stream,
                    lane.generation(),
                    10,
                )
            });
            let admission = owner
                .admit_replay_with_durable_prior(
                    replay_batch(
                        &source,
                        &root_identity,
                        lane.generation(),
                        stream,
                        first_sequence,
                        last_sequence,
                        kind,
                    ),
                    prior.as_ref(),
                    contiguous,
                )
                .expect("invalid replay remains an adapter result");
            assert_ne!(
                admission.admission().disposition(),
                AdapterDisposition::AdmittedWithContinuity
            );
            let audit_request = admission
                .audit_request()
                .expect("invalid current-lane replay audit request");
            assert_eq!(audit_request.source_id(), &source);
            assert_eq!(audit_request.root_identity(), &root_identity);
            assert_eq!(audit_request.generation(), lane.generation());

            let outcome = admission.admission().outcome();
            match outcome {
                AdmissionOutcome::Accepted(ticket) => {
                    let ticket = *ticket;
                    let dispatched = owner.dispatch_next().expect("unproven replay dispatch");
                    assert_eq!(dispatched.ticket(), ticket);
                    assert!(dispatched.normalized().proof().is_unproven());
                    owner
                        .mark_dispatched(ticket)
                        .expect("invalid replay dispatched");
                    owner.mark_applied(ticket).expect("invalid replay applied");
                    owner
                        .mark_unproven_audit_handed_off(ticket)
                        .expect("invalid replay audit handed off");
                }
                AdmissionOutcome::Rejected(_) => {
                    assert_eq!(owner.dispatch_next(), None);
                }
                outcome => panic!("unexpected invalid replay outcome: {outcome:?}"),
            }
            assert_eq!(owner.supervisor().in_flight(), 0);
        }
    }

    #[test]
    fn owner_replay_duplicate_is_suppressed_without_audit_or_extra_accounting() {
        let source = SourceId::from_string("owner-replay-duplicate");
        let root_identity = root(b"owner-replay-duplicate-root");
        let mut owner = owner(1, 1, 8);
        let lane = owner
            .begin_source(source.clone(), root_identity.clone())
            .expect("capturing lane");
        let batch = replay_batch(
            &source,
            &root_identity,
            lane.generation(),
            Some(b"owner-replay-duplicate-stream"),
            Some(11),
            Some(11),
            RawEventKind::Create,
        );
        let stream = BackendStreamIdentity::from_bytes(b"owner-replay-duplicate-stream".to_vec());
        let prior = replay_prior(&source, &root_identity, &stream, lane.generation(), 10);

        let first = owner
            .admit_replay_with_durable_prior(batch.clone(), Some(&prior), true)
            .expect("first replay");
        let second = owner
            .admit_replay_with_durable_prior(batch, Some(&prior), true)
            .expect("duplicate replay");
        assert_eq!(
            first.admission().disposition(),
            AdapterDisposition::AdmittedWithContinuity
        );
        assert_eq!(
            second.admission().disposition(),
            AdapterDisposition::DuplicateSuppressed
        );
        assert_eq!(second.audit_request(), None);
        assert!(matches!(
            second.admission().outcome(),
            AdmissionOutcome::DuplicateSuppressed(_)
        ));
        assert_eq!(owner.supervisor().in_flight(), 1);
    }

    #[test]
    fn owner_replay_terminal_rejects_proofless_tickets_and_lifecycle_fences_old_tickets() {
        let source = SourceId::from_string("owner-replay-fence");
        let root_identity = root(b"owner-replay-fence-root");
        let mut owner = owner(1, 1, 8);
        let lane = owner
            .begin_source(source.clone(), root_identity.clone())
            .expect("capturing lane");
        let live = owner
            .admit_live_with_correlation(live_batch(&source, &root_identity, lane.generation(), 30))
            .expect("live admission");
        let live_ticket = match live.admission().outcome() {
            AdmissionOutcome::Accepted(ticket) => *ticket,
            outcome => panic!("unexpected live outcome: {outcome:?}"),
        };
        owner.dispatch_next().expect("live dispatch");
        owner.mark_dispatched(live_ticket).expect("live dispatched");
        owner.mark_applied(live_ticket).expect("live applied");
        let live_checkpoint = replay_prior(
            &source,
            &root_identity,
            &BackendStreamIdentity::from_bytes(b"live-checkpoint".to_vec()),
            lane.generation(),
            30,
        );
        assert_eq!(
            owner.mark_replay_checkpointed(live_ticket, &live_checkpoint),
            Err(AdmissionError::ReplayCheckpointRequiresContinuityProof)
        );
        assert_eq!(owner.supervisor().in_flight(), 1);
        owner
            .mark_unproven_audit_handed_off(live_ticket)
            .expect("live audit handed off");

        let replay_stream =
            BackendStreamIdentity::from_bytes(b"owner-replay-fence-stream".to_vec());
        let admission_prior = replay_prior(
            &source,
            &root_identity,
            &replay_stream,
            lane.generation(),
            40,
        );
        let replay = owner
            .admit_replay_with_durable_prior(
                replay_batch(
                    &source,
                    &root_identity,
                    lane.generation(),
                    Some(b"owner-replay-fence-stream"),
                    Some(41),
                    Some(41),
                    RawEventKind::Create,
                ),
                Some(&admission_prior),
                true,
            )
            .expect("replay admission");
        let replay_ticket = match replay.admission().outcome() {
            AdmissionOutcome::Accepted(ticket) => *ticket,
            outcome => panic!("unexpected replay outcome: {outcome:?}"),
        };
        owner
            .stop_source(&source)
            .expect("stop invalidates old replay ticket");
        assert_eq!(owner.supervisor().in_flight(), 0);
        assert_eq!(
            owner.mark_replay_checkpointed(replay_ticket, &admission_prior),
            Err(AdmissionError::UnknownTicket)
        );

        let replacement_root = root(b"owner-replay-fence-replacement");
        let restarted = owner
            .begin_source(source.clone(), root_identity)
            .expect("restart lane");
        let replacement_prior = replay_prior(
            &source,
            restarted.root_identity(),
            &BackendStreamIdentity::from_bytes(b"owner-replay-rebind-stream".to_vec()),
            restarted.generation(),
            50,
        );
        let replacement_replay = owner
            .admit_replay_with_durable_prior(
                replay_batch(
                    &source,
                    restarted.root_identity(),
                    restarted.generation(),
                    Some(b"owner-replay-rebind-stream"),
                    Some(51),
                    Some(51),
                    RawEventKind::Create,
                ),
                Some(&replacement_prior),
                true,
            )
            .expect("replacement replay admission");
        let replacement_ticket = match replacement_replay.admission().outcome() {
            AdmissionOutcome::Accepted(ticket) => *ticket,
            outcome => panic!("unexpected replacement replay outcome: {outcome:?}"),
        };
        let rebound = owner
            .rebind_source(&source, replacement_root)
            .expect("rebind lane");
        assert_eq!(rebound.lifecycle(), ReconciliationLifecycle::Starting);
        assert_eq!(
            owner.mark_replay_checkpointed(replacement_ticket, &replacement_prior),
            Err(AdmissionError::UnknownTicket)
        );
        owner.stop_source(&source).expect("stop rebound lane");
        owner
            .remove_source(&source)
            .expect("remove stopped rebound");
        assert_eq!(
            owner.mark_replay_checkpointed(replacement_ticket, &replacement_prior),
            Err(AdmissionError::UnknownTicket)
        );
    }

    #[test]
    fn owner_replay_prior_identity_mismatches_are_dropped_before_adapter_admission() {
        let source = SourceId::from_string("owner-replay-identity");
        let root_identity = root(b"owner-replay-identity-root");
        let stream_a = BackendStreamIdentity::from_bytes(b"owner-stream-a".to_vec());
        let stream_b = BackendStreamIdentity::from_bytes(b"owner-stream-b".to_vec());
        let mut owner = owner(1, 2, 8);
        let lane = owner
            .begin_source(source.clone(), root_identity.clone())
            .expect("capturing lane");
        let mismatched_priors = [
            replay_prior(
                &SourceId::from_string("other-source"),
                &root_identity,
                &stream_b,
                lane.generation(),
                10,
            ),
            replay_prior(
                &source,
                &root(b"other-root"),
                &stream_b,
                lane.generation(),
                10,
            ),
            replay_prior(
                &source,
                &root_identity,
                &stream_b,
                WatcherGeneration::new(lane.generation().get() + 1),
                10,
            ),
            replay_prior(&source, &root_identity, &stream_a, lane.generation(), 10),
        ];

        for (index, prior) in mismatched_priors.iter().enumerate() {
            let sequence = 11 + index as u64;
            let admission = owner
                .admit_replay_with_durable_prior(
                    replay_batch(
                        &source,
                        &root_identity,
                        lane.generation(),
                        Some(stream_b.as_bytes()),
                        Some(sequence),
                        Some(sequence),
                        RawEventKind::Create,
                    ),
                    Some(prior),
                    true,
                )
                .expect("identity mismatch remains an adapter result");
            assert_eq!(
                admission.admission().disposition(),
                AdapterDisposition::SourceAuditRequired
            );
            let audit_request = admission
                .audit_request()
                .expect("mismatch retains current-lane audit request");
            assert_eq!(audit_request.source_id(), &source);
            assert_eq!(audit_request.root_identity(), &root_identity);
            assert_eq!(audit_request.generation(), lane.generation());
            match admission.admission().outcome() {
                AdmissionOutcome::Accepted(ticket) => {
                    let ticket = *ticket;
                    let dispatched = owner.dispatch_next().expect("unproven dispatch");
                    assert!(dispatched.normalized().proof().is_unproven());
                    owner.mark_dispatched(ticket).expect("mismatch dispatched");
                    owner.mark_applied(ticket).expect("mismatch applied");
                    owner
                        .mark_unproven_audit_handed_off(ticket)
                        .expect("mismatch audit handed off");
                }
                outcome => panic!("unexpected identity mismatch outcome: {outcome:?}"),
            }
        }
        assert_eq!(owner.supervisor().in_flight(), 0);
    }

    #[test]
    fn owner_replay_terminal_rejects_old_or_wrong_authority_without_releasing_accounting() {
        let source = SourceId::from_string("owner-replay-terminal");
        let root_identity = root(b"owner-replay-terminal-root");
        let stream = BackendStreamIdentity::from_bytes(b"owner-replay-terminal-stream".to_vec());
        let mut owner = owner(1, 1, 8);
        let lane = owner
            .begin_source(source.clone(), root_identity.clone())
            .expect("capturing lane");
        let admission_prior = replay_prior(&source, &root_identity, &stream, lane.generation(), 10);
        let admission = owner
            .admit_replay_with_durable_prior(
                replay_batch(
                    &source,
                    &root_identity,
                    lane.generation(),
                    Some(stream.as_bytes()),
                    Some(11),
                    Some(12),
                    RawEventKind::Create,
                ),
                Some(&admission_prior),
                true,
            )
            .expect("replay admission");
        let ticket = match admission.admission().outcome() {
            AdmissionOutcome::Accepted(ticket) => *ticket,
            outcome => panic!("unexpected replay outcome: {outcome:?}"),
        };
        owner.dispatch_next().expect("replay dispatch");
        owner.mark_dispatched(ticket).expect("replay dispatched");
        owner.mark_applied(ticket).expect("replay applied");

        let insufficient = replay_prior(&source, &root_identity, &stream, lane.generation(), 11);
        let wrong_tokens = [
            replay_prior(
                &SourceId::from_string("other-source"),
                &root_identity,
                &stream,
                lane.generation(),
                12,
            ),
            replay_prior(
                &source,
                &root(b"other-root"),
                &stream,
                lane.generation(),
                12,
            ),
            replay_prior(
                &source,
                &root_identity,
                &stream,
                WatcherGeneration::new(lane.generation().get() + 1),
                12,
            ),
            replay_prior(
                &source,
                &root_identity,
                &BackendStreamIdentity::from_bytes(b"other-stream".to_vec()),
                lane.generation(),
                12,
            ),
        ];
        assert_eq!(
            owner.mark_replay_checkpointed(ticket, &insufficient),
            Err(AdmissionError::ReplayCheckpointAuthorityMismatch)
        );
        assert_eq!(owner.supervisor().in_flight(), 1);
        for wrong in &wrong_tokens {
            assert_eq!(
                owner.mark_replay_checkpointed(ticket, wrong),
                Err(AdmissionError::ReplayCheckpointAuthorityMismatch)
            );
            assert_eq!(owner.supervisor().in_flight(), 1);
        }

        let terminal = replay_prior(&source, &root_identity, &stream, lane.generation(), 12);
        owner
            .mark_replay_checkpointed(ticket, &terminal)
            .expect("matching terminal checkpoint");
        assert_eq!(owner.supervisor().in_flight(), 0);
        assert_eq!(
            owner.mark_replay_checkpointed(ticket, &terminal),
            Err(AdmissionError::UnknownTicket)
        );
    }

    #[test]
    fn owner_replay_capacity_exhaustion_keeps_current_lane_audit_fallback() {
        let mut owner = owner(1, 1, 1);
        for index in 0..2 {
            let source = SourceId::from_string(format!("unknown-capacity-{index}"));
            let root_identity = root(format!("unknown-capacity-root-{index}").as_bytes());
            let admission = owner
                .admit_replay_with_durable_prior(
                    replay_batch(
                        &source,
                        &root_identity,
                        WatcherGeneration::new(1),
                        Some(b"unknown-capacity-stream"),
                        Some(1),
                        Some(1),
                        RawEventKind::Create,
                    ),
                    None,
                    true,
                )
                .expect("unknown source admission remains bounded");
            assert_eq!(
                admission.admission().disposition(),
                AdapterDisposition::SourceAuditRequired
            );
            assert_eq!(admission.audit_request(), None);
        }

        let source = SourceId::from_string("capacity-current");
        let root_identity = root(b"capacity-current-root");
        let lane = owner
            .begin_source(source.clone(), root_identity.clone())
            .expect("current capturing lane");
        let admission = owner
            .admit_replay_with_durable_prior(
                replay_batch(
                    &source,
                    &root_identity,
                    lane.generation(),
                    Some(b"capacity-current-stream"),
                    Some(11),
                    Some(11),
                    RawEventKind::Create,
                ),
                None,
                true,
            )
            .expect("capacity-exhausted replay remains bounded");
        assert!(matches!(
            admission.admission().outcome(),
            AdmissionOutcome::UncertaintyCapacityExhausted(_)
        ));
        let audit_request = admission
            .audit_request()
            .expect("capacity exhaustion current-lane audit request");
        assert_eq!(audit_request.source_id(), &source);
        assert_eq!(audit_request.root_identity(), &root_identity);
        assert_eq!(audit_request.generation(), lane.generation());
        assert_eq!(owner.supervisor().in_flight(), 0);
    }

    #[test]
    fn owner_replay_unknown_and_stopped_sources_fail_closed_without_inventing_lanes() {
        let source = SourceId::from_string("unknown-replay-source");
        let root_identity = root(b"unknown-replay-root");
        let stream = BackendStreamIdentity::from_bytes(b"unknown-replay-stream".to_vec());
        let mut owner = owner(1, 1, 8);
        let prior = replay_prior(
            &source,
            &root_identity,
            &stream,
            WatcherGeneration::new(1),
            10,
        );
        let unknown = owner
            .admit_replay_with_durable_prior(
                replay_batch(
                    &source,
                    &root_identity,
                    WatcherGeneration::new(1),
                    Some(stream.as_bytes()),
                    Some(11),
                    Some(11),
                    RawEventKind::Create,
                ),
                Some(&prior),
                true,
            )
            .expect("unknown source remains an adapter result");
        assert_eq!(
            unknown.admission().outcome(),
            &AdmissionOutcome::Rejected(AdmissionRejectReason::UnknownLane)
        );
        assert_eq!(unknown.audit_request(), None);
        assert_eq!(owner.lane(&source), None);
        assert_eq!(owner.supervisor().in_flight(), 0);

        let lane = owner
            .begin_source(source.clone(), root_identity.clone())
            .expect("capturing source");
        owner.stop_source(&source).expect("stopped source");
        let stopped_prior = replay_prior(&source, &root_identity, &stream, lane.generation(), 10);
        let stopped = owner
            .admit_replay_with_durable_prior(
                replay_batch(
                    &source,
                    &root_identity,
                    lane.generation(),
                    Some(stream.as_bytes()),
                    Some(11),
                    Some(11),
                    RawEventKind::Create,
                ),
                Some(&stopped_prior),
                true,
            )
            .expect("stopped source remains an adapter result");
        assert_eq!(
            stopped.admission().outcome(),
            &AdmissionOutcome::Rejected(AdmissionRejectReason::NotCapturing)
        );
        let audit_request = stopped
            .audit_request()
            .expect("existing stopped lane audit request");
        assert_eq!(audit_request.source_id(), &source);
        assert_eq!(audit_request.root_identity(), &root_identity);
        assert_eq!(audit_request.generation(), lane.generation());
        assert_eq!(
            owner.lane(&source).expect("stopped lane").lifecycle(),
            ReconciliationLifecycle::Stopped
        );
        assert_eq!(owner.supervisor().in_flight(), 0);
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
