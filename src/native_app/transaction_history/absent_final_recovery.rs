//! Read-only recovery classification for schema-v2 absent-final staging records.
//!
//! The classifier reacquires the prepared target-parent capability and observes both leaves
//! relative to that descriptor. It never claims publication, ownership, or pathname continuity;
//! all returned values are evidence-only and contain no open handles or mutation capabilities.

#![allow(dead_code)]

#[cfg(unix)]
use std::fs::File;
#[cfg(unix)]
use std::path::Path;

use super::absent_final_no_replace::{
    exact_content_evidence_matches, stable_id_and_length_matches,
};
use super::operation_journal::{
    AbsentFinalRecoveryObservation, FilesystemStagedParticipant, FilesystemStagedWaveformRestore,
    PreparedAbsentFinalNoReplace,
};
#[cfg(unix)]
use super::operation_journal::{
    PreparedFileEvidence, PreparedObjectIdentity, descriptor_identity, open_root,
    prepared_file_evidence,
};

/// Whether the durable staging locator is present while a final collision is observed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CollisionStagingState {
    Present,
    Missing,
}

/// A typed reason why live namespace evidence is not safe to interpret as a match or collision.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum AbsentFinalRecoveryAmbiguity {
    TargetParentReacquisitionFailed,
    TargetParentIdentityChanged,
    StagingEntryNotRegular,
    StagingIdentityUnavailable,
    StagingIdentityDrift,
    StagingContentUnavailable,
    StagingContentDrift,
    FinalEntryNotRegular,
    FinalIdentityUnavailable,
    FinalIdentityLengthDrift,
    FinalContentUnavailable,
    FinalContentDrift,
    StagingInspectionRaceOrFailure,
    FinalInspectionRaceOrFailure,
    UnsupportedDescriptorRelativeInspection,
}

/// Read-only classification of one eligible schema-v2 absent-final recovery observation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum AbsentFinalRecoveryClassification {
    StagingPresentFinalAbsent,
    StagingMissingFinalMatches,
    BothPresent,
    NeitherPresent,
    Collision { staging: CollisionStagingState },
    IdentityAmbiguous(AbsentFinalRecoveryAmbiguity),
}

/// Internal inspection result retaining the exact non-handle evidence for a final match.
pub(super) struct AbsentFinalRecoveryInspection {
    pub(super) classification: AbsentFinalRecoveryClassification,
    pub(super) observation: Option<AbsentFinalRecoveryObservation>,
}

#[cfg(unix)]
#[derive(Debug)]
enum LeafInspectionError {
    NotRegular,
    IdentityUnavailable,
    ContentUnavailable,
    RaceOrFailure,
}

#[cfg(unix)]
enum LeafObservation {
    Missing,
    Present {
        identity: PreparedObjectIdentity,
        content: PreparedFileEvidence,
    },
}

/// Classify one already-validated absent-final staging checkpoint without any filesystem or
/// journal mutation. The coordinator performs the schema/phase/evidence eligibility checks
/// before delegating here.
pub(super) fn classify_absent_final_recovery(
    prepared: &PreparedAbsentFinalNoReplace,
    staged: &FilesystemStagedWaveformRestore,
) -> AbsentFinalRecoveryClassification {
    inspect_absent_final_recovery(prepared, staged).classification
}

/// Inspect one eligible absent-final staging checkpoint and retain the exact evidence needed by
/// the journal observation seam. The result contains no handles or mutation capability.
pub(super) fn inspect_absent_final_recovery(
    prepared: &PreparedAbsentFinalNoReplace,
    staged: &FilesystemStagedWaveformRestore,
) -> AbsentFinalRecoveryInspection {
    #[cfg(unix)]
    {
        return classify_unix(prepared, staged);
    }

    #[cfg(not(unix))]
    {
        let _ = (prepared, staged);
        AbsentFinalRecoveryInspection {
            classification: AbsentFinalRecoveryClassification::IdentityAmbiguous(
                AbsentFinalRecoveryAmbiguity::UnsupportedDescriptorRelativeInspection,
            ),
            observation: None,
        }
    }
}

#[cfg(unix)]
fn classify_unix(
    prepared: &PreparedAbsentFinalNoReplace,
    staged: &FilesystemStagedWaveformRestore,
) -> AbsentFinalRecoveryInspection {
    let FilesystemStagedParticipant::CopyValidated {
        staging: expected_staging,
        evidence: expected_content,
    } = &staged.participant;

    let (target_parent, reacquired_parent) = match open_root(&prepared.target_parent.path) {
        Ok(value) => value,
        Err(_) => {
            return inspection(AbsentFinalRecoveryClassification::IdentityAmbiguous(
                AbsentFinalRecoveryAmbiguity::TargetParentReacquisitionFailed,
            ));
        }
    };
    if reacquired_parent.identity.stable_id != prepared.target_parent.identity.stable_id {
        return inspection(AbsentFinalRecoveryClassification::IdentityAmbiguous(
            AbsentFinalRecoveryAmbiguity::TargetParentIdentityChanged,
        ));
    }

    let staging = match inspect_leaf(&target_parent, &prepared.staging.relative_path) {
        Ok(observation) => observation,
        Err(error) => {
            return inspection(AbsentFinalRecoveryClassification::IdentityAmbiguous(
                staging_ambiguity(error),
            ));
        }
    };
    let staging_present = match &staging {
        LeafObservation::Missing => false,
        LeafObservation::Present { identity, content } => {
            if !stable_id_and_length_matches(identity, &expected_staging.identity) {
                return inspection(AbsentFinalRecoveryClassification::IdentityAmbiguous(
                    AbsentFinalRecoveryAmbiguity::StagingIdentityDrift,
                ));
            }
            if !exact_content_evidence_matches(expected_content, content) {
                return inspection(AbsentFinalRecoveryClassification::IdentityAmbiguous(
                    AbsentFinalRecoveryAmbiguity::StagingContentDrift,
                ));
            }
            true
        }
    };

    let final_observation = match inspect_leaf(&target_parent, &prepared.final_leaf) {
        Ok(observation) => observation,
        Err(error) => {
            return inspection(AbsentFinalRecoveryClassification::IdentityAmbiguous(
                final_ambiguity(error),
            ));
        }
    };
    match final_observation {
        LeafObservation::Missing if staging_present => {
            inspection(AbsentFinalRecoveryClassification::StagingPresentFinalAbsent)
        }
        LeafObservation::Missing => inspection(AbsentFinalRecoveryClassification::NeitherPresent),
        LeafObservation::Present { identity, content } => {
            if identity.stable_id != expected_staging.identity.stable_id {
                return inspection(AbsentFinalRecoveryClassification::Collision {
                    staging: if staging_present {
                        CollisionStagingState::Present
                    } else {
                        CollisionStagingState::Missing
                    },
                });
            }
            if !stable_id_and_length_matches(&identity, &expected_staging.identity) {
                return inspection(AbsentFinalRecoveryClassification::IdentityAmbiguous(
                    AbsentFinalRecoveryAmbiguity::FinalIdentityLengthDrift,
                ));
            }
            if !exact_content_evidence_matches(expected_content, &content) {
                return inspection(AbsentFinalRecoveryClassification::IdentityAmbiguous(
                    AbsentFinalRecoveryAmbiguity::FinalContentDrift,
                ));
            }
            if staging_present {
                inspection(AbsentFinalRecoveryClassification::BothPresent)
            } else {
                let PreparedFileEvidence::ContentHash(hash) = content else {
                    return inspection(AbsentFinalRecoveryClassification::IdentityAmbiguous(
                        AbsentFinalRecoveryAmbiguity::FinalContentUnavailable,
                    ));
                };
                AbsentFinalRecoveryInspection {
                    classification: AbsentFinalRecoveryClassification::StagingMissingFinalMatches,
                    observation: Some(AbsentFinalRecoveryObservation {
                        target_parent_stable_id: reacquired_parent.identity.stable_id,
                        final_stable_id: identity.stable_id,
                        final_len: identity.len,
                        final_content: PreparedFileEvidence::ContentHash(hash),
                    }),
                }
            }
        }
    }
}

