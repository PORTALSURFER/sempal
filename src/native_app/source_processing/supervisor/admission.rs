use super::{
    Arc, AtomicBool, CommittedSourceDelta, DatabaseWriterGate, ExternalScanAdmission,
    ExternalScanRegistration, Ordering, PathBuf, ProcessingLane, SampleSource, Shared,
    SourceDeltaQueueResult, resolve_registered_source_for_scan_locked,
};
use crate::native_app::sample_library::source_watcher::RevisionBoundCheckpoint;

/// The typed handoff an external scan must make before releasing its source-processing budget.
///
/// A committed manifest mutation is visible to the supervisor either as its exact revision-aware
/// delta or as an explicit full-reconciliation fallback. Keeping this ownership on the permit
/// makes releasing capacity and publishing the mutation one ordered operation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::native_app) enum ExternalScanHandoff {
    CommittedDelta(CommittedSourceDelta),
    FullReconciliation { reason: &'static str },
}

/// Clone-safe one-shot handoff from a worker's committed source mutation to the GUI projection.
///
/// The worker installs the source-scoped fence before releasing its scarce scan permit. The GUI
/// must resolve the ticket only after applying the exact hydrated projection. Dropping the last
/// unresolved clone takes the conservative full-reconciliation path.
#[derive(Clone)]
pub(in crate::native_app) struct ProjectionHandoffTicket {
    shared: Arc<Shared>,
    state: Arc<super::Mutex<ProjectionTicketState>>,
    source_id: String,
    lifecycle_generation: u64,
    delta: CommittedSourceDelta,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ProjectionTicketState {
    Pending,
    Accepted,
    Rejected,
}

impl std::fmt::Debug for ProjectionHandoffTicket {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ProjectionHandoffTicket")
            .field("source_id", &self.source_id)
            .field("lifecycle_generation", &self.lifecycle_generation)
            .field("revision", &self.delta.revision)
            .finish()
    }
}

impl PartialEq for ProjectionHandoffTicket {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.state, &other.state)
    }
}

impl Eq for ProjectionHandoffTicket {}

impl ProjectionHandoffTicket {
    fn new(
        shared: Arc<Shared>,
        source_id: String,
        lifecycle_generation: u64,
        delta: CommittedSourceDelta,
    ) -> Self {
        Self {
            shared,
            state: Arc::new(super::Mutex::new(ProjectionTicketState::Pending)),
            source_id,
            lifecycle_generation,
            delta,
        }
    }

    /// Accept the handoff after the GUI has applied the exact projection.
    ///
    /// `false` means the ticket was stale, invalid, rejected by the supervisor, or resolved more
    /// than once. Every false outcome is conservative: it requests complete source
    /// reconciliation and never publishes a targeted readiness delta.
    pub(in crate::native_app) fn accept(&self) -> bool {
        if !self.claim_resolution(ProjectionTicketState::Accepted) {
            self.request_full_reconciliation("projection_handoff_duplicate_resolution");
            return false;
        }
        let mut control = self.shared.control();
        let fence_matches = control
            .pending_projection_fences
            .get(&self.source_id)
            .is_some_and(|fence| {
                fence.lifecycle_generation == self.lifecycle_generation
                    && fence.revision == self.delta.revision
            });
        let current = control.source_is_active(&self.source_id)
            && control.source_lifecycle_generations.get(&self.source_id)
                == Some(&self.lifecycle_generation);
        if !current || !fence_matches {
            drop(control);
            self.request_full_reconciliation("projection_handoff_stale_or_invalid");
            return false;
        }
        let fence_matches = control
            .pending_projection_fences
            .get(&self.source_id)
            .is_some_and(|fence| {
                fence.lifecycle_generation == self.lifecycle_generation
                    && fence.revision == self.delta.revision
            });
        if fence_matches {
            control.pending_projection_fences.remove(&self.source_id);
        }
        let accepted = self.delta.is_empty()
            || matches!(
                control.queue_source_delta(
                    &self.source_id,
                    self.lifecycle_generation,
                    &self.delta,
                    "projection_handoff_accepted",
                ),
                SourceDeltaQueueResult::Queued
            );
        if !accepted {
            control.pending_readiness_deltas.remove(&self.source_id);
            control.cancel_source_work(&self.source_id);
            control.mark_source_dirty(&self.source_id, "projection_handoff_delta_rejected");
        }
        drop(control);
        self.shared.wake.notify_one();
        accepted
    }

