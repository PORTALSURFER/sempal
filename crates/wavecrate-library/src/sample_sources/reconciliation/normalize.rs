use std::path::Path;

use super::model::{
    ObservationUncertainty, Proof, RawEventKind, RawObservation, RawObservationEnvelope,
    RawPathHint, RawPathRole, RootRelativePath, RootRelativePathError,
};

/// Monotonic work-scope kind emitted by the pure normalizer.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ReconciliationScopeKind {
    /// Inspect one named entry without claiming its current type or existence.
    ExactEntry,
    /// Inspect one named directory and its descendants.
    Subtree,
    /// Audit the complete source/root boundary.
    SourceAudit,
}

impl ReconciliationScopeKind {
    /// Return the more conservative of two scope kinds.
    pub const fn widen(self, other: Self) -> Self {
        if self as u8 >= other as u8 {
            self
        } else {
            other
        }
    }
}

/// Reason attached to a normalized scope.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NormalizationReason {
    /// A valid path was retained as an entry-level inspection scope.
    ExactObservation,
    /// A directory hint widened a create or modify path to a subtree.
    DirectoryHint,
    /// An empty-folder hint widened the directory namespace to a subtree.
    EmptyFolderHint,
    /// A valid delete path remained an entry-level inspection scope.
    DeleteObservation,
    /// Delete or incomplete evidence widened to the nearest valid parent.
    MissingParent,
    /// Delete evidence was absent, directory-shaped, or otherwise uncertain.
    UncertainDelete,
    /// One endpoint of a structurally complete rename was retained in order.
    RenameEndpoint,
    /// Rename metadata or endpoint classification was incomplete or uncertain.
    IncompleteRename,
    /// One endpoint of a structurally complete copy was retained in order.
    CopyEndpoint,
    /// Copy metadata or endpoint classification was incomplete or uncertain.
    IncompleteCopy,
    /// The observation explicitly concerned the source root.
    RootChanged,
    /// The backend reported overflow.
    Overflow,
    /// The backend reported an error.
    BackendError,
    /// The backend delivered an unsupported event shape.
    Unsupported,
    /// The raw path could not be validated as a relative entry path.
    InvalidPath {
        /// Native validation failure retained as a diagnostic reason.
        error: RootRelativePathError,
    },
    /// A required path was not supplied for a path-bearing event.
    MissingPath,
    /// Explicit raw uncertainty widened the normalized result.
    ExplicitUncertainty {
        /// Bits carried by the raw observation.
        uncertainty: ObservationUncertainty,
    },
}

/// One ordered normalized inspection scope.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReconciliationScope {
    kind: ReconciliationScopeKind,
    path: Option<RootRelativePath>,
    role: Option<RawPathRole>,
    reason: NormalizationReason,
}

impl ReconciliationScope {
    fn exact(path: RootRelativePath, role: RawPathRole, reason: NormalizationReason) -> Self {
        Self {
            kind: ReconciliationScopeKind::ExactEntry,
            path: Some(path),
            role: Some(role),
            reason,
        }
    }

    fn subtree(path: RootRelativePath, role: RawPathRole, reason: NormalizationReason) -> Self {
        Self {
            kind: ReconciliationScopeKind::Subtree,
            path: Some(path),
            role: Some(role),
            reason,
        }
    }

    fn source_audit(reason: NormalizationReason) -> Self {
        Self {
            kind: ReconciliationScopeKind::SourceAudit,
            path: None,
            role: None,
            reason,
        }
    }

    /// Return the monotonic scope kind.
    pub const fn kind(&self) -> ReconciliationScopeKind {
        self.kind
    }

    /// Borrow the scope path when this is an exact-entry or subtree scope.
    pub fn path(&self) -> Option<&RootRelativePath> {
        self.path.as_ref()
    }

    /// Return the raw role retained for a path scope.
    pub const fn role(&self) -> Option<RawPathRole> {
        self.role
    }

    /// Return the syntactic normalization reason.
    pub const fn reason(&self) -> NormalizationReason {
        self.reason
    }
}

