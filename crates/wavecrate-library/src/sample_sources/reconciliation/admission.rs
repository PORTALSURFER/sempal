//! Bounded, backend-neutral admission and lifecycle ownership for watcher evidence.

use std::collections::{HashMap, HashSet, VecDeque};

use crate::sample_sources::SourceId;

use super::model::{
    CaptureBoundary, RawEventKind, RawObservationAccounting, RawObservationEnvelope,
    RawObservationLimits, RootIdentity, WatcherGeneration,
};
use super::normalize::{NormalizedObservation, ReconciliationScopeKind, normalize_observation};

/// An explicitly registered source/root admission lane.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct AdmissionLaneKey {
    source_id: SourceId,
    root_identity: RootIdentity,
}

impl AdmissionLaneKey {
    /// Create a lane key from a configured source and its physical root identity.
    pub fn new(source_id: SourceId, root_identity: RootIdentity) -> Self {
        Self {
            source_id,
            root_identity,
        }
    }

    /// Borrow the configured source identifier.
    pub fn source_id(&self) -> &SourceId {
        &self.source_id
    }

    /// Borrow the configured physical root identity.
    pub fn root_identity(&self) -> &RootIdentity {
        &self.root_identity
    }
}

/// Lifecycle state owned by one admission lane.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReconciliationLifecycle {
    /// The lane has been registered but has not begun capture.
    Starting,
    /// The lane accepts synthetic or backend-produced envelopes.
    Capturing,
    /// The lane accepts no new envelopes and its old work is being retained as uncertainty.
    Stopped,
}

/// A bounded admission configuration.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ReconciliationAdmissionLimits {
    max_lanes: usize,
    per_lane: RawObservationLimits,
    global: RawObservationLimits,
    max_in_flight: usize,
}

impl ReconciliationAdmissionLimits {
    /// Construct limits for a supervisor.
    ///
    /// One ordinary envelope reservation is available to each possible registered lane, so
    /// `max_in_flight` must be at least `max_lanes`. Retained uncertainty is out-of-band and
    /// does not consume this envelope capacity.
    pub fn new(
        max_lanes: usize,
        per_lane: RawObservationLimits,
        global: RawObservationLimits,
        max_in_flight: usize,
    ) -> Result<Self, AdmissionError> {
        if max_lanes == 0 {
            return Err(AdmissionError::ZeroLaneLimit);
        }
        if max_in_flight == 0 || max_in_flight < max_lanes {
            return Err(AdmissionError::InsufficientEnvelopeCapacity);
        }
        Ok(Self {
            max_lanes,
            per_lane,
            global,
            max_in_flight,
        })
    }

    /// Return the maximum number of registered lanes.
    pub const fn max_lanes(self) -> usize {
        self.max_lanes
    }

    /// Return the per-lane observation limits.
    pub const fn per_lane(self) -> RawObservationLimits {
        self.per_lane
    }

    /// Return the global observation limits.
    pub const fn global(self) -> RawObservationLimits {
        self.global
    }

    /// Return the maximum number of live envelopes across all dispatch phases.
    pub const fn max_in_flight(self) -> usize {
        self.max_in_flight
    }
}

/// Configuration or lifecycle errors returned by supervisor control methods.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AdmissionError {
    /// At least one lane is required.
    ZeroLaneLimit,
    /// The envelope budget cannot reserve one ordinary slot per possible lane.
    InsufficientEnvelopeCapacity,
    /// The configured lane registry is full.
    LaneLimitReached,
    /// The source already has a registered lane.
    SourceAlreadyRegistered,
    /// The requested lane is not registered.
    UnknownLane,
    /// The requested lifecycle transition is not valid.
    InvalidLifecycleTransition,
    /// The supplied generation is not the lane's current generation.
    GenerationMismatch,
    /// The monotonic generation counter cannot advance.
    GenerationExhausted,
    /// The supplied dispatch ticket is not live.
    UnknownTicket,
}

/// Why an envelope was not dispatched.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AdmissionRejectReason {
    /// The source has no registered lane.
    UnknownLane,
    /// The envelope root differs from the registered root.
    WrongRoot,
    /// The envelope did not carry a root identity.
    MissingRoot,
    /// The envelope belongs to an older or newer generation.
    StaleGeneration,
    /// The lane is not capturing.
    NotCapturing,
    /// The per-lane or global raw budget is full.
    QueueSaturated,
    /// Checked accounting could not represent the requested addition.
    AccountingOverflow,
    /// An overflow or backend-error marker was retained instead of dispatched.
    UncertaintyMarkerRetained,
    /// The envelope carried an unsupported event marker.
    UnsupportedEvidence,
}

/// A bounded reason retained after raw evidence cannot safely be dispatched.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum UncertaintyReason {
    /// The source was not registered.
    UnknownLane,
    /// The physical root identity did not match.
    WrongRoot,
    /// The envelope omitted a required root identity.
    MissingRoot,
    /// The envelope belonged to a retired generation.
    StaleGeneration,
    /// The lane was stopped or had not begun capture.
    NotCapturing,
    /// The ordinary bounded queue could not accept the envelope.
    QueueSaturated,
    /// A queued or in-flight envelope was cancelled or invalidated.
    Cancellation,
    /// A root rebind retired the envelope's generation.
    Rebind,
    /// The backend reported overflow.
    Overflow,
    /// The backend reported an error.
    BackendError,
    /// The backend supplied unsupported evidence.
    Unsupported,
    /// An accounting operation could not be represented.
    AccountingOverflow,
}

/// A conservative retained uncertainty marker.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RetainedUncertainty {
    source_id: Option<SourceId>,
    root_identity: Option<RootIdentity>,
    generation: Option<WatcherGeneration>,
    boundary: Option<CaptureBoundary>,
    scope: ReconciliationScopeKind,
    reasons: Vec<UncertaintyReason>,
}

impl RetainedUncertainty {
    /// Return the source identity when it remains unambiguous.
    pub fn source_id(&self) -> Option<&SourceId> {
        self.source_id.as_ref()
    }

    /// Return the root identity when it remains unambiguous.
    pub fn root_identity(&self) -> Option<&RootIdentity> {
        self.root_identity.as_ref()
    }

    /// Return the generation when all retained evidence belongs to one generation.
    pub const fn generation(&self) -> Option<WatcherGeneration> {
        self.generation
    }

    /// Return the merged capture boundary, if it is still known.
    pub const fn capture_boundary(&self) -> Option<CaptureBoundary> {
        self.boundary
    }

    /// Return the conservative scope required to clear this marker.
    pub const fn scope(&self) -> ReconciliationScopeKind {
        self.scope
    }

    /// Borrow the bounded, ordered reasons retained by this marker.
    pub fn reasons(&self) -> &[UncertaintyReason] {
        &self.reasons
    }
}

/// An opaque ticket for one admitted envelope.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct DispatchTicket(u64);

impl DispatchTicket {
    /// Return the diagnostic ticket number.
    pub const fn id(self) -> u64 {
        self.0
    }
}

/// The strict phase of one admitted envelope.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DispatchPhase {
    /// The envelope is queued for normalization.
    Queued,
    /// The envelope was selected and is being normalized.
    Normalizing,
    /// Normalization completed and work was handed to a downstream worker.
    Dispatched,
    /// The downstream worker reported completion.
    Applied,
}

/// The result of trying to admit one envelope.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AdmissionOutcome {
    /// The envelope was admitted under this ticket.
    Accepted(DispatchTicket),
    /// The envelope was rejected and a bounded uncertainty marker was retained.
    Rejected(AdmissionRejectReason),
}

/// One normalized envelope and its opaque lifecycle ticket.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DispatchedObservation {
    ticket: DispatchTicket,
    lane: AdmissionLaneKey,
    generation: WatcherGeneration,
    normalized: NormalizedObservation,
}

impl DispatchedObservation {
    /// Return the lifecycle ticket.
    pub const fn ticket(&self) -> DispatchTicket {
        self.ticket
    }

    /// Borrow the lane that admitted this envelope.
    pub const fn lane(&self) -> &AdmissionLaneKey {
        &self.lane
    }

    /// Return the generation that admitted this envelope.
    pub const fn generation(&self) -> WatcherGeneration {
        self.generation
    }