    /// Reject the handoff after projection absence, hydration failure, cancellation, or an
    /// unsuccessful exact-revision apply.
    pub(in crate::native_app) fn reject(&self, reason: &'static str) {
        if !self.claim_resolution(ProjectionTicketState::Rejected) {
            self.request_full_reconciliation("projection_handoff_duplicate_rejection");
            return;
        }
        self.request_full_reconciliation(reason);
    }

    fn claim_resolution(&self, resolution: ProjectionTicketState) -> bool {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        if *state != ProjectionTicketState::Pending {
            return false;
        }
        *state = resolution;
        true
    }

    fn request_full_reconciliation(&self, reason: &'static str) {
        let mut control = self.shared.control();
        let fence_matches = control
            .pending_projection_fences
            .get(&self.source_id)
            .is_some_and(|fence| {
                fence.lifecycle_generation == self.lifecycle_generation
                    && fence.revision == self.delta.revision
            });
        if fence_matches {
            control.pending_projection_fences.remove(&self.source_id);
            control.pending_readiness_deltas.remove(&self.source_id);
        }
        if control.source_is_active(&self.source_id)
            && control.source_lifecycle_generations.get(&self.source_id)
                == Some(&self.lifecycle_generation)
        {
            control.cancel_source_work(&self.source_id);
            control.mark_source_dirty(&self.source_id, reason);
        }
        drop(control);
        self.shared.wake.notify_one();
    }
}

impl Drop for ProjectionHandoffTicket {
    fn drop(&mut self) {
        if Arc::strong_count(&self.state) != 1 {
            return;
        }
        let pending = *self
            .state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            == ProjectionTicketState::Pending;
        if pending {
            self.reject("projection_handoff_dropped_unresolved");
        }
    }
}

#[derive(Clone)]
pub(in crate::native_app) struct SourceProcessingBudgetHandle {
    pub(super) shared: Arc<Shared>,
}

pub(in crate::native_app) struct SourceProcessingBudgetPermit {
    shared: Arc<Shared>,
    pub(super) permit: Option<super::super::scheduler::BudgetPermit>,
    registration_id: u64,
    pub(super) lifecycle_generation: u64,
    pub(super) cancel: Arc<AtomicBool>,
    handoff_registered: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::native_app) enum SourceScanAdmissionState {
    WaitingForSourceActivation,
    WaitingForCapacity { current_owner: Option<String> },
    Admitted,
}

impl SourceProcessingBudgetHandle {
    /// Enqueue a typed watcher checkpoint for the source-processing coordinator.
    ///
    /// Submission is deliberately limited to the in-memory queue and coordinator wakeup. The
    /// caller never opens SQLite, inspects filesystem metadata, or waits on the database writer
    /// gate.
    pub(in crate::native_app) fn submit_watcher_checkpoint(
        &self,
        request: RevisionBoundCheckpoint,
    ) {
        let mut control = self.shared.control();
        control.pending_watcher_checkpoints.push_back(request);
        control.notify("watcher_checkpoint_submitted");
        drop(control);
        self.shared.wake.notify_one();
    }

    #[cfg(test)]
    pub(in crate::native_app) fn pending_watcher_checkpoint_for_tests(
        &self,
    ) -> Option<RevisionBoundCheckpoint> {
        self.shared
            .control()
            .pending_watcher_checkpoints
            .front()
            .cloned()
    }

    #[cfg(test)]
    pub(in crate::native_app) fn lifecycle_generation(&self, source_id: &str) -> Option<u64> {
        self.shared
            .control()
            .source_lifecycle_generations
            .get(source_id)
            .copied()
    }

