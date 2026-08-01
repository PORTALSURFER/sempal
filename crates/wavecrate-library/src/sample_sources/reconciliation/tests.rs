use std::ffi::OsString;
use std::path::{Path, PathBuf};

use super::*;
use crate::sample_sources::SourceId;

fn capture_boundary(first_sequence: Option<u64>, last_sequence: Option<u64>) -> CaptureBoundary {
    CaptureBoundary::try_new(123, first_sequence, last_sequence).expect("valid capture boundary")
}

fn provenance(
    source: &str,
    root: Option<Vec<u8>>,
    stream: Option<Vec<u8>>,
    first_sequence: Option<u64>,
    last_sequence: Option<u64>,
) -> RawObservationProvenance {
    provenance_at_generation(source, root, stream, first_sequence, last_sequence, 7)
}

fn provenance_at_generation(
    source: &str,
    root: Option<Vec<u8>>,
    stream: Option<Vec<u8>>,
    first_sequence: Option<u64>,
    last_sequence: Option<u64>,
    generation: u64,
) -> RawObservationProvenance {
    RawObservationProvenance::new(
        SourceId::from_string(source),
        root.map(RootIdentity::from_bytes),
        stream.map(BackendStreamIdentity::from_bytes),
        WatcherGeneration::new(generation),
        capture_boundary(first_sequence, last_sequence),
    )
}

fn limits() -> RawObservationLimits {
    RawObservationLimits::new(64, usize::MAX, usize::MAX).expect("valid limits")
}

fn path(path: &str, role: RawPathRole) -> RawObservedPath {
    RawObservedPath::new(PathBuf::from(path), role)
}

fn envelope(observations: Vec<RawObservation>) -> RawObservationEnvelope {
    RawObservationEnvelope::try_new(
        provenance("source-a", Some(vec![1]), Some(vec![2]), None, None),
        observations,
        limits(),
    )
    .expect("valid raw envelope")
}

fn normalized(observations: Vec<RawObservation>) -> NormalizedObservation {
    normalize_observation(envelope(observations))
}

fn scope_path(scope: &ReconciliationScope) -> Option<&Path> {
    scope.path().map(RootRelativePath::as_path)
}

#[test]
fn limits_are_checked_and_accounting_is_exact_without_truncation() {
    assert_eq!(
        RawObservationLimits::new(0, 1, 1),
        Err(RawEnvelopeError::ZeroEventLimit)
    );
    assert_eq!(
        RawObservationEnvelope::try_new(
            provenance("source-a", None, None, None, None),
            Vec::new(),
            limits(),
        ),
        Err(RawEnvelopeError::EmptyEnvelope)
    );

    let metadata = RawObservationMetadata::new()
        .with_flags(1)
        .with_event_id(9)
        .with_cursor(vec![8, 7])
        .with_detail(OsString::from("err"));
    let observations = vec![
        RawObservation::new(
            RawEventKind::Create,
            vec![
                path("b", RawPathRole::Subject),
                path("a", RawPathRole::Subject),
            ],
        )
        .with_metadata(metadata),
        RawObservation::new(RawEventKind::Modify, vec![path("b", RawPathRole::Subject)]),
    ];
    let raw = RawObservationEnvelope::try_new(
        provenance("source-a", None, None, None, None),
        observations.clone(),
        limits(),
    )
    .expect("bounded observations");

    assert_eq!(raw.accounting().event_count(), 2);
    assert_eq!(raw.accounting().path_bytes(), 3);
    assert_eq!(raw.accounting().metadata_bytes(), 26);
    assert_eq!(raw.accounting().total_bytes(), Ok(29));
    assert_eq!(raw.observations(), observations.as_slice());
    assert_eq!(
        raw.observations()[0]
            .paths()
            .iter()
            .map(|observed| observed.path())
            .collect::<Vec<_>>(),
        vec![Path::new("b"), Path::new("a")]
    );

    let too_few_events =
        RawObservationLimits::new(1, usize::MAX, usize::MAX).expect("valid event limit");
    assert_eq!(
        RawObservationEnvelope::try_new(
            provenance("source-a", None, None, None, None),
            observations.clone(),
            too_few_events,
        ),
        Err(RawEnvelopeError::LimitExceeded {
            limit: RawEnvelopeLimit::EventCount,
            actual: 2,
            maximum: 1,
        })
    );

    let too_few_paths = RawObservationLimits::new(64, 2, usize::MAX).expect("valid path limit");
    assert!(matches!(
        RawObservationEnvelope::try_new(
            provenance("source-a", None, None, None, None),
            observations.clone(),
            too_few_paths,
        ),
        Err(RawEnvelopeError::LimitExceeded {
            limit: RawEnvelopeLimit::PathBytes,
            actual: 3,
            maximum: 2,
        })
    ));

    let too_few_metadata =
        RawObservationLimits::new(64, usize::MAX, 25).expect("valid metadata limit");
    assert!(matches!(
        RawObservationEnvelope::try_new(
            provenance("source-a", None, None, None, None),
            observations,
            too_few_metadata,
        ),
        Err(RawEnvelopeError::LimitExceeded {
            limit: RawEnvelopeLimit::MetadataBytes,
            actual: 26,
            maximum: 25,
        })
    ));
}

