#[cfg(test)]
use super::AtomicUsize;
#[cfg(test)]
use super::StateMachinePublicationObservation;
#[cfg(not(test))]
use super::recovered_source_retirements;
use super::{
    Arc, AtomicBool, AtomicU64, BTreeMap, BTreeSet, BudgetTracker, Condvar, ControlState,
    DatabaseWriterGate, Mutex, MutexGuard, Ordering, PriorityContext, ProcessingBudgets,
    SampleSource, SourceAuditLifecycleCause, SourceHealthPublicationOutcome, SourceProcessingEvent,
    SourceProcessingEventSink, SourceProcessingHealthEvent, SupervisorTelemetry, sources_by_id,
};

#[derive(Clone)]
pub(super) struct ExternalScanAdmission {
    pub(super) source_id: String,
    pub(super) lifecycle_generation: u64,
}

pub(super) struct ExternalScanRegistration {
    pub(super) source_id: String,
    pub(super) lifecycle_generation: u64,
    pub(super) cancel: Arc<AtomicBool>,
}

#[derive(Default)]
pub(super) struct ExternalScanState {
    pub(super) admissions: BTreeMap<u64, ExternalScanAdmission>,
    pub(super) registrations: BTreeMap<u64, ExternalScanRegistration>,
}

pub(super) struct InFlightWorkGuard {
    pub(super) shared: Arc<Shared>,
    pub(super) source_id: String,
    pub(super) lifecycle_generation: u64,
}

impl Drop for InFlightWorkGuard {
    fn drop(&mut self) {
        let mut in_flight = self
            .shared
            .in_flight_work
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let key = (self.source_id.clone(), self.lifecycle_generation);
        if let Some(count) = in_flight.get_mut(&key) {
            *count = count.saturating_sub(1);
            if *count == 0 {
                in_flight.remove(&key);
            }
        }
        drop(in_flight);
        let mut control = self.shared.control();
        if control.pending_retirements.values().any(|retirement| {
            retirement.source.id.as_str() == self.source_id
                && retirement.lifecycle_generation == self.lifecycle_generation
        }) {
            control.notify("retired_source_work_released");
            drop(control);
            self.shared.wake.notify_one();
        }
    }
}

pub(super) struct Shared {
    pub(super) source_replacement: Mutex<()>,
    pub(super) state: Mutex<ControlState>,
    pub(super) wake: Condvar,
    pub(super) retirement_wake: Condvar,
    pub(super) cancel: AtomicBool,
    pub(super) telemetry: Mutex<SupervisorTelemetry>,
    pub(super) budgets: Mutex<BudgetTracker>,
    pub(super) database_writer: DatabaseWriterGate,
    pub(super) budget_wake: Condvar,
    pub(super) external_scans: Mutex<ExternalScanState>,
    pub(super) external_scan_wake: Condvar,
    pub(super) next_external_scan_id: AtomicU64,
    pub(super) in_flight_work: Mutex<BTreeMap<(String, u64), usize>>,
    pub(super) synthetic_test_execution: AtomicBool,
    pub(super) event_sink: Option<Arc<dyn SourceProcessingEventSink>>,
    pub(super) published_source_health: Mutex<BTreeMap<String, SourceProcessingHealthEvent>>,
    #[cfg(test)]
    pub(super) state_machine_publications: Mutex<Vec<StateMachinePublicationObservation>>,
    #[cfg(test)]
    pub(super) state_machine_reject_next_health_publication: AtomicBool,
    #[cfg(test)]
    pub(super) retirement_cleanup_blocked: AtomicBool,
    #[cfg(test)]
    pub(super) retirement_cleanup_started: AtomicBool,
    #[cfg(test)]
    pub(super) execution_workers_paused: AtomicBool,
    #[cfg(test)]
    pub(super) execution_workers_started: AtomicUsize,
}

