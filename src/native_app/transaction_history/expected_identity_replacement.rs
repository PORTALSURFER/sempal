//! Capability-bound seam for a future expected-identity replacement primitive.
//!
//! This module deliberately contains no namespace mutation.  The production adapter is
//! fail-closed until a platform-specific implementation can prove the complete contract.

use std::fs::File;
use std::path::Path;

use super::capacity_gate::VolumeIdentity;
use super::operation_journal::{PreparedFileEvidence, PreparedObjectIdentity};
use super::publication::{
    PublicationSynchronization, PublicationVisibility, WholePublicationAtomicity,
};

mod sealed {
    pub trait Sealed {}
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
    Unsupported { reason: String },
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

/// Production adapter.  No platform replacement primitive is qualified in this slice.
pub(super) struct ProductionExpectedIdentityReplacementAdapter;

impl sealed::Sealed for ProductionExpectedIdentityReplacementAdapter {}

impl ExpectedIdentityReplacementAdapter for ProductionExpectedIdentityReplacementAdapter {
    fn attempt(
        &self,
        request: ExpectedIdentityReplacementRequest<'_>,
    ) -> ExpectedIdentityReplacementOutcome {
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
            request.volume,
        );
        ExpectedIdentityReplacementOutcome::Unsupported {
            reason: String::from("expected-identity replacement primitive is not qualified"),
        }
    }
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
        assert!(matches!(
            ProductionExpectedIdentityReplacementAdapter.attempt(request),
            ExpectedIdentityReplacementOutcome::Unsupported { .. }
        ));
    }
}
