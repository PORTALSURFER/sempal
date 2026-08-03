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

fn adapter_admission_limits(
    max_in_flight: usize,
    max_retained_uncertainties: usize,
) -> ReconciliationAdmissionLimits {
    ReconciliationAdmissionLimits::new(
        1,
        limits(),
        limits(),
        max_in_flight,
        64,
        max_retained_uncertainties,
    )
    .expect("valid adapter admission limits")
}

fn adapter_batch(
    provenance: RawObservationProvenance,
    observations: Vec<RawObservation>,
) -> SyntheticObservationBatch {
    SyntheticObservationBatch::new(provenance, observations, limits())
}

fn adapter_observation(kind: RawEventKind, name: &str) -> RawObservation {
    RawObservation::new(kind, vec![path(name, RawPathRole::Subject)])
}

fn replay_prior(
    source: &str,
    root: &[u8],
    stream: &[u8],
    generation: u64,
    acknowledged_sequence: u64,
) -> ReplayPriorToken {
    ReplayPriorToken::new(
        SourceId::from_string(source),
        RootIdentity::from_bytes(root.to_vec()),
        BackendStreamIdentity::from_bytes(stream.to_vec()),
        WatcherGeneration::new(generation),
        acknowledged_sequence,
    )
}

fn adapter_registered(
    supervisor: &mut ReconciliationAdmissionSupervisor,
    source: &SourceId,
    root: &RootIdentity,
) -> (AdmissionLaneKey, WatcherGeneration) {
    let (lane, generation) = supervisor
        .register_lane(source.clone(), root.clone())
        .expect("register adapter lane");
    supervisor
        .begin_capture(&lane, generation)
        .expect("begin adapter capture");
    (lane, generation)
}

fn adapter_capture_provenance(
    source: &str,
    root: Option<Vec<u8>>,
    stream: Option<Vec<u8>>,
    generation: u64,
    first_sequence: Option<u64>,
    last_sequence: Option<u64>,
) -> RawObservationProvenance {
    provenance_at_generation(
        source,
        root,
        stream,
        first_sequence,
        last_sequence,
        generation,
    )
}