impl Shared {
    pub(super) fn new(
        sources: Vec<SampleSource>,
        event_sink: Option<Arc<dyn SourceProcessingEventSink>>,
    ) -> Self {
        let sources = sources_by_id(sources);
        let source_work_cancels = sources
            .keys()
            .map(|source_id| (source_id.clone(), Arc::new(AtomicBool::new(false))))
            .collect();
        let mut next_lifecycle_generation = 1_u64;
        let source_lifecycle_generations = sources
            .keys()
            .map(|source_id| {
                let generation = next_lifecycle_generation;
                next_lifecycle_generation = next_lifecycle_generation.wrapping_add(1).max(1);
                (source_id.clone(), generation)
            })
            .collect();
        #[cfg(not(test))]
        let (pending_retirements, next_retirement_id) =
            match recovered_source_retirements(&sources, &mut next_lifecycle_generation) {
                Ok(recovered) => recovered,
                Err(error) => {
                    tracing::warn!(
                        target: "wavecrate::source_processing",
                        error,
                        "Retained source retirement recovery was deferred"
                    );
                    (BTreeMap::new(), 1)
                }
            };
        #[cfg(test)]
        let pending_retirements = BTreeMap::new();
        #[cfg(test)]
        let next_retirement_id = 1_u64;
        let dirty_sources = sources.keys().cloned().collect();
        let safety_probe_sources = sources.keys().cloned().collect();
        #[cfg(not(test))]
        let lifecycle_audits_deferred_until_watcher_ready = true;
        #[cfg(test)]
        let lifecycle_audits_deferred_until_watcher_ready = false;
        Self {
            source_replacement: Mutex::new(()),
            state: Mutex::new(ControlState {
                sources,
                source_work_cancels,
                source_lifecycle_generations,
                next_lifecycle_generation,
                dirty_sources,
                safety_probe_sources,
                lifecycle_audits_deferred_until_watcher_ready,
                deferred_lifecycle_audit_sources: BTreeSet::new(),
                pending_readiness_deltas: BTreeMap::new(),
                awaiting_foreground_refresh_sources: BTreeSet::new(),
                force_manifest_audit_sources: BTreeSet::new(),
                force_reanalysis_sources: BTreeSet::new(),
                quarantined_sources: BTreeSet::new(),
                pending_retirements,
                next_retirement_id,
                wake_generation: 1,
                wake_reason: SourceAuditLifecycleCause::Startup.reason(),
                playback_active: false,
                foreground_active: false,
                shutdown: false,
                priority: PriorityContext::default(),
                #[cfg(test)]
                reject_next_delta_delivery: false,
                #[cfg(test)]
                reject_next_source_replacement: false,
            }),
            wake: Condvar::new(),
            retirement_wake: Condvar::new(),
            cancel: AtomicBool::new(false),
            telemetry: Mutex::new(SupervisorTelemetry::default()),
            budgets: Mutex::new(BudgetTracker::new(ProcessingBudgets::default())),
            database_writer: DatabaseWriterGate::default(),
            budget_wake: Condvar::new(),
            external_scans: Mutex::new(ExternalScanState::default()),
            external_scan_wake: Condvar::new(),
            next_external_scan_id: AtomicU64::new(1),
            in_flight_work: Mutex::new(BTreeMap::new()),
            synthetic_test_execution: AtomicBool::new(false),
            event_sink,
            published_source_health: Mutex::new(BTreeMap::new()),
            #[cfg(test)]
            state_machine_publications: Mutex::new(Vec::new()),
            #[cfg(test)]
            state_machine_reject_next_health_publication: AtomicBool::new(false),
            #[cfg(test)]
            retirement_cleanup_blocked: AtomicBool::new(false),
            #[cfg(test)]
            retirement_cleanup_started: AtomicBool::new(false),
            #[cfg(test)]
            execution_workers_paused: AtomicBool::new(false),
            #[cfg(test)]
            execution_workers_started: AtomicUsize::new(0),
        }
    }

