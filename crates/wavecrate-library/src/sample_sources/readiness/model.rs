/// Stable readiness stages required for a usable source.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ReadinessStage {
    /// The source manifest contains the current file identity.
    IndexedIdentity,
    /// Versioned analysis features are current.
    AnalysisFeatures,
    /// Versioned similarity embedding and aspect descriptors are current.
    EmbeddingAspects,
    /// Source-level ANN membership and layout generation are current.
    SimilarityLayout,
}

impl ReadinessStage {
    pub(crate) const ALL: [Self; 4] = [
        Self::IndexedIdentity,
        Self::AnalysisFeatures,
        Self::EmbeddingAspects,
        Self::SimilarityLayout,
    ];

    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::IndexedIdentity => "indexed_identity",
            Self::AnalysisFeatures => "analysis_features",
            Self::EmbeddingAspects => "embedding_aspects",
            Self::SimilarityLayout => "similarity_layout",
        }
    }

    pub(crate) fn from_stored(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|stage| stage.as_str() == value)
    }

    pub(crate) fn job_type(self) -> &'static str {
        match self {
            Self::IndexedIdentity => "readiness_indexed_identity_v1",
            Self::AnalysisFeatures => "readiness_analysis_features_v1",
            Self::EmbeddingAspects => "readiness_embedding_aspects_v1",
            Self::SimilarityLayout => "readiness_similarity_layout_v1",
        }
    }
}

/// Whether one readiness stage belongs to a file identity or to the source as a whole.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ReadinessScopeKind {
    /// One current file identity.
    File,
    /// One source-level generation.
    Source,
}

impl ReadinessScopeKind {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::File => "file",
            Self::Source => "source",
        }
    }

    pub(crate) fn from_stored(value: &str) -> Option<Self> {
        match value {
            "file" => Some(Self::File),
            "source" => Some(Self::Source),
            _ => None,
        }
    }
}

/// Durable availability of a configured source.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceAvailability {
    /// The enabled source root is available for convergence.
    Active,
    /// The enabled source root is temporarily unavailable.
    Offline,
    /// The source is intentionally disabled and must not be processed.
    Disabled,
}

impl SourceAvailability {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Offline => "offline",
            Self::Disabled => "disabled",
        }
    }

    pub(crate) fn from_stored(value: &str) -> Option<Self> {
        match value {
            "active" => Some(Self::Active),
            "offline" => Some(Self::Offline),
            "disabled" => Some(Self::Disabled),
            _ => None,
        }
    }
}

/// Eligibility of an identity for one desired readiness stage.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReadinessEligibility {
    /// The stage is required and may produce work.
    Eligible,
    /// The file type or shape is unsupported for this stage.
    Unsupported,
    /// The identity is no longer current and must not produce work.
    Deleted,
}

/// Order-independent exact membership checkpoint for source-scoped similarity work.
///
/// Each eligible file identity and content generation contributes one cryptographic digest. The
/// XOR accumulator allows a committed manifest delta to remove an old contribution and add a new
/// one without reading the complete source manifest. The member count is part of the published
/// generation so duplicate cancellation cannot hide a membership change.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ReadinessMembership {
    pub(crate) digest: [u8; 32],
    pub(crate) count: u64,
}

impl ReadinessMembership {
    /// Restore a durable membership checkpoint.
    pub fn from_parts(digest: [u8; 32], count: u64) -> Self {
        Self { digest, count }
    }

    /// Add or remove one identity generation contribution.
    ///
    /// Applying the same contribution twice restores the original digest. Callers must update the
    /// count separately through `add` or `remove` so the durable member cardinality stays exact.
    pub(crate) fn toggle(&mut self, identity: &str, content_generation: &str) {
        let mut hasher = blake3::Hasher::new();
        hasher.update(b"wavecrate-readiness-membership-v1");
        hasher.update(&[0]);
        hasher.update(identity.as_bytes());
        hasher.update(&[0]);
        hasher.update(content_generation.as_bytes());
        for (slot, contribution) in self.digest.iter_mut().zip(hasher.finalize().as_bytes()) {
            *slot ^= contribution;
        }
    }