    /// Borrow the normalized observation and its unchanged raw envelope.
    pub const fn normalized(&self) -> &NormalizedObservation {
        &self.normalized
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct Usage {
    events: usize,
    path_bytes: usize,
    metadata_bytes: usize,
}

impl Usage {
    fn from(accounting: RawObservationAccounting) -> Self {
        Self {
            events: accounting.event_count(),
            path_bytes: accounting.path_bytes(),
            metadata_bytes: accounting.metadata_bytes(),
        }
    }

    fn add(self, other: Self) -> Option<Self> {
        Some(Self {
            events: self.events.checked_add(other.events)?,
            path_bytes: self.path_bytes.checked_add(other.path_bytes)?,
            metadata_bytes: self.metadata_bytes.checked_add(other.metadata_bytes)?,
        })
    }

    fn subtract(self, other: Self) -> Self {
        debug_assert!(self.events >= other.events);
        debug_assert!(self.path_bytes >= other.path_bytes);
        debug_assert!(self.metadata_bytes >= other.metadata_bytes);
        Self {
            events: self.events - other.events,
            path_bytes: self.path_bytes - other.path_bytes,
            metadata_bytes: self.metadata_bytes - other.metadata_bytes,
        }
    }

    fn within(self, limits: RawObservationLimits) -> bool {
        self.events <= limits.max_events()
            && self.path_bytes <= limits.max_path_bytes()
            && self.metadata_bytes <= limits.max_metadata_bytes()
    }
}

fn marker_reason(kind: RawEventKind) -> Option<UncertaintyReason> {
    match kind {
        RawEventKind::Overflow => Some(UncertaintyReason::Overflow),
        RawEventKind::Error => Some(UncertaintyReason::BackendError),
        RawEventKind::Unsupported => Some(UncertaintyReason::Unsupported),
        _ => None,
    }
}

fn marker_reasons(envelope: &RawObservationEnvelope) -> Vec<UncertaintyReason> {
    let mut reasons = envelope
        .observations()
        .iter()
        .filter_map(|observation| marker_reason(observation.kind()))
        .collect::<Vec<_>>();
    reasons.sort_unstable();
    reasons.dedup();
    reasons
}

fn marker_only(envelope: &RawObservationEnvelope) -> bool {
    envelope
        .observations()
        .iter()
        .all(|observation| marker_reason(observation.kind()).is_some())
}

fn marker_rejection_reason(reasons: &[UncertaintyReason]) -> AdmissionRejectReason {
    if reasons.len() == 1 && reasons[0] == UncertaintyReason::Unsupported {
        AdmissionRejectReason::UnsupportedEvidence
    } else {
        AdmissionRejectReason::UncertaintyMarkerRetained
    }
}

fn rejection_reasons(
    marker_reasons: &[UncertaintyReason],
    rejection_reason: UncertaintyReason,
) -> Vec<UncertaintyReason> {
    let mut reasons = marker_reasons.to_vec();
    reasons.push(rejection_reason);
    reasons
}

struct PendingObservation {
    lane: AdmissionLaneKey,
    generation: WatcherGeneration,
    envelope: RawObservationEnvelope,
    usage: Usage,
    phase: DispatchPhase,
}

struct LaneState {
    generation: WatcherGeneration,
    lifecycle: ReconciliationLifecycle,
    queue: VecDeque<DispatchTicket>,
    live_tickets: usize,
    usage: Usage,
    uncertainty: Option<RetainedUncertainty>,
}

/// Owns registered lanes, bounded admission, lifecycle fences, and retained uncertainty.
pub struct ReconciliationAdmissionSupervisor {
    limits: ReconciliationAdmissionLimits,
    lanes: HashMap<AdmissionLaneKey, LaneState>,
    sources: HashMap<SourceId, AdmissionLaneKey>,
    pending: HashMap<DispatchTicket, PendingObservation>,
    runnable: VecDeque<AdmissionLaneKey>,
    runnable_set: HashSet<AdmissionLaneKey>,
    global_usage: Usage,
    next_generation: u64,
    next_ticket: u64,
    global_uncertainty: Option<RetainedUncertainty>,
}

impl ReconciliationAdmissionSupervisor {
    /// Create an empty bounded admission supervisor.
    pub fn new(limits: ReconciliationAdmissionLimits) -> Self {
        Self {
            limits,
            lanes: HashMap::new(),
            sources: HashMap::new(),
            pending: HashMap::new(),
            runnable: VecDeque::new(),
            runnable_set: HashSet::new(),
            global_usage: Usage::default(),
            next_generation: 0,
            next_ticket: 0,
            global_uncertainty: None,
        }
    }

    /// Return the immutable admission configuration.
    pub const fn limits(&self) -> ReconciliationAdmissionLimits {
        self.limits
    }

    /// Register a source/root lane in `Starting` state.
    pub fn register_lane(
        &mut self,
        source_id: SourceId,
        root_identity: RootIdentity,
    ) -> Result<(AdmissionLaneKey, WatcherGeneration), AdmissionError> {
        if self.lanes.len() >= self.limits.max_lanes {
            return Err(AdmissionError::LaneLimitReached);
        }
        if self.sources.contains_key(&source_id) {
            return Err(AdmissionError::SourceAlreadyRegistered);
        }
        let generation = self.allocate_generation()?;
        let lane = AdmissionLaneKey::new(source_id.clone(), root_identity);
        self.sources.insert(source_id, lane.clone());
        self.lanes.insert(
            lane.clone(),
            LaneState {
                generation,
                lifecycle: ReconciliationLifecycle::Starting,
                queue: VecDeque::new(),
                live_tickets: 0,
                usage: Usage::default(),
                uncertainty: None,
            },
        );
        Ok((lane, generation))
    }

    /// Begin capturing for a registered lane and generation.
    pub fn begin_capture(
        &mut self,
        lane: &AdmissionLaneKey,
        generation: WatcherGeneration,
    ) -> Result<(), AdmissionError> {
        let state = self.lane_mut(lane)?;
        if state.generation != generation || state.lifecycle != ReconciliationLifecycle::Starting {
            return Err(if state.generation != generation {
                AdmissionError::GenerationMismatch
            } else {
                AdmissionError::InvalidLifecycleTransition
            });
        }
        state.lifecycle = ReconciliationLifecycle::Capturing;
        Ok(())
    }

    /// Return the current lifecycle state for a registered lane.
    pub fn lifecycle(
        &self,
        lane: &AdmissionLaneKey,
    ) -> Result<ReconciliationLifecycle, AdmissionError> {
        Ok(self.lane(lane)?.lifecycle)
    }

    /// Return the current generation for a registered lane.
    pub fn generation(&self, lane: &AdmissionLaneKey) -> Result<WatcherGeneration, AdmissionError> {
        Ok(self.lane(lane)?.generation)
    }

    /// Stop a capturing lane and retain uncertainty for all invalidated work.
    pub fn stop_lane(
        &mut self,
        lane: &AdmissionLaneKey,
        generation: WatcherGeneration,
    ) -> Result<(), AdmissionError> {
        let current = self.lane(lane)?.generation;
        if current != generation {
            return Err(AdmissionError::GenerationMismatch);
        }
        if self.lane(lane)?.lifecycle == ReconciliationLifecycle::Stopped {
            return Err(AdmissionError::InvalidLifecycleTransition);
        }
        self.invalidate_lane_work(lane, generation, UncertaintyReason::Cancellation);
        self.retain_lane(
            lane,
            RetainedUncertainty::new(
                Some(lane.source_id.clone()),
                Some(lane.root_identity.clone()),
                Some(generation),
                None,
                UncertaintyReason::Cancellation,
            ),
        );
        self.lane_mut(lane)?.lifecycle = ReconciliationLifecycle::Stopped;
        Ok(())
    }

    /// Restart a stopped lane with a strictly newer generation.
    pub fn restart_lane(
        &mut self,
        lane: &AdmissionLaneKey,
    ) -> Result<WatcherGeneration, AdmissionError> {
        if self.lane(lane)?.lifecycle != ReconciliationLifecycle::Stopped {
            return Err(AdmissionError::InvalidLifecycleTransition);
        }
        let generation = self.allocate_generation()?;
        let state = self.lane_mut(lane)?;
        state.generation = generation;
        state.lifecycle = ReconciliationLifecycle::Starting;
        Ok(generation)
    }

    /// Rebind one source to a new physical root, retiring its old generation.
    pub fn rebind_lane(
        &mut self,
        lane: &AdmissionLaneKey,
        generation: WatcherGeneration,
        root_identity: RootIdentity,
    ) -> Result<(AdmissionLaneKey, WatcherGeneration), AdmissionError> {
        if self.lane(lane)?.generation != generation {
            return Err(AdmissionError::GenerationMismatch);
        }
        // Allocate before invalidating the old lane so a failed generation advance leaves the
        // old lane, its tickets, and its source registration intact.
        let new_generation = self.allocate_generation()?;
        self.invalidate_lane_work(lane, generation, UncertaintyReason::Rebind);
        let source_id = lane.source_id.clone();
        let old_uncertainty = self
            .lanes
            .get(lane)
            .and_then(|state| state.uncertainty.clone());
        self.lanes.remove(lane);
        let new_lane = AdmissionLaneKey::new(source_id.clone(), root_identity);
        let mut marker = RetainedUncertainty::new(
            Some(source_id.clone()),
            Some(new_lane.root_identity.clone()),
            Some(new_generation),
            None,
            UncertaintyReason::Rebind,
        );
        if let Some(old_uncertainty) = old_uncertainty {
            marker = marker.merge(old_uncertainty);
        }
        self.sources.insert(source_id, new_lane.clone());
        self.lanes.insert(
            new_lane.clone(),
            LaneState {
                generation: new_generation,
                lifecycle: ReconciliationLifecycle::Starting,
                queue: VecDeque::new(),
                live_tickets: 0,
                usage: Usage::default(),
                uncertainty: Some(marker),
            },
        );
        Ok((new_lane, new_generation))
    }

    /// Admit an envelope into the bounded lane queue.
    pub fn admit(&mut self, envelope: RawObservationEnvelope) -> AdmissionOutcome {
        let envelope_marker_reasons = marker_reasons(&envelope);
        let source_id = envelope.provenance().source_id().clone();
        let Some(lane) = self.sources.get(&source_id).cloned() else {
            self.retain_global(
                RetainedUncertainty::from_envelope_with_generation_and_reasons(
                    &envelope,
                    Some(envelope.provenance().watcher_generation()),
                    rejection_reasons(&envelope_marker_reasons, UncertaintyReason::UnknownLane),
                ),
            );
            return AdmissionOutcome::Rejected(AdmissionRejectReason::UnknownLane);
        };
        let current_generation = self.lanes[&lane].generation;
        let Some(root_identity) = envelope.provenance().root_identity() else {
            self.retain_lane(
                &lane,
                RetainedUncertainty::from_envelope_with_generation_and_reasons(
                    &envelope,
                    Some(current_generation),
                    rejection_reasons(&envelope_marker_reasons, UncertaintyReason::MissingRoot),
                ),
            );
            return AdmissionOutcome::Rejected(AdmissionRejectReason::MissingRoot);
        };
        if root_identity != lane.root_identity() {
            self.retain_lane(
                &lane,
                RetainedUncertainty::from_envelope_with_generation_and_reasons(
                    &envelope,
                    Some(current_generation),
                    rejection_reasons(&envelope_marker_reasons, UncertaintyReason::WrongRoot),
                ),
            );
            return AdmissionOutcome::Rejected(AdmissionRejectReason::WrongRoot);
        }
        if envelope.provenance().watcher_generation() != current_generation {
            self.retain_lane(
                &lane,
                RetainedUncertainty::from_envelope_with_generation_and_reasons(
                    &envelope,
                    Some(current_generation),
                    rejection_reasons(&envelope_marker_reasons, UncertaintyReason::StaleGeneration),
                ),
            );
            return AdmissionOutcome::Rejected(AdmissionRejectReason::StaleGeneration);
        }
        if self.lanes[&lane].lifecycle != ReconciliationLifecycle::Capturing {
            self.retain_lane(
                &lane,
                RetainedUncertainty::from_envelope_with_generation_and_reasons(
                    &envelope,
                    Some(current_generation),
                    rejection_reasons(&envelope_marker_reasons, UncertaintyReason::NotCapturing),
                ),
            );
            return AdmissionOutcome::Rejected(AdmissionRejectReason::NotCapturing);
        }

        if !envelope_marker_reasons.is_empty() {
            self.retain_lane(
                &lane,
                RetainedUncertainty::from_envelope_with_generation_and_reasons(
                    &envelope,
                    Some(current_generation),
                    envelope_marker_reasons.clone(),
                ),
            );
            if marker_only(&envelope) {
                return AdmissionOutcome::Rejected(marker_rejection_reason(
                    &envelope_marker_reasons,
                ));
            }
        }

        let usage = Usage::from(envelope.accounting());
        let lane_usage = self.lanes[&lane].usage;
        let Some(next_lane_usage) = lane_usage.add(usage) else {
            self.retain_lane(
                &lane,
                RetainedUncertainty::from_envelope_with_generation_and_reasons(
                    &envelope,
                    Some(current_generation),
                    rejection_reasons(
                        &envelope_marker_reasons,
                        UncertaintyReason::AccountingOverflow,
                    ),
                ),
            );
            return AdmissionOutcome::Rejected(AdmissionRejectReason::AccountingOverflow);
        };
        let Some(next_global_usage) = self.global_usage.add(usage) else {
            self.retain_lane(
                &lane,
                RetainedUncertainty::from_envelope_with_generation_and_reasons(
                    &envelope,
                    Some(current_generation),
                    rejection_reasons(
                        &envelope_marker_reasons,
                        UncertaintyReason::AccountingOverflow,
                    ),
                ),
            );
            return AdmissionOutcome::Rejected(AdmissionRejectReason::AccountingOverflow);
        };
        let lane_live_tickets = self.lanes[&lane].live_tickets;
        let shared_capacity = self.limits.max_in_flight - self.limits.max_lanes;
        let lane_capacity_available =
            lane_live_tickets == 0 || self.shared_used() < shared_capacity;
        if self.pending.len() >= self.limits.max_in_flight
            || !lane_capacity_available
            || !next_lane_usage.within(self.limits.per_lane)
            || !next_global_usage.within(self.limits.global)
        {
            self.retain_lane(
                &lane,
                RetainedUncertainty::from_envelope_with_generation_and_reasons(
                    &envelope,
                    Some(current_generation),
                    rejection_reasons(&envelope_marker_reasons, UncertaintyReason::QueueSaturated),
                ),
            );
            return AdmissionOutcome::Rejected(AdmissionRejectReason::QueueSaturated);
        }

        let ticket = match self.next_ticket.checked_add(1) {
            Some(value) => {
                self.next_ticket = value;
                DispatchTicket(value)
            }
            None => {
                self.retain_lane(
                    &lane,
                    RetainedUncertainty::from_envelope_with_generation_and_reasons(
                        &envelope,
                        Some(current_generation),
                        rejection_reasons(
                            &envelope_marker_reasons,
                            UncertaintyReason::AccountingOverflow,
                        ),
                    ),
                );
                return AdmissionOutcome::Rejected(AdmissionRejectReason::AccountingOverflow);
            }
        };
        let was_empty = self.lanes[&lane].queue.is_empty();
        let state = self.lanes.get_mut(&lane).expect("lane checked above");
        state.queue.push_back(ticket);
        state.live_tickets += 1;
        state.usage = next_lane_usage;
        self.global_usage = next_global_usage;
        self.pending.insert(
            ticket,
            PendingObservation {
                lane: lane.clone(),
                generation: current_generation,
                usage,
                envelope,
                phase: DispatchPhase::Queued,
            },
        );
        if was_empty && self.runnable_set.insert(lane.clone()) {
            self.runnable.push_back(lane);
        }
        AdmissionOutcome::Accepted(ticket)
    }

    /// Select the next envelope fairly and normalize it without side effects.
    ///
    /// The returned ticket remains in [`DispatchPhase::Normalizing`] until the caller explicitly
    /// invokes [`Self::mark_dispatched`].
    pub fn dispatch_next(&mut self) -> Option<DispatchedObservation> {
        let lane = self.runnable.pop_front()?;
        self.runnable_set.remove(&lane);
        let ticket = self.lanes.get_mut(&lane)?.queue.pop_front()?;
        let pending = self.pending.get_mut(&ticket)?;
        pending.phase = DispatchPhase::Normalizing;
        let normalized = normalize_observation(pending.envelope.clone());
        if !self.lanes[&lane].queue.is_empty() && self.runnable_set.insert(lane.clone()) {
            self.runnable.push_back(lane.clone());
        }
        Some(DispatchedObservation {
            ticket,
            lane,
            generation: pending.generation,
            normalized,
        })
    }

    /// Advance a ticket from normalization to downstream dispatch.
    pub fn mark_dispatched(&mut self, ticket: DispatchTicket) -> Result<(), AdmissionError> {
        self.transition(
            ticket,
            DispatchPhase::Normalizing,
            DispatchPhase::Dispatched,
        )
    }

    /// Advance a ticket from dispatch to worker completion.
    pub fn mark_applied(&mut self, ticket: DispatchTicket) -> Result<(), AdmissionError> {
        self.transition(ticket, DispatchPhase::Dispatched, DispatchPhase::Applied)
    }

    /// Retire a completed ticket and release its bounded accounting.
    pub fn mark_checkpointed(&mut self, ticket: DispatchTicket) -> Result<(), AdmissionError> {
        let pending = self
            .pending
            .get(&ticket)
            .ok_or(AdmissionError::UnknownTicket)?;
        if pending.phase != DispatchPhase::Applied {
            return Err(AdmissionError::InvalidLifecycleTransition);
        }
        let pending = self.pending.remove(&ticket).expect("ticket checked above");
        self.release_usage(&pending.lane, pending.usage);
        Ok(())
    }

    /// Borrow the retained uncertainty for a registered lane, if any.
    pub fn uncertainty(
        &self,
        lane: &AdmissionLaneKey,
    ) -> Result<Option<&RetainedUncertainty>, AdmissionError> {
        Ok(self.lane(lane)?.uncertainty.as_ref())
    }

    /// Borrow the bounded global marker for unknown or unregistered evidence.
    pub const fn global_uncertainty(&self) -> Option<&RetainedUncertainty> {
        self.global_uncertainty.as_ref()
    }

    /// Return the number of live envelopes in queued or in-flight dispatch phases.
    pub fn in_flight(&self) -> usize {
        self.pending.len()
    }

    fn transition(
        &mut self,
        ticket: DispatchTicket,
        from: DispatchPhase,
        to: DispatchPhase,
    ) -> Result<(), AdmissionError> {
        let pending = self
            .pending
            .get_mut(&ticket)
            .ok_or(AdmissionError::UnknownTicket)?;
        if pending.phase != from {
            return Err(AdmissionError::InvalidLifecycleTransition);
        }
        pending.phase = to;
        Ok(())
    }

    fn invalidate_lane_work(
        &mut self,
        lane: &AdmissionLaneKey,
        generation: WatcherGeneration,
        reason: UncertaintyReason,
    ) {
        let tickets: Vec<_> = self
            .pending
            .iter()
            .filter_map(|(ticket, pending)| {
                (pending.lane == *lane && pending.generation == generation).then_some(*ticket)
            })
            .collect();
        for ticket in tickets {
            if let Some(pending) = self.pending.remove(&ticket) {
                self.release_usage(&pending.lane, pending.usage);
                self.retain_lane(
                    lane,
                    RetainedUncertainty::from_envelope_with_generation(
                        &pending.envelope,
                        Some(generation),
                        reason,
                    ),
                );
            }
        }
        if let Some(state) = self.lanes.get_mut(lane) {
            state.queue.clear();
        }
        self.runnable_set.remove(lane);
        self.runnable.retain(|candidate| candidate != lane);
    }

    fn release_usage(&mut self, lane: &AdmissionLaneKey, usage: Usage) {
        if let Some(state) = self.lanes.get_mut(lane) {
            state.live_tickets = state
                .live_tickets
                .checked_sub(1)
                .expect("live ticket count matches pending ticket");
            state.usage = state.usage.subtract(usage);
        }
        self.global_usage = self.global_usage.subtract(usage);
    }

    fn shared_used(&self) -> usize {
        self.lanes.values().fold(0, |used, state| {
            used.saturating_add(state.live_tickets.saturating_sub(1))
        })
    }

    fn retain_lane(&mut self, lane: &AdmissionLaneKey, marker: RetainedUncertainty) {
        if let Some(state) = self.lanes.get_mut(lane) {
            state.uncertainty = Some(match state.uncertainty.take() {
                Some(existing) => existing.merge(marker),
                None => marker,
            });
        }
    }

    fn retain_global(&mut self, marker: RetainedUncertainty) {
        self.global_uncertainty = Some(match self.global_uncertainty.take() {
            Some(existing) => existing.merge(marker),
            None => marker,
        });
    }

    fn allocate_generation(&mut self) -> Result<WatcherGeneration, AdmissionError> {
        self.next_generation = self
            .next_generation
            .checked_add(1)
            .ok_or(AdmissionError::GenerationExhausted)?;
        Ok(WatcherGeneration::new(self.next_generation))
    }

    fn lane(&self, lane: &AdmissionLaneKey) -> Result<&LaneState, AdmissionError> {
        self.lanes.get(lane).ok_or(AdmissionError::UnknownLane)
    }

    fn lane_mut(&mut self, lane: &AdmissionLaneKey) -> Result<&mut LaneState, AdmissionError> {
        self.lanes.get_mut(lane).ok_or(AdmissionError::UnknownLane)
    }
}

impl RetainedUncertainty {
    fn new(
        source_id: Option<SourceId>,
        root_identity: Option<RootIdentity>,
        generation: Option<WatcherGeneration>,
        boundary: Option<CaptureBoundary>,
        reason: UncertaintyReason,
    ) -> Self {
        Self::with_reasons(source_id, root_identity, generation, boundary, vec![reason])
    }

    fn with_reasons(
        source_id: Option<SourceId>,
        root_identity: Option<RootIdentity>,
        generation: Option<WatcherGeneration>,
        boundary: Option<CaptureBoundary>,
        mut reasons: Vec<UncertaintyReason>,
    ) -> Self {
        debug_assert!(!reasons.is_empty());
        reasons.sort_unstable();
        reasons.dedup();
        Self {
            source_id,
            root_identity,
            generation,
            boundary,
            scope: ReconciliationScopeKind::SourceAudit,
            reasons,
        }
    }

    fn from_envelope_with_generation(
        envelope: &RawObservationEnvelope,
        generation: Option<WatcherGeneration>,
        reason: UncertaintyReason,
    ) -> Self {
        Self::new(
            Some(envelope.provenance().source_id().clone()),
            envelope.provenance().root_identity().cloned(),
            generation,
            Some(envelope.provenance().capture_boundary()),
            reason,
        )
    }

    fn from_envelope_with_generation_and_reasons(
        envelope: &RawObservationEnvelope,
        generation: Option<WatcherGeneration>,
        reasons: Vec<UncertaintyReason>,
    ) -> Self {
        Self::with_reasons(
            Some(envelope.provenance().source_id().clone()),
            envelope.provenance().root_identity().cloned(),
            generation,
            Some(envelope.provenance().capture_boundary()),
            reasons,
        )
    }

    fn merge(mut self, other: Self) -> Self {
        self.scope = self.scope.widen(other.scope);
        if self.source_id != other.source_id {
            self.source_id = None;
        }
        if self.root_identity != other.root_identity {
            self.root_identity = None;
        }
        if self.generation != other.generation {
            self.generation = None;
        }
        self.boundary = merge_boundaries(self.boundary, other.boundary);
        for reason in other.reasons {
            if !self.reasons.contains(&reason) {
                self.reasons.push(reason);
                self.reasons.sort_unstable();
            }
        }
        self
    }
}

fn merge_boundaries(
    left: Option<CaptureBoundary>,
    right: Option<CaptureBoundary>,
) -> Option<CaptureBoundary> {
    let (Some(left), Some(right)) = (left, right) else {
        return None;
    };
    let first_sequence = match (left.first_sequence(), right.first_sequence()) {
        (Some(left), Some(right)) => Some(left.min(right)),
        _ => None,
    };
    let last_sequence = match (left.last_sequence(), right.last_sequence()) {
        (Some(left), Some(right)) => Some(left.max(right)),
        _ => None,
    };
    CaptureBoundary::try_new(
        left.captured_at().max(right.captured_at()),
        first_sequence,
        last_sequence,
    )
    .ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sample_sources::reconciliation::{
        BackendStreamIdentity, CaptureBoundary, NormalizationReason, Proof, RawEventKind,
        RawObservation, RawObservationEnvelope, RawObservationProvenance, RawObservedPath,
        RawPathRole, ReconciliationScopeKind,
    };

    fn limits(
        max_lanes: usize,
        max_events: usize,
        max_in_flight: usize,
    ) -> ReconciliationAdmissionLimits {
        ReconciliationAdmissionLimits::new(
            max_lanes,
            RawObservationLimits::new(max_events, usize::MAX, usize::MAX).expect("lane limits"),
            RawObservationLimits::new(max_events * 4, usize::MAX, usize::MAX)
                .expect("global limits"),
            max_in_flight,
        )
        .expect("admission limits")
    }

    fn envelope(
        source_id: &SourceId,
        root: Option<&RootIdentity>,
        generation: WatcherGeneration,
        captured_at: u64,
        kind: RawEventKind,
    ) -> RawObservationEnvelope {
        RawObservationEnvelope::try_new(
            RawObservationProvenance::new(
                source_id.clone(),
                root.cloned(),
                Some(BackendStreamIdentity::from_bytes(b"stream".to_vec())),
                generation,
                CaptureBoundary::try_new(captured_at, None, None).expect("boundary"),
            ),
            vec![RawObservation::new(
                kind,
                vec![RawObservedPath::new(
                    "folder/sample.wav".into(),
                    RawPathRole::Subject,
                )],
            )],
            RawObservationLimits::new(8, usize::MAX, usize::MAX).expect("envelope limits"),
        )
        .expect("envelope")
    }

    fn envelope_with_kinds(
        source_id: &SourceId,
        root: &RootIdentity,
        generation: WatcherGeneration,
        captured_at: u64,
        kinds: &[RawEventKind],
    ) -> RawObservationEnvelope {
        envelope_with_optional_root_and_kinds(source_id, Some(root), generation, captured_at, kinds)
    }

    fn envelope_with_optional_root_and_kinds(
        source_id: &SourceId,
        root: Option<&RootIdentity>,
        generation: WatcherGeneration,
        captured_at: u64,
        kinds: &[RawEventKind],
    ) -> RawObservationEnvelope {
        let observations = kinds
            .iter()
            .map(|kind| {
                let paths = if matches!(
                    kind,
                    RawEventKind::Overflow | RawEventKind::Error | RawEventKind::Unsupported
                ) {
                    Vec::new()
                } else {
                    vec![RawObservedPath::new(
                        "folder/sample.wav".into(),
                        RawPathRole::Subject,
                    )]
                };
                RawObservation::new(*kind, paths)
            })
            .collect();
        RawObservationEnvelope::try_new(
            RawObservationProvenance::new(
                source_id.clone(),
                root.cloned(),
                Some(BackendStreamIdentity::from_bytes(b"stream".to_vec())),
                generation,
                CaptureBoundary::try_new(captured_at, None, None).expect("boundary"),
            ),
            observations,
            RawObservationLimits::new(16, usize::MAX, usize::MAX).expect("envelope limits"),
        )
        .expect("envelope")
    }

    fn marker_bearing_envelope(
        source_id: &SourceId,
        root: Option<&RootIdentity>,
        generation: WatcherGeneration,
        captured_at: u64,
    ) -> RawObservationEnvelope {
        envelope_with_optional_root_and_kinds(
            source_id,
            root,
            generation,
            captured_at,
            &[
                RawEventKind::Create,
                RawEventKind::Overflow,
                RawEventKind::Error,
                RawEventKind::Unsupported,
            ],
        )
    }

    fn registered(
        supervisor: &mut ReconciliationAdmissionSupervisor,
        source: &SourceId,
        root: &RootIdentity,
    ) -> (AdmissionLaneKey, WatcherGeneration) {
        let (lane, generation) = supervisor
            .register_lane(source.clone(), root.clone())
            .expect("register lane");
        supervisor
            .begin_capture(&lane, generation)
            .expect("begin capture");
        (lane, generation)
    }

    fn assert_marker_and_fence(marker: &RetainedUncertainty, fence: UncertaintyReason) {
        let mut expected = vec![
            UncertaintyReason::Overflow,
            UncertaintyReason::BackendError,
            UncertaintyReason::Unsupported,
            fence,
        ];
        expected.sort_unstable();
        expected.dedup();
        assert_eq!(marker.reasons(), expected.as_slice());
    }

    fn assert_mixed_marker_dispatch(
        kinds: &[RawEventKind],
        expected_reasons: &[UncertaintyReason],
        expected_marker_scopes: &[NormalizationReason],
    ) {
        let source = SourceId::from_string("source-a");
        let root = RootIdentity::from_bytes(b"root-a".to_vec());
        let mut supervisor = ReconciliationAdmissionSupervisor::new(limits(1, 8, 2));
        let (lane, generation) = registered(&mut supervisor, &source, &root);
        let raw = envelope_with_kinds(&source, &root, generation, 1, kinds);

        let ticket = match supervisor.admit(raw.clone()) {
            AdmissionOutcome::Accepted(ticket) => ticket,
            outcome => panic!("unexpected outcome: {outcome:?}"),
        };
        assert_eq!(supervisor.in_flight(), 1);
        assert_eq!(
            supervisor
                .uncertainty(&lane)
                .expect("lane")
                .expect("marker")
                .reasons(),
            expected_reasons
        );

        let dispatched = supervisor.dispatch_next().expect("dispatch");
        assert_eq!(dispatched.ticket(), ticket);
        assert_eq!(dispatched.normalized().envelope(), &raw);
        assert_eq!(
            dispatched
                .normalized()
                .envelope()
                .observations()
                .iter()
                .map(|observation| observation.kind())
                .collect::<Vec<_>>()
                .as_slice(),
            kinds
        );

        let scopes = dispatched.normalized().scopes();
        assert_eq!(scopes.len(), 1 + expected_marker_scopes.len());
        assert_eq!(scopes[0].kind(), ReconciliationScopeKind::ExactEntry);
        assert!(
            scopes
                .iter()
                .skip(1)
                .all(|scope| scope.kind() == ReconciliationScopeKind::SourceAudit)
        );
        assert_eq!(
            scopes
                .iter()
                .skip(1)
                .map(|scope| scope.reason())
                .collect::<Vec<_>>()
                .as_slice(),
            expected_marker_scopes
        );

        supervisor
            .mark_dispatched(ticket)
            .expect("dispatched phase");
        supervisor.mark_applied(ticket).expect("applied phase");
        supervisor
            .mark_checkpointed(ticket)
            .expect("checkpointed phase");
        assert_eq!(supervisor.in_flight(), 0);
    }

    #[test]
    fn supported_plus_unsupported_is_admitted_with_source_audit_uncertainty() {
        assert_mixed_marker_dispatch(
            &[RawEventKind::Create, RawEventKind::Unsupported],
            &[UncertaintyReason::Unsupported],
            &[NormalizationReason::Unsupported],
        );
    }

    #[test]
    fn supported_plus_overflow_is_admitted_with_source_audit_uncertainty() {
        assert_mixed_marker_dispatch(
            &[RawEventKind::Create, RawEventKind::Overflow],
            &[UncertaintyReason::Overflow],
            &[NormalizationReason::Overflow],
        );
    }

    #[test]
    fn supported_plus_error_is_admitted_with_source_audit_uncertainty() {
        assert_mixed_marker_dispatch(
            &[RawEventKind::Create, RawEventKind::Error],
            &[UncertaintyReason::BackendError],
            &[NormalizationReason::BackendError],
        );
    }

    #[test]
    fn supported_plus_multiple_marker_kinds_retains_all_uncertainty_and_order() {
        assert_mixed_marker_dispatch(
            &[
                RawEventKind::Modify,
                RawEventKind::Unsupported,
                RawEventKind::Overflow,
                RawEventKind::Error,
            ],
            &[
                UncertaintyReason::Overflow,
                UncertaintyReason::BackendError,
                UncertaintyReason::Unsupported,
            ],
            &[
                NormalizationReason::Unsupported,
                NormalizationReason::Overflow,
                NormalizationReason::BackendError,
            ],
        );
    }

    #[test]
    fn marker_only_multiple_marker_kinds_are_rejected_with_all_reasons_retained() {
        let source = SourceId::from_string("source-a");
        let root = RootIdentity::from_bytes(b"root-a".to_vec());
        let mut supervisor = ReconciliationAdmissionSupervisor::new(limits(1, 8, 2));
        let (lane, generation) = registered(&mut supervisor, &source, &root);
        let raw = envelope_with_kinds(
            &source,
            &root,
            generation,
            1,
            &[
                RawEventKind::Overflow,
                RawEventKind::Error,
                RawEventKind::Unsupported,
            ],
        );

        assert_eq!(
            supervisor.admit(raw),
            AdmissionOutcome::Rejected(AdmissionRejectReason::UncertaintyMarkerRetained)
        );
        assert_eq!(supervisor.in_flight(), 0);
        assert!(supervisor.dispatch_next().is_none());
        assert_eq!(
            supervisor
                .uncertainty(&lane)
                .expect("lane")
                .expect("marker")
                .reasons(),
            &[
                UncertaintyReason::Overflow,
                UncertaintyReason::BackendError,
                UncertaintyReason::Unsupported,
            ]
        );
    }

    #[test]
    fn lifecycle_fences_unknown_root_and_stale_generation_evidence() {
        let source = SourceId::from_string("source-a");
        let root = RootIdentity::from_bytes(b"root-a".to_vec());
        let other_source = SourceId::from_string("source-b");
        let other_root = RootIdentity::from_bytes(b"root-b".to_vec());
        let mut supervisor = ReconciliationAdmissionSupervisor::new(limits(1, 8, 2));
        let (lane, generation) = registered(&mut supervisor, &source, &root);

        assert_eq!(
            supervisor.admit(envelope(
                &other_source,
                Some(&other_root),
                generation,
                1,
                RawEventKind::Create,
            )),
            AdmissionOutcome::Rejected(AdmissionRejectReason::UnknownLane)
        );
        assert!(supervisor.global_uncertainty().is_some());
        assert_eq!(
            supervisor.admit(envelope(&source, None, generation, 2, RawEventKind::Create,)),
            AdmissionOutcome::Rejected(AdmissionRejectReason::MissingRoot)
        );
        assert_eq!(
            supervisor.admit(envelope(
                &source,
                Some(&other_root),
                generation,
                3,
                RawEventKind::Create,
            )),
            AdmissionOutcome::Rejected(AdmissionRejectReason::WrongRoot)
        );
        assert_eq!(
            supervisor.admit(envelope(
                &source,
                Some(&root),
                WatcherGeneration::new(generation.get() + 1),
                4,
                RawEventKind::Create,
            )),
            AdmissionOutcome::Rejected(AdmissionRejectReason::StaleGeneration)
        );
        let reasons = supervisor
            .uncertainty(&lane)
            .expect("lane")
            .expect("marker")
            .reasons();
        assert!(reasons.contains(&UncertaintyReason::MissingRoot));
        assert!(reasons.contains(&UncertaintyReason::WrongRoot));
        assert!(reasons.contains(&UncertaintyReason::StaleGeneration));
    }

    #[test]
    fn marker_reasons_survive_every_admission_fence_in_the_correct_lane() {
        let source = SourceId::from_string("source-a");
        let root = RootIdentity::from_bytes(b"root-a".to_vec());
        let other_source = SourceId::from_string("source-b");
        let other_root = RootIdentity::from_bytes(b"root-b".to_vec());

        let mut unknown_supervisor = ReconciliationAdmissionSupervisor::new(limits(1, 16, 2));
        let (lane, generation) = registered(&mut unknown_supervisor, &source, &root);
        assert_eq!(
            unknown_supervisor.admit(marker_bearing_envelope(
                &other_source,
                Some(&other_root),
                generation,
                1,
            )),
            AdmissionOutcome::Rejected(AdmissionRejectReason::UnknownLane)
        );
        assert_marker_and_fence(
            unknown_supervisor
                .global_uncertainty()
                .expect("unknown-lane marker"),
            UncertaintyReason::UnknownLane,
        );
        assert!(
            unknown_supervisor
                .uncertainty(&lane)
                .expect("known lane")
                .is_none()
        );

        let mut known_supervisor = ReconciliationAdmissionSupervisor::new(limits(1, 16, 2));
        let (lane, generation) = registered(&mut known_supervisor, &source, &root);
        assert_eq!(
            known_supervisor.admit(marker_bearing_envelope(&source, None, generation, 2)),
            AdmissionOutcome::Rejected(AdmissionRejectReason::MissingRoot)
        );
        assert_marker_and_fence(
            known_supervisor
                .uncertainty(&lane)
                .expect("lane")
                .expect("missing-root marker"),
            UncertaintyReason::MissingRoot,
        );

        assert_eq!(
            known_supervisor.admit(marker_bearing_envelope(
                &source,
                Some(&other_root),
                generation,
                3,
            )),
            AdmissionOutcome::Rejected(AdmissionRejectReason::WrongRoot)
        );
        let marker = known_supervisor
            .uncertainty(&lane)
            .expect("lane")
            .expect("wrong-root marker");
        assert!(marker.reasons().contains(&UncertaintyReason::MissingRoot));
        assert!(marker.reasons().contains(&UncertaintyReason::WrongRoot));
        assert!(marker.reasons().contains(&UncertaintyReason::Overflow));
        assert!(marker.reasons().contains(&UncertaintyReason::BackendError));
        assert!(marker.reasons().contains(&UncertaintyReason::Unsupported));

        assert_eq!(
            known_supervisor.admit(marker_bearing_envelope(
                &source,
                Some(&root),
                WatcherGeneration::new(generation.get() + 1),
                4,
            )),
            AdmissionOutcome::Rejected(AdmissionRejectReason::StaleGeneration)
        );
        let marker = known_supervisor
            .uncertainty(&lane)
            .expect("lane")
            .expect("stale-generation marker");
        assert!(marker.reasons().contains(&UncertaintyReason::MissingRoot));
        assert!(marker.reasons().contains(&UncertaintyReason::WrongRoot));
        assert!(
            marker
                .reasons()
                .contains(&UncertaintyReason::StaleGeneration)
        );
        assert!(marker.reasons().contains(&UncertaintyReason::Overflow));
        assert!(marker.reasons().contains(&UncertaintyReason::BackendError));
        assert!(marker.reasons().contains(&UncertaintyReason::Unsupported));
        assert!(known_supervisor.global_uncertainty().is_none());

        let mut starting_supervisor = ReconciliationAdmissionSupervisor::new(limits(1, 16, 2));
        let (starting_lane, starting_generation) = starting_supervisor
            .register_lane(source.clone(), root.clone())
            .expect("starting lane");
        assert_eq!(
            starting_supervisor.admit(marker_bearing_envelope(
                &source,
                Some(&root),
                starting_generation,
                5,
            )),
            AdmissionOutcome::Rejected(AdmissionRejectReason::NotCapturing)
        );
        assert_marker_and_fence(
            starting_supervisor
                .uncertainty(&starting_lane)
                .expect("lane")
                .expect("starting marker"),
            UncertaintyReason::NotCapturing,
        );

        let mut stopped_supervisor = ReconciliationAdmissionSupervisor::new(limits(1, 16, 2));
        let (stopped_lane, stopped_generation) =
            registered(&mut stopped_supervisor, &source, &root);
        stopped_supervisor
            .stop_lane(&stopped_lane, stopped_generation)
            .expect("stop lane");
        assert_eq!(
            stopped_supervisor.admit(marker_bearing_envelope(
                &source,
                Some(&root),
                stopped_generation,
                6,
            )),
            AdmissionOutcome::Rejected(AdmissionRejectReason::NotCapturing)
        );
        let marker = stopped_supervisor
            .uncertainty(&stopped_lane)
            .expect("lane")
            .expect("stopped marker");
        assert!(marker.reasons().contains(&UncertaintyReason::NotCapturing));
        assert!(marker.reasons().contains(&UncertaintyReason::Overflow));
        assert!(marker.reasons().contains(&UncertaintyReason::BackendError));
        assert!(marker.reasons().contains(&UncertaintyReason::Unsupported));
        assert!(marker.reasons().contains(&UncertaintyReason::Cancellation));

        let mut saturated_supervisor = ReconciliationAdmissionSupervisor::new(limits(1, 16, 1));
        let (saturated_lane, saturated_generation) =
            registered(&mut saturated_supervisor, &source, &root);
        assert!(matches!(
            saturated_supervisor.admit(envelope(
                &source,
                Some(&root),
                saturated_generation,
                7,
                RawEventKind::Create,
            )),
            AdmissionOutcome::Accepted(_)
        ));
        assert_eq!(
            saturated_supervisor.admit(marker_bearing_envelope(
                &source,
                Some(&root),
                saturated_generation,
                8,
            )),
            AdmissionOutcome::Rejected(AdmissionRejectReason::QueueSaturated)
        );
        assert_marker_and_fence(
            saturated_supervisor
                .uncertainty(&saturated_lane)
                .expect("lane")
                .expect("queue marker"),
            UncertaintyReason::QueueSaturated,
        );
    }

    #[test]
    fn fifo_and_round_robin_dispatch_preserve_raw_proof() {
        let source_a = SourceId::from_string("source-a");
        let root_a = RootIdentity::from_bytes(b"root-a".to_vec());
        let source_b = SourceId::from_string("source-b");
        let root_b = RootIdentity::from_bytes(b"root-b".to_vec());
        let mut supervisor = ReconciliationAdmissionSupervisor::new(limits(2, 8, 8));
        let (lane_a, generation_a) = registered(&mut supervisor, &source_a, &root_a);
        let (lane_b, generation_b) = registered(&mut supervisor, &source_b, &root_b);

        let a1 = envelope(
            &source_a,
            Some(&root_a),
            generation_a,
            1,
            RawEventKind::Create,
        );
        let a2 = envelope(
            &source_a,
            Some(&root_a),
            generation_a,
            2,
            RawEventKind::Modify,
        );
        let b1 = envelope(
            &source_b,
            Some(&root_b),
            generation_b,
            3,
            RawEventKind::Create,
        );
        let b2 = envelope(
            &source_b,
            Some(&root_b),
            generation_b,
            4,
            RawEventKind::Modify,
        );
        assert!(matches!(
            supervisor.admit(a1),
            AdmissionOutcome::Accepted(_)
        ));
        assert!(matches!(
            supervisor.admit(a2),
            AdmissionOutcome::Accepted(_)
        ));
        assert!(matches!(
            supervisor.admit(b1),
            AdmissionOutcome::Accepted(_)
        ));
        assert!(matches!(
            supervisor.admit(b2),
            AdmissionOutcome::Accepted(_)
        ));

        let first = supervisor.dispatch_next().expect("first dispatch");
        assert_eq!(first.lane(), &lane_a);
        assert_eq!(first.normalized().envelope().proof(), &Proof::Unproven);
        assert_eq!(
            first.normalized().envelope().observations()[0].kind(),
            RawEventKind::Create
        );
        let second = supervisor.dispatch_next().expect("second dispatch");
        assert_eq!(second.lane(), &lane_b);
        let third = supervisor.dispatch_next().expect("third dispatch");
        assert_eq!(third.lane(), &lane_a);
        assert_eq!(
            third.normalized().envelope().observations()[0].kind(),
            RawEventKind::Modify
        );
        let fourth = supervisor.dispatch_next().expect("fourth dispatch");
        assert_eq!(fourth.lane(), &lane_b);
    }

    #[test]
    fn capacity_validation_requires_one_envelope_slot_per_possible_lane() {
        let per_lane = RawObservationLimits::new(8, usize::MAX, usize::MAX).expect("limits");
        let global = RawObservationLimits::new(16, usize::MAX, usize::MAX).expect("limits");
        assert_eq!(
            ReconciliationAdmissionLimits::new(2, per_lane, global, 1),
            Err(AdmissionError::InsufficientEnvelopeCapacity)
        );
    }

    #[test]
    fn max_in_flight_equal_to_lane_limit_gives_each_lane_one_reservation() {
        let source_a = SourceId::from_string("source-a");
        let root_a = RootIdentity::from_bytes(b"root-a".to_vec());
        let source_b = SourceId::from_string("source-b");
        let root_b = RootIdentity::from_bytes(b"root-b".to_vec());
        let mut supervisor = ReconciliationAdmissionSupervisor::new(limits(2, 8, 2));
        let (_lane_a, generation_a) = registered(&mut supervisor, &source_a, &root_a);
        let (_lane_b, generation_b) = registered(&mut supervisor, &source_b, &root_b);

        assert!(matches!(
            supervisor.admit(envelope(
                &source_a,
                Some(&root_a),
                generation_a,
                1,
                RawEventKind::Create,
            )),
            AdmissionOutcome::Accepted(_)
        ));
        assert_eq!(
            supervisor.admit(envelope(
                &source_a,
                Some(&root_a),
                generation_a,
                2,
                RawEventKind::Modify,
            )),
            AdmissionOutcome::Rejected(AdmissionRejectReason::QueueSaturated)
        );
        assert!(matches!(
            supervisor.admit(envelope(
                &source_b,
                Some(&root_b),
                generation_b,
                3,
                RawEventKind::Create,
            )),
            AdmissionOutcome::Accepted(_)
        ));
        assert_eq!(supervisor.in_flight(), 2);
    }

    #[test]
    fn noisy_lane_can_use_shared_pool_without_consuming_another_lane_reservation() {
        let source_a = SourceId::from_string("source-a");
        let root_a = RootIdentity::from_bytes(b"root-a".to_vec());
        let source_b = SourceId::from_string("source-b");
        let root_b = RootIdentity::from_bytes(b"root-b".to_vec());
        let mut supervisor = ReconciliationAdmissionSupervisor::new(limits(2, 8, 4));
        let (_lane_a, generation_a) = registered(&mut supervisor, &source_a, &root_a);
        let (_lane_b, generation_b) = registered(&mut supervisor, &source_b, &root_b);

        for captured_at in 1..=3 {
            assert!(matches!(
                supervisor.admit(envelope(
                    &source_a,
                    Some(&root_a),
                    generation_a,
                    captured_at,
                    RawEventKind::Create,
                )),
                AdmissionOutcome::Accepted(_)
            ));
        }
        assert_eq!(
            supervisor.admit(envelope(
                &source_a,
                Some(&root_a),
                generation_a,
                4,
                RawEventKind::Modify,
            )),
            AdmissionOutcome::Rejected(AdmissionRejectReason::QueueSaturated)
        );
        assert!(matches!(
            supervisor.admit(envelope(
                &source_b,
                Some(&root_b),
                generation_b,
                5,
                RawEventKind::Create,
            )),
            AdmissionOutcome::Accepted(_)
        ));
        assert_eq!(supervisor.in_flight(), 4);
    }

    #[test]
    fn envelope_capacity_spans_dispatch_phases_and_releases_on_checkpoint_or_invalidation() {
        let source = SourceId::from_string("source-a");
        let root = RootIdentity::from_bytes(b"root-a".to_vec());
        let mut supervisor = ReconciliationAdmissionSupervisor::new(limits(1, 8, 1));
        let (lane, generation) = registered(&mut supervisor, &source, &root);
        let ticket = match supervisor.admit(envelope(
            &source,
            Some(&root),
            generation,
            1,
            RawEventKind::Create,
        )) {
            AdmissionOutcome::Accepted(ticket) => ticket,
            outcome => panic!("unexpected outcome: {outcome:?}"),
        };

        let assert_saturated = |supervisor: &mut ReconciliationAdmissionSupervisor, captured_at| {
            assert_eq!(
                supervisor.admit(envelope(
                    &source,
                    Some(&root),
                    generation,
                    captured_at,
                    RawEventKind::Modify,
                )),
                AdmissionOutcome::Rejected(AdmissionRejectReason::QueueSaturated)
            );
        };
        assert_saturated(&mut supervisor, 2);
        let dispatched = supervisor.dispatch_next().expect("dispatch");
        assert_eq!(dispatched.ticket(), ticket);
        assert_saturated(&mut supervisor, 3);
        supervisor.mark_dispatched(ticket).expect("dispatched");
        assert_saturated(&mut supervisor, 4);
        supervisor.mark_applied(ticket).expect("applied");
        assert_saturated(&mut supervisor, 5);
        supervisor.mark_checkpointed(ticket).expect("checkpointed");
        assert_eq!(supervisor.in_flight(), 0);
        assert!(matches!(
            supervisor.admit(envelope(
                &source,
                Some(&root),
                generation,
                6,
                RawEventKind::Create,
            )),
            AdmissionOutcome::Accepted(_)
        ));

        supervisor
            .stop_lane(&lane, generation)
            .expect("stop and invalidate");
        assert_eq!(supervisor.in_flight(), 0);
        let restarted_generation = supervisor.restart_lane(&lane).expect("restart");
        supervisor
            .begin_capture(&lane, restarted_generation)
            .expect("begin capture");
        assert!(matches!(
            supervisor.admit(envelope(
                &source,
                Some(&root),
                restarted_generation,
                7,
                RawEventKind::Create,
            )),
            AdmissionOutcome::Accepted(_)
        ));
    }

    #[test]
    fn accounting_includes_in_flight_work_and_retained_markers_are_out_of_band() {
        let source = SourceId::from_string("source-a");
        let root = RootIdentity::from_bytes(b"root-a".to_vec());
        let mut supervisor = ReconciliationAdmissionSupervisor::new(limits(1, 8, 2));
        let (lane, generation) = registered(&mut supervisor, &source, &root);
        let first = envelope(&source, Some(&root), generation, 1, RawEventKind::Create);
        assert!(matches!(
            supervisor.admit(first),
            AdmissionOutcome::Accepted(_)
        ));
        assert_eq!(supervisor.in_flight(), 1);
        assert!(matches!(
            supervisor.admit(envelope(
                &source,
                Some(&root),
                generation,
                2,
                RawEventKind::Modify,
            )),
            AdmissionOutcome::Accepted(_)
        ));
        assert_eq!(supervisor.in_flight(), 2);
        assert_eq!(
            supervisor.admit(envelope(
                &source,
                Some(&root),
                generation,
                3,
                RawEventKind::Modify,
            )),
            AdmissionOutcome::Rejected(AdmissionRejectReason::QueueSaturated)
        );
        assert_eq!(
            supervisor.admit(marker_bearing_envelope(&source, Some(&root), generation, 4,)),
            AdmissionOutcome::Rejected(AdmissionRejectReason::QueueSaturated)
        );
        let marker = supervisor
            .uncertainty(&lane)
            .expect("lane")
            .expect("marker");
        assert!(
            marker
                .reasons()
                .contains(&UncertaintyReason::QueueSaturated)
        );
        assert!(marker.reasons().contains(&UncertaintyReason::Overflow));
    }

    #[test]
    fn stop_and_rebind_invalidate_old_tickets_before_new_capture() {
        let source = SourceId::from_string("source-a");
        let root = RootIdentity::from_bytes(b"root-a".to_vec());
        let new_root = RootIdentity::from_bytes(b"root-b".to_vec());
        let mut supervisor = ReconciliationAdmissionSupervisor::new(limits(1, 8, 2));
        let (lane, generation) = registered(&mut supervisor, &source, &root);
        let old_ticket = match supervisor.admit(envelope(
            &source,
            Some(&root),
            generation,
            1,
            RawEventKind::Create,
        )) {
            AdmissionOutcome::Accepted(ticket) => ticket,
            outcome => panic!("unexpected outcome: {outcome:?}"),
        };
        let (new_lane, new_generation) = supervisor
            .rebind_lane(&lane, generation, new_root.clone())
            .expect("rebind");
        assert!(new_generation > generation);
        assert_eq!(supervisor.in_flight(), 0);
        assert_eq!(
            supervisor.mark_dispatched(old_ticket),
            Err(AdmissionError::UnknownTicket)
        );
        assert!(
            supervisor
                .uncertainty(&new_lane)
                .expect("new lane")
                .expect("rebind marker")
                .reasons()
                .contains(&UncertaintyReason::Rebind)
        );
        assert_eq!(
            supervisor.admit(envelope(
                &source,
                Some(&new_root),
                new_generation,
                2,
                RawEventKind::Create,
            )),
            AdmissionOutcome::Rejected(AdmissionRejectReason::NotCapturing)
        );
        supervisor
            .begin_capture(&new_lane, new_generation)
            .expect("new capture");
        assert!(matches!(
            supervisor.admit(envelope(
                &source,
                Some(&new_root),
                new_generation,
                3,
                RawEventKind::Create,
            )),
            AdmissionOutcome::Accepted(_)
        ));
    }

    #[test]
    fn starting_can_stop_and_restart_with_uncertainty() {
        let source = SourceId::from_string("source-a");
        let root = RootIdentity::from_bytes(b"root-a".to_vec());
        let mut supervisor = ReconciliationAdmissionSupervisor::new(limits(1, 8, 2));
        let (lane, generation) = supervisor
            .register_lane(source, root)
            .expect("register lane");
        supervisor
            .stop_lane(&lane, generation)
            .expect("stop startup");
        assert_eq!(
            supervisor.lifecycle(&lane).expect("lifecycle"),
            ReconciliationLifecycle::Stopped
        );
        assert!(supervisor.uncertainty(&lane).expect("lane").is_some());
        let restarted_generation = supervisor.restart_lane(&lane).expect("restart");
        assert!(restarted_generation > generation);
        assert_eq!(
            supervisor.lifecycle(&lane).expect("lifecycle"),
            ReconciliationLifecycle::Starting
        );
    }

    #[test]
    fn stop_cancels_work_and_restart_requires_new_capture() {
        let source = SourceId::from_string("source-a");
        let root = RootIdentity::from_bytes(b"root-a".to_vec());
        let mut supervisor = ReconciliationAdmissionSupervisor::new(limits(1, 8, 2));
        let (lane, generation) = registered(&mut supervisor, &source, &root);
        let ticket = match supervisor.admit(envelope(
            &source,
            Some(&root),
            generation,
            1,
            RawEventKind::Create,
        )) {
            AdmissionOutcome::Accepted(ticket) => ticket,
            outcome => panic!("unexpected outcome: {outcome:?}"),
        };
        supervisor
            .stop_lane(&lane, generation)
            .expect("stop capture");
        assert_eq!(supervisor.in_flight(), 0);
        assert_eq!(
            supervisor.mark_dispatched(ticket),
            Err(AdmissionError::UnknownTicket)
        );
        assert!(
            supervisor
                .uncertainty(&lane)
                .expect("lane")
                .expect("cancellation marker")
                .reasons()
                .contains(&UncertaintyReason::Cancellation)
        );
        let restarted_generation = supervisor.restart_lane(&lane).expect("restart");
        assert!(restarted_generation > generation);
        assert_eq!(
            supervisor.admit(envelope(
                &source,
                Some(&root),
                restarted_generation,
                2,
                RawEventKind::Create,
            )),
            AdmissionOutcome::Rejected(AdmissionRejectReason::NotCapturing)
        );
        supervisor
            .begin_capture(&lane, restarted_generation)
            .expect("resume capture");
        assert!(matches!(
            supervisor.admit(envelope(
                &source,
                Some(&root),
                restarted_generation,
                3,
                RawEventKind::Create,
            )),
            AdmissionOutcome::Accepted(_)
        ));
    }

    #[test]
    fn idle_stop_and_restart_retain_a_generation_uncertainty_marker() {
        let source = SourceId::from_string("source-a");
        let root = RootIdentity::from_bytes(b"root-a".to_vec());
        let mut supervisor = ReconciliationAdmissionSupervisor::new(limits(1, 8, 2));
        let (lane, generation) = registered(&mut supervisor, &source, &root);
        supervisor
            .stop_lane(&lane, generation)
            .expect("stop capture");
        assert!(
            supervisor
                .uncertainty(&lane)
                .expect("lane")
                .expect("idle stop marker")
                .reasons()
                .contains(&UncertaintyReason::Cancellation)
        );
        let restarted_generation = supervisor.restart_lane(&lane).expect("restart");
        supervisor
            .begin_capture(&lane, restarted_generation)
            .expect("resume capture");
        assert!(supervisor.uncertainty(&lane).expect("lane").is_some());
    }

    #[test]
    fn generation_exhaustion_does_not_destroy_the_old_lane() {
        let source = SourceId::from_string("source-a");
        let root = RootIdentity::from_bytes(b"root-a".to_vec());
        let new_root = RootIdentity::from_bytes(b"root-b".to_vec());
        let mut supervisor = ReconciliationAdmissionSupervisor::new(limits(1, 8, 2));
        let (lane, generation) = registered(&mut supervisor, &source, &root);
        let ticket = match supervisor.admit(envelope(
            &source,
            Some(&root),
            generation,
            1,
            RawEventKind::Create,
        )) {
            AdmissionOutcome::Accepted(ticket) => ticket,
            outcome => panic!("unexpected outcome: {outcome:?}"),
        };
        supervisor.next_generation = u64::MAX;
        assert_eq!(
            supervisor.rebind_lane(&lane, generation, new_root),
            Err(AdmissionError::GenerationExhausted)
        );
        assert_eq!(supervisor.generation(&lane).expect("old lane"), generation);
        assert_eq!(
            supervisor.lifecycle(&lane).expect("old lifecycle"),
            ReconciliationLifecycle::Capturing
        );
        assert_eq!(supervisor.in_flight(), 1);
        let dispatched = supervisor.dispatch_next().expect("old work remains");
        assert_eq!(dispatched.ticket(), ticket);
        supervisor.mark_dispatched(ticket).expect("dispatched");
        supervisor.mark_applied(ticket).expect("applied");
        supervisor.mark_checkpointed(ticket).expect("checkpointed");
        assert_eq!(supervisor.in_flight(), 0);
    }
}
