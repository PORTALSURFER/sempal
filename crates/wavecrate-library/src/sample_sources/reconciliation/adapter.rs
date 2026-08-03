//! Library-only live and replay admission over the pure reconciliation boundary.
//!
//! Live observations and replay batches that fail continuity validation remain
//! `Proof::Unproven` and retain a conservative `SourceAudit` marker. A valid
//! replay may carry only the checked continuity proof; this adapter never
//! establishes committed source authority.

use std::fmt;

use crate::sample_sources::SourceId;

use super::admission::{
    AdmissionOutcome, DispatchTicket, ReconciliationAcknowledgementIdentity,
    ReconciliationAdmissionSupervisor, RetainedUncertaintyBoundary, UncertaintyReason,
};
use super::model::{
    BackendStreamIdentity, CaptureSequenceEvidence, CaptureSequenceRange,
    DurablePriorAcknowledgement, Proof, RawEnvelopeError, RawEventKind, RawObservation,
    RawObservationEnvelope, RawObservationLimits, RawObservationProvenance, ReplayCoverage,
    RootIdentity, WatcherContinuityProof, WatcherGeneration,
};

/// An owned, ordered batch supplied to the live or replay adapter.
///
/// The adapter retains this exact batch, including raw observation and path order,
/// when bounded envelope construction fails. Live batches are always admitted as
/// `Proof::Unproven`; a replay continuity proof does not establish committed
/// source authority.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SyntheticObservationBatch {
    provenance: RawObservationProvenance,
    observations: Vec<RawObservation>,
    limits: RawObservationLimits,
}

impl SyntheticObservationBatch {
    /// Own a provenance record, ordered raw observations, and their envelope limits.
    pub fn new(
        provenance: RawObservationProvenance,
        observations: Vec<RawObservation>,
        limits: RawObservationLimits,
    ) -> Self {
        Self {
            provenance,
            observations,
            limits,
        }
    }

    /// Borrow the batch provenance without changing it.
    pub const fn provenance(&self) -> &RawObservationProvenance {
        &self.provenance
    }

    /// Borrow raw observations in their original backend delivery order.
    pub fn observations(&self) -> &[RawObservation] {
        &self.observations
    }

    /// Return the limits that will be used to construct the raw envelope.
    pub const fn limits(&self) -> RawObservationLimits {
        self.limits
    }

    /// Consume the batch into its owned envelope-construction parts.
    pub fn into_parts(
        self,
    ) -> (
        RawObservationProvenance,
        Vec<RawObservation>,
        RawObservationLimits,
    ) {
        (self.provenance, self.observations, self.limits)
    }
}

/// A failed adapter envelope construction that retains the original batch.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AdapterError {
    batch: Box<SyntheticObservationBatch>,
    error: RawEnvelopeError,
}

impl AdapterError {
    fn new(batch: SyntheticObservationBatch, error: RawEnvelopeError) -> Self {
        Self {
            batch: Box::new(batch),
            error,
        }
    }

    /// Borrow the exact batch that could not be constructed as an envelope.
    pub const fn batch(&self) -> &SyntheticObservationBatch {
        &self.batch
    }

    /// Borrow the checked envelope construction error.
    pub const fn error(&self) -> &RawEnvelopeError {
        &self.error
    }

    /// Consume the error and recover the exact original batch.
    pub fn into_batch(self) -> SyntheticObservationBatch {
        *self.batch
    }
}

impl fmt::Display for AdapterError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "could not construct reconciliation envelope: {}",
            self.error
        )
    }
}

impl std::error::Error for AdapterError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.error)
    }
}

/// The adapter-level interpretation of an underlying admission outcome.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AdapterDisposition {
    /// A live batch was accepted without promoting it beyond `Proof::Unproven`.
    AdmittedUnproven,
    /// A replay batch was accepted with a checked watcher-continuity proof.
    AdmittedWithContinuity,
    /// An exact recent batch was suppressed without another ticket or marker.
    DuplicateSuppressed,
    /// Evidence remains unproven and requires conservative source audit handling.
    SourceAuditRequired,
    /// The required uncertainty marker could not be retained within capacity.
    UncertaintyCapacityExhausted,
}

/// An adapter disposition together with the unchanged supervisor outcome.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AdapterAdmission {
    disposition: AdapterDisposition,
    outcome: AdmissionOutcome,
}

