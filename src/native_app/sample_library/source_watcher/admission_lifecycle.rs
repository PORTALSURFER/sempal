//! Watcher-local lifecycle ownership for source/root admission lanes.

use std::collections::HashSet;

use wavecrate::sample_sources::{SampleSource, SourceId};
use wavecrate_library::sample_sources::reconciliation::{
    AdapterError, AdmissionError, AdmissionOwnerError, BackendStreamIdentity, CaptureBoundary,
    DispatchTicket, DispatchedObservation, LiveAuditAdmission, OwnedAdmissionLane,
    OwnerReplayAdmission, RawObservation, RawObservationLimits, RawObservationProvenance,
    ReconciliationAcknowledgementOutcome, ReconciliationAdmissionLimits,
    ReconciliationAdmissionOwner, ReconciliationAdmissionSupervisor, ReconciliationLifecycle,
    RootIdentity, SourceAuditReceipt, SourceAuditRequest, SyntheticObservationBatch,
    WatcherGeneration,
};
use wavecrate_library::sample_sources::{SourceDatabase, SourceDbError};

use super::roots::{WatchedRootIdentities, registered_root_identity};

/// Bounded owner capacity for the native watcher lifecycle.
///
/// This is a resource bound, not a source-count policy. If the owner reaches it, lifecycle
/// reconciliation fails closed and the owner is retained for a later retry. Admission stays
/// closed until a later fence proves every registered lane stopped.
const NATIVE_ADMISSION_MAX_IN_FLIGHT: usize = 256;
const NATIVE_ADMISSION_MAX_IN_FLIGHT_PER_LANE: usize = 1;
const NATIVE_ADMISSION_MAX_RECENT_SEQUENCES_PER_LANE: usize = 64;
const NATIVE_ADMISSION_MAX_RETAINED_UNCERTAINTIES: usize = 4096;
const NATIVE_ADMISSION_MAX_EVENTS_PER_LANE: usize = 512;

/// Bounded FSEvents history evidence awaiting owner admission.
///
/// The source lifecycle generation, history worker generation, and owner lane generation are
/// intentionally separate. Only the owner lane generation is allowed into reconciliation
/// provenance; the history worker generation must match the coordinator's active stream
/// generation before any durable prior can be used, and is never promoted into owner authority.
#[allow(dead_code)]
#[derive(Debug)]
pub(super) struct FseventsReplayEvidence {
    /// Historical checkpoint generation; current source authority is transported separately by
    /// `SourceProcessingRegistration` and must never be reconstructed from this evidence.
    pub(super) source_lifecycle_generation: WatcherGeneration,
    pub(super) replay_stream_generation: u64,
    pub(super) backend_device: u64,
    pub(super) replay_start_event_id: u64,
    pub(super) replay_end_event_id: u64,
    pub(super) observations: Vec<RawObservation>,
    pub(super) limits: RawObservationLimits,
}

/// Fail-closed result for the dormant native replay admission seam.
#[allow(dead_code)]
#[derive(Debug)]
pub(super) enum FseventsReplayAdmissionError {
    Database(SourceDbError),
    NoCapturingLane,
    Adapter(AdapterError),
}

#[derive(Debug)]
pub(super) enum FseventsReplayCheckpointError {
    Database(SourceDbError),
    NoCapturingLane,
    MissingDurableAuthority,
    Adapter(AdmissionError),
}

impl FseventsReplayCheckpointError {
    pub(super) fn category(&self) -> &'static str {
        match self {
            Self::Database(error) => {
                let _ = error;
                "database"
            }
            Self::NoCapturingLane => "no_capturing_lane",
            Self::MissingDurableAuthority => "missing_durable_authority",
            Self::Adapter(error) => {
                let _ = error;
                "adapter"
            }
        }
    }
}

/// Owns the one admission owner used by a native source-watcher coordinator.
pub(super) struct AdmissionLifecycle {
    owner: ReconciliationAdmissionOwner,
    admission_closed: bool,
}

impl AdmissionLifecycle {
    /// Create an empty lifecycle owner with the native bounded capacity.
    pub(super) fn new() -> Self {
        Self::from_limits(native_admission_limits())
    }

    fn from_limits(limits: ReconciliationAdmissionLimits) -> Self {
        Self {
            owner: ReconciliationAdmissionOwner::new(ReconciliationAdmissionSupervisor::new(
                limits,
            )),
            admission_closed: false,
        }
    }

    #[cfg(test)]
    pub(super) fn with_limits(limits: ReconciliationAdmissionLimits) -> Self {
        Self::from_limits(limits)
    }

    /// Stop every registered lane while retaining the owner's uncertainty markers.
    ///
    /// A failed stop latches admission closed. Later lifecycle boundaries retry the fence, and
    /// admission only reopens after every registered lane is proven stopped.
    pub(super) fn fence_all(&mut self) -> Result<(), AdmissionOwnerError> {
        self.admission_closed = true;
        let mut source_ids = self.owner.source_ids().cloned().collect::<Vec<_>>();
        source_ids.sort_by(|left, right| left.as_str().cmp(right.as_str()));
        let mut first_error = None;
        for source_id in source_ids {
            if let Err(error) = self.stop_if_capturing(&source_id) {
                first_error.get_or_insert(error);
            }
        }
        if let Some(error) = first_error {
            return Err(error);
        }
        self.admission_closed = false;
        Ok(())
    }