/// Raw evidence retained alongside its ordered normalized scopes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NormalizedObservation {
    envelope: RawObservationEnvelope,
    scopes: Vec<ReconciliationScope>,
}

impl NormalizedObservation {
    /// Borrow the unchanged raw envelope.
    pub const fn envelope(&self) -> &RawObservationEnvelope {
        &self.envelope
    }

    /// Borrow normalized scopes in raw observation and path order.
    pub fn scopes(&self) -> &[ReconciliationScope] {
        &self.scopes
    }

    /// Borrow the unchanged proof carried by the raw envelope.
    pub const fn proof(&self) -> &Proof {
        self.envelope.proof()
    }

    /// Consume the normalized value and return its unchanged raw envelope.
    pub fn into_envelope(self) -> RawObservationEnvelope {
        self.envelope
    }
}

/// Normalize one bounded raw envelope without I/O, sorting, deduplication, or proof promotion.
pub fn normalize_observation(envelope: RawObservationEnvelope) -> NormalizedObservation {
    let mut scopes = Vec::new();
    for observation in envelope.observations() {
        normalize_record(observation, &mut scopes);
    }
    NormalizedObservation { envelope, scopes }
}

fn normalize_record(observation: &RawObservation, scopes: &mut Vec<ReconciliationScope>) {
    match observation.kind() {
        RawEventKind::Create | RawEventKind::Modify => {
            normalize_create_or_modify(observation, scopes)
        }
        RawEventKind::Delete => normalize_delete(observation, scopes),
        RawEventKind::Rename => normalize_rename(observation, scopes),
        RawEventKind::Copy => normalize_copy(observation, scopes),
        RawEventKind::RootChanged => {
            scopes.push(ReconciliationScope::source_audit(
                NormalizationReason::RootChanged,
            ));
        }
        RawEventKind::Overflow => {
            scopes.push(ReconciliationScope::source_audit(
                NormalizationReason::Overflow,
            ));
        }
        RawEventKind::Error => {
            scopes.push(ReconciliationScope::source_audit(
                NormalizationReason::BackendError,
            ));
        }
        RawEventKind::Unsupported => {
            scopes.push(ReconciliationScope::source_audit(
                NormalizationReason::Unsupported,
            ));
        }
    }
}

fn normalize_create_or_modify(observation: &RawObservation, scopes: &mut Vec<ReconciliationScope>) {
    if observation.paths().is_empty() {
        scopes.push(ReconciliationScope::source_audit(
            NormalizationReason::MissingPath,
        ));
    }

    for observed_path in observation.paths() {
        let Some(path) = validated_path(observed_path.path(), observed_path.hint(), scopes) else {
            continue;
        };

        if observed_path.hint() == Some(RawPathHint::Root) {
            scopes.push(ReconciliationScope::source_audit(
                NormalizationReason::RootChanged,
            ));
            continue;
        }

        if observation
            .uncertainty()
            .contains(ObservationUncertainty::MISSING_PARENT)
        {
            push_parent_or_audit(
                &path,
                observed_path.role(),
                NormalizationReason::MissingParent,
                scopes,
            );
            continue;
        }

        match observed_path.hint() {
            Some(RawPathHint::Directory | RawPathHint::Unknown) => {
                scopes.push(ReconciliationScope::subtree(
                    path,
                    observed_path.role(),
                    NormalizationReason::DirectoryHint,
                ))
            }
            Some(RawPathHint::EmptyFolder) => scopes.push(ReconciliationScope::subtree(
                path,
                observed_path.role(),
                NormalizationReason::EmptyFolderHint,
            )),
            Some(RawPathHint::Absent) => push_parent_or_audit(
                &path,
                observed_path.role(),
                NormalizationReason::MissingParent,
                scopes,
            ),
            Some(RawPathHint::Symlink) | None => scopes.push(ReconciliationScope::exact(
                path,
                observed_path.role(),
                NormalizationReason::ExactObservation,
            )),
            Some(RawPathHint::Root) => unreachable!("root hint handled before classification"),
        }
    }

    push_broad_uncertainty(observation, scopes);
}

