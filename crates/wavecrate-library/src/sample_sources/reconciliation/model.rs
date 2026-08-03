use std::ffi::{OsStr, OsString};
use std::fmt;
use std::ops::{BitOr, BitOrAssign};
use std::path::{Component, Path, PathBuf};

use crate::sample_sources::SourceId;

/// Which envelope budget was exceeded.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RawEnvelopeLimit {
    /// The number of raw observations.
    EventCount,
    /// The sum of native path spelling bytes.
    PathBytes,
    /// The sum of encoded observation metadata bytes.
    MetadataBytes,
}

impl RawEnvelopeLimit {
    fn label(self) -> &'static str {
        match self {
            Self::EventCount => "event count",
            Self::PathBytes => "path bytes",
            Self::MetadataBytes => "metadata bytes",
        }
    }
}

/// Which checked accounting counter overflowed while building an envelope.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RawEnvelopeCounter {
    /// The event-count accumulator.
    EventCount,
    /// The native path-byte accumulator.
    PathBytes,
    /// The metadata-byte accumulator.
    MetadataBytes,
    /// A capture sequence boundary increment.
    Sequence,
}

impl RawEnvelopeCounter {
    fn label(self) -> &'static str {
        match self {
            Self::EventCount => "event count",
            Self::PathBytes => "path bytes",
            Self::MetadataBytes => "metadata bytes",
            Self::Sequence => "capture sequence",
        }
    }
}

/// Errors returned by checked reconciliation model constructors.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RawEnvelopeError {
    /// The envelope must retain at least one raw observation.
    EmptyEnvelope,
    /// An event limit of zero cannot admit a non-empty envelope.
    ZeroEventLimit,
    /// A bounded accounting counter exceeded its configured limit.
    LimitExceeded {
        /// Counter that exceeded its limit.
        limit: RawEnvelopeLimit,
        /// Exact observed value.
        actual: usize,
        /// Configured maximum.
        maximum: usize,
    },
    /// A checked accounting operation overflowed `usize` or a sequence increment overflowed `u64`.
    ArithmeticOverflow {
        /// Counter whose checked operation overflowed.
        counter: RawEnvelopeCounter,
    },
    /// Capture sequence boundaries were supplied in reverse order.
    InvalidCaptureBoundary,
    /// A continuity proof was requested without a physical root identity.
    MissingRootIdentity,
    /// A continuity proof was requested without a backend stream identity.
    MissingBackendStreamIdentity,
    /// A continuity proof was requested without a durable prior acknowledgement.
    MissingPriorAcknowledgement,
    /// A continuity proof was requested without replay coverage.
    MissingReplayCoverage,
    /// Replay coverage did not describe a contiguous interval after the acknowledgement.
    GappedReplayCoverage,
    /// Replay coverage did not end at the envelope's capture boundary.
    CoverageBoundaryMismatch,
    /// A proof's source, root, stream, or generation did not match its envelope provenance.
    ProofMismatch,
}

impl fmt::Display for RawEnvelopeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyEnvelope => formatter.write_str("raw observation envelope is empty"),
            Self::ZeroEventLimit => formatter.write_str("raw observation event limit is zero"),
            Self::LimitExceeded {
                limit,
                actual,
                maximum,
            } => write!(
                formatter,
                "raw observation {} limit exceeded: {} > {}",
                limit.label(),
                actual,
                maximum
            ),
            Self::ArithmeticOverflow { counter } => {
                write!(
                    formatter,
                    "raw observation {} accounting overflowed",
                    counter.label()
                )
            }
            Self::InvalidCaptureBoundary => {
                formatter.write_str("capture sequence boundaries are not ordered")
            }
            Self::MissingRootIdentity => {
                formatter.write_str("continuity proof requires a physical root identity")
            }
            Self::MissingBackendStreamIdentity => {
                formatter.write_str("continuity proof requires a backend stream identity")
            }
            Self::MissingPriorAcknowledgement => {
                formatter.write_str("continuity proof requires a durable prior acknowledgement")
            }
            Self::MissingReplayCoverage => {
                formatter.write_str("continuity proof requires replay coverage")
            }
            Self::GappedReplayCoverage => {
                formatter.write_str("replay coverage is not contiguous after the acknowledgement")
            }
            Self::CoverageBoundaryMismatch => {
                formatter.write_str("replay coverage does not match the capture boundary")
            }
            Self::ProofMismatch => {
                formatter.write_str("continuity proof does not match provenance")
            }
        }
    }
}