#[test]
fn capture_sequence_evidence_distinguishes_missing_ambiguous_and_exact() {
    assert_eq!(
        capture_boundary(None, None).sequence_evidence(),
        CaptureSequenceEvidence::Missing
    );
    assert_eq!(
        capture_boundary(Some(11), None).sequence_evidence(),
        CaptureSequenceEvidence::Ambiguous
    );
    assert_eq!(
        capture_boundary(None, Some(12)).sequence_evidence(),
        CaptureSequenceEvidence::Ambiguous
    );
    assert_eq!(
        capture_boundary(Some(11), Some(12)).sequence_evidence(),
        CaptureSequenceEvidence::Exact(CaptureSequenceRange::new(11, 12))
    );
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

fn assert_invalid_replay_case(
    batch: SyntheticObservationBatch,
    prior: Option<&ReplayPriorToken>,
    contiguous: bool,
    expected_reason: UncertaintyReason,
    expect_dispatch: bool,
) {
    let expected_observations = batch.observations().to_vec();
    let source = SourceId::from_string("source-a");
    let root = RootIdentity::from_bytes(b"root-a".to_vec());
    let mut supervisor = ReconciliationAdmissionSupervisor::new(adapter_admission_limits(2, 4));
    adapter_registered(&mut supervisor, &source, &root);

    let admission = {
        let mut adapter = ReconciliationAdapter::new(&mut supervisor);
        adapter
            .admit_replay(batch, prior, contiguous)
            .expect("invalid replay envelope remains constructible")
    };
    assert_eq!(
        admission.disposition(),
        AdapterDisposition::SourceAuditRequired
    );
    assert_eq!(supervisor.uncertainties().len(), 1);
    assert_eq!(
        supervisor.uncertainties()[0].scope(),
        ReconciliationScopeKind::SourceAudit
    );
    assert!(
        supervisor.uncertainties()[0]
            .reasons()
            .contains(&expected_reason)
    );

    match admission.outcome() {
        AdmissionOutcome::Accepted(ticket) => {
            assert!(
                expect_dispatch,
                "expected the invalid replay to be rejected"
            );
            let dispatched = supervisor
                .dispatch_next()
                .expect("accepted replay dispatch");
            assert_eq!(dispatched.ticket(), *ticket);
            assert_eq!(dispatched.normalized().proof(), &Proof::Unproven);
            assert_eq!(
                dispatched.normalized().envelope().observations(),
                expected_observations.as_slice()
            );
        }
        AdmissionOutcome::Rejected(_) => {
            assert!(!expect_dispatch, "expected the invalid replay to dispatch");
        }
        outcome => panic!("unexpected invalid replay outcome: {outcome:?}"),
    }
}

#[test]
fn live_adapter_is_unproven_ordered_and_retains_one_audit_marker() {
    let source = SourceId::from_string("source-a");
    let root = RootIdentity::from_bytes(b"root-a".to_vec());
    let mut supervisor = ReconciliationAdmissionSupervisor::new(adapter_admission_limits(2, 8));
    let (_lane, generation) = adapter_registered(&mut supervisor, &source, &root);
    let observations = vec![
        adapter_observation(RawEventKind::Create, "first.wav"),
        adapter_observation(RawEventKind::Modify, "second.wav"),
    ];
    let expected_observations = observations.clone();
    let batch = adapter_batch(
        adapter_capture_provenance(
            "source-a",
            Some(b"root-a".to_vec()),
            Some(b"stream-a".to_vec()),
            generation.get(),
            Some(10),
            Some(11),
        ),
        observations,
    );

    let admission = {
        let mut adapter = ReconciliationAdapter::new(&mut supervisor);
        adapter.admit_live(batch).expect("live admission")
    };
    assert_eq!(
        admission.disposition(),
        AdapterDisposition::AdmittedUnproven
    );
    let ticket = match admission.outcome() {
        AdmissionOutcome::Accepted(ticket) => *ticket,
        outcome => panic!("unexpected live outcome: {outcome:?}"),
    };
    assert_eq!(supervisor.uncertainties().len(), 1);
    assert_eq!(
        supervisor.uncertainties()[0].scope(),
        ReconciliationScopeKind::SourceAudit
    );
    assert_eq!(
        supervisor.uncertainties()[0].reasons(),
        &[UncertaintyReason::LiveUnproven]
    );

    let dispatched = supervisor.dispatch_next().expect("live dispatch");
    assert_eq!(dispatched.ticket(), ticket);
    assert_eq!(dispatched.normalized().proof(), &Proof::Unproven);
    assert_eq!(
        dispatched.normalized().envelope().observations(),
        expected_observations.as_slice()
    );
}

#[test]
fn live_adapter_correlation_binds_ticket_identity_and_new_marker_boundary() {
    let source = SourceId::from_string("source-a");
    let root = RootIdentity::from_bytes(b"root-a".to_vec());
    let mut supervisor = ReconciliationAdmissionSupervisor::new(adapter_admission_limits(2, 8));
    let (_lane, generation) = adapter_registered(&mut supervisor, &source, &root);

    {
        let mut adapter = ReconciliationAdapter::new(&mut supervisor);
        adapter
            .admit_live(adapter_batch(
                adapter_capture_provenance(
                    "source-a",
                    Some(b"root-a".to_vec()),
                    Some(b"stream-a".to_vec()),
                    generation.get(),
                    Some(10),
                    Some(10),
                ),
                vec![adapter_observation(RawEventKind::Create, "first.wav")],
            ))
            .expect("first live admission");
    }

    let markers_before = supervisor.uncertainties().len();
    let provenance = adapter_capture_provenance(
        "source-a",
        Some(b"root-a".to_vec()),
        Some(b"stream-a".to_vec()),
        generation.get(),
        Some(11),
        Some(11),
    );
    let admission = {
        let mut adapter = ReconciliationAdapter::new(&mut supervisor);
        adapter
            .admit_live_with_correlation(adapter_batch(
                provenance.clone(),
                vec![adapter_observation(RawEventKind::Modify, "second.wav")],
            ))
            .expect("correlated live admission")
    };

    let ticket = match admission.admission().outcome() {
        AdmissionOutcome::Accepted(ticket) => *ticket,
        outcome => panic!("unexpected live outcome: {outcome:?}"),
    };
    let correlation = admission.correlation().expect("live correlation");
    assert_eq!(correlation.ticket(), ticket);
    assert_eq!(correlation.identity().source_id(), provenance.source_id());
    assert_eq!(
        correlation.identity().root_identity(),
        provenance.root_identity().expect("root identity")
    );
    assert_eq!(
        correlation.identity().generation(),
        provenance.watcher_generation()
    );
    let marker = &supervisor.uncertainties()[markers_before];
    assert_eq!(marker.scope(), ReconciliationScopeKind::SourceAudit);
    assert!(marker.reasons().contains(&UncertaintyReason::LiveUnproven));
    assert_eq!(correlation.boundary(), marker.boundary());
}

#[test]
fn live_adapter_correlation_is_none_for_duplicate_admission() {
    let source = SourceId::from_string("source-a");
    let root = RootIdentity::from_bytes(b"root-a".to_vec());
    let mut supervisor = ReconciliationAdmissionSupervisor::new(adapter_admission_limits(2, 8));
    let (_lane, generation) = adapter_registered(&mut supervisor, &source, &root);
    let batch = adapter_batch(
        adapter_capture_provenance(
            "source-a",
            Some(b"root-a".to_vec()),
            Some(b"stream-a".to_vec()),
            generation.get(),
            Some(10),
            Some(10),
        ),
        vec![adapter_observation(RawEventKind::Create, "sample.wav")],
    );

    let (first, duplicate) = {
        let mut adapter = ReconciliationAdapter::new(&mut supervisor);
        (
            adapter
                .admit_live_with_correlation(batch.clone())
                .expect("first live admission"),
            adapter
                .admit_live_with_correlation(batch)
                .expect("duplicate live admission"),
        )
    };
    assert!(first.correlation().is_some());
    assert!(duplicate.correlation().is_none());
    assert!(matches!(
        duplicate.admission().outcome(),
        AdmissionOutcome::DuplicateSuppressed(_)
    ));
}

#[test]
fn live_audit_handoff_retires_applied_ticket_without_authority_or_marker_loss() {
    let source = SourceId::from_string("source-a");
    let root = RootIdentity::from_bytes(b"root-a".to_vec());
    let mut supervisor = ReconciliationAdmissionSupervisor::new(adapter_admission_limits(1, 8));
    let (_lane, generation) = adapter_registered(&mut supervisor, &source, &root);
    let batch = adapter_batch(
        adapter_capture_provenance(
            "source-a",
            Some(b"root-a".to_vec()),
            Some(b"stream-a".to_vec()),
            generation.get(),
            Some(10),
            Some(10),
        ),
        vec![adapter_observation(RawEventKind::Create, "sample.wav")],
    );

    let admission = {
        let mut adapter = ReconciliationAdapter::new(&mut supervisor);
        adapter
            .admit_live_with_correlation(batch.clone())
            .expect("live admission")
    };
    let ticket = match admission.admission().outcome() {
        AdmissionOutcome::Accepted(ticket) => *ticket,
        outcome => panic!("unexpected live outcome: {outcome:?}"),
    };
    let correlation = admission.correlation().expect("live audit correlation");
    let marker_before = supervisor.uncertainties()[0].clone();
    assert_eq!(marker_before.source_id(), Some(&source));
    assert_eq!(marker_before.root_identity(), Some(&root));
    assert_eq!(marker_before.generation(), Some(generation));
    assert_eq!(marker_before.scope(), ReconciliationScopeKind::SourceAudit);
    assert_eq!(marker_before.reasons(), &[UncertaintyReason::LiveUnproven]);
    assert_eq!(correlation.identity().source_id(), &source);
    assert_eq!(correlation.identity().root_identity(), &root);
    assert_eq!(correlation.identity().generation(), generation);
    assert_eq!(correlation.boundary(), marker_before.boundary());
    assert_eq!(supervisor.in_flight(), 1);

    let duplicate = {
        let mut adapter = ReconciliationAdapter::new(&mut supervisor);
        adapter
            .admit_live_with_correlation(batch)
            .expect("duplicate live admission")
    };
    assert!(duplicate.correlation().is_none());
    assert_eq!(
        duplicate.admission().outcome(),
        &AdmissionOutcome::DuplicateSuppressed(CaptureSequenceRange::new(10, 10))
    );
    assert_eq!(supervisor.in_flight(), 1);
    assert_eq!(supervisor.uncertainties(), &[marker_before.clone()]);

    assert_eq!(
        supervisor.mark_unproven_audit_handed_off(ticket),
        Err(AdmissionError::InvalidLifecycleTransition)
    );
    assert_eq!(supervisor.in_flight(), 1);
    assert_eq!(supervisor.uncertainties(), &[marker_before.clone()]);

    let dispatched = supervisor.dispatch_next().expect("live dispatch");
    assert_eq!(dispatched.ticket(), ticket);
    assert_eq!(
        supervisor.mark_unproven_audit_handed_off(ticket),
        Err(AdmissionError::InvalidLifecycleTransition)
    );
    assert_eq!(supervisor.in_flight(), 1);
    supervisor
        .mark_dispatched(ticket)
        .expect("dispatch handoff");
    supervisor.mark_applied(ticket).expect("worker completion");

    supervisor
        .mark_unproven_audit_handed_off(ticket)
        .expect("source-audit scheduler handoff");
    assert_eq!(supervisor.in_flight(), 0);
    assert_eq!(supervisor.uncertainties(), &[marker_before.clone()]);

    assert_eq!(
        supervisor.mark_unproven_audit_handed_off(ticket),
        Err(AdmissionError::UnknownTicket)
    );
    assert_eq!(supervisor.in_flight(), 0);
    assert_eq!(supervisor.uncertainties(), &[marker_before]);

    let next = {
        let mut adapter = ReconciliationAdapter::new(&mut supervisor);
        adapter
            .admit_live(adapter_batch(
                adapter_capture_provenance(
                    "source-a",
                    Some(b"root-a".to_vec()),
                    Some(b"stream-a".to_vec()),
                    generation.get(),
                    Some(11),
                    Some(11),
                ),
                vec![adapter_observation(RawEventKind::Modify, "next.wav")],
            ))
            .expect("capacity released after audit handoff")
    };
    assert!(matches!(next.outcome(), AdmissionOutcome::Accepted(_)));
    assert_eq!(supervisor.in_flight(), 1);
}

#[test]
fn live_adapter_correlation_is_none_for_rejection_and_capacity() {
    let rejected_batch = adapter_batch(
        adapter_capture_provenance(
            "unregistered-source",
            Some(b"root-a".to_vec()),
            Some(b"stream-a".to_vec()),
            1,
            Some(1),
            Some(1),
        ),
        vec![adapter_observation(RawEventKind::Create, "rejected.wav")],
    );
    let mut rejected_supervisor =
        ReconciliationAdmissionSupervisor::new(adapter_admission_limits(2, 8));
    let rejected = {
        let mut adapter = ReconciliationAdapter::new(&mut rejected_supervisor);
        adapter
            .admit_live_with_correlation(rejected_batch)
            .expect("rejected live admission result")
    };
    assert!(rejected.correlation().is_none());
    assert!(matches!(
        rejected.admission().outcome(),
        AdmissionOutcome::Rejected(_)
    ));

    let source = SourceId::from_string("source-a");
    let root = RootIdentity::from_bytes(b"root-a".to_vec());
    let mut capacity_supervisor =
        ReconciliationAdmissionSupervisor::new(adapter_admission_limits(2, 1));
    let (_lane, generation) = adapter_registered(&mut capacity_supervisor, &source, &root);
    {
        let mut adapter = ReconciliationAdapter::new(&mut capacity_supervisor);
        adapter
            .admit_live(adapter_batch(
                adapter_capture_provenance(
                    "source-a",
                    Some(b"root-a".to_vec()),
                    Some(b"stream-a".to_vec()),
                    generation.get(),
                    Some(1),
                    Some(1),
                ),
                vec![adapter_observation(RawEventKind::Create, "first.wav")],
            ))
            .expect("first live admission");
    }
    let capacity = {
        let mut adapter = ReconciliationAdapter::new(&mut capacity_supervisor);
        adapter
            .admit_live_with_correlation(adapter_batch(
                adapter_capture_provenance(
                    "source-a",
                    Some(b"root-a".to_vec()),
                    Some(b"stream-a".to_vec()),
                    generation.get(),
                    Some(2),
                    Some(2),
                ),
                vec![adapter_observation(RawEventKind::Create, "second.wav")],
            ))
            .expect("capacity live admission result")
    };
    assert!(capacity.correlation().is_none());
    assert!(matches!(
        capacity.admission().outcome(),
        AdmissionOutcome::UncertaintyCapacityExhausted(_)
    ));
}

#[test]
fn valid_replay_adapter_attaches_exact_continuity_fields_without_adapter_uncertainty() {
    let source = SourceId::from_string("source-a");
    let root = RootIdentity::from_bytes(b"root-a".to_vec());
    let stream = BackendStreamIdentity::from_bytes(b"stream-a".to_vec());
    let mut supervisor = ReconciliationAdmissionSupervisor::new(adapter_admission_limits(2, 8));
    let (_lane, generation) = adapter_registered(&mut supervisor, &source, &root);
    let prior = ReplayPriorToken::new(source.clone(), root.clone(), stream.clone(), generation, 10);
    let observations = vec![
        adapter_observation(RawEventKind::Create, "first.wav"),
        adapter_observation(RawEventKind::Delete, "second.wav"),
    ];
    let expected_observations = observations.clone();
    let admission = {
        let mut adapter = ReconciliationAdapter::new(&mut supervisor);
        adapter
            .admit_replay(
                adapter_batch(
                    adapter_capture_provenance(
                        "source-a",
                        Some(b"root-a".to_vec()),
                        Some(b"stream-a".to_vec()),
                        generation.get(),
                        Some(11),
                        Some(12),
                    ),
                    observations,
                ),
                Some(&prior),
                true,
            )
            .expect("valid replay admission")
    };
    assert_eq!(
        admission.disposition(),
        AdapterDisposition::AdmittedWithContinuity
    );
    let ticket = match admission.outcome() {
        AdmissionOutcome::Accepted(ticket) => *ticket,
        outcome => panic!("unexpected replay outcome: {outcome:?}"),
    };
    assert!(supervisor.uncertainties().is_empty());

    let dispatched = supervisor.dispatch_next().expect("replay dispatch");
    assert_eq!(dispatched.ticket(), ticket);
    let proof = dispatched
        .normalized()
        .proof()
        .watcher_continuity()
        .expect("continuity proof");
    assert_eq!(proof.source_id(), &source);
    assert_eq!(proof.root_identity(), &root);
    assert_eq!(proof.backend_stream_identity(), &stream);
    assert_eq!(proof.watcher_generation(), generation);
    assert_eq!(proof.prior_acknowledgement().sequence(), 10);
    assert_eq!(proof.replay_coverage().after_sequence(), 10);
    assert_eq!(proof.replay_coverage().through_sequence(), 12);
    assert!(proof.replay_coverage().is_contiguous());
    assert_eq!(
        dispatched.normalized().envelope().observations(),
        expected_observations.as_slice()
    );
}

#[test]
fn invalid_replay_claims_remain_unproven_and_retain_typed_source_audit_evidence() {
    let valid_prior = replay_prior("source-a", b"root-a", b"stream-a", 1, 10);
    let valid_observations = || vec![adapter_observation(RawEventKind::Create, "sample.wav")];

    assert_invalid_replay_case(
        adapter_batch(
            adapter_capture_provenance(
                "source-a",
                Some(b"root-a".to_vec()),
                Some(b"stream-a".to_vec()),
                1,
                Some(11),
                Some(12),
            ),
            valid_observations(),
        ),
        None,
        true,
        UncertaintyReason::ReplayContinuityMissing,
        true,
    );
    assert_invalid_replay_case(
        adapter_batch(
            adapter_capture_provenance(
                "source-a",
                None,
                Some(b"stream-a".to_vec()),
                1,
                Some(11),
                Some(12),
            ),
            valid_observations(),
        ),
        Some(&valid_prior),
        true,
        UncertaintyReason::ReplayContinuityMissing,
        false,
    );
    assert_invalid_replay_case(
        adapter_batch(
            adapter_capture_provenance(
                "source-a",
                Some(b"root-a".to_vec()),
                None,
                1,
                Some(11),
                Some(12),
            ),
            valid_observations(),
        ),
        Some(&valid_prior),
        true,
        UncertaintyReason::ReplayContinuityMissing,
        true,
    );
    for (first_sequence, last_sequence) in [(None, None), (Some(11), None), (None, Some(12))] {
        let expected_reason = if first_sequence.is_none() && last_sequence.is_none() {
            UncertaintyReason::ReplayContinuityMissing
        } else {
            UncertaintyReason::ReplayContinuityAmbiguous
        };
        assert_invalid_replay_case(
            adapter_batch(
                adapter_capture_provenance(
                    "source-a",
                    Some(b"root-a".to_vec()),
                    Some(b"stream-a".to_vec()),
                    1,
                    first_sequence,
                    last_sequence,
                ),
                valid_observations(),
            ),
            Some(&valid_prior),
            true,
            expected_reason,
            true,
        );
    }
    for (first_sequence, last_sequence, acknowledged_sequence, contiguous) in [
        (Some(12), Some(13), 10, true),
        (Some(11), Some(11), 11, true),
        (Some(11), Some(12), 10, false),
    ] {
        let prior = replay_prior("source-a", b"root-a", b"stream-a", 1, acknowledged_sequence);
        assert_invalid_replay_case(
            adapter_batch(
                adapter_capture_provenance(
                    "source-a",
                    Some(b"root-a".to_vec()),
                    Some(b"stream-a".to_vec()),
                    1,
                    first_sequence,
                    last_sequence,
                ),
                valid_observations(),
            ),
            Some(&prior),
            contiguous,
            UncertaintyReason::ReplayContinuityGap,
            true,
        );
    }

    let mismatch_cases = [
        (
            adapter_capture_provenance(
                "source-a",
                Some(b"root-a".to_vec()),
                Some(b"stream-a".to_vec()),
                1,
                Some(11),
                Some(12),
            ),
            replay_prior("source-b", b"root-a", b"stream-a", 1, 10),
            true,
            true,
        ),
        (
            adapter_capture_provenance(
                "source-a",
                Some(b"root-a".to_vec()),
                Some(b"stream-a".to_vec()),
                1,
                Some(11),
                Some(12),
            ),
            replay_prior("source-a", b"root-b", b"stream-a", 1, 10),
            true,
            true,
        ),
        (
            adapter_capture_provenance(
                "source-a",
                Some(b"root-a".to_vec()),
                Some(b"stream-a".to_vec()),
                1,
                Some(11),
                Some(12),
            ),
            replay_prior("source-a", b"root-a", b"stream-b", 1, 10),
            true,
            true,
        ),
        (
            adapter_capture_provenance(
                "source-a",
                Some(b"root-a".to_vec()),
                Some(b"stream-a".to_vec()),
                1,
                Some(11),
                Some(12),
            ),
            replay_prior("source-a", b"root-a", b"stream-a", 8, 10),
            true,
            true,
        ),
    ];
    for (provenance, prior, contiguous, _) in mismatch_cases {
        assert_invalid_replay_case(
            adapter_batch(provenance, valid_observations()),
            Some(&prior),
            contiguous,
            UncertaintyReason::ReplayContinuityMismatch,
            true,
        );
    }

    for kind in [
        RawEventKind::RootChanged,
        RawEventKind::Overflow,
        RawEventKind::Error,
        RawEventKind::Unsupported,
    ] {
        assert_invalid_replay_case(
            adapter_batch(
                adapter_capture_provenance(
                    "source-a",
                    Some(b"root-a".to_vec()),
                    Some(b"stream-a".to_vec()),
                    1,
                    Some(11),
                    Some(12),
                ),
                vec![RawObservation::new(kind, Vec::new())],
            ),
            Some(&valid_prior),
            true,
            UncertaintyReason::ReplayContinuityMismatch,
            !matches!(
                kind,
                RawEventKind::Overflow | RawEventKind::Error | RawEventKind::Unsupported
            ),
        );
    }
    assert_invalid_replay_case(
        adapter_batch(
            adapter_capture_provenance(
                "source-a",
                Some(b"root-a".to_vec()),
                Some(b"stream-a".to_vec()),
                1,
                Some(11),
                Some(12),
            ),
            vec![
                adapter_observation(RawEventKind::Create, "sample.wav")
                    .with_uncertainty(ObservationUncertainty::CONTINUITY),
            ],
        ),
        Some(&valid_prior),
        true,
        UncertaintyReason::ReplayContinuityMismatch,
        true,
    );
}

#[test]
fn adapter_preserves_exact_live_and_replay_duplicate_suppression() {
    let source = SourceId::from_string("source-a");
    let root = RootIdentity::from_bytes(b"root-a".to_vec());
    let provenance = adapter_capture_provenance(
        "source-a",
        Some(b"root-a".to_vec()),
        Some(b"stream-a".to_vec()),
        1,
        Some(10),
        Some(12),
    );
    let batch = adapter_batch(
        provenance.clone(),
        vec![adapter_observation(RawEventKind::Create, "sample.wav")],
    );
    let mut live_supervisor =
        ReconciliationAdmissionSupervisor::new(adapter_admission_limits(2, 8));
    adapter_registered(&mut live_supervisor, &source, &root);
    let (first_live, second_live) = {
        let mut adapter = ReconciliationAdapter::new(&mut live_supervisor);
        (
            adapter.admit_live(batch.clone()).expect("first live"),
            adapter.admit_live(batch).expect("duplicate live"),
        )
    };
    assert_eq!(
        first_live.disposition(),
        AdapterDisposition::AdmittedUnproven
    );
    assert_eq!(
        second_live.disposition(),
        AdapterDisposition::DuplicateSuppressed
    );
    assert!(matches!(
        second_live.outcome(),
        AdmissionOutcome::DuplicateSuppressed(CaptureSequenceRange { .. })
    ));
    assert_eq!(live_supervisor.in_flight(), 1);
    assert_eq!(live_supervisor.uncertainties().len(), 1);

    let prior = replay_prior("source-a", b"root-a", b"stream-a", 1, 9);
    let mut replay_supervisor =
        ReconciliationAdmissionSupervisor::new(adapter_admission_limits(2, 8));
    adapter_registered(&mut replay_supervisor, &source, &root);
    let replay_batch = adapter_batch(
        provenance,
        vec![adapter_observation(RawEventKind::Modify, "sample.wav")],
    );
    let (first_replay, second_replay) = {
        let mut adapter = ReconciliationAdapter::new(&mut replay_supervisor);
        (
            adapter
                .admit_replay(replay_batch.clone(), Some(&prior), true)
                .expect("first replay"),
            adapter
                .admit_replay(replay_batch, Some(&prior), true)
                .expect("duplicate replay"),
        )
    };
    assert_eq!(
        first_replay.disposition(),
        AdapterDisposition::AdmittedWithContinuity
    );
    assert_eq!(
        second_replay.disposition(),
        AdapterDisposition::DuplicateSuppressed
    );
    assert_eq!(replay_supervisor.in_flight(), 1);
    assert!(replay_supervisor.uncertainties().is_empty());
}

#[test]
fn adapter_marker_only_duplicates_remember_live_and_invalid_replay_evidence() {
    for kind in [
        RawEventKind::Overflow,
        RawEventKind::Error,
        RawEventKind::Unsupported,
    ] {
        let source = SourceId::from_string("source-a");
        let root = RootIdentity::from_bytes(b"root-a".to_vec());
        let mut supervisor = ReconciliationAdmissionSupervisor::new(adapter_admission_limits(2, 1));
        let (_lane, generation) = adapter_registered(&mut supervisor, &source, &root);
        let batch = adapter_batch(
            adapter_capture_provenance(
                "source-a",
                Some(b"root-a".to_vec()),
                Some(b"stream-a".to_vec()),
                generation.get(),
                Some(10),
                Some(10),
            ),
            vec![RawObservation::new(kind, Vec::new())],
        );
        let (first, second) = {
            let mut adapter = ReconciliationAdapter::new(&mut supervisor);
            (
                adapter
                    .admit_live(batch.clone())
                    .expect("first marker-only live"),
                adapter
                    .admit_live(batch)
                    .expect("duplicate marker-only live"),
            )
        };
        assert_eq!(first.disposition(), AdapterDisposition::SourceAuditRequired);
        assert!(matches!(first.outcome(), AdmissionOutcome::Rejected(_)));
        assert_eq!(
            second.disposition(),
            AdapterDisposition::DuplicateSuppressed
        );
        assert_eq!(
            second.outcome(),
            &AdmissionOutcome::DuplicateSuppressed(CaptureSequenceRange::new(10, 10))
        );
        assert_eq!(supervisor.in_flight(), 0);
        assert_eq!(supervisor.uncertainties().len(), 1);
    }

    for kind in [
        RawEventKind::Overflow,
        RawEventKind::Error,
        RawEventKind::Unsupported,
    ] {
        let source = SourceId::from_string("source-a");
        let root = RootIdentity::from_bytes(b"root-a".to_vec());
        let mut supervisor = ReconciliationAdmissionSupervisor::new(adapter_admission_limits(2, 1));
        let (_lane, generation) = adapter_registered(&mut supervisor, &source, &root);
        let prior = replay_prior("source-a", b"root-a", b"stream-a", generation.get(), 9);
        let batch = adapter_batch(
            adapter_capture_provenance(
                "source-a",
                Some(b"root-a".to_vec()),
                Some(b"stream-a".to_vec()),
                generation.get(),
                Some(10),
                Some(10),
            ),
            vec![RawObservation::new(kind, Vec::new())],
        );
        let (first, second) = {
            let mut adapter = ReconciliationAdapter::new(&mut supervisor);
            (
                adapter
                    .admit_replay(batch.clone(), Some(&prior), true)
                    .expect("first marker-only replay"),
                adapter
                    .admit_replay(batch, Some(&prior), true)
                    .expect("duplicate marker-only replay"),
            )
        };
        assert_eq!(first.disposition(), AdapterDisposition::SourceAuditRequired);
        assert!(matches!(first.outcome(), AdmissionOutcome::Rejected(_)));
        assert_eq!(
            second.disposition(),
            AdapterDisposition::DuplicateSuppressed
        );
        assert_eq!(
            second.outcome(),
            &AdmissionOutcome::DuplicateSuppressed(CaptureSequenceRange::new(10, 10))
        );
        assert_eq!(supervisor.in_flight(), 0);
        assert_eq!(supervisor.uncertainties().len(), 1);
        assert!(
            supervisor.uncertainties()[0]
                .reasons()
                .contains(&UncertaintyReason::ReplayContinuityMismatch)
        );
    }
}

#[test]
fn adapter_marker_only_same_sequence_conflict_retains_audit_and_suppresses_exact_repeat() {
    let source = SourceId::from_string("source-a");
    let root = RootIdentity::from_bytes(b"root-a".to_vec());
    let provenance = adapter_capture_provenance(
        "source-a",
        Some(b"root-a".to_vec()),
        Some(b"stream-a".to_vec()),
        1,
        Some(10),
        Some(10),
    );
    let first_batch = adapter_batch(
        provenance.clone(),
        vec![RawObservation::new(RawEventKind::Overflow, Vec::new())],
    );
    let conflict_batch = adapter_batch(
        provenance,
        vec![RawObservation::new(RawEventKind::Error, Vec::new())],
    );
    let mut supervisor = ReconciliationAdmissionSupervisor::new(adapter_admission_limits(2, 2));
    adapter_registered(&mut supervisor, &source, &root);
    let (first, conflict, duplicate) = {
        let mut adapter = ReconciliationAdapter::new(&mut supervisor);
        (
            adapter
                .admit_live(first_batch.clone())
                .expect("first marker-only evidence"),
            adapter
                .admit_live(conflict_batch)
                .expect("conflicting marker-only evidence"),
            adapter
                .admit_live(first_batch)
                .expect("exact marker-only repeat"),
        )
    };
    assert!(matches!(first.outcome(), AdmissionOutcome::Rejected(_)));
    assert!(matches!(conflict.outcome(), AdmissionOutcome::Rejected(_)));
    assert!(
        supervisor.uncertainties()[1]
            .reasons()
            .contains(&UncertaintyReason::SequenceConflict)
    );
    assert_eq!(
        duplicate.disposition(),
        AdapterDisposition::DuplicateSuppressed
    );
    assert_eq!(supervisor.in_flight(), 0);
    assert_eq!(supervisor.uncertainties().len(), 2);
}

#[test]
fn adjacent_valid_replays_preserve_fifo_and_raw_order() {
    let source = SourceId::from_string("source-a");
    let root = RootIdentity::from_bytes(b"root-a".to_vec());
    let mut supervisor = ReconciliationAdmissionSupervisor::new(adapter_admission_limits(4, 8));
    adapter_registered(&mut supervisor, &source, &root);
    let first_prior = replay_prior("source-a", b"root-a", b"stream-a", 1, 10);
    let second_prior = replay_prior("source-a", b"root-a", b"stream-a", 1, 12);
    let first_observations = vec![
        adapter_observation(RawEventKind::Create, "first-a.wav"),
        adapter_observation(RawEventKind::Modify, "first-b.wav"),
    ];
    let second_observations = vec![
        adapter_observation(RawEventKind::Delete, "second-a.wav"),
        adapter_observation(RawEventKind::Rename, "second-b.wav"),
    ];
    let first_admission = {
        let mut adapter = ReconciliationAdapter::new(&mut supervisor);
        adapter
            .admit_replay(
                adapter_batch(
                    adapter_capture_provenance(
                        "source-a",
                        Some(b"root-a".to_vec()),
                        Some(b"stream-a".to_vec()),
                        1,
                        Some(11),
                        Some(12),
                    ),
                    first_observations.clone(),
                ),
                Some(&first_prior),
                true,
            )
            .expect("first adjacent replay")
    };
    let second_admission = {
        let mut adapter = ReconciliationAdapter::new(&mut supervisor);
        adapter
            .admit_replay(
                adapter_batch(
                    adapter_capture_provenance(
                        "source-a",
                        Some(b"root-a".to_vec()),
                        Some(b"stream-a".to_vec()),
                        1,
                        Some(13),
                        Some(14),
                    ),
                    second_observations.clone(),
                ),
                Some(&second_prior),
                true,
            )
            .expect("second adjacent replay")
    };
    let first_ticket = match first_admission.outcome() {
        AdmissionOutcome::Accepted(ticket) => *ticket,
        outcome => panic!("unexpected first adjacent outcome: {outcome:?}"),
    };
    let second_ticket = match second_admission.outcome() {
        AdmissionOutcome::Accepted(ticket) => *ticket,
        outcome => panic!("unexpected second adjacent outcome: {outcome:?}"),
    };
    let first_dispatch = supervisor.dispatch_next().expect("first FIFO dispatch");
    let second_dispatch = supervisor.dispatch_next().expect("second FIFO dispatch");
    assert_eq!(first_dispatch.ticket(), first_ticket);
    assert_eq!(second_dispatch.ticket(), second_ticket);
    assert_eq!(
        first_dispatch.normalized().envelope().observations(),
        first_observations.as_slice()
    );
    assert_eq!(
        second_dispatch.normalized().envelope().observations(),
        second_observations.as_slice()
    );
    assert_eq!(
        first_dispatch
            .normalized()
            .proof()
            .watcher_continuity()
            .expect("first proof")
            .replay_coverage()
            .through_sequence(),
        12
    );
    assert_eq!(
        second_dispatch
            .normalized()
            .proof()
            .watcher_continuity()
            .expect("second proof")
            .replay_coverage()
            .through_sequence(),
        14
    );
}

#[test]
fn adapter_error_preserves_original_batch_and_capacity_is_atomic() {
    let invalid_batch = adapter_batch(
        adapter_capture_provenance(
            "source-a",
            Some(b"root-a".to_vec()),
            Some(b"stream-a".to_vec()),
            1,
            None,
            None,
        ),
        Vec::new(),
    );
    let expected_batch = invalid_batch.clone();
    let mut empty_supervisor =
        ReconciliationAdmissionSupervisor::new(adapter_admission_limits(2, 8));
    let error = {
        let mut adapter = ReconciliationAdapter::new(&mut empty_supervisor);
        adapter
            .admit_live(invalid_batch)
            .expect_err("empty batch error")
    };
    assert_eq!(error.error(), &RawEnvelopeError::EmptyEnvelope);
    assert_eq!(error.batch(), &expected_batch);
    assert_eq!(error.into_batch(), expected_batch);

    let source = SourceId::from_string("source-a");
    let root = RootIdentity::from_bytes(b"root-a".to_vec());
    let mut supervisor = ReconciliationAdmissionSupervisor::new(adapter_admission_limits(2, 1));
    adapter_registered(&mut supervisor, &source, &root);
    let first = {
        let mut adapter = ReconciliationAdapter::new(&mut supervisor);
        adapter
            .admit_live(adapter_batch(
                adapter_capture_provenance(
                    "source-a",
                    Some(b"root-a".to_vec()),
                    Some(b"stream-a".to_vec()),
                    1,
                    Some(1),
                    Some(1),
                ),
                vec![adapter_observation(RawEventKind::Create, "first.wav")],
            ))
            .expect("first live admission")
    };
    assert_eq!(first.disposition(), AdapterDisposition::AdmittedUnproven);
    let second_batch = adapter_batch(
        adapter_capture_provenance(
            "source-a",
            Some(b"root-a".to_vec()),
            Some(b"stream-a".to_vec()),
            1,
            Some(2),
            Some(2),
        ),
        vec![adapter_observation(RawEventKind::Create, "second.wav")],
    );
    let expected_second = second_batch.clone();
    let second = {
        let mut adapter = ReconciliationAdapter::new(&mut supervisor);
        adapter.admit_live(second_batch).expect("capacity result")
    };
    assert_eq!(
        second.disposition(),
        AdapterDisposition::UncertaintyCapacityExhausted
    );
    match second.outcome() {
        AdmissionOutcome::UncertaintyCapacityExhausted(envelope) => {
            assert_eq!(envelope.proof(), &Proof::Unproven);
            assert_eq!(envelope.observations(), expected_second.observations());
        }
        outcome => panic!("unexpected capacity outcome: {outcome:?}"),
    }
    assert_eq!(supervisor.in_flight(), 1);
    assert_eq!(supervisor.uncertainties().len(), 1);
}

#[test]
fn adapter_remains_fenced_by_cancellation_restart_and_rebind() {
    let source = SourceId::from_string("source-a");
    let old_root = RootIdentity::from_bytes(b"root-a".to_vec());
    let new_root = RootIdentity::from_bytes(b"root-b".to_vec());
    let mut supervisor = ReconciliationAdmissionSupervisor::new(adapter_admission_limits(4, 8));
    let (lane, generation) = adapter_registered(&mut supervisor, &source, &old_root);

    let first = {
        let mut adapter = ReconciliationAdapter::new(&mut supervisor);
        adapter
            .admit_live(adapter_batch(
                adapter_capture_provenance(
                    "source-a",
                    Some(b"root-a".to_vec()),
                    Some(b"stream-a".to_vec()),
                    generation.get(),
                    Some(1),
                    Some(1),
                ),
                vec![adapter_observation(RawEventKind::Create, "first.wav")],
            ))
            .expect("first live admission")
    };
    assert_eq!(first.disposition(), AdapterDisposition::AdmittedUnproven);
    supervisor.stop_lane(&lane, generation).expect("stop lane");
    let restarted_generation = supervisor.restart_lane(&lane).expect("restart lane");
    supervisor
        .begin_capture(&lane, restarted_generation)
        .expect("begin restarted capture");

    let stale = {
        let mut adapter = ReconciliationAdapter::new(&mut supervisor);
        adapter
            .admit_live(adapter_batch(
                adapter_capture_provenance(
                    "source-a",
                    Some(b"root-a".to_vec()),
                    Some(b"stream-a".to_vec()),
                    generation.get(),
                    Some(2),
                    Some(2),
                ),
                vec![adapter_observation(RawEventKind::Modify, "stale.wav")],
            ))
            .expect("stale live admission result")
    };
    assert_eq!(stale.disposition(), AdapterDisposition::SourceAuditRequired);
    assert_eq!(
        stale.outcome(),
        &AdmissionOutcome::Rejected(AdmissionRejectReason::StaleGeneration)
    );

    let (new_lane, new_generation) = supervisor
        .rebind_lane(&lane, restarted_generation, new_root.clone())
        .expect("rebind lane");
    supervisor
        .begin_capture(&new_lane, new_generation)
        .expect("begin rebound capture");
    let rebound = {
        let mut adapter = ReconciliationAdapter::new(&mut supervisor);
        adapter
            .admit_live(adapter_batch(
                adapter_capture_provenance(
                    "source-a",
                    Some(b"root-b".to_vec()),
                    Some(b"stream-b".to_vec()),
                    new_generation.get(),
                    Some(3),
                    Some(3),
                ),
                vec![adapter_observation(RawEventKind::Create, "rebound.wav")],
            ))
            .expect("rebound live admission")
    };
    assert_eq!(rebound.disposition(), AdapterDisposition::AdmittedUnproven);
    assert!(matches!(rebound.outcome(), AdmissionOutcome::Accepted(_)));
}