    /// Reconcile configured sources with the roots that the current watcher installed.
    ///
    /// Only a root identity recorded by successful watcher installation is eligible for a lane.
    /// Missing or unreadable identities leave a configured source stopped or unregistered. The
    /// owner remains authoritative for all existing lane and generation state.
    pub(super) fn reconcile(
        &mut self,
        sources: &[SampleSource],
        watched_roots: &WatchedRootIdentities,
    ) -> Result<(), AdmissionOwnerError> {
        if self.admission_closed {
            self.fence_all()?;
        }

        let desired_source_ids = sources
            .iter()
            .map(|source| source.id.clone())
            .collect::<HashSet<_>>();

        let mut registered_source_ids = self.owner.source_ids().cloned().collect::<Vec<_>>();
        registered_source_ids.sort_by(|left, right| left.as_str().cmp(right.as_str()));
        for source_id in registered_source_ids {
            if !desired_source_ids.contains(&source_id) {
                self.stop_if_capturing(&source_id)?;
                self.owner.remove_source(&source_id)?;
            }
        }

        for source in sources {
            let Some(root_identity) = registered_root_identity(watched_roots, &source.root) else {
                self.stop_if_capturing(&source.id)?;
                continue;
            };
            self.reconcile_source(source, root_identity)?;
        }
        Ok(())
    }

    pub(super) fn reconcile_source(
        &mut self,
        source: &SampleSource,
        root_identity: RootIdentity,
    ) -> Result<(), AdmissionOwnerError> {
        let Some(existing) = self.owner.lane(&source.id) else {
            return self
                .owner
                .begin_source(source.id.clone(), root_identity)
                .map(|_| ())
                .map_err(Into::into);
        };

        if existing.root_identity() != &root_identity {
            self.owner
                .rebind_source(&source.id, root_identity.clone())?;
            return self
                .owner
                .begin_source(source.id.clone(), root_identity)
                .map(|_| ())
                .map_err(Into::into);
        }

        if existing.lifecycle() == ReconciliationLifecycle::Capturing {
            return Ok(());
        }

        self.owner
            .begin_source(source.id.clone(), root_identity)
            .map(|_| ())
            .map_err(Into::into)
    }

    fn stop_if_capturing(&mut self, source_id: &SourceId) -> Result<(), AdmissionOwnerError> {
        let Some(lane) = self.owner.lane(source_id) else {
            return Ok(());
        };
        if lane.lifecycle() != ReconciliationLifecycle::Stopped {
            self.owner.stop_source(source_id)?;
        }
        Ok(())
    }

    /// Stop one current capturing lane only when its source, root, and owner generation still
    /// match the retained context that requested the fence. A stale context is a no-op so an old
    /// acknowledgement cannot stop a newer lane for the same source.
    pub(super) fn fence_source_if_current(
        &mut self,
        source_id: &SourceId,
        root_identity: &RootIdentity,
        watcher_generation: WatcherGeneration,
    ) -> Result<bool, AdmissionOwnerError> {
        let Some(lane) = self.owner.lane(source_id) else {
            return Ok(false);
        };
        if lane.lifecycle() != ReconciliationLifecycle::Capturing
            || lane.root_identity() != root_identity
            || lane.generation() != watcher_generation
        {
            return Ok(false);
        }
        self.owner.stop_source(source_id)?;
        Ok(true)
    }

    /// Return the owner-authoritative lane snapshot used to bind a live capture.
    pub(super) fn lane_for_capture(&self, source_id: &SourceId) -> Option<OwnedAdmissionLane> {
        if self.admission_closed {
            return None;
        }
        self.owner.lane(source_id)
    }

    /// Check a queued audit request against the owner-held current capturing lane.
    ///
    /// This is deliberately pure: it observes only the in-memory source, root, generation, and
    /// lifecycle identity owned by the admission supervisor. A request from a stopped lane or a
    /// replaced root/generation is never eligible for watcher transport.
    pub(super) fn request_matches_current_capturing_lane(
        &self,
        request: &SourceAuditRequest,
    ) -> bool {
        self.lane_for_capture(request.source_id())
            .is_some_and(|lane| {
                lane.lifecycle() == ReconciliationLifecycle::Capturing
                    && lane.root_identity() == request.root_identity()
                    && lane.generation() == request.generation()
            })
    }

    /// Build the existing bounded request for the current owner-authoritative source lane.
    pub(super) fn source_audit_request_for_current_lane(
        &self,
        source_id: &SourceId,
    ) -> Option<(SourceAuditRequest, bool)> {
        let request = self
            .owner
            .source_audit_request_for_current_lane(source_id)?;
        let marker_backed = self.source_audit_request_is_marker_backed(&request);
        Some((request, marker_backed))
    }

    pub(super) fn source_audit_request_is_marker_backed(
        &self,
        request: &SourceAuditRequest,
    ) -> bool {
        self.owner
            .supervisor()
            .retained_uncertainties()
            .iter()
            .any(|marker| {
                marker.source_id() == Some(request.source_id())
                    && marker.root_identity() == Some(request.root_identity())
                    && marker.generation() == Some(request.generation())
            })
    }

    /// Admit live evidence through the existing owner-held adapter.
    pub(super) fn admit_live_with_correlation(
        &mut self,
        batch: SyntheticObservationBatch,
    ) -> Result<LiveAuditAdmission, AdapterError> {
        self.owner.admit_live_with_correlation(batch)
    }

