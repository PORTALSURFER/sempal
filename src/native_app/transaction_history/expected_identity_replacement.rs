//! Capability-bound seam for a future expected-identity replacement primitive.
//!
//! This module deliberately contains no namespace mutation.  The production adapter is
//! fail-closed until a platform-specific implementation can prove the complete contract.

use std::fs::File;
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::capacity_gate::VolumeIdentity;
use super::operation_journal::{PreparedFileEvidence, PreparedObjectIdentity};
use super::publication::{
    PublicationSynchronization, PublicationVisibility, WholePublicationAtomicity,
};

mod sealed {
    pub trait Sealed {}
}

/// Stable platform family recorded with a replacement qualification assessment.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ReplacementPlatformFamily {
    Macos,
    Windows,
    Linux,
    Other,
}

impl ReplacementPlatformFamily {
    fn current() -> Self {
        #[cfg(target_os = "macos")]
        {
            return Self::Macos;
        }
        #[cfg(target_os = "windows")]
        {
            return Self::Windows;
        }
        #[cfg(target_os = "linux")]
        {
            return Self::Linux;
        }
        #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
        {
            Self::Other
        }
    }
}

/// Bounded filesystem relationship observed before the read-only assessment.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ObservedFilesystemClassification {
    SameVolume,
    DifferentVolume,
    Unavailable,
}

/// Public candidate primitive considered by the current platform/build.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ReplacementCandidatePrimitive {
    MacosRenameAtxNpRenameExcl,
    MacosRenameAtxNpRenameSwap,
    NoPublicCandidate,
}

/// Semantic result of assessing a candidate without invoking it.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ReplacementCandidateAssessment {
    AbsentFinalOnly,
    SwapWithoutExpectedTargetIdentityOperand,
    NoQualifiedCandidate,
}

/// Stable invariant code explaining why expected-identity replacement is not qualified.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ReplacementMissingInvariant {
    AtomicExpectedTargetIdentityComparison,
}

/// Stable decision recorded for an unsupported expected-identity replacement assessment.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ReplacementQualificationDecision {
    PlatformQualificationRequired,
}

/// Stable condition under which a later attempt may reassess the platform boundary.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ReplacementQualificationRetryCondition {
    PlatformBuildOrQualificationPolicyChange,
}

/// Latest bounded evidence explaining an unsupported replacement assessment.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct ReplacementQualificationAssessment {
    pub(crate) platform_family: ReplacementPlatformFamily,
    pub(crate) observed_filesystem: ObservedFilesystemClassification,
    pub(crate) volume: VolumeIdentity,
    pub(crate) candidate: ReplacementCandidatePrimitive,
    pub(crate) candidate_assessment: ReplacementCandidateAssessment,
    pub(crate) missing_invariant: ReplacementMissingInvariant,
    pub(crate) decision: ReplacementQualificationDecision,
    pub(crate) retry_condition: ReplacementQualificationRetryCondition,
}

fn classify_candidate(
    platform_family: ReplacementPlatformFamily,
    observed_filesystem: ObservedFilesystemClassification,
    volume: &VolumeIdentity,
    candidate: ReplacementCandidatePrimitive,
) -> ReplacementQualificationAssessment {
    let candidate_assessment = match candidate {
        ReplacementCandidatePrimitive::MacosRenameAtxNpRenameExcl => {
            ReplacementCandidateAssessment::AbsentFinalOnly
        }
        ReplacementCandidatePrimitive::MacosRenameAtxNpRenameSwap => {
            ReplacementCandidateAssessment::SwapWithoutExpectedTargetIdentityOperand
        }
        ReplacementCandidatePrimitive::NoPublicCandidate => {
            ReplacementCandidateAssessment::NoQualifiedCandidate
        }
    };
    ReplacementQualificationAssessment {
        platform_family,
        observed_filesystem,
        volume: volume.clone(),
        candidate,
        candidate_assessment,
        missing_invariant: ReplacementMissingInvariant::AtomicExpectedTargetIdentityComparison,
        decision: ReplacementQualificationDecision::PlatformQualificationRequired,
        retry_condition: ReplacementQualificationRetryCondition::PlatformBuildOrQualificationPolicyChange,
    }
}