    /// Add one eligible identity generation.
    pub fn add(&mut self, identity: &str, content_generation: &str) {
        self.toggle(identity, content_generation);
        self.count = self.count.saturating_add(1);
    }

    /// Remove one eligible identity generation.
    pub(crate) fn remove(&mut self, identity: &str, content_generation: &str) {
        self.toggle(identity, content_generation);
        self.count = self.count.saturating_sub(1);
    }

    /// Return the stable source-scoped generation token.
    pub fn generation(&self) -> String {
        use std::fmt::Write as _;

        let mut generation = format!("membership-xor-v1:{}:", self.count);
        generation.reserve(64);
        for byte in self.digest {
            write!(&mut generation, "{byte:02x}")
                .expect("writing into an owned string cannot fail");
        }
        generation
    }
}

impl ReadinessEligibility {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Eligible => "eligible",
            Self::Unsupported => "unsupported",
            Self::Deleted => "deleted",
        }
    }

    pub(crate) fn from_stored(value: &str) -> Option<Self> {
        match value {
            "eligible" => Some(Self::Eligible),
            "unsupported" => Some(Self::Unsupported),
            "deleted" => Some(Self::Deleted),
            _ => None,
        }
    }
}

/// Durable desired state for one versioned readiness stage.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReadinessTarget {
    /// Configured source identity.
    pub source_id: String,
    /// File-scoped or source-scoped ownership.
    pub scope_kind: ReadinessScopeKind,
    /// Stable file identity, or the source ID for source-scoped work.
    pub scope_id: String,
    /// Current source-relative path when the scope is a file.
    pub relative_path: Option<String>,
    /// Required readiness stage.
    pub stage: ReadinessStage,
    /// Algorithm, format, or artifact contract version required by the stage.
    pub required_version: String,
    /// Committed source-manifest generation that owns this target.
    pub source_generation: i64,
    /// Non-empty content generation for file-scoped work, or membership generation for source work.
    pub content_generation: String,
    /// Whether this target may produce actionable work.
    pub eligibility: ReadinessEligibility,
}

impl ReadinessTarget {
    /// Build one eligible file-stage target.
    pub fn file(
        source_id: impl Into<String>,
        file_identity: impl Into<String>,
        relative_path: impl Into<String>,
        stage: ReadinessStage,
        required_version: impl Into<String>,
        source_generation: i64,
        content_generation: impl Into<String>,
    ) -> Self {
        Self {
            source_id: source_id.into(),
            scope_kind: ReadinessScopeKind::File,
            scope_id: file_identity.into(),
            relative_path: Some(relative_path.into()),
            stage,
            required_version: required_version.into(),
            source_generation,
            content_generation: content_generation.into(),
            eligibility: ReadinessEligibility::Eligible,
        }
    }

    /// Build one eligible source-stage target.
    pub fn source(
        source_id: impl Into<String>,
        stage: ReadinessStage,
        required_version: impl Into<String>,
        source_generation: i64,
        membership_generation: impl Into<String>,
    ) -> Self {
        let source_id = source_id.into();
        Self {
            scope_kind: ReadinessScopeKind::Source,
            scope_id: source_id.clone(),
            source_id,
            relative_path: None,
            stage,
            required_version: required_version.into(),
            source_generation,
            content_generation: membership_generation.into(),
            eligibility: ReadinessEligibility::Eligible,
        }
    }

    /// Return a copy with terminal eligibility.
    pub fn with_eligibility(mut self, eligibility: ReadinessEligibility) -> Self {
        self.eligibility = eligibility;
        self
    }

    pub(crate) fn key(&self) -> ReadinessKey {
        ReadinessKey {
            source_id: self.source_id.clone(),
            scope_kind: self.scope_kind,
            scope_id: self.scope_id.clone(),
            stage: self.stage,
        }
    }
}