fn normalize_delete(observation: &RawObservation, scopes: &mut Vec<ReconciliationScope>) {
    if observation.paths().is_empty() {
        scopes.push(ReconciliationScope::source_audit(
            NormalizationReason::MissingPath,
        ));
    }

    for observed_path in observation.paths() {
        let Some(path) = validated_path(observed_path.path(), observed_path.hint(), scopes) else {
            continue;
        };

        if observed_path.hint() == Some(RawPathHint::Root) {
            scopes.push(ReconciliationScope::source_audit(
                NormalizationReason::RootChanged,
            ));
            continue;
        }

        let uncertain_parent = observation
            .uncertainty()
            .contains(ObservationUncertainty::MISSING_PARENT);
        let uncertain_hint = matches!(
            observed_path.hint(),
            Some(RawPathHint::Directory)
                | Some(RawPathHint::EmptyFolder)
                | Some(RawPathHint::Absent)
                | Some(RawPathHint::Unknown)
        );
        if uncertain_parent || uncertain_hint {
            push_parent_or_audit(
                &path,
                observed_path.role(),
                if uncertain_parent {
                    NormalizationReason::MissingParent
                } else {
                    NormalizationReason::UncertainDelete
                },
                scopes,
            );
        } else {
            scopes.push(ReconciliationScope::exact(
                path,
                observed_path.role(),
                NormalizationReason::DeleteObservation,
            ));
        }
    }

    push_broad_uncertainty(observation, scopes);
}

fn normalize_rename(observation: &RawObservation, scopes: &mut Vec<ReconciliationScope>) {
    let source_count = observation
        .paths()
        .iter()
        .filter(|path| path.role() == RawPathRole::RenameSource)
        .count();
    let destination_count = observation
        .paths()
        .iter()
        .filter(|path| path.role() == RawPathRole::RenameDestination)
        .count();
    let complete_shape =
        source_count == 1 && destination_count == 1 && observation.paths().len() == 2;

    if observation.paths().is_empty() {
        scopes.push(ReconciliationScope::source_audit(
            NormalizationReason::IncompleteRename,
        ));
    }

    for observed_path in observation.paths() {
        let Some(path) = validated_path(observed_path.path(), observed_path.hint(), scopes) else {
            continue;
        };

        if observed_path.hint() == Some(RawPathHint::Root) {
            scopes.push(ReconciliationScope::source_audit(
                NormalizationReason::RootChanged,
            ));
            continue;
        }

        let endpoint_uncertain = matches!(
            observed_path.hint(),
            Some(RawPathHint::Directory)
                | Some(RawPathHint::EmptyFolder)
                | Some(RawPathHint::Absent)
                | Some(RawPathHint::Unknown)
        ) || observation
            .uncertainty()
            .contains(ObservationUncertainty::MISSING_PARENT);

        if complete_shape && !endpoint_uncertain {
            scopes.push(ReconciliationScope::exact(
                path,
                observed_path.role(),
                NormalizationReason::RenameEndpoint,
            ));
        } else {
            push_parent_or_audit(
                &path,
                observed_path.role(),
                NormalizationReason::IncompleteRename,
                scopes,
            );
        }
    }

    if !complete_shape && observation.paths().is_empty() {
        return;
    }
    if !complete_shape {
        scopes.push(ReconciliationScope::source_audit(
            NormalizationReason::IncompleteRename,
        ));
    }
    push_broad_uncertainty(observation, scopes);
}