impl std::error::Error for RawEnvelopeError {}

/// Why a root-relative path cannot be admitted as a dispatchable path.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RootRelativePathError {
    /// The path has no native spelling.
    Empty,
    /// The path's native spelling contains an embedded NUL byte.
    EmbeddedNul,
    /// The path is absolute.
    Absolute,
    /// The path is rooted without a relative root identity.
    Rooted,
    /// The path contains a parent traversal component.
    ParentTraversal,
    /// The path contains a platform prefix such as a drive or UNC prefix.
    PlatformPrefix,
    /// The path contains only `.` components and therefore names the root rather than an entry.
    NoNormalComponent,
}

impl fmt::Display for RootRelativePathError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::Empty => "path is empty",
            Self::EmbeddedNul => "path contains an embedded NUL byte",
            Self::Absolute => "path is absolute",
            Self::Rooted => "path is rooted",
            Self::ParentTraversal => "path contains parent traversal",
            Self::PlatformPrefix => "path contains a platform prefix",
            Self::NoNormalComponent => "path has no normal component",
        };
        formatter.write_str(message)
    }
}

impl std::error::Error for RootRelativePathError {}

/// Opaque physical identity for a configured source root.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct RootIdentity(Vec<u8>);

impl RootIdentity {
    /// Construct an opaque root identity from backend-supplied bytes.
    pub fn from_bytes(bytes: Vec<u8>) -> Self {
        Self(bytes)
    }

    /// Borrow the backend-supplied identity bytes without decoding them.
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}

/// Opaque identity for one backend watcher stream.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct BackendStreamIdentity(Vec<u8>);

impl BackendStreamIdentity {
    /// Construct an opaque backend stream identity from backend-supplied bytes.
    pub fn from_bytes(bytes: Vec<u8>) -> Self {
        Self(bytes)
    }

    /// Borrow the backend-supplied stream identity bytes without decoding them.
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}

/// Monotonic watcher generation supplied by admission/lifecycle ownership.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct WatcherGeneration(u64);

impl WatcherGeneration {
    /// Construct a watcher generation value.
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    /// Return the generation number.
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// An exact inclusive range of backend capture sequences.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct CaptureSequenceRange {
    /// The first sequence included in the capture.
    pub first: u64,
    /// The last sequence included in the capture.
    pub last: u64,
}

impl CaptureSequenceRange {
    /// Construct an exact inclusive capture range.
    pub const fn new(first: u64, last: u64) -> Self {
        Self { first, last }
    }

    /// Return the first sequence in the range.
    pub const fn first(self) -> u64 {
        self.first
    }

    /// Return the last sequence in the range.
    pub const fn last(self) -> u64 {
        self.last
    }
}

/// The amount of exact sequence evidence supplied by a capture boundary.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CaptureSequenceEvidence {
    /// Neither sequence endpoint was supplied.
    Missing,
    /// Exactly one sequence endpoint was supplied.
    Ambiguous,
    /// Both sequence endpoints were supplied and form an exact range.
    Exact(CaptureSequenceRange),
}

/// Capture-time and optional sequence boundaries supplied by the observing process.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CaptureBoundary {
    captured_at: u64,
    first_sequence: Option<u64>,
    last_sequence: Option<u64>,
}

impl CaptureBoundary {
    /// Construct a capture boundary without consulting a clock.
    pub fn try_new(
        captured_at: u64,
        first_sequence: Option<u64>,
        last_sequence: Option<u64>,
    ) -> Result<Self, RawEnvelopeError> {
        if let (Some(first), Some(last)) = (first_sequence, last_sequence)
            && first > last
        {
            return Err(RawEnvelopeError::InvalidCaptureBoundary);
        }
        Ok(Self {
            captured_at,
            first_sequence,
            last_sequence,
        })
    }

    /// Return the process-supplied capture timestamp or opaque capture marker.
    pub const fn captured_at(self) -> u64 {
        self.captured_at
    }

    /// Return the first optional sequence admitted by this capture.
    pub const fn first_sequence(self) -> Option<u64> {
        self.first_sequence
    }

    /// Return the last optional sequence admitted by this capture.
    pub const fn last_sequence(self) -> Option<u64> {
        self.last_sequence
    }

    /// Classify whether this capture has no, partial, or exact sequence evidence.
    pub const fn sequence_evidence(self) -> CaptureSequenceEvidence {
        match (self.first_sequence, self.last_sequence) {
            (None, None) => CaptureSequenceEvidence::Missing,
            (Some(first), Some(last)) => {
                CaptureSequenceEvidence::Exact(CaptureSequenceRange::new(first, last))
            }
            _ => CaptureSequenceEvidence::Ambiguous,
        }
    }
}