impl AdapterAdmission {
    fn new(disposition: AdapterDisposition, outcome: AdmissionOutcome) -> Self {
        Self {
            disposition,
            outcome,
        }
    }

    /// Return the adapter's typed disposition.
    pub const fn disposition(&self) -> AdapterDisposition {
        self.disposition
    }

    /// Borrow the underlying supervisor admission outcome.
    pub const fn outcome(&self) -> &AdmissionOutcome {
        &self.outcome
    }

    /// Consume the adapter result and return the underlying supervisor outcome.
    pub fn into_outcome(self) -> AdmissionOutcome {
        self.outcome
    }
}

/// An opaque correlation for one accepted live batch and its retained source-audit marker.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LiveAuditCorrelation {
    ticket: DispatchTicket,
    identity: ReconciliationAcknowledgementIdentity,
    boundary: RetainedUncertaintyBoundary,
}

impl LiveAuditCorrelation {
    pub(crate) fn new(
        ticket: DispatchTicket,
        identity: ReconciliationAcknowledgementIdentity,
        boundary: RetainedUncertaintyBoundary,
    ) -> Self {
        Self {
            ticket,
            identity,
            boundary,
        }
    }

    /// Return the ticket assigned to the accepted live batch.
    pub const fn ticket(&self) -> DispatchTicket {
        self.ticket
    }

    /// Borrow the exact identity bound to the retained source-audit marker.
    pub const fn identity(&self) -> &ReconciliationAcknowledgementIdentity {
        &self.identity
    }

    /// Return the retained source-audit watermark boundary.
    pub const fn boundary(&self) -> RetainedUncertaintyBoundary {
        self.boundary
    }
}

/// A live adapter admission together with its optional retained source-audit correlation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LiveAuditAdmission {
    admission: AdapterAdmission,
    correlation: Option<LiveAuditCorrelation>,
}

impl LiveAuditAdmission {
    fn new(admission: AdapterAdmission, correlation: Option<LiveAuditCorrelation>) -> Self {
        Self {
            admission,
            correlation,
        }
    }

    /// Borrow the existing adapter admission.
    pub const fn admission(&self) -> &AdapterAdmission {
        &self.admission
    }

    /// Borrow the optional opaque live-audit correlation.
    pub const fn correlation(&self) -> Option<&LiveAuditCorrelation> {
        self.correlation.as_ref()
    }

    /// Consume the wrapper and return the existing adapter admission.
    pub fn into_admission(self) -> AdapterAdmission {
        self.admission
    }

    /// Consume the wrapper and return the optional live-audit correlation.
    pub fn into_correlation(self) -> Option<LiveAuditCorrelation> {
        self.correlation
    }
}

/// An opaque identity-bound prior used by the replay adapter.
///
/// The constructor is crate-restricted because the acknowledgement sequence is
/// authority supplied by the owning library boundary, not a bare public `u64`.
/// A continuity proof made from this token still does not establish committed
/// source authority.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReplayPriorToken {
    source_id: SourceId,
    root_identity: RootIdentity,
    backend_stream_identity: BackendStreamIdentity,
    watcher_generation: WatcherGeneration,
    acknowledged_sequence: u64,
}

impl ReplayPriorToken {
    /// Construct an identity-bound prior for crate-owned replay sources and tests.
    #[allow(dead_code)]
    pub(crate) fn new(
        source_id: SourceId,
        root_identity: RootIdentity,
        backend_stream_identity: BackendStreamIdentity,
        watcher_generation: WatcherGeneration,
        acknowledged_sequence: u64,
    ) -> Self {
        Self {
            source_id,
            root_identity,
            backend_stream_identity,
            watcher_generation,
            acknowledged_sequence,
        }
    }

    /// Borrow the source identity bound to this prior.
    pub fn source_id(&self) -> &SourceId {
        &self.source_id
    }

    /// Borrow the physical root identity bound to this prior.
    pub fn root_identity(&self) -> &RootIdentity {
        &self.root_identity
    }

    /// Borrow the backend stream identity bound to this prior.
    pub fn backend_stream_identity(&self) -> &BackendStreamIdentity {
        &self.backend_stream_identity
    }

    /// Return the watcher generation bound to this prior.
    pub const fn watcher_generation(&self) -> WatcherGeneration {
        self.watcher_generation
    }