    /// Admit one bounded FSEvents history replay through the current owner lane.
    ///
    /// The database supplies only an opaque prior after validating the persisted source
    /// lifecycle. The owner independently binds that prior to the current root, FSEvents device
    /// stream, and reconciliation-lane generation. The coordinator's active replay-stream
    /// generation fences stale asynchronous history completions before the database is read;
    /// mismatches therefore receive no durable prior and retain the existing bounded audit path.
    #[allow(dead_code)]
    pub(super) fn admit_fsevents_replay(
        &mut self,
        source: &SampleSource,
        database: &SourceDatabase,
        active_replay_stream_generation: u64,
        evidence: FseventsReplayEvidence,
    ) -> Result<OwnerReplayAdmission, FseventsReplayAdmissionError> {
        let lane = self
            .lane_for_capture(&source.id)
            .ok_or(FseventsReplayAdmissionError::NoCapturingLane)?;
        let stream_identity = BackendStreamIdentity::from_fsevents_device(evidence.backend_device);
        let first_sequence = evidence
            .replay_start_event_id
            .checked_add(1)
            .filter(|first| *first <= evidence.replay_end_event_id);
        let capture_boundary = CaptureBoundary::try_new(
            evidence.replay_end_event_id,
            first_sequence,
            first_sequence.map(|_| evidence.replay_end_event_id),
        )
        .expect("replay boundary fallback remains structurally valid");
        let authority_eligible = active_replay_stream_generation != 0
            && evidence.replay_stream_generation != 0
            && evidence.replay_stream_generation == active_replay_stream_generation
            && stream_identity.is_some()
            && first_sequence.is_some();
        let batch = SyntheticObservationBatch::new(
            RawObservationProvenance::new(
                source.id.clone(),
                Some(lane.root_identity().clone()),
                stream_identity,
                lane.generation(),
                capture_boundary,
            ),
            evidence.observations,
            evidence.limits,
        );
        let prior = if authority_eligible {
            database
                .read_durable_replay_prior(
                    &source.id,
                    lane.root_identity(),
                    evidence.source_lifecycle_generation,
                    lane.generation(),
                )
                .map_err(FseventsReplayAdmissionError::Database)?
        } else {
            None
        };
        self.owner
            .admit_replay_with_durable_prior(batch, prior.as_ref(), true)
            .map_err(FseventsReplayAdmissionError::Adapter)
    }

    /// Retire an applied replay only with the opaque authority reread after the source owner has
    /// durably committed the matching revision-bound checkpoint. The supplied lifecycle
    /// generation is the current terminal checkpoint generation; it is intentionally distinct
    /// from the historical generation used to validate replay admission.
    pub(super) fn mark_fsevents_replay_checkpointed(
        &mut self,
        source_id: &SourceId,
        database: &SourceDatabase,
        current_source_lifecycle_generation: WatcherGeneration,
        ticket: DispatchTicket,
    ) -> Result<(), FseventsReplayCheckpointError> {
        let lane = self
            .lane_for_capture(source_id)
            .ok_or(FseventsReplayCheckpointError::NoCapturingLane)?;
        let prior = database
            .read_durable_replay_prior(
                source_id,
                lane.root_identity(),
                current_source_lifecycle_generation,
                lane.generation(),
            )
            .map_err(FseventsReplayCheckpointError::Database)?
            .ok_or(FseventsReplayCheckpointError::MissingDurableAuthority)?;
        self.owner
            .mark_replay_checkpointed(ticket, &prior)
            .map_err(FseventsReplayCheckpointError::Adapter)
    }

    /// Select the next owner-scheduled live envelope.
    pub(super) fn dispatch_next(&mut self) -> Option<DispatchedObservation> {
        if self.admission_closed {
            return None;
        }
        self.owner.dispatch_next()
    }

    /// Advance an owner-scheduled envelope through its handoff phases.
    pub(super) fn mark_dispatched(
        &mut self,
        ticket: DispatchTicket,
    ) -> Result<(), wavecrate_library::sample_sources::reconciliation::AdmissionError> {
        self.owner.mark_dispatched(ticket)
    }

    pub(super) fn mark_applied(
        &mut self,
        ticket: DispatchTicket,
    ) -> Result<(), wavecrate_library::sample_sources::reconciliation::AdmissionError> {
        self.owner.mark_applied(ticket)
    }

    pub(super) fn mark_unproven_audit_handed_off(
        &mut self,
        ticket: DispatchTicket,
    ) -> Result<(), wavecrate_library::sample_sources::reconciliation::AdmissionError> {
        self.owner.mark_unproven_audit_handed_off(ticket)
    }

    /// Apply a complete source-audit receipt through the owner-authoritative typed acknowledgement.
    ///
    /// Receipts are checked against the currently capturing source/root/generation lane by the
    /// owner. No receipt can grant continuity or checkpoint authority.
    pub(super) fn acknowledge_source_audit_receipt(
        &mut self,
        receipt: &SourceAuditReceipt,
    ) -> ReconciliationAcknowledgementOutcome {
        self.owner.acknowledge_source_audit_receipt(receipt)
    }

    pub(super) fn max_in_flight(&self) -> usize {
        self.owner.max_in_flight()
    }

    pub(super) fn max_source_audit_request_entries(&self) -> usize {
        self.owner.max_source_audit_request_entries()
    }

    pub(super) fn in_flight(&self) -> usize {
        self.owner.supervisor().in_flight()
    }

    #[cfg(test)]
    pub(super) fn retained_uncertainties(
        &self,
    ) -> &[wavecrate_library::sample_sources::reconciliation::RetainedUncertainty] {
        self.owner.supervisor().retained_uncertainties()
    }

    #[cfg(test)]
    fn lane(&self, source_id: &SourceId) -> Option<OwnedAdmissionLane> {
        self.owner.lane(source_id)
    }
}