    pub(in crate::native_app) fn acquire_scan_for_generation(
        &self,
        source_id: &str,
        expected_lifecycle_generation: u64,
    ) -> Option<SourceProcessingBudgetPermit> {
        self.acquire_scan_for_generation_with_state(
            source_id,
            expected_lifecycle_generation,
            |_| {},
        )
    }

    pub(in crate::native_app) fn acquire_scan_for_generation_with_state(
        &self,
        source_id: &str,
        expected_lifecycle_generation: u64,
        mut publish_state: impl FnMut(SourceScanAdmissionState),
    ) -> Option<SourceProcessingBudgetPermit> {
        {
            let mut control = self.shared.control();
            let mut waiting_published = false;
            while !control.shutdown
                && control.source_is_configured(source_id)
                && control.source_lifecycle_generations.get(source_id)
                    == Some(&expected_lifecycle_generation)
                && !control.source_is_active(source_id)
            {
                if !waiting_published {
                    publish_state(SourceScanAdmissionState::WaitingForSourceActivation);
                    waiting_published = true;
                }
                control = self
                    .shared
                    .wake
                    .wait(control)
                    .unwrap_or_else(|poison| poison.into_inner());
            }
            if control.shutdown
                || self.shared.cancel.load(Ordering::Acquire)
                || !control.source_is_active(source_id)
                || control.source_lifecycle_generations.get(source_id)
                    != Some(&expected_lifecycle_generation)
            {
                return None;
            }
        }
        // Publish the admission while holding the budget lock so the coordinator cannot start
        // another candidate between observing capacity and observing the external scan. Existing
        // source work is allowed to finish; watcher and UI scans must never cancel the active
        // source merely to acquire the lane sooner.
        let budgets = self.shared.budgets();
        let (admission_id, admission_cancel, lifecycle_generation) = {
            let control = self.shared.control();
            if control.shutdown
                || self.shared.cancel.load(Ordering::Acquire)
                || !control.source_is_active(source_id)
                || control.source_lifecycle_generations.get(source_id)
                    != Some(&expected_lifecycle_generation)
            {
                return None;
            }
            let admission_cancel = Arc::clone(&control.source_work_cancels[source_id]);
            let lifecycle_generation = expected_lifecycle_generation;
            if admission_cancel.load(Ordering::Acquire) {
                return None;
            }
            let admission_id = self
                .shared
                .next_external_scan_id
                .fetch_add(1, Ordering::Relaxed);
            let mut external_scans = self.shared.external_scans();
            external_scans.admissions.insert(
                admission_id,
                ExternalScanAdmission {
                    source_id: source_id.to_string(),
                    lifecycle_generation,
                },
            );
            (admission_id, admission_cancel, lifecycle_generation)
        };
        drop(budgets);
        self.shared.wake.notify_one();
        let mut capacity_wait_published = false;
        loop {
            let mut budgets = self.shared.budgets();
            if let Some(permit) = budgets.try_acquire(source_id, ProcessingLane::Scan) {
                drop(budgets);
                let cancel = Arc::new(AtomicBool::new(false));
                let control = self.shared.control();
                let mut external_scans = self.shared.external_scans();
                external_scans.admissions.remove(&admission_id);
                if control.shutdown
                    || self.shared.cancel.load(Ordering::Acquire)
                    || !control.source_is_active(source_id)
                    || control.source_lifecycle_generations.get(source_id)
                        != Some(&lifecycle_generation)
                    || admission_cancel.load(Ordering::Acquire)
                {
                    drop(external_scans);
                    drop(control);
                    self.shared.external_scan_wake.notify_all();
                    self.shared.budgets().release(permit);
                    self.shared.budget_wake.notify_all();
                    return None;
                }
                external_scans.registrations.insert(
                    admission_id,
                    ExternalScanRegistration {
                        source_id: source_id.to_string(),
                        lifecycle_generation,
                        cancel: Arc::clone(&cancel),
                    },
                );
                drop(external_scans);
                drop(control);
                self.shared.external_scan_wake.notify_all();
                let permit = SourceProcessingBudgetPermit {
                    shared: Arc::clone(&self.shared),
                    permit: Some(permit),
                    registration_id: admission_id,
                    lifecycle_generation,
                    cancel,
                    handoff_registered: false,
                };
                if permit.should_cancel_now() {
                    permit.cancel.store(true, Ordering::Release);
                }
                publish_state(SourceScanAdmissionState::Admitted);
                return Some(permit);
            }
            if !capacity_wait_published {
                let current_owner = budgets
                    .active_sources()
                    .into_iter()
                    .find(|active_source| active_source != source_id);
                publish_state(SourceScanAdmissionState::WaitingForCapacity { current_owner });
                capacity_wait_published = true;
            }
            drop(
                self.shared
                    .budget_wake
                    .wait(budgets)
                    .unwrap_or_else(|poison| poison.into_inner()),
            );
            let control = self.shared.control();
            let unavailable = control.shutdown
                || self.shared.cancel.load(Ordering::Acquire)
                || !control.source_is_active(source_id)
                || control.source_lifecycle_generations.get(source_id)
                    != Some(&lifecycle_generation)
                || admission_cancel.load(Ordering::Acquire);
            drop(control);
            if unavailable {
                self.shared.finish_external_scan_admission(admission_id);
                return None;
            }
        }
    }