/// Origin metadata for one bounded raw observation envelope.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RawObservationProvenance {
    source_id: SourceId,
    root_identity: Option<RootIdentity>,
    backend_stream_identity: Option<BackendStreamIdentity>,
    watcher_generation: WatcherGeneration,
    capture_boundary: CaptureBoundary,
}

impl RawObservationProvenance {
    /// Bind provenance to one source, optional physical root, backend stream, generation, and capture boundary.
    pub fn new(
        source_id: SourceId,
        root_identity: Option<RootIdentity>,
        backend_stream_identity: Option<BackendStreamIdentity>,
        watcher_generation: WatcherGeneration,
        capture_boundary: CaptureBoundary,
    ) -> Self {
        Self {
            source_id,
            root_identity,
            backend_stream_identity,
            watcher_generation,
            capture_boundary,
        }
    }

    /// Borrow the associated source identifier.
    pub fn source_id(&self) -> &SourceId {
        &self.source_id
    }

    /// Borrow the physical root identity when the backend supplied one.
    pub fn root_identity(&self) -> Option<&RootIdentity> {
        self.root_identity.as_ref()
    }

    /// Borrow the backend stream identity when the backend supplied one.
    pub fn backend_stream_identity(&self) -> Option<&BackendStreamIdentity> {
        self.backend_stream_identity.as_ref()
    }

    /// Return the watcher generation that admitted this envelope.
    pub const fn watcher_generation(&self) -> WatcherGeneration {
        self.watcher_generation
    }

    /// Return the observing process's capture boundary.
    pub const fn capture_boundary(&self) -> CaptureBoundary {
        self.capture_boundary
    }
}

/// Backend-neutral kind of one raw watcher observation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RawEventKind {
    /// A path was observed as newly created.
    Create,
    /// A path was observed as modified.
    Modify,
    /// A path was observed as deleted.
    Delete,
    /// A source path was observed moving to a destination path.
    Rename,
    /// A source path was observed copied to a destination path.
    Copy,
    /// The physical or configured source root changed.
    RootChanged,
    /// The backend reported that coverage or delivery overflowed.
    Overflow,
    /// The backend reported an error.
    Error,
    /// The backend delivered an event shape this model does not interpret as supported.
    Unsupported,
}

/// Role of one raw path within its event record.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RawPathRole {
    /// A generic event subject.
    Subject,
    /// The source endpoint of a rename.
    RenameSource,
    /// The destination endpoint of a rename.
    RenameDestination,
    /// The source endpoint of a copy.
    CopySource,
    /// The destination endpoint of a copy.
    CopyDestination,
}

/// Backend hint attached to one observed path.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RawPathHint {
    /// The path is known or hinted to be a directory.
    Directory,
    /// The path identifies an empty-folder notification.
    EmptyFolder,
    /// The path is a link/reparse-point entry and must remain entry-level.
    Symlink,
    /// The path was observed absent or otherwise not classifiable as an entry.
    Absent,
    /// The path's current entry type is explicitly uncertain.
    Unknown,
    /// The event subject is the physical source root itself.
    Root,
}

/// Lossless native spelling and role for one observed path.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RawObservedPath {
    path: PathBuf,
    role: RawPathRole,
    hint: Option<RawPathHint>,
}

impl RawObservedPath {
    /// Construct a raw path without validating or rewriting its native spelling.
    pub fn new(path: PathBuf, role: RawPathRole) -> Self {
        Self {
            path,
            role,
            hint: None,
        }
    }

    /// Attach a backend entry-type or root hint while retaining the original path.
    pub fn with_hint(mut self, hint: RawPathHint) -> Self {
        self.hint = Some(hint);
        self
    }

    /// Borrow the original native path spelling.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Return the role assigned by the backend adapter.
    pub const fn role(&self) -> RawPathRole {
        self.role
    }

    /// Return the optional backend hint.
    pub const fn hint(&self) -> Option<RawPathHint> {
        self.hint
    }

    fn encoded_metadata_bytes(&self) -> Result<usize, RawEnvelopeError> {
        let hint_bytes = usize::from(self.hint.is_some());
        1usize
            .checked_add(hint_bytes)
            .ok_or(RawEnvelopeError::ArithmeticOverflow {
                counter: RawEnvelopeCounter::MetadataBytes,
            })
    }
}