#[test]
fn root_relative_paths_validate_without_rebuilding_native_spelling() {
    let original = PathBuf::from("folder/./item");
    let relative = RootRelativePath::try_from_path(original.clone()).expect("valid relative path");
    assert_eq!(relative.as_path(), original.as_path());
    assert_eq!(relative.clone().into_path(), original);

    assert_eq!(
        RootRelativePath::try_from_path(PathBuf::new()),
        Err(RootRelativePathError::Empty)
    );
    assert_eq!(
        RootRelativePath::try_from_path(PathBuf::from(".")),
        Err(RootRelativePathError::NoNormalComponent)
    );
    assert_eq!(
        RootRelativePath::try_from_path(PathBuf::from("../escape")),
        Err(RootRelativePathError::ParentTraversal)
    );
    assert_eq!(
        RootRelativePath::try_from_path(PathBuf::from("folder/../escape")),
        Err(RootRelativePathError::ParentTraversal)
    );
    assert_eq!(
        RootRelativePath::try_from_path(PathBuf::from("/absolute")),
        Err(RootRelativePathError::Absolute)
    );

    #[cfg(windows)]
    assert_eq!(
        RootRelativePath::try_from_path(PathBuf::from(r"C:\absolute")),
        Err(RootRelativePathError::PlatformPrefix)
    );
}

#[cfg(unix)]
#[test]
fn root_relative_paths_preserve_non_utf8_native_spelling() {
    use std::os::unix::ffi::OsStringExt;

    let native = PathBuf::from(OsString::from_vec(vec![b'n', b'a', 0x80]));
    let relative = RootRelativePath::try_from_path(native.clone()).expect("valid native path");
    assert_eq!(
        relative.as_path().as_os_str().as_encoded_bytes(),
        &[b'n', b'a', 0x80]
    );
}

#[test]
fn root_relative_paths_reject_embedded_nul_before_component_validation() {
    assert_eq!(
        RootRelativePath::try_from_path(PathBuf::from("../embedded\0nul")),
        Err(RootRelativePathError::EmbeddedNul)
    );
    assert_eq!(
        RootRelativePathError::EmbeddedNul.to_string(),
        "path contains an embedded NUL byte"
    );
}

#[test]
fn embedded_nul_create_and_modify_evidence_is_retained_as_source_audit() {
    let embedded_nul_path = PathBuf::from("embedded\0nul");
    let observations = vec![
        RawObservation::new(
            RawEventKind::Create,
            vec![RawObservedPath::new(
                embedded_nul_path.clone(),
                RawPathRole::Subject,
            )],
        ),
        RawObservation::new(
            RawEventKind::Modify,
            vec![RawObservedPath::new(
                embedded_nul_path,
                RawPathRole::Subject,
            )],
        ),
    ];
    let raw = RawObservationEnvelope::try_new(
        provenance("source-a", None, None, None, None),
        observations.clone(),
        limits(),
    )
    .expect("raw embedded-NUL evidence is admissible");

    let normalized = normalize_observation(raw.clone());

    assert_eq!(normalized.scopes().len(), observations.len());
    assert!(normalized.scopes().iter().all(|scope| {
        scope.kind() == ReconciliationScopeKind::SourceAudit
            && scope.path().is_none()
            && scope.reason()
                == NormalizationReason::InvalidPath {
                    error: RootRelativePathError::EmbeddedNul,
                }
    }));
    assert_eq!(normalized.envelope(), &raw);
    assert_eq!(
        normalized.envelope().observations(),
        observations.as_slice()
    );
}

