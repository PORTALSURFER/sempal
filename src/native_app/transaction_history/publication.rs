//! Typed evidence for the journal's filesystem-publication boundary.
//!
//! This module contains no filesystem operations.  A future file-owner adapter must
//! supply the evidence after performing and qualifying the platform operation; the
//! journal only validates the durable claims and their relationship to staging.

use serde::{Deserialize, Serialize};

use super::absent_final_no_replace::{QualifiedAbsentFinalNoReplace, RootPathContinuity};
use super::expected_identity_replacement::QualifiedExpectedIdentityReplacement;
use super::operation_journal::{
    FilesystemStagedParticipant, FilesystemStagedWaveformRestore, PreparedFileEvidence,
    PreparedObjectIdentity, PreparedWaveformRestore, ReplaceExpectedIdentity,
};
#[cfg(test)]
use super::operation_journal::{
    PreparedLeafLocator, PreparedRestoreDirection, PreparedRestoreEvidence, PreparedRootCapability,
    PreparedStagingLocator,
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

/// Scope carried by absent-final evidence.  The retained target-parent descriptor is authoritative;
/// configured root-path continuity is deliberately not part of the qualification claim.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
enum AbsentFinalCapabilityScope {
    TargetParentDescriptor {
        target_parent_identity: PreparedObjectIdentity,
        root_path_continuity: RootPathContinuity,
    },
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
        capability_scope: AbsentFinalCapabilityScope,
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

/// Construct publication evidence only from a sealed, qualified adapter result.
pub(super) fn from_qualified_adapter_result(
    qualified: QualifiedExpectedIdentityReplacement,
) -> FilesystemPublishedWaveformRestore {
    let (
        expected_target,
        displaced_target,
        reopened_final,
        reopened_content,
        visibility,
        whole_publication,
        synchronization,
    ) = qualified.into_publication_parts();
    FilesystemPublishedWaveformRestore {
        mode: FilesystemPublicationMode::NonAtomicCopyValidatePublish,
        final_claim: FinalNamespaceClaim::ExpectedIdentityReplacement {
            primitive: FinalClaimPrimitive::QualifiedExpectedIdentityReplacement,
            result: FinalClaimResult::ExpectedIdentityReplaced,
            expected_target,
            displaced_target,
        },
        reopened_final: ReopenedFinalEvidence {
            identity: reopened_final,
            content: reopened_content,
        },
        visibility,
        whole_publication,
        synchronization,
    }
}

/// Construct absent-final publication evidence only from the sealed, qualified adapter result.
#[allow(dead_code)]
pub(super) fn from_qualified_absent_final_no_replace_result(
    qualified: QualifiedAbsentFinalNoReplace,
) -> FilesystemPublishedWaveformRestore {
    let (
        target_parent_identity,
        root_path_continuity,
        reopened_final,
        reopened_content,
        visibility,
        synchronization,
    ) = qualified.into_publication_parts();
    FilesystemPublishedWaveformRestore {
        mode: FilesystemPublicationMode::NonAtomicCopyValidatePublish,
        final_claim: FinalNamespaceClaim::AbsentFinalNoReplace {
            primitive: FinalClaimPrimitive::QualifiedAbsentFinalNoReplace,
            result: FinalClaimResult::AbsentFinalInstalled,
            capability_scope: AbsentFinalCapabilityScope::TargetParentDescriptor {
                target_parent_identity,
                root_path_continuity,
            },
        },
        reopened_final: ReopenedFinalEvidence {
            identity: reopened_final,
            content: reopened_content,
        },
        visibility,
        whole_publication: WholePublicationAtomicity::WholePublicationNonAtomic,
        synchronization,
    }
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
        FinalNamespaceClaim::AbsentFinalNoReplace {
            primitive, result, ..
        } => {
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

/// Validate the distinct absent-final publication boundary without consulting expected-target
/// replacement evidence.  The existing waveform-restore validator intentionally rejects this
/// claim because that operation requires an existing target; this validator is for the later
/// absent-final qualification seam only.  It requires the sealed target-parent capability scope
/// and explicitly accepts only `NotClaimed` root-path continuity.
#[allow(dead_code)]
pub(super) fn validate_absent_final_no_replace_publication(
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
            "absent-final publication mode is not the qualified bytewise staging mode",
        ));
    }
    if published.whole_publication != WholePublicationAtomicity::WholePublicationNonAtomic {
        return Err(String::from(
            "absent-final publication cannot claim whole-publication atomicity",
        ));
    }
    if published.visibility != PublicationVisibility::VisibilityVerified {
        return Err(String::from(
            "absent-final publication visibility is not verified",
        ));
    }
    if published.synchronization == PublicationSynchronization::PowerLossSynchronized {
        return Err(String::from(
            "absent-final publication cannot claim power-loss synchronization",
        ));
    }

    let FinalNamespaceClaim::AbsentFinalNoReplace {
        primitive,
        result,
        capability_scope,
    } = &published.final_claim
    else {
        return Err(String::from(
            "publication evidence is not an absent-final no-replace claim",
        ));
    };
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
    let AbsentFinalCapabilityScope::TargetParentDescriptor {
        target_parent_identity,
        root_path_continuity,
    } = capability_scope;
    if target_parent_identity != &prepared.target_root.identity {
        return Err(String::from(
            "absent-final capability scope does not match the prepared target parent",
        ));
    }
    if !matches!(root_path_continuity, RootPathContinuity::NotClaimed) {
        return Err(String::from(
            "absent-final evidence overclaims root pathname continuity",
        ));
    }
    if published.reopened_final.identity.stable_id != staging.identity.stable_id
        || published.reopened_final.identity.len != staging.identity.len
    {
        return Err(String::from(
            "reopened absent-final identity does not match validated staging identity",
        ));
    }
    validate_file_evidence(staged_content, &published.reopened_final.content)
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
    let FilesystemStagedParticipant::CopyValidated { staging, .. } = &staged.participant;
    let qualified = super::expected_identity_replacement::test_qualified_success(prepared, staged);
    let mut publication = from_qualified_adapter_result(qualified);

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
pub(crate) fn test_absent_final_publication_evidence(
    target_parent_identity: &PreparedObjectIdentity,
    staged: &FilesystemStagedWaveformRestore,
) -> FilesystemPublishedWaveformRestore {
    let FilesystemStagedParticipant::CopyValidated { staging, evidence } = &staged.participant;
    let Some(qualified) = super::absent_final_no_replace::test_qualified_success(
        target_parent_identity,
        &staging.identity,
        evidence,
    ) else {
        panic!("absent-final test evidence must contain a content hash");
    };
    from_qualified_absent_final_no_replace_result(qualified)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn absent_final_fixture() -> (PreparedWaveformRestore, FilesystemStagedWaveformRestore) {
        let staging_identity = PreparedObjectIdentity {
            stable_id: String::from("fixture-staging"),
            change_marker: None,
            len: 4,
        };
        let staging_content = PreparedFileEvidence::ContentHash([7; 32]);
        let staged = FilesystemStagedWaveformRestore {
            participant: FilesystemStagedParticipant::CopyValidated {
                staging: PreparedLeafLocator {
                    relative_path: std::path::PathBuf::from("staging.wav"),
                    identity: staging_identity.clone(),
                },
                evidence: staging_content.clone(),
            },
        };

        let root_identity = PreparedObjectIdentity {
            stable_id: String::from("fixture-root"),
            change_marker: None,
            len: 0,
        };
        let target_identity = PreparedObjectIdentity {
            stable_id: String::from("fixture-target"),
            change_marker: None,
            len: 4,
        };
        let root = PreparedRootCapability {
            path: std::path::PathBuf::from("/fixture"),
            identity: root_identity.clone(),
        };
        let target = PreparedLeafLocator {
            relative_path: std::path::PathBuf::from("target.wav"),
            identity: target_identity.clone(),
        };
        let prepared = PreparedWaveformRestore {
            direction: PreparedRestoreDirection::Undo,
            source_id: String::from("fixture-source"),
            source_root: root.clone(),
            target_root: root.clone(),
            target,
            backup_root: root,
            backup: PreparedLeafLocator {
                relative_path: std::path::PathBuf::from("backup.wav"),
                identity: target_identity.clone(),
            },
            replacement: ReplaceExpectedIdentity::Existing(target_identity),
            staging: PreparedStagingLocator {
                relative_path: std::path::PathBuf::from("staging.wav"),
                absent: false,
            },
            evidence: PreparedRestoreEvidence {
                target: PreparedFileEvidence::ContentHash([1; 32]),
                backup: staging_content,
            },
        };
        (prepared, staged)
    }

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

    #[test]
    fn absent_final_evidence_has_a_distinct_validator_boundary() {
        let (prepared, staged) = absent_final_fixture();
        let published =
            test_absent_final_publication_evidence(&prepared.target_root.identity, &staged);

        assert!(
            validate_absent_final_no_replace_publication(&prepared, &staged, &published).is_ok()
        );
        let waveform_error = validate_publication_evidence(&prepared, &staged, &published)
            .expect_err("waveform restore validator must reject absent-final evidence");
        assert!(waveform_error.contains("expected-identity replacement"));

        let mut invalid_content = published.clone();
        invalid_content.reopened_final.content = PreparedFileEvidence::Unverifiable;
        assert!(
            validate_absent_final_no_replace_publication(&prepared, &staged, &invalid_content)
                .is_err()
        );

        let mut invalid_sync = published.clone();
        invalid_sync.synchronization = PublicationSynchronization::PowerLossSynchronized;
        assert!(
            validate_absent_final_no_replace_publication(&prepared, &staged, &invalid_sync)
                .is_err()
        );

        let mut wrong_scope = published;
        let FinalNamespaceClaim::AbsentFinalNoReplace {
            capability_scope:
                AbsentFinalCapabilityScope::TargetParentDescriptor {
                    target_parent_identity,
                    ..
                },
            ..
        } = &mut wrong_scope.final_claim
        else {
            panic!("test evidence must carry absent-final capability scope");
        };
        target_parent_identity.stable_id = String::from("wrong-target-parent");
        assert!(
            validate_absent_final_no_replace_publication(&prepared, &staged, &wrong_scope).is_err()
        );
    }
}