/// Persisted completion marker for one exact readiness generation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReadinessArtifact {
    /// Configured source identity.
    pub source_id: String,
    /// File-scoped or source-scoped ownership.
    pub scope_kind: ReadinessScopeKind,
    /// Stable file identity, or source identity for source-scoped work.
    pub scope_id: String,
    /// Completed stage.
    pub stage: ReadinessStage,
    /// Version produced by the completed work.
    pub artifact_version: String,
    /// Source generation captured by the work.
    pub source_generation: i64,
    /// Non-empty file content or source membership generation captured by the work.
    pub content_generation: String,
    /// Completion timestamp used for diagnostics only.
    pub completed_at: i64,
}

impl ReadinessArtifact {
    /// Build a completion marker that exactly matches a desired target.
    pub fn for_target(target: &ReadinessTarget, completed_at: i64) -> Self {
        Self {
            source_id: target.source_id.clone(),
            scope_kind: target.scope_kind,
            scope_id: target.scope_id.clone(),
            stage: target.stage,
            artifact_version: target.required_version.clone(),
            source_generation: target.source_generation,
            content_generation: target.content_generation.clone(),
            completed_at,
        }
    }
}

/// One durably claimed readiness unit tied to the exact target observed at claim time.
///
/// `claim_generation` fences every lease mutation and completion, so a worker whose lease expired
/// cannot publish after another worker has reclaimed the same target. `failure_attempts` is kept
/// separate so benign cancellation and lease recovery never consume the retry allowance.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClaimedReadinessWork {
    /// Exact desired target captured when the work was claimed.
    pub target: ReadinessTarget,
    /// Monotonic claim generation for this target's durable work row.
    pub claim_generation: u32,
    /// Retryable failures recorded before this claim began.
    pub failure_attempts: u32,
    /// Current lease deadline captured by the claim operation.
    pub lease_expires_at: i64,
    /// Durable state transition that made this work claimable.
    pub origin: ReadinessClaimOrigin,
}

impl ClaimedReadinessWork {
    /// Exact target owned by this claim generation.
    pub fn target(&self) -> &ReadinessTarget {
        &self.target
    }

    /// Monotonic generation used to fence stale workers.
    pub fn claim_generation(&self) -> u32 {
        self.claim_generation
    }

    /// Retryable failures recorded before this claim began.
    pub fn failure_attempts(&self) -> u32 {
        self.failure_attempts
    }

    /// Current lease deadline captured by the claim operation.
    pub fn lease_expires_at(&self) -> i64 {
        self.lease_expires_at
    }

    /// Durable state transition that made this work claimable.
    pub fn origin(&self) -> ReadinessClaimOrigin {
        self.origin
    }
}

/// Durable state transition that made one readiness job claimable.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReadinessClaimOrigin {
    /// Newly persisted work, or work explicitly returned to the pending state.
    Pending,
    /// A retryable execution failure whose retry deadline is due.
    Retry,
    /// Running work whose durable lease has expired.
    ExpiredLease,
    /// Compatibility work written without a lease by an older Wavecrate version.
    LegacyNullLease,
}

impl ReadinessClaimOrigin {
    /// Stable diagnostic value for logs and telemetry.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Retry => "retry",
            Self::ExpiredLease => "expired_lease",
            Self::LegacyNullLease => "legacy_null_lease",
        }
    }
}

/// Stable failure classes understood by readiness reconciliation and telemetry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReadinessFailureClassification {
    /// The operation may succeed after bounded backoff.
    Retryable,
    /// The exact target cannot complete without a content or product change.
    Permanent,
    /// The target's media or stage is not supported.
    Unsupported,
}

impl ReadinessFailureClassification {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Retryable => "retryable",
            Self::Permanent => "permanent",
            Self::Unsupported => "unsupported",
        }
    }
}

/// Bounded exponential retry policy for readiness work.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReadinessRetryPolicy {
    initial_delay_seconds: i64,
    max_delay_seconds: i64,
    max_attempts: u32,
}