/// Handles and relative names supplied by the journal owner after all pre-attempt checks.
///
/// The request intentionally has no absolute mutation-authoritative pathname.  The retained
/// target-parent, target, and staging handles are the capabilities an implementation may use;
/// relative names are included only to describe the already-validated namespace entries.
pub(super) struct ExpectedIdentityReplacementRequest<'a> {
    pub(super) target_parent: &'a File,
    pub(super) target: &'a File,
    pub(super) staging: &'a File,
    pub(super) target_leaf: &'a Path,
    pub(super) staging_leaf: &'a Path,
    pub(super) target_parent_identity: &'a PreparedObjectIdentity,
    pub(super) expected_target: &'a PreparedObjectIdentity,
    pub(super) staging_identity: &'a PreparedObjectIdentity,
    pub(super) staging_content: &'a PreparedFileEvidence,
    pub(super) volume: &'a VolumeIdentity,
}

/// A sealed result that contains all evidence needed to construct publication evidence.
///
/// Only adapter implementations in this module can construct this type.  In particular, the
/// journal cannot turn a pathname, metadata-only observation, or arbitrary test fixture into a
/// successful publication claim.
pub(super) struct QualifiedExpectedIdentityReplacement {
    expected_target: PreparedObjectIdentity,
    displaced_target: PreparedObjectIdentity,
    reopened_final: PreparedObjectIdentity,
    reopened_content: PreparedFileEvidence,
    visibility: PublicationVisibility,
    whole_publication: WholePublicationAtomicity,
    synchronization: PublicationSynchronization,
}

impl QualifiedExpectedIdentityReplacement {
    pub(super) fn into_publication_parts(
        self,
    ) -> (
        PreparedObjectIdentity,
        PreparedObjectIdentity,
        PreparedObjectIdentity,
        PreparedFileEvidence,
        PublicationVisibility,
        WholePublicationAtomicity,
        PublicationSynchronization,
    ) {
        (
            self.expected_target,
            self.displaced_target,
            self.reopened_final,
            self.reopened_content,
            self.visibility,
            self.whole_publication,
            self.synchronization,
        )
    }
}

/// Outcome of one invocation of the expected-identity replacement seam.
#[allow(dead_code)]
pub(super) enum ExpectedIdentityReplacementOutcome {
    QualifiedSuccess(QualifiedExpectedIdentityReplacement),
    PlatformQualificationRequired {
        assessment: ReplacementQualificationAssessment,
    },
    Drift { reason: String },
    Ambiguous { reason: String },
}

/// Sealed capability-bound adapter contract.
pub(super) trait ExpectedIdentityReplacementAdapter: sealed::Sealed {
    fn attempt(
        &self,
        request: ExpectedIdentityReplacementRequest<'_>,
    ) -> ExpectedIdentityReplacementOutcome;
}

/// Production adapter.  Assessment is read-only; no platform replacement primitive is qualified
/// in this slice.
pub(super) struct ProductionExpectedIdentityReplacementAdapter;

impl sealed::Sealed for ProductionExpectedIdentityReplacementAdapter {}

impl ExpectedIdentityReplacementAdapter for ProductionExpectedIdentityReplacementAdapter {
    fn attempt(
        &self,
        request: ExpectedIdentityReplacementRequest<'_>,
    ) -> ExpectedIdentityReplacementOutcome {
        let assessment = classify_candidate(
            ReplacementPlatformFamily::current(),
            // The journal has already validated the target, staging, and capacity facts against
            // one descriptor-derived volume identity.  This is an observation, not a filesystem
            // capability or qualification claim.
            ObservedFilesystemClassification::SameVolume,
            request.volume,
            #[cfg(target_os = "macos")]
            ReplacementCandidatePrimitive::MacosRenameAtxNpRenameSwap,
            #[cfg(not(target_os = "macos"))]
            ReplacementCandidatePrimitive::NoPublicCandidate,
        );
        let _ = (
            request.target_parent,
            request.target,
            request.staging,
            request.target_leaf,
            request.staging_leaf,
            request.target_parent_identity,
            request.expected_target,
            request.staging_identity,
            request.staging_content,
        );
        ExpectedIdentityReplacementOutcome::PlatformQualificationRequired { assessment }
    }
}