/// Explicit uncertainty carried by raw evidence.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct ObservationUncertainty(u16);

impl ObservationUncertainty {
    /// Uncertainty about backend delivery ordering.
    pub const ORDERING: Self = Self(1 << 0);
    /// Uncertainty about whether all affected paths were delivered.
    pub const PATH_COVERAGE: Self = Self(1 << 1);
    /// Uncertainty about continuity with the prior backend boundary.
    pub const CONTINUITY: Self = Self(1 << 2);
    /// An affected parent may be missing or inaccessible.
    pub const MISSING_PARENT: Self = Self(1 << 3);
    /// The backend reported overflow or loss of delivery capacity.
    pub const OVERFLOW: Self = Self(1 << 4);
    /// The backend reported an error affecting observation completeness.
    pub const BACKEND_ERROR: Self = Self(1 << 5);

    /// Construct an empty uncertainty set.
    pub const fn empty() -> Self {
        Self(0)
    }

    /// Construct an uncertainty set from raw bits.
    pub const fn from_bits(bits: u16) -> Self {
        Self(bits)
    }

    /// Return the raw uncertainty bits.
    pub const fn bits(self) -> u16 {
        self.0
    }

    /// Return whether no uncertainty bits are set.
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// Return whether every bit in `other` is present.
    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }
}

impl BitOr for ObservationUncertainty {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self::Output {
        Self(self.0 | rhs.0)
    }
}

impl BitOrAssign for ObservationUncertainty {
    fn bitor_assign(&mut self, rhs: Self) {
        self.0 |= rhs.0;
    }
}

/// Optional backend metadata retained on one raw observation.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RawObservationMetadata {
    flags: u64,
    rename_cookie: Option<u64>,
    event_id: Option<u64>,
    cursor: Option<Vec<u8>>,
    detail: Option<OsString>,
}

impl RawObservationMetadata {
    /// Construct empty optional metadata.
    pub fn new() -> Self {
        Self::default()
    }

    /// Retain backend flags when they are present.
    pub fn with_flags(mut self, flags: u64) -> Self {
        self.flags = flags;
        self
    }

    /// Retain a backend rename cookie.
    pub fn with_rename_cookie(mut self, rename_cookie: u64) -> Self {
        self.rename_cookie = Some(rename_cookie);
        self
    }

    /// Retain a backend event identifier.
    pub fn with_event_id(mut self, event_id: u64) -> Self {
        self.event_id = Some(event_id);
        self
    }

    /// Retain opaque backend cursor bytes without decoding or rewriting them.
    pub fn with_cursor(mut self, cursor: Vec<u8>) -> Self {
        self.cursor = Some(cursor);
        self
    }

    /// Retain native backend error or diagnostic detail.
    pub fn with_detail(mut self, detail: OsString) -> Self {
        self.detail = Some(detail);
        self
    }

    /// Return backend flags, with zero meaning no flags were retained.
    pub const fn flags(&self) -> u64 {
        self.flags
    }

    /// Return the optional rename cookie.
    pub const fn rename_cookie(&self) -> Option<u64> {
        self.rename_cookie
    }

    /// Return the optional backend event identifier.
    pub const fn event_id(&self) -> Option<u64> {
        self.event_id
    }

    /// Borrow opaque backend cursor bytes.
    pub fn cursor(&self) -> Option<&[u8]> {
        self.cursor.as_deref()
    }

    /// Borrow native backend detail.
    pub fn detail(&self) -> Option<&OsStr> {
        self.detail.as_deref()
    }

    fn encoded_metadata_bytes(&self) -> Result<usize, RawEnvelopeError> {
        let flags_bytes = if self.flags == 0 {
            0
        } else {
            std::mem::size_of::<u64>()
        };
        let rename_cookie_bytes = if self.rename_cookie.is_some() {
            std::mem::size_of::<u64>()
        } else {
            0
        };
        let event_id_bytes = if self.event_id.is_some() {
            std::mem::size_of::<u64>()
        } else {
            0
        };
        let cursor_bytes = self.cursor.as_ref().map_or(0, Vec::len);
        let detail_bytes = self
            .detail
            .as_ref()
            .map_or(0, |detail| detail.as_os_str().as_encoded_bytes().len());

        flags_bytes
            .checked_add(rename_cookie_bytes)
            .and_then(|value| value.checked_add(event_id_bytes))
            .and_then(|value| value.checked_add(cursor_bytes))
            .and_then(|value| value.checked_add(detail_bytes))
            .ok_or(RawEnvelopeError::ArithmeticOverflow {
                counter: RawEnvelopeCounter::MetadataBytes,
            })
    }
}