#[test]
fn create_modify_directory_empty_folder_symlink_and_duplicates_keep_order() {
    let result = normalized(vec![RawObservation::new(
        RawEventKind::Create,
        vec![
            path("file", RawPathRole::Subject),
            path("directory", RawPathRole::Subject).with_hint(RawPathHint::Directory),
            path("empty", RawPathRole::Subject).with_hint(RawPathHint::EmptyFolder),
            path("link", RawPathRole::Subject).with_hint(RawPathHint::Symlink),
            path("file", RawPathRole::Subject),
        ],
    )]);
    let scopes = result.scopes();

    assert_eq!(scopes.len(), 5);
    assert_eq!(scopes[0].kind(), ReconciliationScopeKind::ExactEntry);
    assert_eq!(scope_path(&scopes[0]), Some(Path::new("file")));
    assert_eq!(scopes[1].kind(), ReconciliationScopeKind::Subtree);
    assert_eq!(scope_path(&scopes[1]), Some(Path::new("directory")));
    assert_eq!(scopes[2].kind(), ReconciliationScopeKind::Subtree);
    assert_eq!(scope_path(&scopes[2]), Some(Path::new("empty")));
    assert_eq!(scopes[3].kind(), ReconciliationScopeKind::ExactEntry);
    assert_eq!(scope_path(&scopes[3]), Some(Path::new("link")));
    assert_eq!(scopes[4].kind(), ReconciliationScopeKind::ExactEntry);
    assert_eq!(scope_path(&scopes[4]), Some(Path::new("file")));
    assert_eq!(scopes[0].role(), Some(RawPathRole::Subject));
}

#[test]
fn delete_root_and_missing_parent_evidence_widens_conservatively() {
    let result = normalized(vec![
        RawObservation::new(
            RawEventKind::Delete,
            vec![path("file", RawPathRole::Subject)],
        ),
        RawObservation::new(
            RawEventKind::Delete,
            vec![path("folder/file", RawPathRole::Subject).with_hint(RawPathHint::Directory)],
        ),
        RawObservation::new(
            RawEventKind::Delete,
            vec![path("folder/missing", RawPathRole::Subject).with_hint(RawPathHint::Absent)],
        ),
        RawObservation::new(
            RawEventKind::Delete,
            vec![path("top", RawPathRole::Subject)],
        )
        .with_uncertainty(ObservationUncertainty::MISSING_PARENT),
        RawObservation::new(
            RawEventKind::Delete,
            vec![path("root-marker", RawPathRole::Subject).with_hint(RawPathHint::Root)],
        ),
    ]);
    let scopes = result.scopes();

    assert_eq!(scopes[0].kind(), ReconciliationScopeKind::ExactEntry);
    assert_eq!(scope_path(&scopes[0]), Some(Path::new("file")));
    assert_eq!(scopes[1].kind(), ReconciliationScopeKind::Subtree);
    assert_eq!(scope_path(&scopes[1]), Some(Path::new("folder")));
    assert_eq!(scopes[2].kind(), ReconciliationScopeKind::Subtree);
    assert_eq!(scope_path(&scopes[2]), Some(Path::new("folder")));
    assert_eq!(scopes[3].kind(), ReconciliationScopeKind::SourceAudit);
    assert_eq!(scopes[4].kind(), ReconciliationScopeKind::SourceAudit);
    assert_eq!(scopes.len(), 5);
}