    /// Wait for source replacement to finish, then resolve the descriptor
    /// against the authoritative configured set.
    ///
    /// This is intentionally exposed on the background-only budget handle:
    /// source replacement briefly fences retirement admission and publication,
    /// so callers on the UI thread must use the supervisor's non-blocking
    /// registration method instead.
    pub(in crate::native_app) fn register_source_for_scan_waiting(
        &self,
        source: SampleSource,
    ) -> Result<u64, String> {
        let _replacement = self
            .shared
            .source_replacement
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        resolve_registered_source_for_scan_locked(self.shared.as_ref(), &source)
    }

    #[cfg(test)]
    pub(in crate::native_app) fn acquire_scan(
        &self,
        source_id: &str,
    ) -> Option<SourceProcessingBudgetPermit> {
        let lifecycle_generation = self.lifecycle_generation(source_id)?;
        self.acquire_scan_for_generation(source_id, lifecycle_generation)
    }
}

impl SourceProcessingBudgetPermit {
    pub(in crate::native_app) fn cancel_token(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.cancel)
    }

    pub(in crate::native_app) fn lifecycle_generation(&self) -> u64 {
        self.lifecycle_generation
    }

    pub(in crate::native_app) fn scan_writer(&self) -> DatabaseWriterGate {
        self.shared.database_writer.clone()
    }

    /// Install the source-scoped projection fence and release the scarce worker capacity before
    /// handing the exact committed delta to the GUI.
    pub(in crate::native_app) fn release_after_projection_handoff(
        mut self,
        delta: CommittedSourceDelta,
    ) -> ProjectionHandoffTicket {
        let source_id = self
            .permit
            .as_ref()
            .map(|permit| permit.source_id().to_string())
            .unwrap_or_default();
        let ticket = ProjectionHandoffTicket::new(
            Arc::clone(&self.shared),
            source_id.clone(),
            self.lifecycle_generation,
            delta,
        );
        let mut control = self.shared.control();
        let current = control.source_is_active(&source_id)
            && control.source_lifecycle_generations.get(&source_id)
                == Some(&self.lifecycle_generation);
        if current && !control.pending_projection_fences.contains_key(&source_id) {
            control.pending_projection_fences.insert(
                source_id,
                super::PendingProjectionFence {
                    lifecycle_generation: self.lifecycle_generation,
                    revision: ticket.delta.revision,
                },
            );
            self.handoff_registered = true;
        } else {
            drop(control);
            ticket.reject("projection_handoff_fence_install_rejected");
            self.handoff_registered = true;
            drop(self);
            return ticket;
        }
        drop(control);
        self.shared.wake.notify_one();
        drop(self);
        ticket
    }

    /// Register the committed scan result before releasing the source-processing budget.
    ///
    /// This consumes the permit so callers cannot accidentally release capacity first and rely
    /// on delayed GUI delivery to tell the supervisor what changed. If the exact delta cannot be
    /// admitted for the current lifecycle, the handoff is conservatively promoted to a full
    /// reconciliation while the permit is still owned.
    pub(in crate::native_app) fn release_after_handoff(mut self, handoff: ExternalScanHandoff) {
        self.register_handoff(handoff);
        drop(self);
    }

    fn register_handoff(&mut self, handoff: ExternalScanHandoff) {
        if self.handoff_registered {
            return;
        }
        let source_id = self
            .permit
            .as_ref()
            .map(|permit| permit.source_id().to_string());
        let Some(source_id) = source_id else {
            self.handoff_registered = true;
            return;
        };
        let mut control = self.shared.control();
        let current_generation = control
            .source_lifecycle_generations
            .get(&source_id)
            .copied();
        let current = control.source_is_active(&source_id)
            && current_generation == Some(self.lifecycle_generation);
        if current {
            match handoff {
                ExternalScanHandoff::CommittedDelta(delta) if !delta.is_empty() => {
                    if matches!(
                        control.queue_source_delta(
                            &source_id,
                            self.lifecycle_generation,
                            &delta,
                            "external_scan_committed_delta",
                        ),
                        SourceDeltaQueueResult::Rejected
                    ) {
                        control.pending_readiness_deltas.remove(&source_id);
                        control.cancel_source_work(&source_id);
                        control
                            .mark_source_dirty(&source_id, "external_scan_delta_handoff_fallback");
                    }
                }
                ExternalScanHandoff::CommittedDelta(_) => {}
                ExternalScanHandoff::FullReconciliation { reason } => {
                    control.pending_readiness_deltas.remove(&source_id);
                    control.cancel_source_work(&source_id);
                    control.mark_source_dirty(&source_id, reason);
                }
            }
        }
        drop(control);
        self.handoff_registered = true;
        self.shared.wake.notify_one();
    }

    fn should_cancel_now(&self) -> bool {
        if self.shared.cancel.load(Ordering::Acquire) {
            return true;
        }
        let control = self.shared.control();
        control.shutdown
            || self
                .permit
                .as_ref()
                .is_some_and(|permit| !control.source_is_active(permit.source_id()))
            || self.permit.as_ref().is_some_and(|permit| {
                control.source_lifecycle_generations.get(permit.source_id())
                    != Some(&self.lifecycle_generation)
            })
    }
}