    /// Return the acknowledged backend sequence bound to this prior.
    pub const fn acknowledged_sequence(&self) -> u64 {
        self.acknowledged_sequence
    }
}

#[derive(Clone, Copy)]
enum AdmissionKind {
    Live,
    ValidReplay,
    InvalidReplay,
}

/// Borrows an existing admission supervisor for library-only live/replay input.
pub struct ReconciliationAdapter<'a> {
    supervisor: &'a mut ReconciliationAdmissionSupervisor,
}

impl<'a> ReconciliationAdapter<'a> {
    /// Borrow a supervisor for pure live and replay admission.
    pub fn new(supervisor: &'a mut ReconciliationAdmissionSupervisor) -> Self {
        Self { supervisor }
    }

    /// Admit a live batch as unproven evidence and retain one SourceAudit marker.
    ///
    /// This method uses only `Proof::Unproven`. It performs no filesystem or
    /// database work and does not establish committed source authority.
    pub fn admit_live(
        &mut self,
        batch: SyntheticObservationBatch,
    ) -> Result<AdapterAdmission, AdapterError> {
        let envelope = try_unproven_envelope(batch)?;
        let outcome = self
            .supervisor
            .admit_with_required_uncertainty(envelope, UncertaintyReason::LiveUnproven);
        Ok(AdapterAdmission::new(
            disposition(AdmissionKind::Live, &outcome),
            outcome,
        ))
    }

    /// Admit a live batch and correlate an accepted result with its new audit marker.
    ///
    /// The correlation carries no authority and is absent when admission is not accepted or
    /// when the required newly retained marker cannot be matched exactly.
    pub fn admit_live_with_correlation(
        &mut self,
        batch: SyntheticObservationBatch,
    ) -> Result<LiveAuditAdmission, AdapterError> {
        let provenance = batch.provenance().clone();
        let uncertainty_start = self.supervisor.uncertainties().len();
        let admission = self.admit_live(batch)?;
        let correlation = match admission.outcome() {
            AdmissionOutcome::Accepted(ticket) => self
                .supervisor
                .uncertainties()
                .get(uncertainty_start..)
                .and_then(|markers| {
                    markers.iter().find(|marker| {
                        marker.source_id() == Some(provenance.source_id())
                            && marker.root_identity() == provenance.root_identity()
                            && marker.generation() == Some(provenance.watcher_generation())
                            && marker.reasons().contains(&UncertaintyReason::LiveUnproven)
                    })
                })
                .and_then(|marker| {
                    provenance.root_identity().map(|root_identity| {
                        LiveAuditCorrelation::new(
                            *ticket,
                            ReconciliationAcknowledgementIdentity::new(
                                provenance.source_id().clone(),
                                root_identity.clone(),
                                provenance.watcher_generation(),
                            ),
                            marker.boundary(),
                        )
                    })
                }),
            _ => None,
        };
        Ok(LiveAuditAdmission::new(admission, correlation))
    }

    /// Admit replay after validating its identity and contiguous capture claim.
    ///
    /// A missing, ambiguous, gapped, mismatched, or uncertain replay is rebuilt
    /// as `Proof::Unproven` and retained for conservative `SourceAudit`. Only a
    /// fully validated replay receives `WatcherContinuityProof`; neither branch
    /// establishes committed source authority.
    pub fn admit_replay(
        &mut self,
        batch: SyntheticObservationBatch,
        prior: Option<&ReplayPriorToken>,
        contiguous: bool,
    ) -> Result<AdapterAdmission, AdapterError> {
        let (kind, envelope, required_reason) = match prior {
            None => (
                AdmissionKind::InvalidReplay,
                try_unproven_envelope(batch)?,
                Some(UncertaintyReason::ReplayContinuityMissing),
            ),
            Some(prior) => match validate_replay(&batch, Some(prior), contiguous) {
                Ok(range) => (
                    AdmissionKind::ValidReplay,
                    try_replay_envelope(batch, prior, range, contiguous)?,
                    None,
                ),
                Err(reason) => (
                    AdmissionKind::InvalidReplay,
                    try_unproven_envelope(batch)?,
                    Some(reason),
                ),
            },
        };
        let outcome = match required_reason {
            Some(reason) => self
                .supervisor
                .admit_with_required_uncertainty(envelope, reason),
            None => self.supervisor.admit(envelope),
        };
        Ok(AdapterAdmission::new(disposition(kind, &outcome), outcome))
    }
}