#[cfg(test)]
fn production_assessment_for_test(
    filesystem: ObservedFilesystemClassification,
    candidate: ReplacementCandidatePrimitive,
) -> ReplacementQualificationAssessment {
    classify_candidate(
        ReplacementPlatformFamily::current(),
        filesystem,
        &VolumeIdentity { device: 1 },
        candidate,
    )
}

#[cfg(test)]
fn assert_unsupported_assessment(
    assessment: &ReplacementQualificationAssessment,
) {
    assert_eq!(
        assessment.missing_invariant,
        ReplacementMissingInvariant::AtomicExpectedTargetIdentityComparison
    );
    assert_eq!(
        assessment.decision,
        ReplacementQualificationDecision::PlatformQualificationRequired
    );
    assert_eq!(
        assessment.retry_condition,
        ReplacementQualificationRetryCondition::PlatformBuildOrQualificationPolicyChange
    );
}

#[cfg(test)]
fn candidate_assessment(
    candidate: ReplacementCandidatePrimitive,
) -> ReplacementCandidateAssessment {
    production_assessment_for_test(ObservedFilesystemClassification::SameVolume, candidate)
        .candidate_assessment
}

#[cfg(test)]
#[derive(Clone, Copy)]
#[allow(dead_code)]
pub(super) enum TestQualifiedAdapterDrift {
    ExpectedTarget,
    DisplacedTarget,
    ReopenedFinal,
    ReopenedContent,
    Visibility,
    Atomicity,
    Synchronization,
}

#[cfg(test)]
/// Test-only adapter used to exercise the sealed adapter-to-journal evidence boundary.
#[allow(dead_code)]
pub(super) struct TestQualifiedExpectedIdentityReplacementAdapter {
    pub(super) drift: Option<TestQualifiedAdapterDrift>,
}

#[cfg(test)]
impl sealed::Sealed for TestQualifiedExpectedIdentityReplacementAdapter {}

#[cfg(test)]
impl ExpectedIdentityReplacementAdapter for TestQualifiedExpectedIdentityReplacementAdapter {
    fn attempt(
        &self,
        request: ExpectedIdentityReplacementRequest<'_>,
    ) -> ExpectedIdentityReplacementOutcome {
        let mut qualified = QualifiedExpectedIdentityReplacement {
            expected_target: request.expected_target.clone(),
            displaced_target: request.expected_target.clone(),
            reopened_final: request.staging_identity.clone(),
            reopened_content: request.staging_content.clone(),
            visibility: PublicationVisibility::VisibilityVerified,
            whole_publication: WholePublicationAtomicity::WholePublicationNonAtomic,
            synchronization: PublicationSynchronization::SyncUnsupportedOrUnverified,
        };

        match self.drift {
            Some(TestQualifiedAdapterDrift::ExpectedTarget) => {
                qualified.expected_target.len = qualified.expected_target.len.saturating_add(1);
            }
            Some(TestQualifiedAdapterDrift::DisplacedTarget) => {
                qualified.displaced_target.len = qualified.displaced_target.len.saturating_add(1);
            }
            Some(TestQualifiedAdapterDrift::ReopenedFinal) => {
                qualified.reopened_final.len = qualified.reopened_final.len.saturating_add(1);
            }
            Some(TestQualifiedAdapterDrift::ReopenedContent) => {
                qualified.reopened_content = PreparedFileEvidence::ContentHash([9; 32]);
            }
            Some(TestQualifiedAdapterDrift::Visibility) => {
                qualified.visibility = PublicationVisibility::VisibilityUnverified;
            }
            Some(TestQualifiedAdapterDrift::Atomicity) => {
                qualified.whole_publication = WholePublicationAtomicity::WholePublicationAtomic;
            }
            Some(TestQualifiedAdapterDrift::Synchronization) => {
                qualified.synchronization = PublicationSynchronization::PowerLossSynchronized;
            }
            None => {}
        }

        ExpectedIdentityReplacementOutcome::QualifiedSuccess(qualified)
    }
}