/// One ordered raw backend observation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RawObservation {
    kind: RawEventKind,
    paths: Vec<RawObservedPath>,
    metadata: RawObservationMetadata,
    uncertainty: ObservationUncertainty,
}

impl RawObservation {
    /// Construct one raw observation while retaining path order and duplicates.
    pub fn new(kind: RawEventKind, paths: Vec<RawObservedPath>) -> Self {
        Self {
            kind,
            paths,
            metadata: RawObservationMetadata::default(),
            uncertainty: ObservationUncertainty::empty(),
        }
    }

    /// Attach optional backend metadata.
    pub fn with_metadata(mut self, metadata: RawObservationMetadata) -> Self {
        self.metadata = metadata;
        self
    }

    /// Attach explicit uncertainty bits.
    pub fn with_uncertainty(mut self, uncertainty: ObservationUncertainty) -> Self {
        self.uncertainty = uncertainty;
        self
    }

    /// Return the backend-neutral event kind.
    pub const fn kind(&self) -> RawEventKind {
        self.kind
    }

    /// Borrow paths in their original backend delivery order.
    pub fn paths(&self) -> &[RawObservedPath] {
        &self.paths
    }

    /// Borrow optional backend metadata.
    pub const fn metadata(&self) -> &RawObservationMetadata {
        &self.metadata
    }

    /// Return explicit uncertainty bits.
    pub const fn uncertainty(&self) -> ObservationUncertainty {
        self.uncertainty
    }

    fn accounting(&self) -> Result<(usize, usize), RawEnvelopeError> {
        let mut path_bytes = 0usize;
        // The event kind is one byte in the bounded backend-neutral encoding.
        let mut metadata_bytes = 1usize
            .checked_add(self.metadata.encoded_metadata_bytes()?)
            .ok_or(RawEnvelopeError::ArithmeticOverflow {
                counter: RawEnvelopeCounter::MetadataBytes,
            })?;
        if !self.uncertainty.is_empty() {
            metadata_bytes = metadata_bytes
                .checked_add(std::mem::size_of::<u16>())
                .ok_or(RawEnvelopeError::ArithmeticOverflow {
                    counter: RawEnvelopeCounter::MetadataBytes,
                })?;
        }

        for path in &self.paths {
            path_bytes = path_bytes
                .checked_add(path.path.as_os_str().as_encoded_bytes().len())
                .ok_or(RawEnvelopeError::ArithmeticOverflow {
                    counter: RawEnvelopeCounter::PathBytes,
                })?;
            metadata_bytes = metadata_bytes
                .checked_add(path.encoded_metadata_bytes()?)
                .ok_or(RawEnvelopeError::ArithmeticOverflow {
                    counter: RawEnvelopeCounter::MetadataBytes,
                })?;
        }

        Ok((path_bytes, metadata_bytes))
    }
}

/// Exact accounting for the contents of one raw envelope.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RawObservationAccounting {
    event_count: usize,
    path_bytes: usize,
    metadata_bytes: usize,
}

impl RawObservationAccounting {
    /// Return the number of raw observations.
    pub const fn event_count(self) -> usize {
        self.event_count
    }

    /// Return the sum of native path spelling bytes.
    pub const fn path_bytes(self) -> usize {
        self.path_bytes
    }

    /// Return the sum of encoded metadata bytes.
    pub const fn metadata_bytes(self) -> usize {
        self.metadata_bytes
    }

    /// Return path plus metadata bytes with checked arithmetic.
    pub fn total_bytes(self) -> Result<usize, RawEnvelopeError> {
        self.path_bytes.checked_add(self.metadata_bytes).ok_or(
            RawEnvelopeError::ArithmeticOverflow {
                counter: RawEnvelopeCounter::MetadataBytes,
            },
        )
    }
}

/// Limits used to admit one bounded, non-empty raw observation envelope.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RawObservationLimits {
    max_events: usize,
    max_path_bytes: usize,
    max_metadata_bytes: usize,
}

impl RawObservationLimits {
    /// Construct limits; the event limit must be nonzero while byte limits may be zero for marker-only budgets.
    pub const fn new(
        max_events: usize,
        max_path_bytes: usize,
        max_metadata_bytes: usize,
    ) -> Result<Self, RawEnvelopeError> {
        if max_events == 0 {
            return Err(RawEnvelopeError::ZeroEventLimit);
        }
        Ok(Self {
            max_events,
            max_path_bytes,
            max_metadata_bytes,
        })
    }