impl Drop for SourceProcessingBudgetPermit {
    fn drop(&mut self) {
        // A panic, cancellation, or early-return path that skipped the explicit handoff must still
        // publish a conservative full reconciliation before capacity becomes available again.
        if !self.handoff_registered {
            self.register_handoff(ExternalScanHandoff::FullReconciliation {
                reason: "external_scan_dropped_without_handoff",
            });
        }
        let registration = self
            .shared
            .external_scans()
            .registrations
            .remove(&self.registration_id);
        self.shared.external_scan_wake.notify_all();
        if registration.as_ref().is_some_and(|registration| {
            self.shared
                .control()
                .pending_retirements
                .values()
                .any(|retirement| {
                    retirement.source.id.as_str() == registration.source_id
                        && retirement.lifecycle_generation == registration.lifecycle_generation
                })
        }) {
            let mut control = self.shared.control();
            control.notify("retired_external_scan_released");
            drop(control);
            self.shared.wake.notify_one();
        }
        if let Some(permit) = self.permit.take() {
            self.shared.budgets().release(permit);
            self.shared.budget_wake.notify_all();
            self.shared.wake.notify_one();
        }
    }
}

pub(super) fn install_worker_app_root(app_root: PathBuf) -> wavecrate::app_dirs::AppRootGuard {
    wavecrate::app_dirs::AppRootGuard::set(app_root)
        .expect("source-processing worker should inherit the resolved persistence root")
}