fn inspection(classification: AbsentFinalRecoveryClassification) -> AbsentFinalRecoveryInspection {
    AbsentFinalRecoveryInspection {
        classification,
        observation: None,
    }
}

#[cfg(unix)]
fn inspect_leaf(root: &File, relative: &Path) -> Result<LeafObservation, LeafInspectionError> {
    use std::ffi::CString;
    use std::os::fd::{AsRawFd, FromRawFd};

    let mut components = relative.components();
    let Some(std::path::Component::Normal(component)) = components.next() else {
        return Err(LeafInspectionError::RaceOrFailure);
    };
    if components.next().is_some() || Path::new(component) != relative {
        return Err(LeafInspectionError::RaceOrFailure);
    }
    let name = CString::new(component.as_encoded_bytes())
        .map_err(|_| LeafInspectionError::RaceOrFailure)?;

    let mut metadata = std::mem::MaybeUninit::<libc::stat>::zeroed();
    let result = unsafe {
        libc::fstatat(
            root.as_raw_fd(),
            name.as_ptr(),
            metadata.as_mut_ptr(),
            libc::AT_SYMLINK_NOFOLLOW,
        )
    };
    if result != 0 {
        let error = std::io::Error::last_os_error();
        if error.raw_os_error() == Some(libc::ENOENT) {
            return Ok(LeafObservation::Missing);
        }
        return Err(LeafInspectionError::RaceOrFailure);
    }
    let metadata = unsafe { metadata.assume_init() };
    if metadata.st_mode & libc::S_IFMT != libc::S_IFREG {
        return Err(LeafInspectionError::NotRegular);
    }

    let fd = unsafe {
        libc::openat(
            root.as_raw_fd(),
            name.as_ptr(),
            libc::O_RDONLY | libc::O_CLOEXEC | libc::O_NOFOLLOW | libc::O_NONBLOCK,
        )
    };
    if fd < 0 {
        return Err(LeafInspectionError::RaceOrFailure);
    }
    let file = unsafe { File::from_raw_fd(fd) };
    let before =
        descriptor_identity(&file).map_err(|_| LeafInspectionError::IdentityUnavailable)?;
    let content = prepared_file_evidence(&file);
    let after = descriptor_identity(&file).map_err(|_| LeafInspectionError::IdentityUnavailable)?;
    if !stable_id_and_length_matches(&before, &after) {
        return Err(LeafInspectionError::RaceOrFailure);
    }
    if !matches!(content, PreparedFileEvidence::ContentHash(_)) {
        return Err(LeafInspectionError::ContentUnavailable);
    }
    Ok(LeafObservation::Present {
        identity: after,
        content,
    })
}

#[cfg(unix)]
fn staging_ambiguity(error: LeafInspectionError) -> AbsentFinalRecoveryAmbiguity {
    match error {
        LeafInspectionError::NotRegular => AbsentFinalRecoveryAmbiguity::StagingEntryNotRegular,
        LeafInspectionError::IdentityUnavailable => {
            AbsentFinalRecoveryAmbiguity::StagingIdentityUnavailable
        }
        LeafInspectionError::ContentUnavailable => {
            AbsentFinalRecoveryAmbiguity::StagingContentUnavailable
        }
        LeafInspectionError::RaceOrFailure => {
            AbsentFinalRecoveryAmbiguity::StagingInspectionRaceOrFailure
        }
    }
}