fn disposition(kind: AdmissionKind, outcome: &AdmissionOutcome) -> AdapterDisposition {
    match outcome {
        AdmissionOutcome::Accepted(_) => match kind {
            AdmissionKind::Live => AdapterDisposition::AdmittedUnproven,
            AdmissionKind::ValidReplay => AdapterDisposition::AdmittedWithContinuity,
            AdmissionKind::InvalidReplay => AdapterDisposition::SourceAuditRequired,
        },
        AdmissionOutcome::DuplicateSuppressed(_) => AdapterDisposition::DuplicateSuppressed,
        AdmissionOutcome::Rejected(_) => AdapterDisposition::SourceAuditRequired,
        AdmissionOutcome::UncertaintyCapacityExhausted(_) => {
            AdapterDisposition::UncertaintyCapacityExhausted
        }
    }
}

fn try_unproven_envelope(
    batch: SyntheticObservationBatch,
) -> Result<RawObservationEnvelope, AdapterError> {
    let original = batch.clone();
    let (provenance, observations, limits) = batch.into_parts();
    RawObservationEnvelope::try_new(provenance, observations, limits)
        .map_err(|error| AdapterError::new(original, error))
}

fn try_replay_envelope(
    batch: SyntheticObservationBatch,
    prior: &ReplayPriorToken,
    range: CaptureSequenceRange,
    contiguous: bool,
) -> Result<RawObservationEnvelope, AdapterError> {
    let original = batch.clone();
    let provenance = batch.provenance().clone();
    let acknowledgement = DurablePriorAcknowledgement::new(prior.acknowledged_sequence);
    let coverage = ReplayCoverage::try_new(prior.acknowledged_sequence, range.last(), contiguous)
        .map_err(|error| AdapterError::new(original.clone(), error))?;
    let proof = WatcherContinuityProof::try_new(&provenance, Some(acknowledgement), Some(coverage))
        .map_err(|error| AdapterError::new(original.clone(), error))?;
    let (provenance, observations, limits) = batch.into_parts();
    RawObservationEnvelope::try_new_with_proof(
        provenance,
        observations,
        limits,
        Proof::WatcherContinuity(proof),
    )
    .map_err(|error| AdapterError::new(original, error))
}

fn validate_replay(
    batch: &SyntheticObservationBatch,
    prior: Option<&ReplayPriorToken>,
    contiguous: bool,
) -> Result<CaptureSequenceRange, UncertaintyReason> {
    let Some(prior) = prior else {
        return Err(UncertaintyReason::ReplayContinuityMissing);
    };
    let provenance = batch.provenance();
    if provenance.root_identity().is_none() || provenance.backend_stream_identity().is_none() {
        return Err(UncertaintyReason::ReplayContinuityMissing);
    }
    if provenance.source_id() != prior.source_id()
        || provenance.root_identity() != Some(prior.root_identity())
        || provenance.backend_stream_identity() != Some(prior.backend_stream_identity())
        || provenance.watcher_generation() != prior.watcher_generation()
    {
        return Err(UncertaintyReason::ReplayContinuityMismatch);
    }

    let range = match provenance.capture_boundary().sequence_evidence() {
        CaptureSequenceEvidence::Missing => {
            return Err(UncertaintyReason::ReplayContinuityMissing);
        }
        CaptureSequenceEvidence::Ambiguous => {
            return Err(UncertaintyReason::ReplayContinuityAmbiguous);
        }
        CaptureSequenceEvidence::Exact(range) => range,
    };
    if !contiguous {
        return Err(UncertaintyReason::ReplayContinuityGap);
    }
    let Some(expected_first) = prior.acknowledged_sequence.checked_add(1) else {
        return Err(UncertaintyReason::ReplayContinuityGap);
    };
    if range.first() != expected_first || range.last() <= prior.acknowledged_sequence {
        return Err(UncertaintyReason::ReplayContinuityGap);
    }
    if batch.observations().iter().any(|observation| {
        matches!(
            observation.kind(),
            RawEventKind::RootChanged
                | RawEventKind::Overflow
                | RawEventKind::Error
                | RawEventKind::Unsupported
        ) || !observation.uncertainty().is_empty()
    }) {
        return Err(UncertaintyReason::ReplayContinuityMismatch);
    }
    Ok(range)
}