fn native_admission_limits() -> ReconciliationAdmissionLimits {
    let per_lane =
        RawObservationLimits::new(NATIVE_ADMISSION_MAX_EVENTS_PER_LANE, usize::MAX, usize::MAX)
            .expect("native per-lane admission limits");
    let global_events = NATIVE_ADMISSION_MAX_EVENTS_PER_LANE
        .checked_mul(NATIVE_ADMISSION_MAX_IN_FLIGHT)
        .expect("native global event limit");
    let global = RawObservationLimits::new(global_events, usize::MAX, usize::MAX)
        .expect("native global admission limits");
    ReconciliationAdmissionLimits::new_with_per_lane_capacity(
        usize::MAX,
        per_lane,
        global,
        NATIVE_ADMISSION_MAX_IN_FLIGHT,
        NATIVE_ADMISSION_MAX_IN_FLIGHT_PER_LANE,
        NATIVE_ADMISSION_MAX_RECENT_SEQUENCES_PER_LANE,
        NATIVE_ADMISSION_MAX_RETAINED_UNCERTAINTIES,
    )
    .expect("native admission limits")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{collections::HashMap, path::PathBuf};
    use tempfile::tempdir;
    use wavecrate_library::sample_sources::db::META_SOURCE_WATCHER_CHECKPOINT;
    use wavecrate_library::sample_sources::reconciliation::{
        AdapterDisposition, AdmissionOutcome, BackendStreamIdentity, Proof, RawEventKind,
        RawObservation, RawObservationProvenance, RawObservedPath, RawPathRole,
        SyntheticObservationBatch,
    };

    fn source(id: &str, root: &str) -> SampleSource {
        SampleSource::new_with_id(SourceId::from_string(id), PathBuf::from(root))
    }

    fn root(identity: &[u8]) -> RootIdentity {
        RootIdentity::from_bytes(identity.to_vec())
    }

    fn watched(entries: &[(&str, Option<&[u8]>)]) -> WatchedRootIdentities {
        entries
            .iter()
            .map(|(path, identity)| {
                (
                    PathBuf::from(path),
                    identity.map(|identity| String::from_utf8_lossy(identity).into_owned()),
                )
            })
            .collect()
    }

    fn limits(
        max_lanes: usize,
        max_in_flight: usize,
        max_retained_uncertainties: usize,
    ) -> ReconciliationAdmissionLimits {
        let per_lane = RawObservationLimits::new(8, usize::MAX, usize::MAX).expect("lane limits");
        let global = RawObservationLimits::new(32, usize::MAX, usize::MAX).expect("global limits");
        ReconciliationAdmissionLimits::new(
            max_lanes,
            per_lane,
            global,
            max_in_flight,
            8,
            max_retained_uncertainties,
        )
        .expect("admission limits")
    }

    fn checkpoint(
        source_id: &SourceId,
        root_identity: &str,
        source_lifecycle_generation: u64,
        backend_device: u64,
        replay_start_event_id: u64,
        replay_end_event_id: u64,
    ) -> String {
        serde_json::json!({
            "root_identity": root_identity,
            "event_id": replay_end_event_id,
            "format_version": 3,
            "source_id": source_id.as_str(),
            "lifecycle_generation": source_lifecycle_generation,
            "source_revision": 12,
            "cause": "targeted_replay",
            "continuity_proof": {
                "root_identity": root_identity,
                "backend": "fsevents",
                "backend_device": backend_device,
                "watcher_generation": 41,
                "replay_coverage_start_event_id": replay_start_event_id,
                "replay_coverage_end_event_id": replay_end_event_id,
                "acknowledged_end_event_id": replay_end_event_id
            }
        })
        .to_string()
    }

    fn replay_observation() -> RawObservation {
        RawObservation::new(
            RawEventKind::Create,
            vec![RawObservedPath::new(
                PathBuf::from("replayed.wav"),
                RawPathRole::Subject,
            )],
        )
    }

    fn live_batch(
        source_id: &SourceId,
        root_identity: &RootIdentity,
        generation: WatcherGeneration,
        captured_at: u64,
    ) -> SyntheticObservationBatch {
        SyntheticObservationBatch::new(
            RawObservationProvenance::new(
                source_id.clone(),
                Some(root_identity.clone()),
                Some(BackendStreamIdentity::from_bytes(b"live-stream".to_vec())),
                generation,
                CaptureBoundary::try_new(captured_at, None, None).expect("capture boundary"),
            ),
            vec![RawObservation::new(
                RawEventKind::Create,
                vec![RawObservedPath::new(
                    PathBuf::from("live.wav"),
                    RawPathRole::Subject,
                )],
            )],
            RawObservationLimits::new(8, usize::MAX, usize::MAX).expect("live limits"),
        )
    }

    fn admit_unproven_replay(
        checkpoint_value: &str,
        source_lifecycle_generation: u64,
        replay_stream_generation: u64,
        active_replay_stream_generation: u64,
        backend_device: u64,
        replay_start_event_id: u64,
        replay_end_event_id: u64,
    ) -> (AdapterDisposition, bool, bool) {
        let directory = tempdir().expect("source root");
        let source = source(
            "replay-source",
            directory.path().to_str().expect("source path"),
        );
        let database =
            SourceDatabase::open_for_source_write(directory.path()).expect("source database");
        database
            .set_metadata(META_SOURCE_WATCHER_CHECKPOINT, checkpoint_value)
            .expect("checkpoint metadata");

        let mut lifecycle = AdmissionLifecycle::with_limits(limits(1, 1, 8));
        lifecycle
            .reconcile(
                std::slice::from_ref(&source),
                &HashMap::from([(source.root.clone(), Some("identity-a".to_string()))]),
            )
            .expect("lifecycle binding");
        let admission = lifecycle
            .admit_fsevents_replay(
                &source,
                &database,
                active_replay_stream_generation,
                FseventsReplayEvidence {
                    source_lifecycle_generation: WatcherGeneration::new(
                        source_lifecycle_generation,
                    ),
                    replay_stream_generation,
                    backend_device,
                    replay_start_event_id,
                    replay_end_event_id,
                    observations: vec![replay_observation()],
                    limits: RawObservationLimits::new(8, usize::MAX, usize::MAX)
                        .expect("replay limits"),
                },
            )
            .expect("replay admission");
        let disposition = admission.admission().disposition();
        let has_audit_request = admission.audit_request().is_some();
        let ticket = match admission.admission().outcome() {
            AdmissionOutcome::Accepted(ticket) => *ticket,
            outcome => panic!("expected accepted unproven replay, got {outcome:?}"),
        };
        let dispatched = lifecycle.dispatch_next().expect("unproven replay dispatch");
        assert_eq!(dispatched.ticket(), ticket);
        assert_eq!(dispatched.normalized().proof(), &Proof::Unproven);
        lifecycle
            .mark_dispatched(ticket)
            .expect("unproven replay dispatched");
        lifecycle
            .mark_applied(ticket)
            .expect("unproven replay applied");
        lifecycle
            .mark_unproven_audit_handed_off(ticket)
            .expect("unproven replay audit handed off");
        (
            disposition,
            has_audit_request,
            !lifecycle.retained_uncertainties().is_empty(),
        )
    }

    #[test]
    fn fsevents_replay_admission_binds_distinct_lifecycle_generations() {
        let directory = tempdir().expect("source root");
        let source = source(
            "replay-source",
            directory.path().to_str().expect("source path"),
        );
        let database =
            SourceDatabase::open_for_source_write(directory.path()).expect("source database");
        database
            .set_metadata(
                META_SOURCE_WATCHER_CHECKPOINT,
                &checkpoint(&source.id, "identity-a", 7, 99, 7, 17),
            )
            .expect("checkpoint metadata");

        let mut lifecycle = AdmissionLifecycle::with_limits(limits(1, 1, 8));
        lifecycle
            .reconcile(
                std::slice::from_ref(&source),
                &HashMap::from([(source.root.clone(), Some("identity-a".to_string()))]),
            )
            .expect("lifecycle binding");
        let owner_lane = lifecycle.lane(&source.id).expect("owner lane");
        assert_ne!(owner_lane.generation(), WatcherGeneration::new(7));
        assert_ne!(owner_lane.generation().get(), 42);

        let admission = lifecycle
            .admit_fsevents_replay(
                &source,
                &database,
                42,
                FseventsReplayEvidence {
                    source_lifecycle_generation: WatcherGeneration::new(7),
                    replay_stream_generation: 42,
                    backend_device: 99,
                    replay_start_event_id: 17,
                    replay_end_event_id: 18,
                    observations: vec![replay_observation()],
                    limits: RawObservationLimits::new(8, usize::MAX, usize::MAX)
                        .expect("replay limits"),
                },
            )
            .expect("valid replay admission");

        assert_eq!(
            admission.admission().disposition(),
            AdapterDisposition::AdmittedWithContinuity
        );
        assert!(admission.audit_request().is_none());
        let ticket = match admission.admission().outcome() {
            AdmissionOutcome::Accepted(ticket) => *ticket,
            outcome => panic!("expected accepted replay, got {outcome:?}"),
        };
        let dispatched = lifecycle.dispatch_next().expect("replay dispatch");
        assert_eq!(dispatched.ticket(), ticket);
        assert_eq!(dispatched.generation(), owner_lane.generation());
        assert_eq!(
            dispatched
                .normalized()
                .envelope()
                .provenance()
                .watcher_generation(),
            owner_lane.generation()
        );
        let proof = dispatched
            .normalized()
            .proof()
            .watcher_continuity()
            .expect("continuity proof");
        assert_eq!(
            proof.backend_stream_identity(),
            &BackendStreamIdentity::from_fsevents_device(99).expect("FSEvents stream")
        );
    }

    #[test]
    fn stale_nonzero_replay_worker_generation_remains_a_bounded_audit() {
        let source_id = SourceId::from_string("replay-source");
        let checkpoint_value = checkpoint(&source_id, "identity-a", 7, 99, 7, 17);
        let (disposition, has_audit_request, retained_uncertainty) =
            admit_unproven_replay(&checkpoint_value, 7, 41, 42, 99, 17, 18);

        assert!(matches!(
            disposition,
            AdapterDisposition::AdmittedUnproven | AdapterDisposition::SourceAuditRequired
        ));
        assert!(has_audit_request);
        assert!(retained_uncertainty);
    }

    #[test]
    fn malformed_native_replay_fields_remain_bounded_audits() {
        let source_id = SourceId::from_string("replay-source");
        let checkpoint_value = checkpoint(&source_id, "identity-a", 7, 99, 7, 17);
        for (replay_stream_generation, active_generation, backend_device, start, end) in [
            (42, 42, 0, 17, 18),
            (42, 42, 99, 18, 18),
            (0, 42, 99, 17, 18),
            (42, 0, 99, 17, 18),
        ] {
            let (disposition, has_audit_request, retained_uncertainty) = admit_unproven_replay(
                &checkpoint_value,
                7,
                replay_stream_generation,
                active_generation,
                backend_device,
                start,
                end,
            );
            assert!(matches!(
                disposition,
                AdapterDisposition::AdmittedUnproven | AdapterDisposition::SourceAuditRequired
            ));
            assert!(has_audit_request);
            assert!(retained_uncertainty);
        }
    }

    #[test]
    fn stale_malformed_root_device_and_gapped_checkpoints_remain_bounded_audits() {
        let source_id = SourceId::from_string("replay-source");
        let cases = [
            checkpoint(&source_id, "identity-a", 6, 99, 7, 17),
            checkpoint(&source_id, "identity-b", 7, 99, 7, 17),
            checkpoint(&source_id, "identity-a", 7, 99, 7, 17),
            checkpoint(&source_id, "identity-a", 7, 99, 7, 17),
            String::from("not-json"),
        ];
        let evidence = [
            (7, 99, 17, 18),
            (7, 99, 17, 18),
            (7, 100, 17, 18),
            (7, 99, 18, 19),
            (7, 99, 17, 18),
        ];

        for (checkpoint_value, (source_lifecycle_generation, backend_device, start, end)) in
            cases.iter().zip(evidence)
        {
            let (disposition, has_audit_request, retained_uncertainty) = admit_unproven_replay(
                checkpoint_value,
                source_lifecycle_generation,
                42,
                42,
                backend_device,
                start,
                end,
            );
            assert!(matches!(
                disposition,
                AdapterDisposition::AdmittedUnproven | AdapterDisposition::SourceAuditRequired
            ));
            assert!(has_audit_request);
            assert!(retained_uncertainty);
        }
    }

    #[test]
    fn startup_binds_only_identity_qualified_watched_roots() {
        let source = source("startup", "root-a");
        let mut lifecycle = AdmissionLifecycle::with_limits(limits(2, 2, 8));

        lifecycle
            .reconcile(
                std::slice::from_ref(&source),
                &watched(&[("root-a", Some(b"identity-a"))]),
            )
            .expect("startup binding");

        let lane = lifecycle.lane(&source.id).expect("startup lane");
        assert_eq!(lane.root_identity(), &root(b"identity-a"));
        assert_eq!(lane.generation().get(), 1);
        assert_eq!(lifecycle.owner.supervisor().in_flight(), 0);
    }

    #[test]
    fn native_limits_allow_more_than_256_identity_qualified_sources() {
        let limits = native_admission_limits();
        assert_eq!(limits.max_lanes(), usize::MAX);
        assert_eq!(limits.max_in_flight(), NATIVE_ADMISSION_MAX_IN_FLIGHT);

        let mut sources = Vec::with_capacity(257);
        let mut watched_roots = HashMap::with_capacity(257);
        for index in 0..257 {
            let source = source(&format!("native-{index}"), &format!("root-{index}"));
            watched_roots.insert(source.root.clone(), Some(format!("identity-{index}")));
            sources.push(source);
        }

        let mut lifecycle = AdmissionLifecycle::with_limits(limits);
        lifecycle
            .reconcile(&sources, &watched_roots)
            .expect("native lifecycle lane registry");

        assert_eq!(lifecycle.owner.source_ids().count(), 257);
        assert!(sources.iter().all(|source| {
            lifecycle
                .lane(&source.id)
                .is_some_and(|lane| lane.lifecycle() == ReconciliationLifecycle::Capturing)
        }));
        assert_eq!(lifecycle.owner.supervisor().in_flight(), 0);
    }

    #[test]
    fn repeated_same_sources_are_a_no_op_for_lane_and_generation() {
        let source = source("repeat", "root-a");
        let sources = [source.clone()];
        let watched = watched(&[("root-a", Some(b"identity-a"))]);
        let mut lifecycle = AdmissionLifecycle::with_limits(limits(2, 2, 8));

        lifecycle
            .reconcile(&sources, &watched)
            .expect("first binding");
        let first = lifecycle.lane(&source.id).expect("first lane");
        lifecycle
            .reconcile(&sources, &watched)
            .expect("repeat binding");

        assert_eq!(lifecycle.lane(&source.id).expect("repeated lane"), first);
        assert_eq!(lifecycle.owner.supervisor().in_flight(), 0);
    }

    #[test]
    fn source_add_and_remove_are_independent_and_removal_retains_uncertainty() {
        let first = source("first", "root-a");
        let second = source("second", "root-b");
        let mut lifecycle = AdmissionLifecycle::with_limits(limits(2, 2, 8));
        lifecycle
            .reconcile(
                std::slice::from_ref(&first),
                &watched(&[("root-a", Some(b"identity-a"))]),
            )
            .expect("first binding");

        lifecycle
            .reconcile(
                &[first.clone(), second.clone()],
                &watched(&[
                    ("root-a", Some(b"identity-a")),
                    ("root-b", Some(b"identity-b")),
                ]),
            )
            .expect("independent addition");

        assert_eq!(
            lifecycle.lane(&first.id).expect("first lane").lifecycle(),
            ReconciliationLifecycle::Capturing
        );
        assert_eq!(
            lifecycle.lane(&second.id).expect("second lane").lifecycle(),
            ReconciliationLifecycle::Capturing
        );

        lifecycle
            .reconcile(
                std::slice::from_ref(&second),
                &watched(&[("root-b", Some(b"identity-b"))]),
            )
            .expect("independent removal");

        assert!(lifecycle.lane(&first.id).is_none());
        assert_eq!(
            lifecycle.lane(&second.id).expect("second lane").lifecycle(),
            ReconciliationLifecycle::Capturing
        );
        assert!(
            !lifecycle
                .owner
                .supervisor()
                .retained_uncertainties()
                .is_empty()
        );
        assert_eq!(lifecycle.owner.supervisor().in_flight(), 0);
    }

    #[test]
    fn source_local_fence_preserves_separate_lane_and_ticket() {
        let first = source("local-fence-first", "root-a");
        let second = source("local-fence-second", "root-b");
        let sources = [first.clone(), second.clone()];
        let mut lifecycle = AdmissionLifecycle::with_limits(limits(2, 2, 8));
        let watched = watched(&[
            ("root-a", Some(b"identity-a")),
            ("root-b", Some(b"identity-b")),
        ]);
        lifecycle
            .reconcile(&sources, &watched)
            .expect("source lanes");
        let first_lane = lifecycle.lane(&first.id).expect("first lane");
        let second_lane = lifecycle.lane(&second.id).expect("second lane");

        let first_ticket = match lifecycle
            .admit_live_with_correlation(live_batch(
                &first.id,
                first_lane.root_identity(),
                first_lane.generation(),
                1,
            ))
            .expect("first live admission")
            .admission()
            .outcome()
        {
            AdmissionOutcome::Accepted(ticket) => *ticket,
            outcome => panic!("unexpected first outcome: {outcome:?}"),
        };
        let second_ticket = match lifecycle
            .admit_live_with_correlation(live_batch(
                &second.id,
                second_lane.root_identity(),
                second_lane.generation(),
                2,
            ))
            .expect("second live admission")
            .admission()
            .outcome()
        {
            AdmissionOutcome::Accepted(ticket) => *ticket,
            outcome => panic!("unexpected second outcome: {outcome:?}"),
        };
        assert_ne!(first_ticket, second_ticket);
        assert_eq!(lifecycle.in_flight(), 2);

        assert!(
            lifecycle
                .fence_source_if_current(
                    &first.id,
                    first_lane.root_identity(),
                    first_lane.generation(),
                )
                .expect("source-local fence")
        );
        assert_eq!(lifecycle.in_flight(), 1);
        assert_eq!(
            lifecycle.lane(&second.id).expect("second lane remains"),
            second_lane
        );

        let dispatched = lifecycle
            .dispatch_next()
            .expect("second ticket remains queued");
        assert_eq!(dispatched.ticket(), second_ticket);
        lifecycle
            .mark_dispatched(second_ticket)
            .expect("second ticket dispatched");
        lifecycle
            .mark_applied(second_ticket)
            .expect("second ticket applied");
        assert_eq!(lifecycle.in_flight(), 1);

        lifecycle
            .reconcile(&sources, &watched)
            .expect("restart first lane");
        let restarted_first = lifecycle.lane(&first.id).expect("restarted first lane");
        assert!(restarted_first.generation() > first_lane.generation());
        assert!(
            !lifecycle
                .fence_source_if_current(
                    &first.id,
                    first_lane.root_identity(),
                    first_lane.generation(),
                )
                .expect("stale source-local fence")
        );
        assert_eq!(
            lifecycle.lane(&first.id).expect("current first lane"),
            restarted_first
        );
    }

    #[test]
    fn same_id_root_rebind_stops_old_generation_then_begins_new_root() {
        let initial_source = source("rebind", "root-a");
        let mut lifecycle = AdmissionLifecycle::with_limits(limits(1, 1, 16));
        lifecycle
            .reconcile(
                std::slice::from_ref(&initial_source),
                &watched(&[("root-a", Some(b"identity-a"))]),
            )
            .expect("old binding");
        let old = lifecycle.lane(&initial_source.id).expect("old lane");

        let replacement = source("rebind", "root-b");
        lifecycle
            .reconcile(
                std::slice::from_ref(&replacement),
                &watched(&[("root-b", Some(b"identity-b"))]),
            )
            .expect("rebound binding");
        let new = lifecycle.lane(&replacement.id).expect("new lane");

        assert_eq!(new.root_identity(), &root(b"identity-b"));
        assert!(new.generation() > old.generation());
        assert_eq!(new.lifecycle(), ReconciliationLifecycle::Capturing);
        assert_eq!(lifecycle.owner.supervisor().in_flight(), 0);
    }

    #[test]
    fn audit_request_matches_only_the_current_capturing_root_and_generation() {
        let initial_source = source("request-fence", "root-a");
        let mut lifecycle = AdmissionLifecycle::with_limits(limits(1, 1, 16));
        lifecycle
            .reconcile(
                std::slice::from_ref(&initial_source),
                &watched(&[("root-a", Some(b"identity-a"))]),
            )
            .expect("old binding");
        let old_request = lifecycle
            .source_audit_request_for_current_lane(&initial_source.id)
            .expect("old lane request")
            .0;
        assert!(lifecycle.request_matches_current_capturing_lane(&old_request));

        let replacement = source("request-fence", "root-b");
        lifecycle
            .reconcile(
                std::slice::from_ref(&replacement),
                &watched(&[("root-b", Some(b"identity-b"))]),
            )
            .expect("root rebind");
        let rebound_request = lifecycle
            .source_audit_request_for_current_lane(&replacement.id)
            .expect("rebound lane request")
            .0;
        assert!(!lifecycle.request_matches_current_capturing_lane(&old_request));
        assert!(lifecycle.request_matches_current_capturing_lane(&rebound_request));
        assert!(rebound_request.generation() > old_request.generation());

        lifecycle.fence_all().expect("stop current lane");
        assert!(!lifecycle.request_matches_current_capturing_lane(&rebound_request));
        lifecycle
            .reconcile(
                std::slice::from_ref(&replacement),
                &watched(&[("root-b", Some(b"identity-b"))]),
            )
            .expect("restart current lane");
        let restarted_request = lifecycle
            .source_audit_request_for_current_lane(&replacement.id)
            .expect("restarted lane request")
            .0;
        assert!(!lifecycle.request_matches_current_capturing_lane(&rebound_request));
        assert!(lifecycle.request_matches_current_capturing_lane(&restarted_request));
        assert!(restarted_request.generation() > rebound_request.generation());
    }

    #[test]
    fn same_root_source_id_replacement_removes_old_lane_and_begins_new_one() {
        let old_source = source("old-id", "root-a");
        let new_source = source("new-id", "root-a");
        let mut lifecycle = AdmissionLifecycle::with_limits(limits(2, 2, 8));
        lifecycle
            .reconcile(
                std::slice::from_ref(&old_source),
                &watched(&[("root-a", Some(b"identity-a"))]),
            )
            .expect("old binding");

        lifecycle
            .reconcile(
                std::slice::from_ref(&new_source),
                &watched(&[("root-a", Some(b"identity-a"))]),
            )
            .expect("source-id replacement");

        assert!(lifecycle.lane(&old_source.id).is_none());
        assert_eq!(
            lifecycle
                .lane(&new_source.id)
                .expect("new source lane")
                .lifecycle(),
            ReconciliationLifecycle::Capturing
        );
        assert_eq!(
            lifecycle.owner.source_ids().collect::<Vec<_>>(),
            vec![&new_source.id]
        );
    }

    #[test]
    fn restart_same_source_and_root_allocates_a_new_generation() {
        let source = source("restart", "root-a");
        let watched = watched(&[("root-a", Some(b"identity-a"))]);
        let mut lifecycle = AdmissionLifecycle::with_limits(limits(1, 1, 8));
        lifecycle
            .reconcile(std::slice::from_ref(&source), &watched)
            .expect("initial binding");
        let first = lifecycle.lane(&source.id).expect("initial lane");

        lifecycle.fence_all().expect("fence restart");
        lifecycle
            .reconcile(std::slice::from_ref(&source), &watched)
            .expect("restart binding");
        let restarted = lifecycle.lane(&source.id).expect("restarted lane");

        assert_eq!(restarted.root_identity(), first.root_identity());
        assert!(restarted.generation() > first.generation());
        assert_eq!(restarted.lifecycle(), ReconciliationLifecycle::Capturing);
    }

    #[test]
    fn unavailable_identity_never_registers_a_lane_and_stops_existing_capture() {
        let source = source("unavailable", "root-a");
        let mut lifecycle = AdmissionLifecycle::with_limits(limits(1, 1, 8));
        lifecycle
            .reconcile(
                std::slice::from_ref(&source),
                &watched(&[("root-a", Some(b"identity-a"))]),
            )
            .expect("initial binding");

        lifecycle
            .reconcile(std::slice::from_ref(&source), &watched(&[("root-a", None)]))
            .expect("unavailable identity");

        assert_eq!(
            lifecycle
                .lane(&source.id)
                .expect("retained lane")
                .lifecycle(),
            ReconciliationLifecycle::Stopped
        );
        assert_eq!(lifecycle.owner.supervisor().in_flight(), 0);
    }

    #[test]
    fn partial_watch_cleanup_fences_successfully_bound_roots_without_synthesizing_missing_ones() {
        let first = source("partial-first", "root-a");
        let second = source("partial-second", "root-b");
        let sources = [first.clone(), second.clone()];
        let mut lifecycle = AdmissionLifecycle::with_limits(limits(2, 2, 8));

        lifecycle
            .reconcile(
                &sources,
                &watched(&[("root-a", Some(b"identity-a")), ("root-b", None)]),
            )
            .expect("partial binding");
        assert_eq!(
            lifecycle.lane(&first.id).expect("watched lane").lifecycle(),
            ReconciliationLifecycle::Capturing
        );
        assert!(lifecycle.lane(&second.id).is_none());

        lifecycle.fence_all().expect("partial cleanup");
        assert_eq!(
            lifecycle.lane(&first.id).expect("fenced lane").lifecycle(),
            ReconciliationLifecycle::Stopped
        );
        assert_eq!(lifecycle.owner.supervisor().in_flight(), 0);
    }

    #[test]
    fn capacity_failure_keeps_owner_state_and_fails_closed() {
        let first = source("capacity-first", "root-a");
        let second = source("capacity-second", "root-b");
        let mut lifecycle = AdmissionLifecycle::with_limits(limits(1, 1, 8));
        lifecycle
            .reconcile(
                std::slice::from_ref(&first),
                &watched(&[("root-a", Some(b"identity-a"))]),
            )
            .expect("initial capacity binding");

        let error = lifecycle
            .reconcile(
                &[first.clone(), second.clone()],
                &watched(&[
                    ("root-a", Some(b"identity-a")),
                    ("root-b", Some(b"identity-b")),
                ]),
            )
            .expect_err("second lane must fail closed at capacity");

        assert_eq!(
            error,
            AdmissionOwnerError::Supervisor(
                wavecrate_library::sample_sources::reconciliation::AdmissionError::LaneLimitReached
            )
        );
        assert_eq!(
            lifecycle
                .lane(&first.id)
                .expect("owner state retained")
                .lifecycle(),
            ReconciliationLifecycle::Capturing
        );
        assert!(lifecycle.lane(&second.id).is_none());
        lifecycle.fence_all().expect("failed lifecycle cleanup");
        assert_eq!(lifecycle.owner.supervisor().in_flight(), 0);
    }

    #[test]
    fn retained_uncertainty_fence_failure_blocks_later_reconcile() {
        let initial = source("fence-capacity", "root-a");
        let replacement = source("fence-capacity", "root-b");
        let mut lifecycle = AdmissionLifecycle::with_limits(limits(1, 1, 2));

        lifecycle
            .reconcile(
                std::slice::from_ref(&initial),
                &watched(&[("root-a", Some(b"identity-a"))]),
            )
            .expect("initial binding");
        lifecycle
            .reconcile(
                std::slice::from_ref(&replacement),
                &watched(&[("root-b", Some(b"identity-b"))]),
            )
            .expect("root retirement and replacement binding");

        let retained_uncertainties = lifecycle
            .owner
            .supervisor()
            .retained_uncertainties()
            .to_vec();
        assert_eq!(retained_uncertainties.len(), 2);
        let generation = lifecycle
            .lane(&replacement.id)
            .expect("replacement lane")
            .generation();
        let expected_error = AdmissionOwnerError::Supervisor(
            wavecrate_library::sample_sources::reconciliation::AdmissionError::UncertaintyCapacityExhausted,
        );
        assert_eq!(lifecycle.fence_all(), Err(expected_error));

        assert_eq!(
            lifecycle.reconcile(
                std::slice::from_ref(&replacement),
                &watched(&[("root-b", Some(b"identity-b"))]),
            ),
            Err(expected_error)
        );
        let lane = lifecycle
            .lane(&replacement.id)
            .expect("capturing lane remains authoritative");
        assert_eq!(lane.generation(), generation);
        assert_eq!(lane.lifecycle(), ReconciliationLifecycle::Capturing);
        assert_eq!(
            lifecycle.owner.supervisor().retained_uncertainties(),
            retained_uncertainties.as_slice()
        );
    }

    #[test]
    fn helper_does_not_construct_or_dispatch_observation_work() {
        let source = source("no-adapter", "root-a");
        let mut lifecycle = AdmissionLifecycle::with_limits(limits(1, 1, 8));

        lifecycle
            .reconcile(
                std::slice::from_ref(&source),
                &HashMap::from([(PathBuf::from("root-a"), Some("identity-a".to_string()))]),
            )
            .expect("lifecycle binding");

        assert_eq!(lifecycle.owner.supervisor().in_flight(), 0);
        assert!(
            lifecycle
                .owner
                .supervisor()
                .retained_uncertainties()
                .is_empty()
        );
    }
}
