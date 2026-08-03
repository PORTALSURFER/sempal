//! Pure, backend-neutral Finder observation modeling and normalization.

mod admission;
mod model;
mod normalize;

#[cfg(test)]
mod tests;

pub use admission::{
    AdmissionError, AdmissionLaneKey, AdmissionOutcome, AdmissionRejectReason,
    CommittedAuthoritativeReconciliationAcknowledgement, DispatchPhase, DispatchTicket,
    DispatchedObservation, ReconciliationAcknowledgementIdentity,
    ReconciliationAcknowledgementOutcome, ReconciliationAdmissionLimits,
    ReconciliationAdmissionSupervisor, ReconciliationLifecycle, RetainedUncertainty,
    RetainedUncertaintyBoundary, UncertaintyReason,
};
pub use model::{
    BackendStreamIdentity, CaptureBoundary, CaptureSequenceEvidence, CaptureSequenceRange,
    DurablePriorAcknowledgement, ObservationUncertainty, Proof, RawEnvelopeCounter,
    RawEnvelopeError, RawEnvelopeLimit, RawEventKind, RawObservation, RawObservationAccounting,
    RawObservationEnvelope, RawObservationLimits, RawObservationMetadata, RawObservationProvenance,
    RawObservedPath, RawPathHint, RawPathRole, ReplayCoverage, RootIdentity, RootRelativePath,
    RootRelativePathError, WatcherContinuityProof, WatcherGeneration,
};
pub use normalize::{
    NormalizationReason, NormalizedObservation, ReconciliationScope, ReconciliationScopeKind,
    normalize_observation,
};