#[cfg(unix)]
fn final_ambiguity(error: LeafInspectionError) -> AbsentFinalRecoveryAmbiguity {
    match error {
        LeafInspectionError::NotRegular => AbsentFinalRecoveryAmbiguity::FinalEntryNotRegular,
        LeafInspectionError::IdentityUnavailable => {
            AbsentFinalRecoveryAmbiguity::FinalIdentityUnavailable
        }
        LeafInspectionError::ContentUnavailable => {
            AbsentFinalRecoveryAmbiguity::FinalContentUnavailable
        }
        LeafInspectionError::RaceOrFailure => {
            AbsentFinalRecoveryAmbiguity::FinalInspectionRaceOrFailure
        }
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::native_app::transaction_history::capacity_gate::{
        CapacityAllocationClass, DurableCapacityPlan, DurableVolumeCapacity,
        PROTECTED_FREE_SPACE_FLOOR, VolumeIdentity,
    };
    use crate::native_app::transaction_history::operation_journal::{
        AbsentFinalObservation, AbsentFinalTransactionOwnedProof, FilesystemStagedParticipant,
        FilesystemStagedWaveformRestore, OperationActor, OperationIntent,
        OperationJournalCoordinator, OperationKind, OperationPhase, PreparedAbsentFinalNoReplace,
        PreparedFileEvidence, PreparedLeafLocator, PreparedRestoreDirection,
        PreparedRootCapability, PreparedStagingLocator, descriptor_identity,
        prepared_file_evidence,
    };
    use std::ffi::CString;
    use std::fs::{self, File};
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::symlink;
    use std::path::PathBuf;
    use tempfile::TempDir;

    const STAGING_LEAF: &str = "staging.wav";
    const FINAL_LEAF: &str = "final.wav";
    const STAGING_BYTES: &[u8] = b"durable staged waveform";

    struct Fixture {
        _directory: TempDir,
        target_parent_path: PathBuf,
        prepared: PreparedAbsentFinalNoReplace,
        staged: FilesystemStagedWaveformRestore,
    }

    impl Fixture {
        fn new() -> Self {
            let directory = tempfile::tempdir().expect("fixture directory");
            let base = directory
                .path()
                .canonicalize()
                .expect("canonical fixture directory");
            let target_parent_path = base.join("target-parent");
            fs::create_dir(&target_parent_path).expect("target parent");
            let staging_path = target_parent_path.join(STAGING_LEAF);
            fs::write(&staging_path, STAGING_BYTES).expect("staging bytes");

            let target_parent = File::open(&target_parent_path).expect("target parent handle");
            let target_parent_identity =
                descriptor_identity(&target_parent).expect("target parent identity");
            let staging = File::open(&staging_path).expect("staging handle");
            let staging_identity = descriptor_identity(&staging).expect("staging identity");
            let evidence = prepared_file_evidence(&staging);
            assert!(matches!(evidence, PreparedFileEvidence::ContentHash(_)));
            let target_parent = PreparedRootCapability {
                path: target_parent_path.clone(),
                identity: target_parent_identity,
            };
            let prepared = PreparedAbsentFinalNoReplace {
                direction: PreparedRestoreDirection::Undo,
                source_id: String::from("absent-final-recovery-fixture"),
                source_root: target_parent.clone(),
                target_parent,
                final_leaf: PathBuf::from(FINAL_LEAF),
                staging: PreparedStagingLocator {
                    relative_path: PathBuf::from(STAGING_LEAF),
                    absent: true,
                },
                final_observation: AbsentFinalObservation::ObservedAbsent,
                copy_validated_evidence: evidence.clone(),
            };
            let staged = FilesystemStagedWaveformRestore {
                participant: FilesystemStagedParticipant::CopyValidated {
                    staging: PreparedLeafLocator {
                        relative_path: PathBuf::from(STAGING_LEAF),
                        identity: staging_identity,
                    },
                    evidence,
                },
            };
            Self {
                _directory: directory,
                target_parent_path,
                prepared,
                staged,
            }
        }

        fn staging_path(&self) -> PathBuf {
            self.target_parent_path.join(STAGING_LEAF)
        }

        fn final_path(&self) -> PathBuf {
            self.target_parent_path.join(FINAL_LEAF)
        }

        fn classify(&self) -> AbsentFinalRecoveryClassification {
            classify_absent_final_recovery(&self.prepared, &self.staged)
        }

        fn set_staged_change_marker(&mut self, marker: &str) {
            let FilesystemStagedParticipant::CopyValidated { staging, .. } =
                &mut self.staged.participant;
            staging.identity.change_marker = Some(String::from(marker));
        }

        fn replace_parent(&self) -> PathBuf {
            let moved = self
                .target_parent_path
                .parent()
                .expect("fixture parent")
                .join("moved-target-parent");
            fs::rename(&self.target_parent_path, &moved).expect("move target parent");
            fs::create_dir(&self.target_parent_path).expect("replacement target parent");
            moved
        }

        fn remove_parent_contents(&self) {
            let _ = fs::remove_file(self.staging_path());
            let _ = fs::remove_file(self.final_path());
        }
    }

    fn assert_ambiguous(fixture: &Fixture, expected: AbsentFinalRecoveryAmbiguity) {
        assert_eq!(
            fixture.classify(),
            AbsentFinalRecoveryClassification::IdentityAmbiguous(expected)
        );
    }

    #[test]
    fn presence_matrix_classifies_staging_and_final_without_mutation() {
        let fixture = Fixture::new();
        assert_eq!(
            fixture.classify(),
            AbsentFinalRecoveryClassification::StagingPresentFinalAbsent
        );
        assert!(fixture.staging_path().is_file());
        assert!(!fixture.final_path().exists());

        let fixture = Fixture::new();
        fs::hard_link(fixture.staging_path(), fixture.final_path()).expect("hard link final");
        assert_eq!(
            fixture.classify(),
            AbsentFinalRecoveryClassification::BothPresent
        );
        assert!(fixture.staging_path().is_file());
        assert!(fixture.final_path().is_file());

        let fixture = Fixture::new();
        fs::rename(fixture.staging_path(), fixture.final_path()).expect("rename staging to final");
        assert_eq!(
            fixture.classify(),
            AbsentFinalRecoveryClassification::StagingMissingFinalMatches
        );
        assert!(!fixture.staging_path().exists());
        assert!(fixture.final_path().is_file());

        let fixture = Fixture::new();
        fs::remove_file(fixture.staging_path()).expect("remove staging");
        assert_eq!(
            fixture.classify(),
            AbsentFinalRecoveryClassification::NeitherPresent
        );
    }

    #[test]
    fn staging_and_final_change_marker_drift_does_not_change_matching_identity() {
        let mut fixture = Fixture::new();
        fixture.set_staged_change_marker("stale-change-marker");
        assert_eq!(
            fixture.classify(),
            AbsentFinalRecoveryClassification::StagingPresentFinalAbsent
        );

        let fixture = Fixture::new();
        fs::rename(fixture.staging_path(), fixture.final_path()).expect("rename staging to final");
        assert_eq!(
            fixture.classify(),
            AbsentFinalRecoveryClassification::StagingMissingFinalMatches
        );
    }

    #[test]
    fn different_final_identity_is_a_collision_with_staging_presence() {
        let fixture = Fixture::new();
        fs::write(fixture.final_path(), b"foreign final object").expect("foreign final");
        assert_eq!(
            fixture.classify(),
            AbsentFinalRecoveryClassification::Collision {
                staging: CollisionStagingState::Present,
            }
        );

        let fixture = Fixture::new();
        fs::write(fixture.final_path(), STAGING_BYTES).expect("same bytes, new final object");
        assert_ne!(
            descriptor_identity(&File::open(fixture.final_path()).expect("foreign final handle"))
                .expect("foreign final identity")
                .stable_id,
            match &fixture.staged.participant {
                FilesystemStagedParticipant::CopyValidated { staging, .. } => {
                    staging.identity.stable_id.clone()
                }
            }
        );
        assert_eq!(
            fixture.classify(),
            AbsentFinalRecoveryClassification::Collision {
                staging: CollisionStagingState::Present,
            }
        );
    }

    #[test]
    fn different_final_identity_is_a_collision_after_staging_disappears() {
        let fixture = Fixture::new();
        fs::remove_file(fixture.staging_path()).expect("remove staging");
        fs::write(fixture.final_path(), b"foreign final object").expect("foreign final");
        assert_eq!(
            fixture.classify(),
            AbsentFinalRecoveryClassification::Collision {
                staging: CollisionStagingState::Missing,
            }
        );
    }

    #[test]
    fn replaced_staging_and_changed_same_identity_final_are_ambiguous() {
        let fixture = Fixture::new();
        fs::remove_file(fixture.staging_path()).expect("remove staging");
        fs::write(fixture.staging_path(), STAGING_BYTES).expect("replacement staging");
        assert_ambiguous(&fixture, AbsentFinalRecoveryAmbiguity::StagingIdentityDrift);

        let fixture = Fixture::new();
        fs::rename(fixture.staging_path(), fixture.final_path()).expect("rename staging to final");
        fs::write(fixture.final_path(), vec![b'x'; STAGING_BYTES.len()])
            .expect("change final content in place");
        assert_ambiguous(&fixture, AbsentFinalRecoveryAmbiguity::FinalContentDrift);
    }

    #[test]
    fn replaced_missing_and_symlink_parent_are_ambiguous() {
        let fixture = Fixture::new();
        let moved = fixture.replace_parent();
        assert_ambiguous(
            &fixture,
            AbsentFinalRecoveryAmbiguity::TargetParentIdentityChanged,
        );
        assert!(moved.is_dir());

        let fixture = Fixture::new();
        fixture.remove_parent_contents();
        fs::remove_dir(&fixture.target_parent_path).expect("remove target parent");
        assert_ambiguous(
            &fixture,
            AbsentFinalRecoveryAmbiguity::TargetParentReacquisitionFailed,
        );

        let fixture = Fixture::new();
        let moved = fixture.replace_parent();
        let replacement = fixture
            .target_parent_path
            .parent()
            .expect("fixture parent")
            .join("symlink-target-parent");
        fs::create_dir(&replacement).expect("symlink target parent");
        fs::remove_dir(&fixture.target_parent_path).expect("remove replacement directory");
        symlink(&replacement, &fixture.target_parent_path).expect("symlink target parent");
        assert_ambiguous(
            &fixture,
            AbsentFinalRecoveryAmbiguity::TargetParentReacquisitionFailed,
        );
        assert!(moved.is_dir());
    }

    #[test]
    fn symlink_and_special_final_are_ambiguous_without_following_or_blocking() {
        let fixture = Fixture::new();
        let foreign = fixture
            .target_parent_path
            .parent()
            .expect("fixture parent")
            .join("foreign-target");
        fs::write(&foreign, b"foreign").expect("foreign target");
        symlink(&foreign, fixture.final_path()).expect("symlink final");
        assert_ambiguous(&fixture, AbsentFinalRecoveryAmbiguity::FinalEntryNotRegular);

        let fixture = Fixture::new();
        let name = CString::new(fixture.final_path().as_os_str().as_bytes())
            .expect("fifo path without NUL");
        assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
        assert_ambiguous(&fixture, AbsentFinalRecoveryAmbiguity::FinalEntryNotRegular);
    }

    #[test]
    fn coordinator_rejects_v1_and_wrong_phase_records() {
        let v1_directory = tempfile::tempdir().expect("v1 journal directory");
        let mut v1_journal = OperationJournalCoordinator::open(v1_directory.path().to_path_buf())
            .expect("open journal");
        let v1 = v1_journal
            .admit(
                OperationIntent {
                    actor: OperationActor::User,
                    kind: OperationKind::FileHistory,
                    label: String::from("v1"),
                },
                serde_json::json!({"schema": 1}),
            )
            .expect("admit v1 record");
        assert!(matches!(
            v1_journal.classify_schema_v2_absent_final_recovery(v1),
            Err(crate::native_app::transaction_history::operation_journal::JournalError::InvalidPublicationEvidence { .. })
        ));
        drop(v1_journal);

        let directory = tempfile::tempdir().expect("v2 journal directory");
        let fixture = Fixture::new();
        let mut journal = OperationJournalCoordinator::open(directory.path().to_path_buf())
            .expect("open v2 journal");
        let v2 = journal
            .admit_schema_v2_absent_final_for_test(
                OperationIntent {
                    actor: OperationActor::User,
                    kind: OperationKind::FileHistory,
                    label: String::from("wrong phase"),
                },
                serde_json::json!({"schema": 2}),
                fixture.prepared,
                fixture.staged,
                valid_capacity_plan(),
            )
            .expect("admit v2 record");
        journal
            .update(
                v2,
                OperationPhase::Terminal,
                crate::native_app::transaction_history::operation_journal::OperationDisposition::CancelledBeforePublish,
            )
            .expect("cancel before publish");
        assert!(matches!(
            journal.classify_schema_v2_absent_final_recovery(v2),
            Err(crate::native_app::transaction_history::operation_journal::JournalError::InvalidPublicationEvidence { .. })
        ));
    }

    #[test]
    fn journal_restart_and_classification_are_byte_and_state_read_only() {
        let directory = tempfile::tempdir().expect("journal directory");
        let fixture = Fixture::new();
        let mut journal = OperationJournalCoordinator::open(directory.path().to_path_buf())
            .expect("open journal");
        let operation_id = journal
            .admit_schema_v2_absent_final_for_test(
                test_intent(),
                serde_json::json!({"schema": 2, "fixture": true}),
                fixture.prepared.clone(),
                fixture.staged.clone(),
                valid_capacity_plan(),
            )
            .expect("admit absent-final record");
        let record_path = journal.record_path_for_test(operation_id);
        let bytes_before = fs::read(&record_path).expect("record bytes before classification");
        let record_before = journal
            .record(operation_id)
            .cloned()
            .expect("record before");
        let capacity_before = journal.capacity_claims_for_test();

        assert_eq!(
            journal
                .classify_schema_v2_absent_final_recovery(operation_id)
                .expect("first classification"),
            AbsentFinalRecoveryClassification::StagingPresentFinalAbsent
        );
        assert_eq!(
            journal
                .classify_schema_v2_absent_final_recovery(operation_id)
                .expect("repeat classification"),
            AbsentFinalRecoveryClassification::StagingPresentFinalAbsent
        );
        assert_eq!(
            fs::read(&record_path).expect("record bytes after classification"),
            bytes_before
        );
        assert_eq!(journal.record(operation_id), Some(&record_before));
        assert_eq!(journal.capacity_claims_for_test(), capacity_before);
        drop(journal);

        let reopened = OperationJournalCoordinator::open(directory.path().to_path_buf())
            .expect("reopen journal");
        assert_eq!(reopened.record(operation_id), Some(&record_before));
        assert_eq!(
            reopened
                .classify_schema_v2_absent_final_recovery(operation_id)
                .expect("restart classification"),
            AbsentFinalRecoveryClassification::StagingPresentFinalAbsent
        );
        assert_eq!(
            fs::read(&record_path).expect("record bytes after restart"),
            bytes_before
        );
        assert_eq!(reopened.capacity_claims_for_test(), capacity_before);
    }

    #[test]
    fn coordinator_records_matching_observation_idempotently_and_after_restart() {
        let directory = tempfile::tempdir().expect("journal directory");
        let fixture = Fixture::new();
        let mut journal = OperationJournalCoordinator::open(directory.path().to_path_buf())
            .expect("open journal");
        let operation_id = journal
            .admit_schema_v2_absent_final_for_test(
                test_intent(),
                serde_json::json!({"schema": 2}),
                fixture.prepared.clone(),
                fixture.staged.clone(),
                valid_capacity_plan(),
            )
            .expect("admit absent-final record");
        fs::rename(fixture.staging_path(), fixture.final_path()).expect("rename into final");
        let record_path = journal.record_path_for_test(operation_id);
        let capacity_before = journal.capacity_claims_for_test();

        assert_eq!(
            journal
                .record_schema_v2_absent_final_recovery_observation(operation_id)
                .expect("record matching observation"),
            AbsentFinalRecoveryClassification::StagingMissingFinalMatches
        );
        let record_after_first = journal.record(operation_id).cloned().expect("record");
        let bytes_after_first = fs::read(&record_path).expect("record bytes");
        let observation = record_after_first
            .absent_final_recovery_observation
            .as_ref()
            .expect("recovery observation");
        assert_eq!(
            observation.target_parent_stable_id,
            fixture.prepared.target_parent.identity.stable_id
        );
        assert_eq!(
            observation.final_stable_id,
            match &fixture.staged.participant {
                FilesystemStagedParticipant::CopyValidated { staging, .. } => {
                    staging.identity.stable_id.clone()
                }
            }
        );
        assert_eq!(observation.final_len, STAGING_BYTES.len() as u64);
        assert!(matches!(
            observation.final_content,
            PreparedFileEvidence::ContentHash(_)
        ));
        assert_eq!(record_after_first.phase, OperationPhase::FilesystemStaged);
        assert_eq!(journal.capacity_claims_for_test(), capacity_before);

        journal
            .record_schema_v2_absent_final_recovery_observation(operation_id)
            .expect("repeat equivalent observation");
        assert_eq!(journal.record(operation_id), Some(&record_after_first));
        assert_eq!(
            fs::read(&record_path).expect("repeat record bytes"),
            bytes_after_first
        );
        assert_eq!(journal.capacity_claims_for_test(), capacity_before);
        drop(journal);

        let reopened = OperationJournalCoordinator::open(directory.path().to_path_buf())
            .expect("reopen journal");
        assert_eq!(
            reopened
                .record(operation_id)
                .and_then(|record| record.absent_final_recovery_observation.as_ref()),
            record_after_first
                .absent_final_recovery_observation
                .as_ref()
        );
        assert_eq!(
            reopened
                .classify_schema_v2_absent_final_recovery(operation_id)
                .expect("revalidate live matching state"),
            AbsentFinalRecoveryClassification::StagingMissingFinalMatches
        );
        fs::write(fixture.final_path(), vec![b'x'; STAGING_BYTES.len()])
            .expect("drift final content");
        assert_eq!(
            reopened
                .classify_schema_v2_absent_final_recovery(operation_id)
                .expect("classify changed live state"),
            AbsentFinalRecoveryClassification::IdentityAmbiguous(
                AbsentFinalRecoveryAmbiguity::FinalContentDrift
            )
        );
        assert_eq!(
            reopened
                .record(operation_id)
                .and_then(|record| record.absent_final_recovery_observation.as_ref()),
            record_after_first
                .absent_final_recovery_observation
                .as_ref()
        );
    }

    #[test]
    fn coordinator_records_transaction_owned_proof_idempotently_and_after_restart() {
        let directory = tempfile::tempdir().expect("journal directory");
        let fixture = Fixture::new();
        let mut journal = OperationJournalCoordinator::open(directory.path().to_path_buf())
            .expect("open journal");
        let operation_id = journal
            .admit_schema_v2_absent_final_for_test(
                test_intent(),
                serde_json::json!({"schema": 2}),
                fixture.prepared.clone(),
                fixture.staged.clone(),
                valid_capacity_plan(),
            )
            .expect("admit absent-final record");
        fs::rename(fixture.staging_path(), fixture.final_path()).expect("rename into final");
        journal
            .record_schema_v2_absent_final_recovery_observation(operation_id)
            .expect("record matching observation");
        let record_path = journal.record_path_for_test(operation_id);
        let capacity_before = journal.capacity_claims_for_test();
        let final_bytes_before = fs::read(fixture.final_path()).expect("final bytes before proof");
        let record_before_proof = journal.record(operation_id).cloned().expect("record");

        assert_eq!(
            journal
                .record_schema_v2_absent_final_transaction_owned_proof(operation_id)
                .expect("record transaction-owned proof"),
            AbsentFinalRecoveryClassification::StagingMissingFinalMatches
        );
        let record_after_first = journal.record(operation_id).cloned().expect("record");
        let proof = record_after_first
            .absent_final_transaction_owned_proof
            .as_ref()
            .expect("transaction-owned proof");
        let observation = record_before_proof
            .absent_final_recovery_observation
            .as_ref()
            .expect("recovery observation");
        assert_eq!(
            proof,
            &AbsentFinalTransactionOwnedProof {
                target_parent_stable_id: observation.target_parent_stable_id.clone(),
                final_stable_id: observation.final_stable_id.clone(),
                final_len: observation.final_len,
                final_content: observation.final_content.clone(),
            }
        );
        assert_eq!(record_after_first.phase, OperationPhase::FilesystemStaged);
        assert_eq!(
            record_after_first.created_unix_ms,
            record_before_proof.created_unix_ms
        );
        assert_eq!(journal.capacity_claims_for_test(), capacity_before);
        assert_eq!(fs::read(fixture.final_path()).unwrap(), final_bytes_before);
        assert!(!fixture.staging_path().exists());
        let bytes_after_first = fs::read(&record_path).expect("record bytes after proof");

        journal
            .record_schema_v2_absent_final_transaction_owned_proof(operation_id)
            .expect("repeat transaction-owned proof");
        assert_eq!(journal.record(operation_id), Some(&record_after_first));
        assert_eq!(fs::read(&record_path).unwrap(), bytes_after_first);
        assert_eq!(journal.capacity_claims_for_test(), capacity_before);
        assert_eq!(fs::read(fixture.final_path()).unwrap(), final_bytes_before);
        drop(journal);

        let mut reopened = OperationJournalCoordinator::open(directory.path().to_path_buf())
            .expect("reopen journal");
        assert_eq!(reopened.record(operation_id), Some(&record_after_first));
        reopened
            .record_schema_v2_absent_final_transaction_owned_proof(operation_id)
            .expect("repeat transaction-owned proof after restart");
        assert_eq!(reopened.record(operation_id), Some(&record_after_first));
        assert_eq!(fs::read(&record_path).unwrap(), bytes_after_first);
        assert_eq!(reopened.capacity_claims_for_test(), capacity_before);
        assert_eq!(fs::read(fixture.final_path()).unwrap(), final_bytes_before);
        assert!(!fixture.staging_path().exists());
    }

    #[test]
    fn coordinator_requires_existing_matching_observation_for_transaction_owned_proof() {
        let directory = tempfile::tempdir().expect("journal directory");
        let fixture = Fixture::new();
        let mut journal = OperationJournalCoordinator::open(directory.path().to_path_buf())
            .expect("open journal");
        let operation_id = journal
            .admit_schema_v2_absent_final_for_test(
                test_intent(),
                serde_json::json!({"schema": 2}),
                fixture.prepared.clone(),
                fixture.staged.clone(),
                valid_capacity_plan(),
            )
            .expect("admit absent-final record");
        fs::rename(fixture.staging_path(), fixture.final_path()).expect("rename into final");
        let record_path = journal.record_path_for_test(operation_id);
        let record_before = journal.record(operation_id).cloned().expect("record");
        let bytes_before = fs::read(&record_path).expect("record bytes");
        let capacity_before = journal.capacity_claims_for_test();
        let final_bytes_before = fs::read(fixture.final_path()).expect("final bytes");

        assert!(matches!(
            journal.record_schema_v2_absent_final_transaction_owned_proof(operation_id),
            Err(crate::native_app::transaction_history::operation_journal::JournalError::InvalidRecoveryObservation { .. })
        ));
        assert_eq!(journal.record(operation_id), Some(&record_before));
        assert_eq!(fs::read(&record_path).unwrap(), bytes_before);
        assert_eq!(journal.capacity_claims_for_test(), capacity_before);
        assert_eq!(fs::read(fixture.final_path()).unwrap(), final_bytes_before);
        assert!(!fixture.staging_path().exists());
    }

    #[test]
    fn coordinator_rejects_stale_final_parent_and_identity_transaction_owned_proof() {
        let directory = tempfile::tempdir().expect("journal directory");
        let fixture = Fixture::new();
        let mut journal = OperationJournalCoordinator::open(directory.path().to_path_buf())
            .expect("open journal");
        let operation_id = journal
            .admit_schema_v2_absent_final_for_test(
                test_intent(),
                serde_json::json!({"schema": 2}),
                fixture.prepared.clone(),
                fixture.staged.clone(),
                valid_capacity_plan(),
            )
            .expect("admit absent-final record");
        fs::rename(fixture.staging_path(), fixture.final_path()).expect("rename into final");
        journal
            .record_schema_v2_absent_final_recovery_observation(operation_id)
            .expect("record matching observation");
        let record_path = journal.record_path_for_test(operation_id);
        let record_before = journal.record(operation_id).cloned().expect("record");
        let bytes_before = fs::read(&record_path).expect("record bytes");
        let capacity_before = journal.capacity_claims_for_test();

        fs::write(fixture.final_path(), vec![b'x'; STAGING_BYTES.len()])
            .expect("drift final content");
        let stale_final_bytes = fs::read(fixture.final_path()).expect("stale final bytes");
        assert!(matches!(
            journal.record_schema_v2_absent_final_transaction_owned_proof(operation_id),
            Err(crate::native_app::transaction_history::operation_journal::JournalError::InvalidRecoveryObservation { .. })
        ));
        assert_eq!(journal.record(operation_id), Some(&record_before));
        assert_eq!(fs::read(&record_path).unwrap(), bytes_before);
        assert_eq!(journal.capacity_claims_for_test(), capacity_before);
        assert_eq!(fs::read(fixture.final_path()).unwrap(), stale_final_bytes);

        fs::remove_file(fixture.final_path()).expect("remove stale final");
        fs::write(fixture.final_path(), STAGING_BYTES).expect("replace final identity");
        let conflicting_final_bytes = fs::read(fixture.final_path()).expect("conflicting bytes");
        assert!(matches!(
            journal.record_schema_v2_absent_final_transaction_owned_proof(operation_id),
            Err(crate::native_app::transaction_history::operation_journal::JournalError::InvalidRecoveryObservation { .. })
        ));
        assert_eq!(journal.record(operation_id), Some(&record_before));
        assert_eq!(fs::read(&record_path).unwrap(), bytes_before);
        assert_eq!(journal.capacity_claims_for_test(), capacity_before);
        assert_eq!(
            fs::read(fixture.final_path()).unwrap(),
            conflicting_final_bytes
        );

        let moved_parent = fixture.replace_parent();
        let moved_final_bytes = fs::read(moved_parent.join(FINAL_LEAF)).expect("moved final bytes");
        assert!(matches!(
            journal.record_schema_v2_absent_final_transaction_owned_proof(operation_id),
            Err(crate::native_app::transaction_history::operation_journal::JournalError::InvalidRecoveryObservation { .. })
        ));
        assert_eq!(journal.record(operation_id), Some(&record_before));
        assert_eq!(fs::read(&record_path).unwrap(), bytes_before);
        assert_eq!(journal.capacity_claims_for_test(), capacity_before);
        assert_eq!(
            fs::read(moved_parent.join(FINAL_LEAF)).unwrap(),
            moved_final_bytes
        );
        assert!(!fixture.final_path().exists());
    }

    #[test]
    fn coordinator_rejects_nonmatching_transaction_owned_proof_classifications() {
        let cases = [
            AbsentFinalRecoveryClassification::StagingPresentFinalAbsent,
            AbsentFinalRecoveryClassification::BothPresent,
            AbsentFinalRecoveryClassification::NeitherPresent,
            AbsentFinalRecoveryClassification::Collision {
                staging: CollisionStagingState::Present,
            },
            AbsentFinalRecoveryClassification::Collision {
                staging: CollisionStagingState::Missing,
            },
        ];
        for expected in cases {
            let directory = tempfile::tempdir().expect("journal directory");
            let fixture = Fixture::new();
            let mut journal = OperationJournalCoordinator::open(directory.path().to_path_buf())
                .expect("open journal");
            let operation_id = journal
                .admit_schema_v2_absent_final_for_test(
                    test_intent(),
                    serde_json::json!({"schema": 2}),
                    fixture.prepared.clone(),
                    fixture.staged.clone(),
                    valid_capacity_plan(),
                )
                .expect("admit absent-final record");
            match expected {
                AbsentFinalRecoveryClassification::StagingPresentFinalAbsent => {}
                AbsentFinalRecoveryClassification::BothPresent => {
                    fs::hard_link(fixture.staging_path(), fixture.final_path())
                        .expect("hard-link final");
                }
                AbsentFinalRecoveryClassification::NeitherPresent => {
                    fs::remove_file(fixture.staging_path()).expect("remove staging");
                }
                AbsentFinalRecoveryClassification::Collision {
                    staging: CollisionStagingState::Present,
                } => {
                    fs::write(fixture.final_path(), b"foreign final").expect("foreign final");
                }
                AbsentFinalRecoveryClassification::Collision {
                    staging: CollisionStagingState::Missing,
                } => {
                    fs::remove_file(fixture.staging_path()).expect("remove staging");
                    fs::write(fixture.final_path(), b"foreign final").expect("foreign final");
                }
                other => panic!("unexpected nonmatching classification {other:?}"),
            }
            let path = journal.record_path_for_test(operation_id);
            let bytes_before = fs::read(&path).expect("record bytes before proof");
            let record_before = journal.record(operation_id).cloned().expect("record");
            let capacity_before = journal.capacity_claims_for_test();
            assert_eq!(
                journal
                    .classify_schema_v2_absent_final_recovery(operation_id)
                    .expect("classify live state"),
                expected
            );
            assert!(matches!(
                journal.record_schema_v2_absent_final_transaction_owned_proof(operation_id),
                Err(crate::native_app::transaction_history::operation_journal::JournalError::InvalidRecoveryObservation { .. })
            ));
            assert_eq!(journal.record(operation_id), Some(&record_before));
            assert_eq!(fs::read(&path).unwrap(), bytes_before);
            assert_eq!(journal.capacity_claims_for_test(), capacity_before);
            assert!(
                journal
                    .record(operation_id)
                    .unwrap()
                    .absent_final_transaction_owned_proof
                    .is_none()
            );
        }
    }

    #[test]
    fn coordinator_rejects_stale_or_conflicting_observation_without_durable_change() {
        let directory = tempfile::tempdir().expect("journal directory");
        let fixture = Fixture::new();
        let mut journal = OperationJournalCoordinator::open(directory.path().to_path_buf())
            .expect("open journal");
        let operation_id = journal
            .admit_schema_v2_absent_final_for_test(
                test_intent(),
                serde_json::json!({"schema": 2}),
                fixture.prepared.clone(),
                fixture.staged.clone(),
                valid_capacity_plan(),
            )
            .expect("admit absent-final record");
        fs::rename(fixture.staging_path(), fixture.final_path()).expect("rename into final");
        journal
            .record_schema_v2_absent_final_recovery_observation(operation_id)
            .expect("record initial observation");
        let record_before = journal
            .record(operation_id)
            .cloned()
            .expect("record before");
        let path = journal.record_path_for_test(operation_id);
        let bytes_before = fs::read(&path).expect("bytes before stale evidence");
        let capacity_before = journal.capacity_claims_for_test();

        fs::write(fixture.final_path(), vec![b'x'; STAGING_BYTES.len()])
            .expect("drift final content");
        assert!(matches!(
            journal.record_schema_v2_absent_final_recovery_observation(operation_id),
            Err(crate::native_app::transaction_history::operation_journal::JournalError::InvalidRecoveryObservation { .. })
        ));
        assert_eq!(journal.record(operation_id), Some(&record_before));
        assert_eq!(
            fs::read(&path).expect("bytes after stale evidence"),
            bytes_before
        );
        assert_eq!(journal.capacity_claims_for_test(), capacity_before);

        fs::remove_file(fixture.final_path()).expect("remove stale final");
        fs::write(fixture.final_path(), STAGING_BYTES).expect("replace final identity");
        assert!(matches!(
            journal.record_schema_v2_absent_final_recovery_observation(operation_id),
            Err(crate::native_app::transaction_history::operation_journal::JournalError::InvalidRecoveryObservation { .. })
        ));
        assert_eq!(journal.record(operation_id), Some(&record_before));
        assert_eq!(
            fs::read(&path).expect("bytes after conflicting evidence"),
            bytes_before
        );
        assert_eq!(journal.capacity_claims_for_test(), capacity_before);
    }

    #[test]
    fn coordinator_does_not_record_nonmatching_classifications() {
        let cases = [
            AbsentFinalRecoveryClassification::StagingPresentFinalAbsent,
            AbsentFinalRecoveryClassification::BothPresent,
            AbsentFinalRecoveryClassification::NeitherPresent,
            AbsentFinalRecoveryClassification::Collision {
                staging: CollisionStagingState::Present,
            },
        ];
        for expected in cases {
            let directory = tempfile::tempdir().expect("journal directory");
            let fixture = Fixture::new();
            let mut journal = OperationJournalCoordinator::open(directory.path().to_path_buf())
                .expect("open journal");
            let operation_id = journal
                .admit_schema_v2_absent_final_for_test(
                    test_intent(),
                    serde_json::json!({"schema": 2}),
                    fixture.prepared.clone(),
                    fixture.staged.clone(),
                    valid_capacity_plan(),
                )
                .expect("admit absent-final record");
            match expected {
                AbsentFinalRecoveryClassification::StagingPresentFinalAbsent => {}
                AbsentFinalRecoveryClassification::BothPresent => {
                    fs::hard_link(fixture.staging_path(), fixture.final_path())
                        .expect("hard-link final");
                }
                AbsentFinalRecoveryClassification::NeitherPresent => {
                    fs::remove_file(fixture.staging_path()).expect("remove staging");
                }
                AbsentFinalRecoveryClassification::Collision { .. } => {
                    fs::write(fixture.final_path(), b"foreign final").expect("foreign final");
                }
                other => panic!("unexpected nonmatching fixture {other:?}"),
            }
            let path = journal.record_path_for_test(operation_id);
            let bytes_before = fs::read(&path).expect("bytes before nonmatching classification");
            let record_before = journal.record(operation_id).cloned().expect("record");
            let capacity_before = journal.capacity_claims_for_test();
            assert_eq!(
                journal
                    .record_schema_v2_absent_final_recovery_observation(operation_id)
                    .expect("nonmatching classification"),
                expected
            );
            assert_eq!(journal.record(operation_id), Some(&record_before));
            assert_eq!(
                fs::read(&path).expect("bytes after nonmatching classification"),
                bytes_before
            );
            assert_eq!(journal.capacity_claims_for_test(), capacity_before);
            assert!(
                journal
                    .record(operation_id)
                    .unwrap()
                    .absent_final_recovery_observation
                    .is_none()
            );
        }
    }

    #[test]
    fn malformed_copy_evidence_is_rejected_before_live_inspection() {
        let directory = tempfile::tempdir().expect("journal directory");
        let fixture = Fixture::new();
        let mut prepared = fixture.prepared.clone();
        prepared.copy_validated_evidence = PreparedFileEvidence::Metadata {
            len: STAGING_BYTES.len() as u64,
            modified_ns: None,
            is_dir: false,
        };
        let mut journal = OperationJournalCoordinator::open(directory.path().to_path_buf())
            .expect("open journal");
        let result = journal.admit_schema_v2_absent_final_for_test(
            test_intent(),
            serde_json::json!({"schema": 2}),
            prepared,
            fixture.staged.clone(),
            valid_capacity_plan(),
        );
        assert!(matches!(
            result,
            Err(
                crate::native_app::transaction_history::operation_journal::JournalError::Write { .. }
            )
        ));

        let mut staged = fixture.staged.clone();
        let FilesystemStagedParticipant::CopyValidated { staging, .. } = &mut staged.participant;
        staging.identity.stable_id.clear();
        let result = journal.admit_schema_v2_absent_final_for_test(
            test_intent(),
            serde_json::json!({"schema": 2}),
            fixture.prepared,
            staged,
            valid_capacity_plan(),
        );
        assert!(matches!(
            result,
            Err(
                crate::native_app::transaction_history::operation_journal::JournalError::Write { .. }
            )
        ));
        assert_eq!(journal.recovery_summary().record_count, 0);
    }

    fn test_intent() -> OperationIntent {
        OperationIntent {
            actor: OperationActor::User,
            kind: OperationKind::FileHistory,
            label: String::from("absent-final recovery"),
        }
    }

    fn valid_capacity_plan() -> DurableCapacityPlan {
        DurableCapacityPlan {
            volumes: vec![DurableVolumeCapacity {
                identity: VolumeIdentity { device: 77 },
                allocation_unit: 4096,
                allocation_class: CapacityAllocationClass::DestinationStaging,
                logical_bytes: 4096,
                allocated_bytes: 4096,
                protected_free_bytes: PROTECTED_FREE_SPACE_FLOOR,
            }],
        }
    }
}