    /// Return the maximum number of raw observations.
    pub const fn max_events(self) -> usize {
        self.max_events
    }

    /// Return the maximum native path spelling bytes.
    pub const fn max_path_bytes(self) -> usize {
        self.max_path_bytes
    }

    /// Return the maximum encoded metadata bytes.
    pub const fn max_metadata_bytes(self) -> usize {
        self.max_metadata_bytes
    }

    fn check(self, accounting: RawObservationAccounting) -> Result<(), RawEnvelopeError> {
        let checks = [
            (
                RawEnvelopeLimit::EventCount,
                accounting.event_count,
                self.max_events,
            ),
            (
                RawEnvelopeLimit::PathBytes,
                accounting.path_bytes,
                self.max_path_bytes,
            ),
            (
                RawEnvelopeLimit::MetadataBytes,
                accounting.metadata_bytes,
                self.max_metadata_bytes,
            ),
        ];
        for (limit, actual, maximum) in checks {
            if actual > maximum {
                return Err(RawEnvelopeError::LimitExceeded {
                    limit,
                    actual,
                    maximum,
                });
            }
        }
        Ok(())
    }
}

/// A bounded, ordered, non-empty raw observation envelope.
///
/// External consumers construct unproven envelopes through [`Self::try_new`].
///
/// ```compile_fail
/// # use wavecrate_library::sample_sources::reconciliation::{
/// #     Proof, RawObservation, RawObservationEnvelope, RawObservationLimits,
/// #     RawObservationProvenance,
/// # };
/// # let provenance: RawObservationProvenance = unimplemented!();
/// # let observations: Vec<RawObservation> = Vec::new();
/// # let limits: RawObservationLimits = unimplemented!();
/// let _ = RawObservationEnvelope::try_new_with_proof(
///     provenance,
///     observations,
///     limits,
///     Proof::Unproven,
/// );
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RawObservationEnvelope {
    provenance: RawObservationProvenance,
    proof: Proof,
    observations: Vec<RawObservation>,
    limits: RawObservationLimits,
    accounting: RawObservationAccounting,
}

impl RawObservationEnvelope {
    /// Admit an envelope with the default `Proof::Unproven` state.
    pub fn try_new(
        provenance: RawObservationProvenance,
        observations: Vec<RawObservation>,
        limits: RawObservationLimits,
    ) -> Result<Self, RawEnvelopeError> {
        Self::try_new_with_proof(provenance, observations, limits, Proof::Unproven)
    }

    /// Admit an envelope carrying a checked proof without changing the raw evidence.
    // This module-private seam is reserved for the later library-owned replay authority.
    #[allow(dead_code)]
    pub(in crate::sample_sources::reconciliation) fn try_new_with_proof(
        provenance: RawObservationProvenance,
        observations: Vec<RawObservation>,
        limits: RawObservationLimits,
        proof: Proof,
    ) -> Result<Self, RawEnvelopeError> {
        if observations.is_empty() {
            return Err(RawEnvelopeError::EmptyEnvelope);
        }

        let mut path_bytes = 0usize;
        let mut metadata_bytes = 0usize;
        for observation in &observations {
            let (observation_path_bytes, observation_metadata_bytes) = observation.accounting()?;
            path_bytes = path_bytes.checked_add(observation_path_bytes).ok_or(
                RawEnvelopeError::ArithmeticOverflow {
                    counter: RawEnvelopeCounter::PathBytes,
                },
            )?;
            metadata_bytes = metadata_bytes
                .checked_add(observation_metadata_bytes)
                .ok_or(RawEnvelopeError::ArithmeticOverflow {
                    counter: RawEnvelopeCounter::MetadataBytes,
                })?;
        }

        let accounting = RawObservationAccounting {
            event_count: observations.len(),
            path_bytes,
            metadata_bytes,
        };
        limits.check(accounting)?;

        if let Proof::WatcherContinuity(proof) = &proof {
            proof.validate_against(&provenance)?;
        }

        Ok(Self {
            provenance,
            proof,
            observations,
            limits,
            accounting,
        })
    }

    /// Borrow the envelope's origin metadata.
    pub const fn provenance(&self) -> &RawObservationProvenance {
        &self.provenance
    }

    /// Borrow the separate proof state.
    pub const fn proof(&self) -> &Proof {
        &self.proof
    }

    /// Borrow raw observations in backend delivery order.
    pub fn observations(&self) -> &[RawObservation] {
        &self.observations
    }