impl ReadinessRetryPolicy {
    /// Build a valid bounded policy.
    ///
    /// Returns `None` for zero/negative delays, an initial delay above the maximum, or zero
    /// attempts.
    pub fn new(
        initial_delay_seconds: i64,
        max_delay_seconds: i64,
        max_attempts: u32,
    ) -> Option<Self> {
        (initial_delay_seconds > 0
            && max_delay_seconds >= initial_delay_seconds
            && max_attempts > 0)
            .then_some(Self {
                initial_delay_seconds,
                max_delay_seconds,
                max_attempts,
            })
    }

    /// Maximum number of recorded retryable failures before work becomes terminal.
    pub fn max_attempts(self) -> u32 {
        self.max_attempts
    }

    /// Delay for a failed claim attempt, saturating at the configured maximum.
    pub fn delay_for_attempt(self, attempt: u32) -> i64 {
        let exponent = attempt.saturating_sub(1).min(62);
        self.initial_delay_seconds
            .checked_mul(1_i64 << exponent)
            .unwrap_or(i64::MAX)
            .min(self.max_delay_seconds)
    }
}

/// Outcome of a generation-fenced work-state mutation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReadinessWorkMutationOutcome {
    /// The claimed row still belonged to the caller and was updated.
    Recorded,
    /// The target changed, the lease expired, or a newer claim generation owns the row.
    RejectedStale,
}

/// Outcome of renewing a claimed readiness lease.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReadinessLeaseRenewalOutcome {
    /// The claim remains current through the returned deadline.
    Renewed {
        /// Persisted lease deadline after renewal.
        lease_expires_at: i64,
    },
    /// The target changed, the lease expired, or a newer claim generation owns the row.
    RejectedStale,
}

/// Outcome of recording a readiness failure.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReadinessFailureOutcome {
    /// A retryable failure was scheduled at the returned deadline.
    RetryScheduled {
        /// Earliest time at which the exact target may be reclaimed.
        retry_at: i64,
    },
    /// The worker explicitly classified the target as permanently failed.
    Permanent,
    /// The worker classified the target as unsupported.
    Unsupported,
    /// A retryable failure consumed the configured final attempt.
    AttemptsExhausted,
    /// The target changed, the lease expired, or a newer claim generation owns the row.
    RejectedStale,
}

/// Current durable readiness queue state for telemetry.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ReadinessWorkStats {
    /// All readiness-managed work rows.
    pub total: usize,
    /// Immediately claimable pending rows.
    pub pending: usize,
    /// Rows with an unexpired lease.
    pub running: usize,
    /// Running rows whose lease has expired and may be recovered.
    pub expired_leases: usize,
    /// Retryable failures whose deadline has arrived.
    pub retries_due: usize,
    /// Retryable failures waiting for their deadline.
    pub retries_waiting: usize,
    /// Earliest persisted retry deadline that has not arrived yet.
    pub earliest_retry_at: Option<i64>,
    /// Earliest running lease deadline that has not arrived yet.
    pub earliest_lease_expiry_at: Option<i64>,
    /// Permanently failed rows.
    pub permanent_failures: usize,
    /// Unsupported rows.
    pub unsupported: usize,
    /// Pending rows most recently returned by explicit cancellation.
    pub cancelled: usize,
    /// Successfully completed rows retained for diagnostics.
    pub completed: usize,
}

impl ReadinessWorkStats {
    /// Whether durable readiness work can be claimed immediately.
    pub fn has_actionable_work(&self) -> bool {
        self.pending > 0 || self.expired_leases > 0 || self.retries_due > 0
    }

    /// Return the earliest future retry or lease deadline.
    pub fn earliest_future_deadline(&self) -> Option<i64> {
        match (self.earliest_retry_at, self.earliest_lease_expiry_at) {
            (Some(retry_at), Some(lease_expires_at)) => Some(retry_at.min(lease_expires_at)),
            (Some(retry_at), None) => Some(retry_at),
            (None, Some(lease_expires_at)) => Some(lease_expires_at),
            (None, None) => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct ReadinessKey {
    pub(crate) source_id: String,
    pub(crate) scope_kind: ReadinessScopeKind,
    pub(crate) scope_id: String,
    pub(crate) stage: ReadinessStage,
}
