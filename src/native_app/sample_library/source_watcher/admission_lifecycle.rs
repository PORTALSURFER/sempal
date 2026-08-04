//! Watcher-local lifecycle ownership for source/root admission lanes.

use std::collections::HashSet;

use wavecrate::sample_sources::{SampleSource, SourceId};
use wavecrate_library::sample_sources::reconciliation::{
    AdapterError, AdmissionOwnerError, DispatchTicket, DispatchedObservation, LiveAuditAdmission,
    OwnedAdmissionLane, RawObservationLimits, ReconciliationAcknowledgementOutcome,
    ReconciliationAdmissionLimits, ReconciliationAdmissionOwner, ReconciliationAdmissionSupervisor,
    ReconciliationLifecycle, RootIdentity, SourceAuditReceipt, SourceAuditRequest,
    SyntheticObservationBatch,
};

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

    fn reconcile_source(
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

    /// Return the owner-authoritative lane snapshot used to bind a live capture.
    pub(super) fn lane_for_capture(&self, source_id: &SourceId) -> Option<OwnedAdmissionLane> {
        if self.admission_closed {
            return None;
        }
        self.owner.lane(source_id)
    }

    /// Build the existing bounded request for the current owner-authoritative source lane.
    pub(super) fn source_audit_request_for_current_lane(
        &self,
        source_id: &SourceId,
    ) -> Option<SourceAuditRequest> {
        self.owner.source_audit_request_for_current_lane(source_id)
    }

    /// Admit live evidence through the existing owner-held adapter.
    pub(super) fn admit_live_with_correlation(
        &mut self,
        batch: SyntheticObservationBatch,
    ) -> Result<LiveAuditAdmission, AdapterError> {
        self.owner.admit_live_with_correlation(batch)
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
        self.lane_for_capture(source_id)
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
