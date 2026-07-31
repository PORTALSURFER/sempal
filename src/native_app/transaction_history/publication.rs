//! Typed evidence for the journal's filesystem-publication boundary.
//!
//! This module contains no filesystem operations.  A future file-owner adapter must
//! supply the evidence after performing and qualifying the platform operation; the
//! journal only validates the durable claims and their relationship to staging.

use serde::{Deserialize, Serialize};

use super::operation_journal::{
    FilesystemStagedParticipant, FilesystemStagedWaveformRestore, PreparedFileEvidence,
    PreparedObjectIdentity, PreparedWaveformRestore, ReplaceExpectedIdentity,
};

/// The transfer mode selected by the file owner from live filesystem evidence.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) enum FilesystemPublicationMode {
    /// A bytewise or otherwise non-atomic transfer into destination-local staging.
    NonAtomicCopyValidatePublish,
    /// Reserved for a future qualified all-atomic transfer sequence.
    AtomicDestinationNoReplace,
}

/// The scope of atomicity proved by the complete publication sequence.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) enum WholePublicationAtomicity {
    WholePublicationAtomic,
    WholePublicationNonAtomic,
}

/// Visibility is intentionally independent from synchronization and atomicity.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) enum PublicationVisibility {
    VisibilityVerified,
    VisibilityUnverified,
}

/// Synchronization evidence is retained separately from the visibility claim.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) enum PublicationSynchronization {
    PowerLossSynchronized,
    BestEffortSync,
    SyncUnsupportedOrUnverified,
}

/// Qualification state of the final-name primitive result.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
enum FinalClaimPrimitive {
    QualifiedExpectedIdentityReplacement,
    QualifiedAbsentFinalNoReplace,
    Unqualified { reason: String },
}

/// Result reported by the qualified final-name primitive.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
enum FinalClaimResult {
    ExpectedIdentityReplaced,
    AbsentFinalInstalled,
    NotEstablished,
}

/// Evidence for the final namespace claim.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
enum FinalNamespaceClaim {
    /// A qualified handle-bound replacement matched the expected existing target.
    ExpectedIdentityReplacement {
        primitive: FinalClaimPrimitive,
        result: FinalClaimResult,
        expected_target: PreparedObjectIdentity,
        displaced_target: PreparedObjectIdentity,
    },
    /// A qualified atomic no-replace claim installed an object at an absent final name.
    AbsentFinalNoReplace {
        primitive: FinalClaimPrimitive,
        result: FinalClaimResult,
    },
}

/// Identity and content evidence captured by reopening the final object.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct ReopenedFinalEvidence {
    identity: PreparedObjectIdentity,
    content: PreparedFileEvidence,
}

/// Typed evidence required to advance a waveform restore to `FilesystemPublished`.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct FilesystemPublishedWaveformRestore {
    mode: FilesystemPublicationMode,
    final_claim: FinalNamespaceClaim,
    reopened_final: ReopenedFinalEvidence,
    visibility: PublicationVisibility,
    whole_publication: WholePublicationAtomicity,
    synchronization: PublicationSynchronization,
}

/// Validate publication evidence without inferring anything from a pathname or touching the
/// filesystem.  This is deliberately the only validation path used by journal recovery and the
/// guarded transition.
pub(super) fn validate_publication_evidence(
    prepared: &PreparedWaveformRestore,
    staged: &FilesystemStagedWaveformRestore,
    published: &FilesystemPublishedWaveformRestore,
) -> Result<(), String> {
    let FilesystemStagedParticipant::CopyValidated {
        staging,
        evidence: staged_content,
    } = &staged.participant;

    if published.mode != FilesystemPublicationMode::NonAtomicCopyValidatePublish {
        return Err(String::from(
            "waveform restore publication mode is not the qualified bytewise staging mode",
        ));
    }
    if published.whole_publication != WholePublicationAtomicity::WholePublicationNonAtomic {
        return Err(String::from(
            "bytewise waveform restore staging cannot claim whole-publication atomicity",
        ));
    }
    if published.visibility != PublicationVisibility::VisibilityVerified {
        return Err(String::from(
            "waveform restore publication visibility is not verified",
        ));
    }
    if published.synchronization == PublicationSynchronization::PowerLossSynchronized {
        return Err(String::from(
            "non-atomic waveform restore publication cannot claim power-loss synchronization",
        ));
    }

    if published.reopened_final.identity != staging.identity {
        return Err(String::from(
            "reopened final identity does not match validated staging identity",
        ));
    }
    validate_file_evidence(staged_content, &published.reopened_final.content)?;

    match &published.final_claim {
        FinalNamespaceClaim::ExpectedIdentityReplacement {
            primitive,
            result,
            expected_target,
            displaced_target,
        } => {
            if !matches!(
                primitive,
                FinalClaimPrimitive::QualifiedExpectedIdentityReplacement
            ) {
                return Err(String::from(
                    "expected-identity replacement primitive is not qualified",
                ));
            }
            if *result != FinalClaimResult::ExpectedIdentityReplaced {
                return Err(String::from(
                    "expected-identity replacement result is not established",
                ));
            }
            let ReplaceExpectedIdentity::Existing(prepared_target) = &prepared.replacement;
            if expected_target != prepared_target {
                return Err(String::from(
                    "publication expected target identity does not match preparation",
                ));
            }
            if displaced_target != expected_target {
                return Err(String::from(
                    "publication displaced target identity does not match expected identity",
                ));
            }
        }
        FinalNamespaceClaim::AbsentFinalNoReplace { primitive, result } => {
            if !matches!(
                primitive,
                FinalClaimPrimitive::QualifiedAbsentFinalNoReplace
            ) {
                return Err(String::from(
                    "absent-final no-replace primitive is not qualified",
                ));
            }
            if *result != FinalClaimResult::AbsentFinalInstalled {
                return Err(String::from(
                    "absent-final no-replace result is not established",
                ));
            }
            return Err(String::from(
                "waveform restore requires expected-identity replacement evidence",
            ));
        }
    }

    Ok(())
}

