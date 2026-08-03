//! Pure, backend-neutral Finder observation modeling and normalization.

mod admission;
mod model;
mod normalize;

#[cfg(test)]
mod tests;

pub use admission::{
    AdmissionError, AdmissionLaneKey, AdmissionOutcome, AdmissionRejectReason, DispatchPhase,
    DispatchTicket, DispatchedObservation, ReconciliationAdmissionLimits,
    ReconciliationAdmissionSupervisor, ReconciliationLifecycle, RetainedUncertainty,
    UncertaintyReason,
};
pub use model::{
    BackendStreamIdentity, CaptureBoundary, DurablePriorAcknowledgement, ObservationUncertainty,
    Proof, RawEnvelopeCounter, RawEnvelopeError, RawEnvelopeLimit, RawEventKind, RawObservation,
    RawObservationAccounting, RawObservationEnvelope, RawObservationLimits, RawObservationMetadata,
    RawObservationProvenance, RawObservedPath, RawPathHint, RawPathRole, ReplayCoverage,
    RootIdentity, RootRelativePath, RootRelativePathError, WatcherContinuityProof,
    WatcherGeneration,
};
pub use normalize::{
    NormalizationReason, NormalizedObservation, ReconciliationScope, ReconciliationScopeKind,
    normalize_observation,
};
