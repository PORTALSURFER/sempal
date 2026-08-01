//! Capability-bound seam for a future expected-identity replacement primitive.
//!
//! This module deliberately contains no namespace mutation.  The production adapter is
//! fail-closed until a platform-specific implementation can prove the complete contract.

use std::fs::File;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

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
        retry_condition:
            ReplacementQualificationRetryCondition::PlatformBuildOrQualificationPolicyChange,
    }
}

/// Filesystem-free durable expectations handed from the journal owner to the physical file
/// owner. The request contains no descriptor, store, or mutation authority.
pub(in crate::native_app) struct ExpectedIdentityReplacementOwnerRequest {
    operation_id: Uuid,
    target_root_path: PathBuf,
    target_leaf: PathBuf,
    staging_leaf: PathBuf,
    expected_target_root: PreparedObjectIdentity,
    expected_target: PreparedObjectIdentity,
    expected_staging: PreparedObjectIdentity,
    expected_target_content: PreparedFileEvidence,
    expected_staging_content: PreparedFileEvidence,
    expected_volume: VolumeIdentity,
}

impl ExpectedIdentityReplacementOwnerRequest {
    /// Construct an owned request only from a complete, already-validated durable snapshot.
    pub(super) fn try_new(
        operation_id: Uuid,
        target_root_path: PathBuf,
        target_leaf: PathBuf,
        staging_leaf: PathBuf,
        expected_target_root: PreparedObjectIdentity,
        expected_target: PreparedObjectIdentity,
        expected_staging: PreparedObjectIdentity,
        expected_target_content: PreparedFileEvidence,
        expected_staging_content: PreparedFileEvidence,
        expected_volume: VolumeIdentity,
    ) -> Result<Self, String> {
        if !target_root_path.is_absolute() {
            return Err(String::from("target root locator must be absolute"));
        }
        if !is_single_clean_normal_leaf(&target_leaf) {
            return Err(String::from(
                "target locator must be a single clean normal leaf",
            ));
        }
        if !is_single_clean_normal_leaf(&staging_leaf) {
            return Err(String::from(
                "staging locator must be a single clean normal leaf",
            ));
        }
        if target_leaf == staging_leaf {
            return Err(String::from("target and staging locators must be distinct"));
        }
        if expected_target_root.stable_id.is_empty()
            || expected_target.stable_id.is_empty()
            || expected_staging.stable_id.is_empty()
        {
            return Err(String::from(
                "expected filesystem identities must not be empty",
            ));
        }
        Ok(Self {
            operation_id,
            target_root_path,
            target_leaf,
            staging_leaf,
            expected_target_root,
            expected_target,
            expected_staging,
            expected_target_content,
            expected_staging_content,
            expected_volume,
        })
    }

    #[cfg(test)]
    pub(super) fn replace_expected_volume_for_test(&mut self, volume: VolumeIdentity) {
        self.expected_volume = volume;
    }

    #[cfg(test)]
    pub(super) fn replace_expected_target_root_for_test(
        &mut self,
        identity: PreparedObjectIdentity,
    ) {
        self.expected_target_root = identity;
    }
}

/// Handles and relative names supplied by the journal owner after all pre-attempt checks.
///
/// The request intentionally has no absolute mutation-authoritative pathname.  The retained
/// target-parent, target, and staging handles are the capabilities an implementation may use;
/// relative names are included only to describe the already-validated namespace entries.
struct ExpectedIdentityReplacementRequest<'a> {
    target_parent: &'a File,
    target: &'a File,
    staging: &'a File,
    target_leaf: &'a Path,
    staging_leaf: &'a Path,
    target_parent_identity: &'a PreparedObjectIdentity,
    expected_target: &'a PreparedObjectIdentity,
    staging_identity: &'a PreparedObjectIdentity,
    staging_content: &'a PreparedFileEvidence,
    volume: &'a VolumeIdentity,
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
    Drift {
        reason: String,
    },
    Ambiguous {
        reason: String,
    },
}

/// Opaque result returned by the physical file owner. It is deliberately neither cloneable nor
/// serializable; only the operation-bound coordinator may consume it.
pub(in crate::native_app) struct OperationBoundExpectedIdentityReplacementResult {
    operation_id: Uuid,
    outcome: ExpectedIdentityReplacementOutcome,
}