#[test]
fn complete_and_incomplete_rename_preserve_endpoint_roles_and_order() {
    let result = normalized(vec![
        RawObservation::new(
            RawEventKind::Rename,
            vec![
                path("old/file", RawPathRole::RenameSource),
                path("new/file", RawPathRole::RenameDestination),
            ],
        ),
        RawObservation::new(
            RawEventKind::Rename,
            vec![path("old/only", RawPathRole::RenameSource)],
        ),
        RawObservation::new(
            RawEventKind::Rename,
            vec![
                path("old/directory", RawPathRole::RenameSource).with_hint(RawPathHint::Directory),
                path("new/directory", RawPathRole::RenameDestination),
            ],
        ),
    ]);
    let scopes = result.scopes();

    assert_eq!(scopes[0].kind(), ReconciliationScopeKind::ExactEntry);
    assert_eq!(scope_path(&scopes[0]), Some(Path::new("old/file")));
    assert_eq!(scopes[0].role(), Some(RawPathRole::RenameSource));
    assert_eq!(scopes[1].kind(), ReconciliationScopeKind::ExactEntry);
    assert_eq!(scope_path(&scopes[1]), Some(Path::new("new/file")));
    assert_eq!(scopes[1].role(), Some(RawPathRole::RenameDestination));
    assert_eq!(scopes[2].kind(), ReconciliationScopeKind::Subtree);
    assert_eq!(scope_path(&scopes[2]), Some(Path::new("old")));
    assert_eq!(scopes[3].kind(), ReconciliationScopeKind::SourceAudit);
    assert_eq!(scopes[4].kind(), ReconciliationScopeKind::Subtree);
    assert_eq!(scope_path(&scopes[4]), Some(Path::new("old")));
    assert_eq!(scopes[5].kind(), ReconciliationScopeKind::ExactEntry);
    assert_eq!(scope_path(&scopes[5]), Some(Path::new("new/directory")));
}

#[test]
fn complete_and_uncertain_copy_keep_destination_and_optional_source() {
    let result = normalized(vec![
        RawObservation::new(
            RawEventKind::Copy,
            vec![
                path("source/file", RawPathRole::CopySource),
                path("destination/file", RawPathRole::CopyDestination),
            ],
        ),
        RawObservation::new(
            RawEventKind::Copy,
            vec![
                path("destination/dir", RawPathRole::CopyDestination)
                    .with_hint(RawPathHint::Directory),
            ],
        ),
        RawObservation::new(
            RawEventKind::Copy,
            vec![
                path("destination/unknown", RawPathRole::CopyDestination)
                    .with_hint(RawPathHint::Unknown),
            ],
        ),
    ]);
    let scopes = result.scopes();

    assert_eq!(scopes[0].kind(), ReconciliationScopeKind::ExactEntry);
    assert_eq!(scope_path(&scopes[0]), Some(Path::new("source/file")));
    assert_eq!(scopes[0].role(), Some(RawPathRole::CopySource));
    assert_eq!(scopes[1].kind(), ReconciliationScopeKind::ExactEntry);
    assert_eq!(scope_path(&scopes[1]), Some(Path::new("destination/file")));
    assert_eq!(scopes[1].role(), Some(RawPathRole::CopyDestination));
    assert_eq!(scopes[2].kind(), ReconciliationScopeKind::Subtree);
    assert_eq!(scope_path(&scopes[2]), Some(Path::new("destination")));
    assert_eq!(scopes[3].kind(), ReconciliationScopeKind::Subtree);
    assert_eq!(scope_path(&scopes[3]), Some(Path::new("destination")));
}

#[test]
fn invalid_paths_unsupported_only_and_mixed_evidence_are_visible() {
    let result = normalized(vec![
        RawObservation::new(
            RawEventKind::Create,
            vec![
                path("valid", RawPathRole::Subject),
                path("../escape", RawPathRole::Subject),
            ],
        ),
        RawObservation::new(RawEventKind::Unsupported, Vec::new()),
    ]);
    let scopes = result.scopes();

    assert_eq!(scopes.len(), 3);
    assert_eq!(scopes[0].kind(), ReconciliationScopeKind::ExactEntry);
    assert_eq!(scope_path(&scopes[0]), Some(Path::new("valid")));
    assert_eq!(scopes[1].kind(), ReconciliationScopeKind::SourceAudit);
    assert!(matches!(
        scopes[1].reason(),
        NormalizationReason::InvalidPath {
            error: RootRelativePathError::ParentTraversal
        }
    ));
    assert_eq!(scopes[2].kind(), ReconciliationScopeKind::SourceAudit);
    assert_eq!(scopes[2].reason(), NormalizationReason::Unsupported);
}