    pub(super) fn control(&self) -> MutexGuard<'_, ControlState> {
        self.state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
    }

    pub(super) fn publish_event(&self, event: SourceProcessingEvent) -> bool {
        let lifecycle_guard = if let Some(lifecycle) = event.lifecycle() {
            let control = self.control();
            if !control.source_is_active(&lifecycle.source_id)
                || control
                    .source_lifecycle_generations
                    .get(&lifecycle.source_id)
                    != Some(&lifecycle.generation)
            {
                return false;
            }
            Some(control)
        } else {
            None
        };
        let published = self
            .event_sink
            .as_ref()
            .is_some_and(|sink| sink.try_publish(event));
        drop(lifecycle_guard);
        published
    }

    pub(super) fn publish_source_health(&self, health: SourceProcessingHealthEvent) -> bool {
        self.publish_source_health_outcome(health) == SourceHealthPublicationOutcome::Published
    }

    pub(super) fn publish_source_health_outcome(
        &self,
        health: SourceProcessingHealthEvent,
    ) -> SourceHealthPublicationOutcome {
        let control = self.control();
        if !control.source_is_active(&health.lifecycle.source_id)
            || control
                .source_lifecycle_generations
                .get(&health.lifecycle.source_id)
                != Some(&health.lifecycle.generation)
        {
            return SourceHealthPublicationOutcome::Superseded;
        }
        let mut published_health = self
            .published_source_health
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        if published_health.get(&health.lifecycle.source_id) == Some(&health) {
            return SourceHealthPublicationOutcome::AlreadyPublished;
        }
        #[cfg(test)]
        let reject_for_state_machine = self
            .state_machine_reject_next_health_publication
            .swap(false, Ordering::AcqRel);
        #[cfg(not(test))]
        let reject_for_state_machine = false;
        let outcome = if reject_for_state_machine {
            SourceHealthPublicationOutcome::Rejected
        } else if let Some(sink) = &self.event_sink {
            if sink.try_publish(SourceProcessingEvent::Health(health.clone())) {
                SourceHealthPublicationOutcome::Published
            } else {
                SourceHealthPublicationOutcome::Rejected
            }
        } else {
            SourceHealthPublicationOutcome::NoSink
        };
        if outcome == SourceHealthPublicationOutcome::Published {
            published_health.insert(health.lifecycle.source_id.clone(), health);
        }
        drop(published_health);
        drop(control);
        outcome
    }

    pub(super) fn telemetry(&self) -> MutexGuard<'_, SupervisorTelemetry> {
        self.telemetry
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
    }

    pub(super) fn budgets(&self) -> MutexGuard<'_, BudgetTracker> {
        self.budgets
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
    }

    pub(super) fn external_scans(&self) -> MutexGuard<'_, ExternalScanState> {
        self.external_scans
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
    }

    pub(super) fn has_external_scan_admission(&self) -> bool {
        !self.external_scans().admissions.is_empty()
    }

    pub(super) fn cancel_external_scans(
        &self,
        should_cancel: impl Fn(&ExternalScanRegistration) -> bool,
    ) {
        for registration in self.external_scans().registrations.values() {
            if should_cancel(registration) {
                registration.cancel.store(true, Ordering::Release);
            }
        }
    }

    pub(super) fn wait_for_external_scans(&self) {
        let registrations = self.external_scans();
        drop(
            self.external_scan_wake
                .wait_while(registrations, |state| {
                    !state.admissions.is_empty() || !state.registrations.is_empty()
                })
                .unwrap_or_else(|poison| poison.into_inner()),
        );
    }

    pub(super) fn source_has_external_activity(
        &self,
        source_id: &str,
        lifecycle_generation: u64,
    ) -> bool {
        let scans = self.external_scans();
        scans.admissions.values().any(|admitted| {
            admitted.source_id == source_id && admitted.lifecycle_generation == lifecycle_generation
        }) || scans.registrations.values().any(|registration| {
            registration.source_id == source_id
                && registration.lifecycle_generation == lifecycle_generation
        })
    }

    pub(super) fn finish_external_scan_admission(&self, admission_id: u64) {
        let mut external_scans = self.external_scans();
        let admission = external_scans.admissions.remove(&admission_id);
        drop(external_scans);
        self.external_scan_wake.notify_all();
        if admission.as_ref().is_some_and(|admission| {
            self.control()
                .pending_retirements
                .values()
                .any(|retirement| {
                    retirement.source.id.as_str() == admission.source_id
                        && retirement.lifecycle_generation == admission.lifecycle_generation
                })
        }) {
            let mut control = self.control();
            control.notify("retired_external_admission_released");
            drop(control);
            self.wake.notify_one();
        }
    }

    pub(super) fn begin_in_flight_work(
        self: &Arc<Self>,
        source_id: &str,
        expected_cancel: &Arc<AtomicBool>,
    ) -> Option<InFlightWorkGuard> {
        let control = self.control();
        let current_cancel = control.source_work_cancels.get(source_id)?;
        if control.shutdown
            || !control.source_is_active(source_id)
            || !Arc::ptr_eq(current_cancel, expected_cancel)
            || expected_cancel.load(Ordering::Acquire)
        {
            return None;
        }
        let mut in_flight = self
            .in_flight_work
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let lifecycle_generation = control
            .source_lifecycle_generations
            .get(source_id)
            .copied()?;
        *in_flight
            .entry((source_id.to_string(), lifecycle_generation))
            .or_default() += 1;
        drop(in_flight);
        drop(control);
        Some(InFlightWorkGuard {
            shared: Arc::clone(self),
            source_id: source_id.to_string(),
            lifecycle_generation,
        })
    }

    pub(super) fn source_has_in_flight_work(
        &self,
        source_id: &str,
        lifecycle_generation: u64,
    ) -> bool {
        self.in_flight_work
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .get(&(source_id.to_string(), lifecycle_generation))
            .is_some_and(|count| *count > 0)
    }
}