fn normalize_copy(observation: &RawObservation, scopes: &mut Vec<ReconciliationScope>) {
    let explicit_destination_count = observation
        .paths()
        .iter()
        .filter(|path| path.role() == RawPathRole::CopyDestination)
        .count();
    let source_count = observation
        .paths()
        .iter()
        .filter(|path| path.role() == RawPathRole::CopySource)
        .count();
    let generic_destination_count = if explicit_destination_count == 0 {
        observation
            .paths()
            .iter()
            .filter(|path| path.role() == RawPathRole::Subject)
            .count()
    } else {
        0
    };
    let destination_count = explicit_destination_count + generic_destination_count;
    let complete_shape = destination_count == 1
        && source_count <= 1
        && observation.paths().len() == destination_count + source_count;

    if observation.paths().is_empty() {
        scopes.push(ReconciliationScope::source_audit(
            NormalizationReason::IncompleteCopy,
        ));
    }

    for observed_path in observation.paths() {
        let Some(path) = validated_path(observed_path.path(), observed_path.hint(), scopes) else {
            continue;
        };

        if observed_path.hint() == Some(RawPathHint::Root) {
            scopes.push(ReconciliationScope::source_audit(
                NormalizationReason::RootChanged,
            ));
            continue;
        }

        let endpoint_uncertain = matches!(
            observed_path.hint(),
            Some(RawPathHint::Directory)
                | Some(RawPathHint::EmptyFolder)
                | Some(RawPathHint::Absent)
                | Some(RawPathHint::Unknown)
        ) || observation
            .uncertainty()
            .contains(ObservationUncertainty::MISSING_PARENT);

        if complete_shape && !endpoint_uncertain {
            scopes.push(ReconciliationScope::exact(
                path,
                observed_path.role(),
                NormalizationReason::CopyEndpoint,
            ));
        } else {
            push_parent_or_audit(
                &path,
                observed_path.role(),
                NormalizationReason::IncompleteCopy,
                scopes,
            );
        }
    }

    if !complete_shape && observation.paths().is_empty() {
        return;
    }
    if !complete_shape {
        scopes.push(ReconciliationScope::source_audit(
            NormalizationReason::IncompleteCopy,
        ));
    }
    push_broad_uncertainty(observation, scopes);
}

fn validated_path(
    path: &Path,
    hint: Option<RawPathHint>,
    scopes: &mut Vec<ReconciliationScope>,
) -> Option<RootRelativePath> {
    match RootRelativePath::try_from(path) {
        Ok(path) => Some(path),
        Err(error) => {
            scopes.push(ReconciliationScope::source_audit(
                NormalizationReason::InvalidPath { error },
            ));
            if hint == Some(RawPathHint::Root) {
                scopes.push(ReconciliationScope::source_audit(
                    NormalizationReason::RootChanged,
                ));
            }
            None
        }
    }
}

fn push_parent_or_audit(
    path: &RootRelativePath,
    role: RawPathRole,
    reason: NormalizationReason,
    scopes: &mut Vec<ReconciliationScope>,
) {
    if let Some(parent) = path.parent() {
        scopes.push(ReconciliationScope::subtree(parent, role, reason));
    } else {
        scopes.push(ReconciliationScope::source_audit(reason));
    }
}

fn push_broad_uncertainty(observation: &RawObservation, scopes: &mut Vec<ReconciliationScope>) {
    let uncertainty = observation.uncertainty();
    let known_bits = ObservationUncertainty::ORDERING.bits()
        | ObservationUncertainty::PATH_COVERAGE.bits()
        | ObservationUncertainty::CONTINUITY.bits()
        | ObservationUncertainty::MISSING_PARENT.bits()
        | ObservationUncertainty::OVERFLOW.bits()
        | ObservationUncertainty::BACKEND_ERROR.bits();
    let broad = uncertainty.bits() & !known_bits != 0
        || uncertainty.contains(ObservationUncertainty::ORDERING)
        || uncertainty.contains(ObservationUncertainty::PATH_COVERAGE)
        || uncertainty.contains(ObservationUncertainty::CONTINUITY)
        || uncertainty.contains(ObservationUncertainty::OVERFLOW)
        || uncertainty.contains(ObservationUncertainty::BACKEND_ERROR);
    if broad {
        scopes.push(ReconciliationScope::source_audit(
            NormalizationReason::ExplicitUncertainty { uncertainty },
        ));
    }
}
