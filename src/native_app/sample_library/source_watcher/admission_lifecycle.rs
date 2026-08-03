//! Watcher-local lifecycle ownership for source/root admission lanes.

use std::collections::HashSet;

use wavecrate::sample_sources::{SampleSource, SourceId};
use wavecrate_library::sample_sources::reconciliation::{
    AdmissionOwnerError, RawObservationLimits, ReconciliationAdmissionLimits,
    ReconciliationAdmissionOwner, ReconciliationAdmissionSupervisor, ReconciliationLifecycle,
    RootIdentity,
};

#[cfg(test)]
use wavecrate_library::sample_sources::reconciliation::OwnedAdmissionLane;

use super::roots::{WatchedRootIdentities, registered_root_identity};

/// Bounded owner capacity for the native watcher lifecycle.
///
/// This is a resource bound, not a source-count policy. If the owner reaches it, lifecycle
/// reconciliation fails closed and the owner is retained for a later retry.
const NATIVE_ADMISSION_MAX_LANES: usize = 256;
const NATIVE_ADMISSION_MAX_IN_FLIGHT: usize = NATIVE_ADMISSION_MAX_LANES;
const NATIVE_ADMISSION_MAX_RECENT_SEQUENCES_PER_LANE: usize = 64;
const NATIVE_ADMISSION_MAX_RETAINED_UNCERTAINTIES: usize = 4096;
const NATIVE_ADMISSION_MAX_EVENTS_PER_LANE: usize = 512;

/// Owns the one admission owner used by a native source-watcher coordinator.
pub(super) struct AdmissionLifecycle {
    owner: ReconciliationAdmissionOwner,
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
        }
    }

    #[cfg(test)]
    fn with_limits(limits: ReconciliationAdmissionLimits) -> Self {
        Self::from_limits(limits)
    }

    /// Stop every registered lane while retaining the owner's uncertainty markers.
    pub(super) fn fence_all(&mut self) -> Result<(), AdmissionOwnerError> {
        let mut source_ids = self.owner.source_ids().cloned().collect::<Vec<_>>();
        source_ids.sort_by(|left, right| left.as_str().cmp(right.as_str()));
        for source_id in source_ids {
            self.stop_if_capturing(&source_id)?;
        }
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
        .checked_mul(NATIVE_ADMISSION_MAX_LANES)
        .expect("native global event limit");
    let global = RawObservationLimits::new(global_events, usize::MAX, usize::MAX)
        .expect("native global admission limits");
    ReconciliationAdmissionLimits::new(
        NATIVE_ADMISSION_MAX_LANES,
        per_lane,
        global,
        NATIVE_ADMISSION_MAX_IN_FLIGHT,
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