#[test]
fn root_overflow_and_error_events_widen_to_ordered_audits() {
    let result = normalized(vec![
        RawObservation::new(
            RawEventKind::Create,
            vec![path("file", RawPathRole::Subject)],
        ),
        RawObservation::new(RawEventKind::RootChanged, Vec::new()),
        RawObservation::new(RawEventKind::Overflow, Vec::new()),
        RawObservation::new(RawEventKind::Error, Vec::new()),
        RawObservation::new(RawEventKind::Unsupported, Vec::new()),
    ]);
    let scopes = result.scopes();

    assert_eq!(scopes.len(), 5);
    assert_eq!(scopes[0].kind(), ReconciliationScopeKind::ExactEntry);
    assert_eq!(scope_path(&scopes[0]), Some(Path::new("file")));
    assert_eq!(scopes[1].reason(), NormalizationReason::RootChanged);
    assert_eq!(scopes[2].reason(), NormalizationReason::Overflow);
    assert_eq!(scopes[3].reason(), NormalizationReason::BackendError);
    assert_eq!(scopes[4].reason(), NormalizationReason::Unsupported);
    assert!(
        scopes
            .iter()
            .skip(1)
            .all(|scope| scope.kind() == ReconciliationScopeKind::SourceAudit)
    );
}

#[test]
fn broad_uncertainty_adds_audit_without_dropping_supported_scopes() {
    let result = normalized(vec![
        RawObservation::new(
            RawEventKind::Modify,
            vec![path("folder/file", RawPathRole::Subject)],
        )
        .with_uncertainty(
            ObservationUncertainty::PATH_COVERAGE | ObservationUncertainty::MISSING_PARENT,
        ),
    ]);
    let scopes = result.scopes();

    assert_eq!(scopes.len(), 2);
    assert_eq!(scopes[0].kind(), ReconciliationScopeKind::Subtree);
    assert_eq!(scope_path(&scopes[0]), Some(Path::new("folder")));
    assert_eq!(scopes[1].kind(), ReconciliationScopeKind::SourceAudit);
    assert!(matches!(
        scopes[1].reason(),
        NormalizationReason::ExplicitUncertainty { uncertainty }
            if uncertainty.contains(ObservationUncertainty::PATH_COVERAGE)
    ));
    assert_eq!(
        ReconciliationScopeKind::ExactEntry.widen(ReconciliationScopeKind::Subtree),
        ReconciliationScopeKind::Subtree
    );
    assert_eq!(
        ReconciliationScopeKind::Subtree.widen(ReconciliationScopeKind::SourceAudit),
        ReconciliationScopeKind::SourceAudit
    );
}

#[test]
fn proof_is_unproven_by_default_and_checked_proof_is_preserved_exactly() {
    let valid_provenance = provenance("source-a", Some(vec![1]), Some(vec![2]), Some(11), Some(12));
    let observations = vec![RawObservation::new(
        RawEventKind::Create,
        vec![path("file", RawPathRole::Subject)],
    )];
    let unproven =
        RawObservationEnvelope::try_new(valid_provenance.clone(), observations.clone(), limits())
            .expect("unproven envelope");
    assert_eq!(unproven.proof(), &Proof::Unproven);

    let acknowledgement = DurablePriorAcknowledgement::new(10);
    let coverage = ReplayCoverage::try_new(10, 12, true).expect("contiguous coverage");
    let proof =
        WatcherContinuityProof::try_new(&valid_provenance, Some(acknowledgement), Some(coverage))
            .expect("valid continuity proof");
    let proven = RawObservationEnvelope::try_new_with_proof(
        valid_provenance,
        observations,
        limits(),
        Proof::WatcherContinuity(proof.clone()),
    )
    .expect("proven envelope");
    let normalized = normalize_observation(proven.clone());

    assert_eq!(normalized.proof(), proven.proof());
    assert_eq!(normalized.envelope(), &proven);
    assert_eq!(normalized.proof().watcher_continuity(), Some(&proof));
}