fn validate_file_evidence(
    expected: &PreparedFileEvidence,
    actual: &PreparedFileEvidence,
) -> Result<(), String> {
    let valid = match (expected, actual) {
        (
            PreparedFileEvidence::ContentHash(expected),
            PreparedFileEvidence::ContentHash(actual),
        ) => expected == actual,
        _ => false,
    };
    if valid {
        Ok(())
    } else {
        Err(String::from(
            "reopened final content evidence does not match validated staging",
        ))
    }
}

#[cfg(test)]
#[derive(Clone, Copy)]
pub(crate) enum TestPublicationDrift {
    UnqualifiedReplacement,
    ExpectedIdentity,
    DisplacedIdentity,
    ReopenedIdentity,
    ReopenedContent,
    MetadataOnly,
    Unverifiable,
    Visibility,
    Atomicity,
    Synchronization,
}

#[cfg(test)]
pub(crate) fn test_publication_evidence(
    prepared: &PreparedWaveformRestore,
    staged: &FilesystemStagedWaveformRestore,
    drift: Option<TestPublicationDrift>,
) -> FilesystemPublishedWaveformRestore {
    let FilesystemStagedParticipant::CopyValidated { staging, evidence } = &staged.participant;
    let expected_target = match &prepared.replacement {
        ReplaceExpectedIdentity::Existing(identity) => identity.clone(),
    };
    let mut publication = FilesystemPublishedWaveformRestore {
        mode: FilesystemPublicationMode::NonAtomicCopyValidatePublish,
        final_claim: FinalNamespaceClaim::ExpectedIdentityReplacement {
            primitive: FinalClaimPrimitive::QualifiedExpectedIdentityReplacement,
            result: FinalClaimResult::ExpectedIdentityReplaced,
            expected_target: expected_target.clone(),
            displaced_target: expected_target,
        },
        reopened_final: ReopenedFinalEvidence {
            identity: staging.identity.clone(),
            content: evidence.clone(),
        },
        visibility: PublicationVisibility::VisibilityVerified,
        whole_publication: WholePublicationAtomicity::WholePublicationNonAtomic,
        synchronization: PublicationSynchronization::SyncUnsupportedOrUnverified,
    };

    match drift {
        Some(TestPublicationDrift::UnqualifiedReplacement) => {
            if let FinalNamespaceClaim::ExpectedIdentityReplacement { primitive, .. } =
                &mut publication.final_claim
            {
                *primitive = FinalClaimPrimitive::Unqualified {
                    reason: String::from("test"),
                };
            }
        }
        Some(TestPublicationDrift::ExpectedIdentity) => {
            if let FinalNamespaceClaim::ExpectedIdentityReplacement {
                expected_target, ..
            } = &mut publication.final_claim
            {
                expected_target.len = expected_target.len.saturating_add(1);
            }
        }
        Some(TestPublicationDrift::DisplacedIdentity) => {
            if let FinalNamespaceClaim::ExpectedIdentityReplacement {
                displaced_target, ..
            } = &mut publication.final_claim
            {
                displaced_target.len = displaced_target.len.saturating_add(1);
            }
        }
        Some(TestPublicationDrift::ReopenedIdentity) => {
            publication.reopened_final.identity.len =
                publication.reopened_final.identity.len.saturating_add(1);
        }
        Some(TestPublicationDrift::ReopenedContent) => {
            publication.reopened_final.content = PreparedFileEvidence::ContentHash([9; 32]);
        }
        Some(TestPublicationDrift::MetadataOnly) => {
            publication.reopened_final.content = PreparedFileEvidence::Metadata {
                len: staging.identity.len,
                modified_ns: None,
                is_dir: false,
            };
        }
        Some(TestPublicationDrift::Unverifiable) => {
            publication.reopened_final.content = PreparedFileEvidence::Unverifiable;
        }
        Some(TestPublicationDrift::Visibility) => {
            publication.visibility = PublicationVisibility::VisibilityUnverified;
        }
        Some(TestPublicationDrift::Atomicity) => {
            publication.whole_publication = WholePublicationAtomicity::WholePublicationAtomic;
        }
        Some(TestPublicationDrift::Synchronization) => {
            publication.synchronization = PublicationSynchronization::PowerLossSynchronized;
        }
        None => {}
    }
    publication
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn publication_evidence_types_keep_visibility_and_synchronization_separate() {
        let _ = (
            PublicationVisibility::VisibilityVerified,
            PublicationSynchronization::SyncUnsupportedOrUnverified,
        );
        assert_ne!(
            WholePublicationAtomicity::WholePublicationAtomic,
            WholePublicationAtomicity::WholePublicationNonAtomic
        );
    }
}