#[derive(Debug, Eq, PartialEq)]
pub(super) enum ExpectedIdentityReplacementResultError {
    OperationIdDrift,
}

impl OperationBoundExpectedIdentityReplacementResult {
    pub(super) fn operation_id(&self) -> Uuid {
        self.operation_id
    }

    pub(super) fn into_outcome(
        self,
        expected_operation_id: Uuid,
    ) -> Result<ExpectedIdentityReplacementOutcome, ExpectedIdentityReplacementResultError> {
        if self.operation_id != expected_operation_id {
            return Err(ExpectedIdentityReplacementResultError::OperationIdDrift);
        }
        Ok(self.outcome)
    }

    #[cfg(test)]
    pub(super) fn replace_operation_id_for_test(mut self, operation_id: Uuid) -> Self {
        self.operation_id = operation_id;
        self
    }
}

/// Sealed capability-bound adapter contract.
trait ExpectedIdentityReplacementAdapter: sealed::Sealed {
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

struct ReacquiredExpectedIdentityRestore {
    target_root: File,
    target_parent: File,
    target: File,
    staging: File,
    target_parent_identity: PreparedObjectIdentity,
    target_identity: PreparedObjectIdentity,
    staging_identity: PreparedObjectIdentity,
    staging_content: PreparedFileEvidence,
    volume: VolumeIdentity,
}

/// Execute the expected-identity attempt at the physical file-owner boundary. All descriptor
/// acquisition and live validation happen here, before the private borrowed adapter request is
/// constructed. The current production adapter remains qualification-only and never mutates.
pub(in crate::native_app) fn acquire_expected_identity_publication(
    request: ExpectedIdentityReplacementOwnerRequest,
) -> OperationBoundExpectedIdentityReplacementResult {
    acquire_expected_identity_publication_with_adapter(
        request,
        &ProductionExpectedIdentityReplacementAdapter,
    )
}

fn acquire_expected_identity_publication_with_adapter<A>(
    request: ExpectedIdentityReplacementOwnerRequest,
    adapter: &A,
) -> OperationBoundExpectedIdentityReplacementResult
where
    A: ExpectedIdentityReplacementAdapter,
{
    let operation_id = request.operation_id;

    #[cfg(not(unix))]
    {
        return OperationBoundExpectedIdentityReplacementResult {
            operation_id,
            outcome: ExpectedIdentityReplacementOutcome::PlatformQualificationRequired {
                assessment: classify_candidate(
                    ReplacementPlatformFamily::current(),
                    ObservedFilesystemClassification::Unavailable,
                    &request.expected_volume,
                    ReplacementCandidatePrimitive::NoPublicCandidate,
                ),
            },
        };
    }

    #[cfg(unix)]
    let outcome = match reacquire_expected_identity_restore(&request) {
        Ok(reacquired) => {
            let borrowed_request = ExpectedIdentityReplacementRequest {
                target_parent: &reacquired.target_parent,
                target: &reacquired.target,
                staging: &reacquired.staging,
                target_leaf: &request.target_leaf,
                staging_leaf: &request.staging_leaf,
                target_parent_identity: &reacquired.target_parent_identity,
                expected_target: &reacquired.target_identity,
                staging_identity: &reacquired.staging_identity,
                staging_content: &reacquired.staging_content,
                volume: &reacquired.volume,
            };
            let _target_root = &reacquired.target_root;
            adapter.attempt(borrowed_request)
        }
        Err(reason) => ExpectedIdentityReplacementOutcome::Drift { reason },
    };

    OperationBoundExpectedIdentityReplacementResult {
        operation_id,
        outcome,
    }
}

#[cfg(test)]
pub(super) fn acquire_expected_identity_publication_with_test_adapter(
    request: ExpectedIdentityReplacementOwnerRequest,
    adapter: &TestQualifiedExpectedIdentityReplacementAdapter,
) -> OperationBoundExpectedIdentityReplacementResult {
    acquire_expected_identity_publication_with_adapter(request, adapter)
}

#[cfg(unix)]
fn reacquire_expected_identity_restore(
    request: &ExpectedIdentityReplacementOwnerRequest,
) -> Result<ReacquiredExpectedIdentityRestore, String> {
    if !is_single_clean_normal_leaf(&request.target_leaf)
        || !is_single_clean_normal_leaf(&request.staging_leaf)
    {
        return Err(String::from(
            "expected-identity publication locator is not a clean leaf",
        ));
    }

    // The root is opened without following the final path component. The two leaf opens below
    // are descriptor-relative O_NOFOLLOW operations, so the absolute root pathname is never a
    // mutation fallback and cannot escape the verified target directory.
    let (target_root, target_root_capability) =
        super::operation_journal::open_root(&request.target_root_path)?;
    if target_root_capability.identity.stable_id != request.expected_target_root.stable_id {
        return Err(String::from(
            "target root identity changed since preparation",
        ));
    }
    let target_parent = target_root
        .try_clone()
        .map_err(|error| format!("could not retain target parent capability: {error}"))?;

    let target_display = request.target_root_path.join(&request.target_leaf);
    let (target, target_identity) = super::operation_journal::open_leaf_relative(
        &target_parent,
        &request.target_leaf,
        &target_display,
    )?;
    if target_identity != request.expected_target {
        return Err(String::from(
            "target leaf identity changed since preparation",
        ));
    }
    validate_expected_evidence(
        "target leaf",
        &request.expected_target_content,
        &super::operation_journal::prepared_file_evidence(&target),
    )?;
    let target_volume = super::capacity_gate::descriptor_capacity_facts(&target)
        .map_err(|error| error.to_string())?
        .identity;
    if target_volume != request.expected_volume {
        return Err(String::from("target volume identity changed since staging"));
    }

    let staging_display = request.target_root_path.join(&request.staging_leaf);
    let (staging, staging_identity) = super::operation_journal::open_leaf_relative(
        &target_parent,
        &request.staging_leaf,
        &staging_display,
    )?;
    let staging_volume = super::capacity_gate::descriptor_capacity_facts(&staging)
        .map_err(|error| error.to_string())?
        .identity;
    if staging_volume != request.expected_volume {
        return Err(String::from(
            "staging volume identity changed since staging",
        ));
    }
    if staging_identity != request.expected_staging {
        return Err(String::from("staging identity changed since CopyValidated"));
    }
    let staging_content = super::operation_journal::prepared_file_evidence(&staging);
    validate_expected_evidence(
        "staging",
        &request.expected_staging_content,
        &staging_content,
    )?;

    Ok(ReacquiredExpectedIdentityRestore {
        target_root,
        target_parent,
        target,
        staging,
        target_parent_identity: target_root_capability.identity,
        target_identity,
        staging_identity,
        staging_content,
        volume: request.expected_volume.clone(),
    })
}

#[cfg(unix)]
fn validate_expected_evidence(
    label: &str,
    expected: &PreparedFileEvidence,
    actual: &PreparedFileEvidence,
) -> Result<(), String> {
    let valid = match (expected, actual) {
        (PreparedFileEvidence::Missing, PreparedFileEvidence::Missing) => true,
        (
            PreparedFileEvidence::ContentHash(expected),
            PreparedFileEvidence::ContentHash(actual),
        ) => expected == actual,
        (
            PreparedFileEvidence::Metadata {
                len: expected_len,
                is_dir: expected_is_dir,
                ..
            },
            PreparedFileEvidence::Metadata {
                len: actual_len,
                is_dir: actual_is_dir,
                ..
            },
        ) => expected_len == actual_len && expected_is_dir == actual_is_dir,
        (PreparedFileEvidence::Unverifiable, PreparedFileEvidence::Unverifiable) => true,
        _ => false,
    };
    if valid {
        Ok(())
    } else {
        Err(format!(
            "{label} content evidence changed since preparation"
        ))
    }
}

fn is_single_clean_normal_leaf(path: &Path) -> bool {
    if path.as_os_str().is_empty() || path.is_absolute() {
        return false;
    }
    let mut components = path.components();
    let Some(std::path::Component::Normal(component)) = components.next() else {
        return false;
    };
    components.next().is_none()
        && Path::new(component) == path
        && !component.as_encoded_bytes().contains(&0)
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
fn assert_unsupported_assessment(assessment: &ReplacementQualificationAssessment) {
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
        let outcome =
            ExpectedIdentityReplacementOutcome::PlatformQualificationRequired { assessment };
        assert!(matches!(
            outcome,
            ExpectedIdentityReplacementOutcome::PlatformQualificationRequired { .. }
        ));
    }
}