#[test]
fn continuity_proof_rejects_missing_identity_gaps_and_mismatches() {
    assert_eq!(
        CaptureBoundary::try_new(123, Some(12), Some(11)),
        Err(RawEnvelopeError::InvalidCaptureBoundary)
    );

    let no_root = provenance("source-a", None, Some(vec![2]), Some(11), Some(12));
    let coverage = ReplayCoverage::try_new(10, 12, true).expect("coverage");
    assert_eq!(
        WatcherContinuityProof::try_new(
            &no_root,
            Some(DurablePriorAcknowledgement::new(10)),
            Some(coverage),
        ),
        Err(RawEnvelopeError::MissingRootIdentity)
    );

    let no_stream = provenance("source-a", Some(vec![1]), None, Some(11), Some(12));
    assert_eq!(
        WatcherContinuityProof::try_new(
            &no_stream,
            Some(DurablePriorAcknowledgement::new(10)),
            Some(coverage),
        ),
        Err(RawEnvelopeError::MissingBackendStreamIdentity)
    );

    let valid = provenance("source-a", Some(vec![1]), Some(vec![2]), Some(11), Some(12));
    assert_eq!(
        WatcherContinuityProof::try_new(&valid, None, Some(coverage)),
        Err(RawEnvelopeError::MissingPriorAcknowledgement)
    );
    assert_eq!(
        WatcherContinuityProof::try_new(&valid, Some(DurablePriorAcknowledgement::new(10)), None,),
        Err(RawEnvelopeError::MissingReplayCoverage)
    );

    let gapped = ReplayCoverage::try_new(9, 12, true).expect("ordered coverage");
    assert_eq!(
        WatcherContinuityProof::try_new(
            &valid,
            Some(DurablePriorAcknowledgement::new(10)),
            Some(gapped),
        ),
        Err(RawEnvelopeError::GappedReplayCoverage)
    );
    let noncontiguous = ReplayCoverage::try_new(10, 12, false).expect("non-contiguous coverage");
    assert_eq!(
        WatcherContinuityProof::try_new(
            &valid,
            Some(DurablePriorAcknowledgement::new(10)),
            Some(noncontiguous),
        ),
        Err(RawEnvelopeError::GappedReplayCoverage)
    );
    let wrong_boundary = ReplayCoverage::try_new(10, 11, true).expect("wrong boundary");
    assert_eq!(
        WatcherContinuityProof::try_new(
            &valid,
            Some(DurablePriorAcknowledgement::new(10)),
            Some(wrong_boundary),
        ),
        Err(RawEnvelopeError::CoverageBoundaryMismatch)
    );

    let proof = WatcherContinuityProof::try_new(
        &valid,
        Some(DurablePriorAcknowledgement::new(10)),
        Some(coverage),
    )
    .expect("valid proof");
    let wrong_envelope_provenance =
        provenance("source-b", Some(vec![1]), Some(vec![2]), Some(11), Some(12));
    assert_eq!(
        RawObservationEnvelope::try_new_with_proof(
            wrong_envelope_provenance,
            vec![RawObservation::new(
                RawEventKind::Create,
                vec![path("file", RawPathRole::Subject)],
            )],
            limits(),
            Proof::WatcherContinuity(proof.clone()),
        ),
        Err(RawEnvelopeError::ProofMismatch)
    );

    for mismatched_provenance in [
        provenance_at_generation(
            "source-a",
            Some(vec![9]),
            Some(vec![2]),
            Some(11),
            Some(12),
            7,
        ),
        provenance_at_generation(
            "source-a",
            Some(vec![1]),
            Some(vec![9]),
            Some(11),
            Some(12),
            7,
        ),
        provenance_at_generation(
            "source-a",
            Some(vec![1]),
            Some(vec![2]),
            Some(11),
            Some(12),
            8,
        ),
    ] {
        assert_eq!(
            RawObservationEnvelope::try_new_with_proof(
                mismatched_provenance,
                vec![RawObservation::new(
                    RawEventKind::Create,
                    vec![path("file", RawPathRole::Subject)],
                )],
                limits(),
                Proof::WatcherContinuity(proof.clone()),
            ),
            Err(RawEnvelopeError::ProofMismatch)
        );
    }
}