#[cfg(test)]
pub(super) fn test_qualified_success(
    prepared: &super::operation_journal::PreparedWaveformRestore,
    staged: &super::operation_journal::FilesystemStagedWaveformRestore,
) -> QualifiedExpectedIdentityReplacement {
    let super::operation_journal::FilesystemStagedParticipant::CopyValidated { staging, evidence } =
        &staged.participant;
    let expected_target = match &prepared.replacement {
        super::operation_journal::ReplaceExpectedIdentity::Existing(identity) => identity.clone(),
    };
    QualifiedExpectedIdentityReplacement {
        expected_target: expected_target.clone(),
        displaced_target: expected_target,
        reopened_final: staging.identity.clone(),
        reopened_content: evidence.clone(),
        visibility: PublicationVisibility::VisibilityVerified,
        whole_publication: WholePublicationAtomicity::WholePublicationNonAtomic,
        synchronization: PublicationSynchronization::SyncUnsupportedOrUnverified,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn production_adapter_is_fail_closed() {
        let target_root = File::open(".").expect("current directory");
        let target = target_root.try_clone().expect("target handle");
        let staging = target_root.try_clone().expect("staging handle");
        let root_identity = PreparedObjectIdentity {
            stable_id: String::from("root"),
            change_marker: None,
            len: 0,
        };
        let target_identity = PreparedObjectIdentity {
            stable_id: String::from("target"),
            change_marker: None,
            len: 0,
        };
        let request = ExpectedIdentityReplacementRequest {
            target_parent: &target_root,
            target: &target,
            staging: &staging,
            target_leaf: Path::new("target.wav"),
            staging_leaf: Path::new("stage"),
            target_parent_identity: &root_identity,
            expected_target: &target_identity,
            staging_identity: &target_identity,
            staging_content: &PreparedFileEvidence::ContentHash([1; 32]),
            volume: &VolumeIdentity { device: 1 },
        };
        let ExpectedIdentityReplacementOutcome::PlatformQualificationRequired { assessment } =
            ProductionExpectedIdentityReplacementAdapter.attempt(request)
        else {
            panic!("production assessment must remain qualification-required");
        };
        assert_unsupported_assessment(&assessment);
        assert_eq!(assessment.volume, VolumeIdentity { device: 1 });
    }

    #[test]
    fn candidate_classification_keeps_exclusive_and_swap_semantics_distinct() {
        assert_eq!(
            candidate_assessment(ReplacementCandidatePrimitive::MacosRenameAtxNpRenameExcl),
            ReplacementCandidateAssessment::AbsentFinalOnly
        );
        assert_eq!(
            candidate_assessment(ReplacementCandidatePrimitive::MacosRenameAtxNpRenameSwap),
            ReplacementCandidateAssessment::SwapWithoutExpectedTargetIdentityOperand
        );
    }

    #[test]
    fn filesystem_classification_does_not_qualify_a_candidate() {
        let baseline = production_assessment_for_test(
            ObservedFilesystemClassification::SameVolume,
            ReplacementCandidatePrimitive::MacosRenameAtxNpRenameSwap,
        );
        for filesystem in [
            ObservedFilesystemClassification::DifferentVolume,
            ObservedFilesystemClassification::Unavailable,
        ] {
            let assessment = production_assessment_for_test(
                filesystem,
                ReplacementCandidatePrimitive::MacosRenameAtxNpRenameSwap,
            );
            assert_eq!(assessment.candidate, baseline.candidate);
            assert_eq!(
                assessment.candidate_assessment,
                baseline.candidate_assessment
            );
            assert_eq!(assessment.missing_invariant, baseline.missing_invariant);
            assert_eq!(assessment.decision, baseline.decision);
            assert_eq!(assessment.retry_condition, baseline.retry_condition);
        }
    }

    #[test]
    fn qualification_required_assessment_cannot_construct_qualified_success() {
        let assessment = production_assessment_for_test(
            ObservedFilesystemClassification::SameVolume,
            ReplacementCandidatePrimitive::MacosRenameAtxNpRenameSwap,
        );
        assert_unsupported_assessment(&assessment);
        let outcome = ExpectedIdentityReplacementOutcome::PlatformQualificationRequired {
            assessment,
        };
        assert!(matches!(
            outcome,
            ExpectedIdentityReplacementOutcome::PlatformQualificationRequired { .. }
        ));
    }
}