    /// Return the limits that admitted this envelope.
    pub const fn limits(&self) -> RawObservationLimits {
        self.limits
    }

    /// Return exact event, path-byte, and metadata-byte accounting.
    pub const fn accounting(&self) -> RawObservationAccounting {
        self.accounting
    }
}

/// Separate proof state for raw watcher evidence.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Proof {
    /// No continuity or targeted-authority proof is available.
    #[default]
    Unproven,
    /// A replay adapter supplied a structurally checked continuity proof.
    WatcherContinuity(WatcherContinuityProof),
}

impl Proof {
    /// Return whether this proof state is unproven.
    pub const fn is_unproven(&self) -> bool {
        matches!(self, Self::Unproven)
    }

    /// Borrow the continuity proof when present.
    pub const fn watcher_continuity(&self) -> Option<&WatcherContinuityProof> {
        match self {
            Self::Unproven => None,
            Self::WatcherContinuity(proof) => Some(proof),
        }
    }
}

/// Durable prior backend acknowledgement used as the start of replay coverage.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DurablePriorAcknowledgement {
    sequence: u64,
}

impl DurablePriorAcknowledgement {
    /// Construct an acknowledgement that has already been durably recorded by the owner.
    // This module-private seam is reserved for the later library-owned replay authority.
    #[allow(dead_code)]
    pub(in crate::sample_sources::reconciliation) const fn new(sequence: u64) -> Self {
        Self { sequence }
    }

    /// Return the acknowledged backend sequence.
    pub const fn sequence(self) -> u64 {
        self.sequence
    }
}

/// Claimed replay interval after a durable acknowledgement.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReplayCoverage {
    after_sequence: u64,
    through_sequence: u64,
    contiguous: bool,
}

impl ReplayCoverage {
    /// Construct a replay interval and retain whether the adapter proved contiguity.
    // This module-private seam is reserved for the later library-owned replay authority.
    #[allow(dead_code)]
    pub(in crate::sample_sources::reconciliation) const fn try_new(
        after_sequence: u64,
        through_sequence: u64,
        contiguous: bool,
    ) -> Result<Self, RawEnvelopeError> {
        if through_sequence < after_sequence {
            return Err(RawEnvelopeError::GappedReplayCoverage);
        }
        Ok(Self {
            after_sequence,
            through_sequence,
            contiguous,
        })
    }

    /// Return the sequence immediately before the claimed replay interval.
    pub const fn after_sequence(self) -> u64 {
        self.after_sequence
    }

    /// Return the current batch's inclusive terminal sequence.
    pub const fn through_sequence(self) -> u64 {
        self.through_sequence
    }

    /// Return whether the replay adapter proved the interval gap-free.
    pub const fn is_contiguous(self) -> bool {
        self.contiguous
    }
}

/// Structurally checked replay continuity proof.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WatcherContinuityProof {
    source_id: SourceId,
    root_identity: RootIdentity,
    backend_stream_identity: BackendStreamIdentity,
    watcher_generation: WatcherGeneration,
    prior_acknowledgement: DurablePriorAcknowledgement,
    replay_coverage: ReplayCoverage,
}

impl WatcherContinuityProof {
    /// Construct a proof bound to provenance, durable acknowledgement, and contiguous coverage.
    // This module-private seam is reserved for the later library-owned replay authority.
    #[allow(dead_code)]
    pub(in crate::sample_sources::reconciliation) fn try_new(
        provenance: &RawObservationProvenance,
        prior_acknowledgement: Option<DurablePriorAcknowledgement>,
        replay_coverage: Option<ReplayCoverage>,
    ) -> Result<Self, RawEnvelopeError> {
        let root_identity = provenance
            .root_identity
            .clone()
            .ok_or(RawEnvelopeError::MissingRootIdentity)?;
        let backend_stream_identity = provenance
            .backend_stream_identity
            .clone()
            .ok_or(RawEnvelopeError::MissingBackendStreamIdentity)?;
        let prior_acknowledgement =
            prior_acknowledgement.ok_or(RawEnvelopeError::MissingPriorAcknowledgement)?;
        let replay_coverage = replay_coverage.ok_or(RawEnvelopeError::MissingReplayCoverage)?;
        let proof = Self {
            source_id: provenance.source_id.clone(),
            root_identity,
            backend_stream_identity,
            watcher_generation: provenance.watcher_generation,
            prior_acknowledgement,
            replay_coverage,
        };
        proof.validate_against(provenance)?;
        Ok(proof)
    }

    /// Borrow the proof's source identifier.
    pub fn source_id(&self) -> &SourceId {
        &self.source_id
    }

    /// Borrow the proof's physical root identity.
    pub fn root_identity(&self) -> &RootIdentity {
        &self.root_identity
    }

    /// Borrow the proof's backend stream identity.
    pub fn backend_stream_identity(&self) -> &BackendStreamIdentity {
        &self.backend_stream_identity
    }

    /// Return the proof's watcher generation.
    pub const fn watcher_generation(&self) -> WatcherGeneration {
        self.watcher_generation
    }

    /// Return the durable prior acknowledgement.
    pub const fn prior_acknowledgement(&self) -> DurablePriorAcknowledgement {
        self.prior_acknowledgement
    }

    /// Return the claimed contiguous replay interval.
    pub const fn replay_coverage(&self) -> ReplayCoverage {
        self.replay_coverage
    }

    fn validate_against(
        &self,
        provenance: &RawObservationProvenance,
    ) -> Result<(), RawEnvelopeError> {
        if self.source_id != *provenance.source_id()
            || Some(&self.root_identity) != provenance.root_identity()
            || Some(&self.backend_stream_identity) != provenance.backend_stream_identity()
            || self.watcher_generation != provenance.watcher_generation
        {
            return Err(RawEnvelopeError::ProofMismatch);
        }

        if !self.replay_coverage.is_contiguous()
            || self.replay_coverage.after_sequence() != self.prior_acknowledgement.sequence()
            || self.replay_coverage.through_sequence() <= self.prior_acknowledgement.sequence()
        {
            return Err(RawEnvelopeError::GappedReplayCoverage);
        }

        let boundary = provenance.capture_boundary();
        if boundary.last_sequence() != Some(self.replay_coverage.through_sequence()) {
            return Err(RawEnvelopeError::CoverageBoundaryMismatch);
        }
        if let Some(first_sequence) = boundary.first_sequence() {
            let expected_first = self.prior_acknowledgement.sequence().checked_add(1).ok_or(
                RawEnvelopeError::ArithmeticOverflow {
                    counter: RawEnvelopeCounter::Sequence,
                },
            )?;
            if first_sequence != expected_first {
                return Err(RawEnvelopeError::GappedReplayCoverage);
            }
        }
        Ok(())
    }
}

/// Lossless root-relative path validated without rebuilding or normalizing it.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct RootRelativePath(PathBuf);

impl RootRelativePath {
    /// Validate and retain a native path spelling exactly as supplied.
    pub fn try_from_path(path: PathBuf) -> Result<Self, RootRelativePathError> {
        if path.as_os_str().is_empty() {
            return Err(RootRelativePathError::Empty);
        }
        if path.as_os_str().as_encoded_bytes().contains(&0) {
            return Err(RootRelativePathError::EmbeddedNul);
        }

        let mut has_normal_component = false;
        for component in path.components() {
            match component {
                Component::Prefix(_) => return Err(RootRelativePathError::PlatformPrefix),
                Component::ParentDir => return Err(RootRelativePathError::ParentTraversal),
                Component::CurDir => {}
                Component::RootDir => {}
                Component::Normal(_) => has_normal_component = true,
            }
        }
        if path.is_absolute() {
            return Err(RootRelativePathError::Absolute);
        }
        if path
            .components()
            .any(|component| component == Component::RootDir)
        {
            return Err(RootRelativePathError::Rooted);
        }
        if !has_normal_component {
            return Err(RootRelativePathError::NoNormalComponent);
        }
        Ok(Self(path))
    }

    /// Borrow the validated native path spelling.
    pub fn as_path(&self) -> &Path {
        &self.0
    }

    /// Consume the validated path without rewriting it.
    pub fn into_path(self) -> PathBuf {
        self.0
    }

    /// Return the nearest valid lexical parent, or `None` at the source root.
    pub fn parent(&self) -> Option<Self> {
        self.0
            .parent()
            .and_then(|parent| Self::try_from_path(parent.to_path_buf()).ok())
    }
}

impl TryFrom<PathBuf> for RootRelativePath {
    type Error = RootRelativePathError;

    fn try_from(path: PathBuf) -> Result<Self, Self::Error> {
        Self::try_from_path(path)
    }
}

impl TryFrom<&Path> for RootRelativePath {
    type Error = RootRelativePathError;

    fn try_from(path: &Path) -> Result<Self, Self::Error> {
        Self::try_from_path(path.to_path_buf())
    }
}
