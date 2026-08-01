//! Profile-local durable intent journal for future transaction recovery.
//!
//! This module records intent and coordinator state only. It deliberately does not
//! contain, replay, or interpret filesystem actions. Startup recovery is a bounded
//! scan that retains every record it cannot decode and reports that attention is
//! required.

#![allow(dead_code)]

use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use uuid::Uuid;

use super::AbsentFinalRecoveryClassification;
use super::absent_final_recovery::inspect_absent_final_recovery;
pub(crate) use super::capacity_gate::VolumeIdentity;
use super::capacity_gate::{DurableCapacityPlan, RejectedBeforeIntent};
pub(crate) use super::expected_identity_replacement::ReplacementQualificationAssessment;
use super::expected_identity_replacement::{
    ExpectedIdentityReplacementAdapter, ExpectedIdentityReplacementOutcome,
    ExpectedIdentityReplacementRequest, ProductionExpectedIdentityReplacementAdapter,
};
#[cfg(test)]
pub(crate) use super::expected_identity_replacement::{
    ObservedFilesystemClassification, ReplacementCandidateAssessment,
    ReplacementCandidatePrimitive, ReplacementMissingInvariant, ReplacementPlatformFamily,
    ReplacementQualificationDecision, ReplacementQualificationRetryCondition,
};
use super::publication::{
    FilesystemPublishedWaveformRestore, is_absent_final_no_replace_publication,
    validate_absent_final_no_replace_publication, validate_publication_evidence,
};

const SCHEMA_V1: u32 = 1;
const SCHEMA_V2: u32 = 2;
const CURRENT_SCHEMA_VERSION: u32 = SCHEMA_V1;
const MAX_RECORD_BYTES: usize = 1024 * 1024;
const RECORD_SUFFIX: &str = ".json";
const LOCK_FILE_NAME: &str = "owner.lock";

/// Typed actor that admitted an operation to the journal.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) enum OperationActor {
    /// A user initiated operation.
    User,
    /// An operation observed from an external source.
    ExternalFs,
}

/// Typed operation family. Payload details remain bounded and opaque to this foundation.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) enum OperationKind {
    /// An operation originating from transaction history.
    FileHistory,
    /// A future application-owned file operation.
    FileMutation,
    /// A future external filesystem reconciliation operation.
    ExternalReconciliation,
}

/// Durable intent recorded before any future filesystem execution is admitted.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct OperationIntent {
    /// Actor that requested or observed the operation.
    pub(crate) actor: OperationActor,
    /// Operation family.
    pub(crate) kind: OperationKind,
    /// Bounded user-facing label, when available.
    pub(crate) label: String,
}

/// Ordered journal phases. This foundation stores state but never executes a phase.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) enum OperationPhase {
    /// Intent and its initial record are durable.
    IntentDurable,
    /// Capabilities and participants have been prepared.
    Prepared,
    /// A filesystem staging participant has been created.
    FilesystemStaged,
    /// Filesystem publication has been observed and verified by a future coordinator.
    FilesystemPublished,
    /// Source participants have reconciled.
    SourceReconciled,
    /// Global participants have reconciled.
    GlobalReconciled,
    /// The UI projection has been published.
    ProjectionPublished,
    /// Readiness work has been scheduled.
    ReadinessScheduled,
    /// The operation has reached a terminal state.
    Terminal,
}

impl OperationPhase {
    fn is_pre_publication(self) -> bool {
        matches!(
            self,
            Self::IntentDurable | Self::Prepared | Self::FilesystemStaged
        )
    }

    fn is_post_publication(self) -> bool {
        matches!(
            self,
            Self::FilesystemPublished
                | Self::SourceReconciled
                | Self::GlobalReconciled
                | Self::ProjectionPublished
                | Self::ReadinessScheduled
        )
    }
}

/// Durable disposition overlay for a journal record.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) enum OperationDisposition {
    /// No disposition overlay is currently active.
    None,
    /// Work may be retried by a future coordinator.
    RetryPending,
    /// A participant is incomplete and needs retry.
    PartialNeedsRetry,
    /// Evidence is uncertain and requires an audit.
    AuditRequired,
    /// Cancellation was requested before publication.
    CancelRequestedBeforePublish,
    /// Cancellation was requested after publication.
    CancelRequestedAfterPublish,
    /// Operation completed successfully.
    Succeeded,
    /// Operation completed while optional artifacts remain deferred.
    SucceededWithDeferredArtifacts,
    /// Operation was cancelled before publication.
    CancelledBeforePublish,
    /// Operation was cancelled after publication.
    CancelledAfterPublish,
    /// Operation was rolled back.
    RolledBack,
    /// Operation is blocked pending a user decision.
    BlockedByUser,
    /// Operation failed while preserving evidence.
    FailedPreservingData,
    /// Operation has unresolved data-loss risk.
    FailedDataLossRisk,
}

impl OperationDisposition {
    fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Succeeded
                | Self::SucceededWithDeferredArtifacts
                | Self::CancelledBeforePublish
                | Self::CancelledAfterPublish
                | Self::RolledBack
                | Self::BlockedByUser
                | Self::FailedPreservingData
                | Self::FailedDataLossRisk
        )
    }
}

/// The direction captured by a prepared waveform restore.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) enum PreparedRestoreDirection {
    Undo,
    Redo,
}

/// Stable identity of a filesystem object observed through an open descriptor.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct PreparedObjectIdentity {
    pub(crate) stable_id: String,
    pub(crate) change_marker: Option<String>,
    pub(crate) len: u64,
}

/// A root capability retained as a bounded, typed locator and identity.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct PreparedRootCapability {
    pub(crate) path: PathBuf,
    pub(crate) identity: PreparedObjectIdentity,
}

/// A regular leaf locator relative to one of the prepared roots.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct PreparedLeafLocator {
    pub(crate) relative_path: PathBuf,
    pub(crate) identity: PreparedObjectIdentity,
}

/// Advisory destination-local staging name. Preparation never creates this path.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct PreparedStagingLocator {
    pub(crate) relative_path: PathBuf,
    pub(crate) absent: bool,
}

/// Identity expected by the future replacement executor.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) enum ReplaceExpectedIdentity {
    Existing(PreparedObjectIdentity),
}

/// Compact, serializable evidence captured while preparing a restore.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct PreparedRestoreEvidence {
    pub(crate) target: PreparedFileEvidence,
    pub(crate) backup: PreparedFileEvidence,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) enum PreparedFileEvidence {
    Missing,
    ContentHash([u8; 32]),
    Metadata {
        len: u64,
        modified_ns: Option<i64>,
        is_dir: bool,
    },
    Unverifiable,
}

/// Typed descriptor proving that one non-extracted waveform restore passed preparation.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct PreparedWaveformRestore {
    pub(crate) direction: PreparedRestoreDirection,
    pub(crate) source_id: String,
    pub(crate) source_root: PreparedRootCapability,
    pub(crate) target_root: PreparedRootCapability,
    pub(crate) target: PreparedLeafLocator,
    pub(crate) backup_root: PreparedRootCapability,
    pub(crate) backup: PreparedLeafLocator,
    pub(crate) replacement: ReplaceExpectedIdentity,
    pub(crate) staging: PreparedStagingLocator,
    pub(crate) evidence: PreparedRestoreEvidence,
}

/// The advisory observation captured before a no-replace final-name claim.
///
/// This is deliberately not an object identity.  An absent final target has no identity to
/// preserve, and the observation is only a preparation hint; the qualified adapter must prove
/// the final claim through the retained target-parent capability.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) enum AbsentFinalObservation {
    ObservedAbsent,
}

/// Preparation evidence for a destination whose final name was observed absent.
///
/// Unlike `PreparedWaveformRestore`, this contract has no target leaf identity or replacement
/// operand.  The final leaf and staging leaf are namespace locators only; the durable
/// `CopyValidated` participant supplies the staged object identity and exact content evidence.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct PreparedAbsentFinalNoReplace {
    pub(crate) direction: PreparedRestoreDirection,
    pub(crate) source_id: String,
    pub(crate) source_root: PreparedRootCapability,
    pub(crate) target_parent: PreparedRootCapability,
    pub(crate) final_leaf: PathBuf,
    pub(crate) staging: PreparedStagingLocator,
    pub(crate) final_observation: AbsentFinalObservation,
    pub(crate) copy_validated_evidence: PreparedFileEvidence,
}

/// Non-handle evidence observed after a staged absent-final entry became the final entry.
///
/// This is a journal observation only. It does not establish ownership, publication, adoption,
/// pathname continuity, or a retained filesystem capability.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct AbsentFinalRecoveryObservation {
    pub(crate) target_parent_stable_id: String,
    pub(crate) final_stable_id: String,
    pub(crate) final_len: u64,
    pub(crate) final_content: PreparedFileEvidence,
}

/// Non-handle evidence proving that a matching absent-final recovery observation was re-opened
/// and content-verified as the transaction-owned final object.
///
/// This is deliberately distinct from both `AbsentFinalRecoveryObservation` and publication
/// evidence. It carries no pathname, continuity claim, open capability, or mutation authority.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct AbsentFinalTransactionOwnedProof {
    pub(crate) target_parent_stable_id: String,
    pub(crate) final_stable_id: String,
    pub(crate) final_len: u64,
    pub(crate) final_content: PreparedFileEvidence,
}

/// Tagged runtime preparation contract used to select the publication guard.
///
/// Schema-v1 records adapt only to `ExistingExpectedIdentity`.  The absent-final variant is
/// schema-v2-only and is created only through the explicit test seam in this module; production
/// waveform-restore admission continues to construct the v1 existing-target contract.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum PreparedTargetContract {
    ExistingExpectedIdentity(PreparedWaveformRestore),
    AbsentFinalNoReplace(PreparedAbsentFinalNoReplace),
}

impl PreparedTargetContract {
    fn as_existing(&self) -> Option<&PreparedWaveformRestore> {
        match self {
            Self::ExistingExpectedIdentity(prepared) => Some(prepared),
            Self::AbsentFinalNoReplace(_) => None,
        }
    }

    fn as_existing_mut(&mut self) -> Option<&mut PreparedWaveformRestore> {
        match self {
            Self::ExistingExpectedIdentity(prepared) => Some(prepared),
            Self::AbsentFinalNoReplace(_) => None,
        }
    }
}

/// The filesystem participant checkpoint recorded after a complete staged copy.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) enum FilesystemStagedParticipant {
    CopyValidated {
        staging: PreparedLeafLocator,
        evidence: PreparedFileEvidence,
    },
}

/// Typed evidence proving that the prepared restore has one validated staging participant.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct FilesystemStagedWaveformRestore {
    pub(crate) participant: FilesystemStagedParticipant,
}

/// Profile recovery root opened and identity-checked by the journal owner thread.
#[derive(Clone, Debug)]
pub(crate) struct RecoveryRootCapability {
    pub(crate) path: PathBuf,
    pub(crate) file: Arc<File>,
    pub(crate) identity: String,
}

impl PartialEq for RecoveryRootCapability {
    fn eq(&self, other: &Self) -> bool {
        self.path == other.path && self.identity == other.identity
    }
}

impl Eq for RecoveryRootCapability {}

pub(crate) fn open_recovery_root_capability(
    path_override: Option<PathBuf>,
) -> Result<RecoveryRootCapability, JournalError> {
    let path = match path_override {
        Some(path) => path,
        None => wavecrate::app_dirs::app_root_dir()
            .map_err(|error| JournalError::AppDirectory(error.to_string()))?,
    };
    let file =
        super::capacity_gate::open_no_follow_path(&path).map_err(|source| JournalError::Write {
            path: path.clone(),
            source,
        })?;
    if !file
        .metadata()
        .map_err(|source| JournalError::Write {
            path: path.clone(),
            source,
        })?
        .is_dir()
    {
        return Err(JournalError::Write {
            path,
            source: io::Error::new(
                io::ErrorKind::InvalidData,
                "recovery root is not a directory",
            ),
        });
    }
    let identity =
        wavecrate_library::filesystem_identity::stable_filesystem_identity_from_open_file(&file)
            .ok_or_else(|| JournalError::Write {
                path: path.clone(),
                source: io::Error::new(
                    io::ErrorKind::InvalidData,
                    "recovery root identity unavailable",
                ),
            })?;
    Ok(RecoveryRootCapability {
        path,
        file: Arc::new(file),
        identity,
    })
}

/// One complete, bounded durable operation record.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct OperationRecord {
    /// Record schema version.
    pub(crate) schema_version: u32,
    /// Stable operation identity.
    pub(crate) operation_id: Uuid,
    /// Durable intent.
    pub(crate) intent: OperationIntent,
    /// Last durable coordinator phase.
    pub(crate) phase: OperationPhase,
    /// Last durable disposition overlay.
    pub(crate) disposition: OperationDisposition,
    /// Bounded coordinator metadata. Filesystem payloads are intentionally not interpreted here.
    pub(crate) payload: Value,
    /// Optional physical-capacity claims.  `None` is retained for legacy records and blocks
    /// bounded capacity admission while the operation remains unresolved.
    pub(crate) capacity_plan: Option<DurableCapacityPlan>,
    /// Tagged typed preparation evidence. Schema-v1 records adapt to the existing-target variant.
    pub(crate) prepared: Option<PreparedTargetContract>,
    /// Typed filesystem staging evidence. Legacy records do not contain this checkpoint.
    pub(crate) staged: Option<FilesystemStagedWaveformRestore>,
    /// Optional schema-v2-only evidence from the read-only absent-final classifier.
    pub(crate) absent_final_recovery_observation: Option<AbsentFinalRecoveryObservation>,
    /// Optional schema-v2-only evidence that the observed final was reopened and content-verified
    /// as transaction-owned. This is not publication evidence or an open capability.
    pub(crate) absent_final_transaction_owned_proof: Option<AbsentFinalTransactionOwnedProof>,
    /// Typed filesystem publication evidence. Legacy records do not contain this checkpoint.
    pub(crate) published: Option<FilesystemPublishedWaveformRestore>,
    /// Latest bounded assessment explaining why expected-identity replacement is not qualified.
    /// Legacy schema-v1 records do not contain this field.
    pub(crate) replacement_qualification: Option<ReplacementQualificationAssessment>,
    /// Creation timestamp in Unix milliseconds.
    pub(crate) created_unix_ms: i64,
    /// Last update timestamp in Unix milliseconds.
    pub(crate) updated_unix_ms: i64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct SchemaV2EvidencePresence {
    prepared: bool,
    staged: bool,
    absent_final_recovery_observation: bool,
    absent_final_transaction_owned_proof: bool,
    published: bool,
}

impl SchemaV2EvidencePresence {
    const NONE: Self = Self {
        prepared: false,
        staged: false,
        absent_final_recovery_observation: false,
        absent_final_transaction_owned_proof: false,
        published: false,
    };
    const PREPARED: Self = Self {
        prepared: true,
        staged: false,
        absent_final_recovery_observation: false,
        absent_final_transaction_owned_proof: false,
        published: false,
    };
    const PREPARED_STAGED: Self = Self {
        prepared: true,
        staged: true,
        absent_final_recovery_observation: false,
        absent_final_transaction_owned_proof: false,
        published: false,
    };
    const PREPARED_STAGED_WITH_ABSENT_FINAL_RECOVERY_OBSERVATION: Self = Self {
        prepared: true,
        staged: true,
        absent_final_recovery_observation: true,
        absent_final_transaction_owned_proof: false,
        published: false,
    };
    const PREPARED_STAGED_WITH_ABSENT_FINAL_RECOVERY_PROOF: Self = Self {
        prepared: true,
        staged: true,
        absent_final_recovery_observation: true,
        absent_final_transaction_owned_proof: true,
        published: false,
    };
    const ALL: Self = Self {
        prepared: true,
        staged: true,
        absent_final_recovery_observation: false,
        absent_final_transaction_owned_proof: false,
        published: true,
    };
    fn from_record(record: &OperationRecord) -> Self {
        Self {
            prepared: record.prepared.is_some(),
            staged: record.staged.is_some(),
            absent_final_recovery_observation: record.absent_final_recovery_observation.is_some(),
            absent_final_transaction_owned_proof: record
                .absent_final_transaction_owned_proof
                .is_some(),
            published: record.published.is_some(),
        }
    }
}

/// The exact schema-v1 representation owned by this journal's decoder and writer.
///
/// `OperationRecord` is runtime state.  Keeping this persisted form private and strict means
/// that a future field cannot be accepted into runtime state and then erased by a later v1 write.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct PersistedOperationRecordV1 {
    schema_version: u32,
    operation_id: Uuid,
    intent: OperationIntent,
    phase: OperationPhase,
    disposition: OperationDisposition,
    payload: Value,
    #[serde(default)]
    capacity_plan: Option<DurableCapacityPlan>,
    #[serde(default)]
    prepared: Option<PreparedWaveformRestore>,
    #[serde(default)]
    staged: Option<FilesystemStagedWaveformRestore>,
    #[serde(default)]
    published: Option<FilesystemPublishedWaveformRestore>,
    #[serde(default)]
    replacement_qualification: Option<ReplacementQualificationAssessment>,
    created_unix_ms: i64,
    updated_unix_ms: i64,
}

impl From<PersistedOperationRecordV1> for OperationRecord {
    fn from(record: PersistedOperationRecordV1) -> Self {
        Self {
            schema_version: record.schema_version,
            operation_id: record.operation_id,
            intent: record.intent,
            phase: record.phase,
            disposition: record.disposition,
            payload: record.payload,
            capacity_plan: record.capacity_plan,
            prepared: record
                .prepared
                .map(PreparedTargetContract::ExistingExpectedIdentity),
            staged: record.staged,
            absent_final_recovery_observation: None,
            absent_final_transaction_owned_proof: None,
            published: record.published,
            replacement_qualification: record.replacement_qualification,
            created_unix_ms: record.created_unix_ms,
            updated_unix_ms: record.updated_unix_ms,
        }
    }
}

fn persisted_v1_from_record(
    record: &OperationRecord,
) -> Result<PersistedOperationRecordV1, io::Error> {
    if record.absent_final_recovery_observation.is_some() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "schema-v1 cannot encode absent-final recovery observation evidence",
        ));
    }
    if record.absent_final_transaction_owned_proof.is_some() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "schema-v1 cannot encode absent-final transaction-owned proof evidence",
        ));
    }
    let prepared = match record.prepared.as_ref() {
        None => None,
        Some(PreparedTargetContract::ExistingExpectedIdentity(prepared)) => Some(prepared.clone()),
        Some(PreparedTargetContract::AbsentFinalNoReplace(_)) => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "schema-v1 cannot encode absent-final preparation evidence",
            ));
        }
    };
    if record
        .published
        .as_ref()
        .is_some_and(is_absent_final_no_replace_publication)
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "schema-v1 cannot encode absent-final publication evidence",
        ));
    }
    Ok(PersistedOperationRecordV1 {
        schema_version: record.schema_version,
        operation_id: record.operation_id,
        intent: record.intent.clone(),
        phase: record.phase,
        disposition: record.disposition,
        payload: record.payload.clone(),
        capacity_plan: record.capacity_plan.clone(),
        prepared,
        staged: record.staged.clone(),
        published: record.published.clone(),
        replacement_qualification: record.replacement_qualification.clone(),
        created_unix_ms: record.created_unix_ms,
        updated_unix_ms: record.updated_unix_ms,
    })
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct PersistedOperationRecordV2 {
    schema_version: u32,
    operation_id: Uuid,
    intent: OperationIntent,
    phase: OperationPhase,
    disposition: OperationDisposition,
    payload: Value,
    #[serde(default)]
    capacity_plan: Option<DurableCapacityPlan>,
    #[serde(default)]
    prepared: Option<PersistedPreparedTargetContractV2>,
    #[serde(default)]
    staged: Option<FilesystemStagedWaveformRestore>,
    #[serde(default)]
    absent_final_recovery_observation: Option<AbsentFinalRecoveryObservation>,
    #[serde(default)]
    absent_final_transaction_owned_proof: Option<AbsentFinalTransactionOwnedProof>,
    #[serde(default)]
    published: Option<FilesystemPublishedWaveformRestore>,
    #[serde(default)]
    replacement_qualification: Option<ReplacementQualificationAssessment>,
    created_unix_ms: i64,
    updated_unix_ms: i64,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
enum PersistedPreparedTargetContractV2 {
    ExistingExpectedIdentity(PreparedWaveformRestore),
    AbsentFinalNoReplace(PreparedAbsentFinalNoReplace),
}

impl From<PersistedPreparedTargetContractV2> for PreparedTargetContract {
    fn from(contract: PersistedPreparedTargetContractV2) -> Self {
        match contract {
            PersistedPreparedTargetContractV2::ExistingExpectedIdentity(prepared) => {
                Self::ExistingExpectedIdentity(prepared)
            }
            PersistedPreparedTargetContractV2::AbsentFinalNoReplace(prepared) => {
                Self::AbsentFinalNoReplace(prepared)
            }
        }
    }
}

impl TryFrom<&PreparedTargetContract> for PersistedPreparedTargetContractV2 {
    type Error = io::Error;

    fn try_from(contract: &PreparedTargetContract) -> Result<Self, Self::Error> {
        Ok(match contract {
            PreparedTargetContract::ExistingExpectedIdentity(prepared) => {
                Self::ExistingExpectedIdentity(prepared.clone())
            }
            PreparedTargetContract::AbsentFinalNoReplace(prepared) => {
                Self::AbsentFinalNoReplace(prepared.clone())
            }
        })
    }
}

impl From<PersistedOperationRecordV2> for OperationRecord {
    fn from(record: PersistedOperationRecordV2) -> Self {
        Self {
            schema_version: record.schema_version,
            operation_id: record.operation_id,
            intent: record.intent,
            phase: record.phase,
            disposition: record.disposition,
            payload: record.payload,
            capacity_plan: record.capacity_plan,
            prepared: record.prepared.map(Into::into),
            staged: record.staged,
            absent_final_recovery_observation: record.absent_final_recovery_observation,
            absent_final_transaction_owned_proof: record.absent_final_transaction_owned_proof,
            published: record.published,
            replacement_qualification: record.replacement_qualification,
            created_unix_ms: record.created_unix_ms,
            updated_unix_ms: record.updated_unix_ms,
        }
    }
}

fn persisted_v2_from_record(
    record: &OperationRecord,
) -> Result<PersistedOperationRecordV2, io::Error> {
    Ok(PersistedOperationRecordV2 {
        schema_version: record.schema_version,
        operation_id: record.operation_id,
        intent: record.intent.clone(),
        phase: record.phase,
        disposition: record.disposition,
        payload: record.payload.clone(),
        capacity_plan: record.capacity_plan.clone(),
        prepared: record
            .prepared
            .as_ref()
            .map(PersistedPreparedTargetContractV2::try_from)
            .transpose()?,
        staged: record.staged.clone(),
        absent_final_recovery_observation: record.absent_final_recovery_observation.clone(),
        absent_final_transaction_owned_proof: record.absent_final_transaction_owned_proof.clone(),
        published: record.published.clone(),
        replacement_qualification: record.replacement_qualification.clone(),
        created_unix_ms: record.created_unix_ms,
        updated_unix_ms: record.updated_unix_ms,
    })
}

enum DecodedPersistedRecord {
    V1(PersistedOperationRecordV1),
    V2(PersistedOperationRecordV2),
    UnknownVersion(u32),
}

fn is_supported_schema_version(version: u32) -> bool {
    matches!(version, SCHEMA_V1 | SCHEMA_V2)
}

fn strict_object<'a>(
    value: &'a Value,
    allowed_fields: &[&str],
) -> Result<&'a Map<String, Value>, &'static str> {
    let object = value
        .as_object()
        .ok_or("persisted value must be a JSON object")?;
    if object
        .keys()
        .any(|field| !allowed_fields.contains(&field.as_str()))
    {
        return Err("persisted object contains an unknown field");
    }
    Ok(object)
}

fn tagged_object<'a>(value: &'a Value) -> Result<(&'a str, &'a Value), &'static str> {
    let object = value
        .as_object()
        .ok_or("persisted tagged value must be a JSON object")?;
    if object.len() != 1 {
        return Err("persisted tagged object must contain exactly one variant");
    }
    let (variant, value) = object
        .iter()
        .next()
        .ok_or("persisted tagged object must contain exactly one variant")?;
    Ok((variant.as_str(), value))
}

fn required_field<'a>(
    object: &'a Map<String, Value>,
    field: &str,
) -> Result<&'a Value, &'static str> {
    object
        .get(field)
        .ok_or("persisted object is missing a required field")
}

fn validate_optional_field(
    object: &Map<String, Value>,
    field: &str,
    validate: fn(&Value) -> Result<(), &'static str>,
) -> Result<(), &'static str> {
    if let Some(value) = object.get(field) {
        if !value.is_null() {
            validate(value)?;
        }
    }
    Ok(())
}

fn validate_persisted_intent_value(value: &Value) -> Result<(), &'static str> {
    strict_object(value, &["actor", "kind", "label"])?;
    Ok(())
}

fn validate_persisted_volume_identity_value(value: &Value) -> Result<(), &'static str> {
    strict_object(value, &["device"])?;
    Ok(())
}

fn validate_persisted_capacity_plan_value(value: &Value) -> Result<(), &'static str> {
    let object = strict_object(value, &["volumes"])?;
    let volumes = required_field(object, "volumes")?
        .as_array()
        .ok_or("persisted capacity volumes must be an array")?;
    for volume in volumes {
        let volume = strict_object(
            volume,
            &[
                "identity",
                "allocation_unit",
                "allocation_class",
                "logical_bytes",
                "allocated_bytes",
                "protected_free_bytes",
            ],
        )?;
        validate_persisted_volume_identity_value(required_field(volume, "identity")?)?;
    }
    Ok(())
}

fn validate_persisted_object_identity_value(value: &Value) -> Result<(), &'static str> {
    strict_object(value, &["stable_id", "change_marker", "len"])?;
    Ok(())
}

fn validate_persisted_root_capability_value(value: &Value) -> Result<(), &'static str> {
    let object = strict_object(value, &["path", "identity"])?;
    validate_persisted_object_identity_value(required_field(object, "identity")?)?;
    Ok(())
}

fn validate_persisted_leaf_locator_value(value: &Value) -> Result<(), &'static str> {
    let object = strict_object(value, &["relative_path", "identity"])?;
    validate_persisted_object_identity_value(required_field(object, "identity")?)?;
    Ok(())
}

fn validate_persisted_replace_expected_identity_value(value: &Value) -> Result<(), &'static str> {
    let (variant, value) = tagged_object(value)?;
    if variant != "Existing" {
        return Err("persisted replacement identity contains an unknown variant");
    }
    validate_persisted_object_identity_value(value)
}

fn validate_persisted_file_evidence_value(value: &Value) -> Result<(), &'static str> {
    match value {
        Value::String(_) => Ok(()),
        Value::Object(_) => {
            let (variant, value) = tagged_object(value)?;
            match variant {
                "ContentHash" => value
                    .as_array()
                    .map(|_| ())
                    .ok_or("persisted content hash must be an array"),
                "Metadata" => {
                    strict_object(value, &["len", "modified_ns", "is_dir"])?;
                    Ok(())
                }
                _ => Err("persisted file evidence contains an unknown variant"),
            }
        }
        _ => Err("persisted file evidence must be a string or tagged object"),
    }
}

fn validate_persisted_prepared_evidence_value(value: &Value) -> Result<(), &'static str> {
    let object = strict_object(value, &["target", "backup"])?;
    validate_persisted_file_evidence_value(required_field(object, "target")?)?;
    validate_persisted_file_evidence_value(required_field(object, "backup")?)?;
    Ok(())
}

fn validate_persisted_staging_locator_value(value: &Value) -> Result<(), &'static str> {
    strict_object(value, &["relative_path", "absent"])?;
    Ok(())
}

fn validate_persisted_prepared_value(value: &Value) -> Result<(), &'static str> {
    let object = strict_object(
        value,
        &[
            "direction",
            "source_id",
            "source_root",
            "target_root",
            "target",
            "backup_root",
            "backup",
            "replacement",
            "staging",
            "evidence",
        ],
    )?;
    validate_persisted_root_capability_value(required_field(object, "source_root")?)?;
    validate_persisted_root_capability_value(required_field(object, "target_root")?)?;
    validate_persisted_leaf_locator_value(required_field(object, "target")?)?;
    validate_persisted_root_capability_value(required_field(object, "backup_root")?)?;
    validate_persisted_leaf_locator_value(required_field(object, "backup")?)?;
    validate_persisted_replace_expected_identity_value(required_field(object, "replacement")?)?;
    validate_persisted_staging_locator_value(required_field(object, "staging")?)?;
    validate_persisted_prepared_evidence_value(required_field(object, "evidence")?)?;
    Ok(())
}

fn validate_persisted_absent_final_observation_value(value: &Value) -> Result<(), &'static str> {
    match value {
        Value::String(observation) if observation == "ObservedAbsent" => Ok(()),
        Value::String(_) => Err("persisted absent-final observation contains an unknown variant"),
        _ => Err("persisted absent-final observation must be a string"),
    }
}

fn validate_persisted_absent_final_prepared_value(value: &Value) -> Result<(), &'static str> {
    let object = strict_object(
        value,
        &[
            "direction",
            "source_id",
            "source_root",
            "target_parent",
            "final_leaf",
            "staging",
            "final_observation",
            "copy_validated_evidence",
        ],
    )?;
    validate_persisted_root_capability_value(required_field(object, "source_root")?)?;
    validate_persisted_root_capability_value(required_field(object, "target_parent")?)?;
    if !required_field(object, "final_leaf")?.is_string() {
        return Err("persisted absent-final leaf must be a string");
    }
    validate_persisted_staging_locator_value(required_field(object, "staging")?)?;
    validate_persisted_absent_final_observation_value(required_field(
        object,
        "final_observation",
    )?)?;
    validate_persisted_file_evidence_value(required_field(object, "copy_validated_evidence")?)?;
    Ok(())
}

fn validate_persisted_absent_final_recovery_observation_value(
    value: &Value,
) -> Result<(), &'static str> {
    let object = strict_object(
        value,
        &[
            "target_parent_stable_id",
            "final_stable_id",
            "final_len",
            "final_content",
        ],
    )?;
    for field in ["target_parent_stable_id", "final_stable_id"] {
        if required_field(object, field)?
            .as_str()
            .is_none_or(str::is_empty)
        {
            return Err("persisted absent-final recovery identity must be a non-empty string");
        }
    }
    if !required_field(object, "final_len")?.is_u64() {
        return Err("persisted absent-final recovery final length must be an unsigned integer");
    }
    let final_content = required_field(object, "final_content")?;
    validate_persisted_file_evidence_value(final_content)?;
    let (variant, hash) = tagged_object(final_content)?;
    if variant != "ContentHash" {
        return Err("persisted absent-final recovery content must be ContentHash");
    }
    let hash = hash
        .as_array()
        .ok_or("persisted absent-final recovery content hash must be an array")?;
    if hash.len() != 32
        || hash
            .iter()
            .any(|byte| byte.as_u64().is_none_or(|byte| byte > 255))
    {
        return Err("persisted absent-final recovery content hash must contain 32 bytes");
    }
    Ok(())
}

fn validate_persisted_absent_final_transaction_owned_proof_value(
    value: &Value,
) -> Result<(), &'static str> {
    let object = strict_object(
        value,
        &[
            "target_parent_stable_id",
            "final_stable_id",
            "final_len",
            "final_content",
        ],
    )?;
    for field in ["target_parent_stable_id", "final_stable_id"] {
        if required_field(object, field)?
            .as_str()
            .is_none_or(str::is_empty)
        {
            return Err(
                "persisted absent-final transaction-owned proof identity must be a non-empty string",
            );
        }
    }
    if !required_field(object, "final_len")?.is_u64() {
        return Err(
            "persisted absent-final transaction-owned proof final length must be an unsigned integer",
        );
    }
    let final_content = required_field(object, "final_content")?;
    validate_persisted_file_evidence_value(final_content)?;
    let (variant, hash) = tagged_object(final_content)?;
    if variant != "ContentHash" {
        return Err("persisted absent-final transaction-owned proof content must be ContentHash");
    }
    let hash = hash
        .as_array()
        .ok_or("persisted absent-final transaction-owned proof content hash must be an array")?;
    if hash.len() != 32
        || hash
            .iter()
            .any(|byte| byte.as_u64().is_none_or(|byte| byte > 255))
    {
        return Err(
            "persisted absent-final transaction-owned proof content hash must contain 32 bytes",
        );
    }
    Ok(())
}

fn validate_persisted_prepared_contract_v2_value(value: &Value) -> Result<(), &'static str> {
    let (variant, value) = tagged_object(value)?;
    match variant {
        "ExistingExpectedIdentity" => validate_persisted_prepared_value(value),
        "AbsentFinalNoReplace" => validate_persisted_absent_final_prepared_value(value),
        _ => Err("persisted preparation contract contains an unknown variant"),
    }
}

fn validate_persisted_staged_value(value: &Value) -> Result<(), &'static str> {
    let object = strict_object(value, &["participant"])?;
    let (variant, value) = tagged_object(required_field(object, "participant")?)?;
    if variant != "CopyValidated" {
        return Err("persisted staged participant contains an unknown variant");
    }
    let participant = strict_object(value, &["staging", "evidence"])?;
    validate_persisted_leaf_locator_value(required_field(participant, "staging")?)?;
    validate_persisted_file_evidence_value(required_field(participant, "evidence")?)?;
    Ok(())
}

fn validate_persisted_final_claim_primitive_value(value: &Value) -> Result<(), &'static str> {
    if value.is_string() {
        return Ok(());
    }
    let (variant, value) = tagged_object(value)?;
    if variant != "Unqualified" {
        return Err("persisted final claim primitive contains an unknown variant");
    }
    strict_object(value, &["reason"])?;
    Ok(())
}

fn validate_persisted_capability_scope_value(value: &Value) -> Result<(), &'static str> {
    let (variant, value) = tagged_object(value)?;
    if variant != "TargetParentDescriptor" {
        return Err("persisted capability scope contains an unknown variant");
    }
    let scope = strict_object(value, &["target_parent_identity", "root_path_continuity"])?;
    validate_persisted_object_identity_value(required_field(scope, "target_parent_identity")?)?;
    Ok(())
}

fn validate_persisted_final_claim_value(value: &Value) -> Result<(), &'static str> {
    let (variant, value) = tagged_object(value)?;
    match variant {
        "ExpectedIdentityReplacement" => {
            let claim = strict_object(
                value,
                &["primitive", "result", "expected_target", "displaced_target"],
            )?;
            validate_persisted_final_claim_primitive_value(required_field(claim, "primitive")?)?;
            validate_persisted_object_identity_value(required_field(claim, "expected_target")?)?;
            validate_persisted_object_identity_value(required_field(claim, "displaced_target")?)?;
        }
        "AbsentFinalNoReplace" => {
            let claim = strict_object(value, &["primitive", "result", "capability_scope"])?;
            validate_persisted_final_claim_primitive_value(required_field(claim, "primitive")?)?;
            validate_persisted_capability_scope_value(required_field(claim, "capability_scope")?)?;
        }
        _ => return Err("persisted final claim contains an unknown variant"),
    }
    Ok(())
}

fn validate_persisted_published_value(value: &Value) -> Result<(), &'static str> {
    let object = strict_object(
        value,
        &[
            "mode",
            "final_claim",
            "reopened_final",
            "visibility",
            "whole_publication",
            "synchronization",
        ],
    )?;
    validate_persisted_final_claim_value(required_field(object, "final_claim")?)?;
    let reopened = strict_object(
        required_field(object, "reopened_final")?,
        &["identity", "content"],
    )?;
    validate_persisted_object_identity_value(required_field(reopened, "identity")?)?;
    validate_persisted_file_evidence_value(required_field(reopened, "content")?)?;
    Ok(())
}

fn validate_persisted_replacement_qualification_value(value: &Value) -> Result<(), &'static str> {
    let object = strict_object(
        value,
        &[
            "platform_family",
            "observed_filesystem",
            "volume",
            "candidate",
            "candidate_assessment",
            "missing_invariant",
            "decision",
            "retry_condition",
        ],
    )?;
    validate_persisted_volume_identity_value(required_field(object, "volume")?)?;
    Ok(())
}

fn validate_persisted_v1_object_boundaries(value: &Value) -> Result<(), &'static str> {
    let object = strict_object(
        value,
        &[
            "schema_version",
            "operation_id",
            "intent",
            "phase",
            "disposition",
            "payload",
            "capacity_plan",
            "prepared",
            "staged",
            "published",
            "replacement_qualification",
            "created_unix_ms",
            "updated_unix_ms",
        ],
    )?;
    validate_persisted_intent_value(required_field(object, "intent")?)?;
    validate_optional_field(
        object,
        "capacity_plan",
        validate_persisted_capacity_plan_value,
    )?;
    validate_optional_field(object, "prepared", validate_persisted_prepared_value)?;
    validate_optional_field(object, "staged", validate_persisted_staged_value)?;
    validate_optional_field(object, "published", validate_persisted_published_value)?;
    validate_optional_field(
        object,
        "replacement_qualification",
        validate_persisted_replacement_qualification_value,
    )?;
    // `payload` is intentionally opaque JSON metadata, not a persisted DTO boundary.
    Ok(())
}

fn validate_persisted_v2_object_boundaries(value: &Value) -> Result<(), &'static str> {
    let object = strict_object(
        value,
        &[
            "schema_version",
            "operation_id",
            "intent",
            "phase",
            "disposition",
            "payload",
            "capacity_plan",
            "prepared",
            "staged",
            "absent_final_recovery_observation",
            "absent_final_transaction_owned_proof",
            "published",
            "replacement_qualification",
            "created_unix_ms",
            "updated_unix_ms",
        ],
    )?;
    validate_persisted_intent_value(required_field(object, "intent")?)?;
    validate_optional_field(
        object,
        "capacity_plan",
        validate_persisted_capacity_plan_value,
    )?;
    if let Some(prepared) = object.get("prepared") {
        if !prepared.is_null() {
            validate_persisted_prepared_contract_v2_value(prepared)?;
        }
    }
    validate_optional_field(object, "staged", validate_persisted_staged_value)?;
    validate_optional_field(
        object,
        "absent_final_recovery_observation",
        validate_persisted_absent_final_recovery_observation_value,
    )?;
    validate_optional_field(
        object,
        "absent_final_transaction_owned_proof",
        validate_persisted_absent_final_transaction_owned_proof_value,
    )?;
    validate_optional_field(object, "published", validate_persisted_published_value)?;
    validate_optional_field(
        object,
        "replacement_qualification",
        validate_persisted_replacement_qualification_value,
    )?;
    Ok(())
}

/// Dispatch a parsed JSON object to the exact persisted schema decoder.
fn dispatch_persisted_record(value: Value) -> Result<DecodedPersistedRecord, ()> {
    let version = value
        .get("schema_version")
        .and_then(Value::as_u64)
        .and_then(|value| u32::try_from(value).ok())
        .ok_or(())?;
    match version {
        SCHEMA_V1 => {
            validate_persisted_v1_object_boundaries(&value).map_err(|_| ())?;
            serde_json::from_value::<PersistedOperationRecordV1>(value)
                .map(DecodedPersistedRecord::V1)
                .map_err(|_| ())
        }
        SCHEMA_V2 => {
            validate_persisted_v2_object_boundaries(&value).map_err(|_| ())?;
            serde_json::from_value::<PersistedOperationRecordV2>(value)
                .map(DecodedPersistedRecord::V2)
                .map_err(|_| ())
        }
        _ => Ok(DecodedPersistedRecord::UnknownVersion(version)),
    }
}

fn encode_schema_v1(record: &OperationRecord) -> io::Result<Vec<u8>> {
    if record.schema_version != SCHEMA_V1 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "unsupported schema version",
        ));
    }
    serde_json::to_vec_pretty(&persisted_v1_from_record(record)?).map_err(io::Error::other)
}

fn encode_schema_v2(record: &OperationRecord) -> io::Result<Vec<u8>> {
    if record.schema_version != SCHEMA_V2 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "unsupported schema version",
        ));
    }
    serde_json::to_vec_pretty(&persisted_v2_from_record(record)?).map_err(io::Error::other)
}

fn encode_record(record: &OperationRecord) -> io::Result<Vec<u8>> {
    match record.schema_version {
        SCHEMA_V1 => encode_schema_v1(record),
        SCHEMA_V2 => encode_schema_v2(record),
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "unsupported schema version",
        )),
    }
}

impl OperationRecord {
    /// Construct a new intent record in the durable `IntentDurable` phase.
    pub(crate) fn new(intent: OperationIntent, payload: Value) -> Self {
        Self::new_with_capacity_plan(intent, payload, None)
    }

    fn new_with_capacity_plan(
        intent: OperationIntent,
        payload: Value,
        capacity_plan: Option<DurableCapacityPlan>,
    ) -> Self {
        let now = unix_millis();
        Self {
            schema_version: SCHEMA_V1,
            operation_id: Uuid::new_v4(),
            intent,
            phase: OperationPhase::IntentDurable,
            disposition: OperationDisposition::None,
            payload,
            capacity_plan,
            prepared: None,
            staged: None,
            absent_final_recovery_observation: None,
            absent_final_transaction_owned_proof: None,
            published: None,
            replacement_qualification: None,
            created_unix_ms: now,
            updated_unix_ms: now,
        }
    }

    #[cfg(test)]
    fn new_v2_absent_final_with_capacity_plan(
        intent: OperationIntent,
        payload: Value,
        prepared: PreparedAbsentFinalNoReplace,
        staged: FilesystemStagedWaveformRestore,
        capacity_plan: DurableCapacityPlan,
    ) -> Self {
        let mut record = Self::new_with_capacity_plan(intent, payload, Some(capacity_plan));
        record.schema_version = SCHEMA_V2;
        record.phase = OperationPhase::FilesystemStaged;
        record.prepared = Some(PreparedTargetContract::AbsentFinalNoReplace(prepared));
        record.staged = Some(staged);
        record
    }

    fn with_update(&self, phase: OperationPhase, disposition: OperationDisposition) -> Self {
        let mut updated = self.clone();
        updated.phase = phase;
        updated.disposition = disposition;
        if phase != OperationPhase::FilesystemStaged {
            updated.absent_final_recovery_observation = None;
            updated.absent_final_transaction_owned_proof = None;
        }
        updated.updated_unix_ms = unix_millis();
        updated
    }

    fn with_prepared(&self, prepared: PreparedTargetContract) -> Self {
        let mut updated = self.clone();
        updated.phase = OperationPhase::Prepared;
        updated.disposition = OperationDisposition::None;
        updated.prepared = Some(prepared);
        updated.updated_unix_ms = unix_millis();
        updated
    }

    fn with_staged(&self, staged: FilesystemStagedWaveformRestore) -> Self {
        let mut updated = self.clone();
        updated.phase = OperationPhase::FilesystemStaged;
        updated.disposition = OperationDisposition::None;
        updated.staged = Some(staged);
        updated.updated_unix_ms = unix_millis();
        updated
    }

    fn with_published(&self, published: FilesystemPublishedWaveformRestore) -> Self {
        let mut updated = self.clone();
        updated.phase = OperationPhase::FilesystemPublished;
        updated.disposition = OperationDisposition::None;
        updated.absent_final_recovery_observation = None;
        updated.absent_final_transaction_owned_proof = None;
        updated.published = Some(published);
        // A qualified publication supersedes any prior unsupported assessment.  Keeping both
        // would make the durable record contradict its terminal filesystem evidence.
        updated.replacement_qualification = None;
        updated.updated_unix_ms = unix_millis();
        updated
    }

    fn with_absent_final_recovery_observation(
        &self,
        observation: AbsentFinalRecoveryObservation,
    ) -> Self {
        let mut updated = self.clone();
        updated.absent_final_recovery_observation = Some(observation);
        updated.updated_unix_ms = unix_millis();
        updated
    }

    fn with_absent_final_transaction_owned_proof(
        &self,
        proof: AbsentFinalTransactionOwnedProof,
    ) -> Self {
        let mut updated = self.clone();
        updated.absent_final_transaction_owned_proof = Some(proof);
        updated.updated_unix_ms = unix_millis();
        updated
    }
}

fn validate_record_publication(record: &OperationRecord) -> Result<(), String> {
    let prepared = record
        .prepared
        .as_ref()
        .ok_or_else(|| String::from("publication evidence is missing prepared evidence"))?;
    let staged = record
        .staged
        .as_ref()
        .ok_or_else(|| String::from("publication evidence is missing staged evidence"))?;
    validate_staged_checkpoint(prepared, staged)?;
    let published = record
        .published
        .as_ref()
        .ok_or_else(|| String::from("publication evidence is missing"))?;
    match prepared {
        PreparedTargetContract::ExistingExpectedIdentity(prepared) => {
            validate_publication_evidence(prepared, staged, published)
        }
        PreparedTargetContract::AbsentFinalNoReplace(_) => {
            validate_absent_final_no_replace_publication(
                record.prepared.as_ref().expect("prepared evidence present"),
                staged,
                published,
            )
        }
    }
}

fn validate_prepared_contract(prepared: &PreparedTargetContract) -> Result<(), String> {
    let PreparedTargetContract::AbsentFinalNoReplace(prepared) = prepared else {
        return Ok(());
    };
    if !prepared.source_root.path.is_absolute()
        || prepared.source_root.identity.stable_id.is_empty()
    {
        return Err(String::from(
            "absent-final source-root capability is not an absolute, identified root",
        ));
    }
    if !prepared.target_parent.path.is_absolute() {
        return Err(String::from(
            "absent-final target-parent capability path must be absolute",
        ));
    }
    if prepared.target_parent.identity.stable_id.is_empty() {
        return Err(String::from(
            "absent-final target-parent capability identity is empty",
        ));
    }
    single_clean_normal_leaf("final", &prepared.final_leaf)?;
    single_clean_normal_leaf("staging", &prepared.staging.relative_path)?;
    if prepared.final_leaf == prepared.staging.relative_path {
        return Err(String::from(
            "absent-final final and staging leaves must be distinct",
        ));
    }
    if !prepared.staging.absent {
        return Err(String::from(
            "absent-final staging locator must retain its absent preparation observation",
        ));
    }
    if prepared.final_observation != AbsentFinalObservation::ObservedAbsent {
        return Err(String::from(
            "absent-final preparation observation is not qualified as absent",
        ));
    }
    if !matches!(
        prepared.copy_validated_evidence,
        PreparedFileEvidence::ContentHash(_)
    ) {
        return Err(String::from(
            "absent-final preparation requires exact CopyValidated content evidence",
        ));
    }
    Ok(())
}

fn validate_absent_final_recovery_observation_shape(
    observation: &AbsentFinalRecoveryObservation,
) -> Result<(), String> {
    if observation.target_parent_stable_id.is_empty() {
        return Err(String::from(
            "absent-final recovery observation target-parent identity is empty",
        ));
    }
    if observation.final_stable_id.is_empty() {
        return Err(String::from(
            "absent-final recovery observation final identity is empty",
        ));
    }
    if !matches!(
        observation.final_content,
        PreparedFileEvidence::ContentHash(_)
    ) {
        return Err(String::from(
            "absent-final recovery observation requires exact final content hash evidence",
        ));
    }
    Ok(())
}

fn validate_absent_final_recovery_observation_record(
    record: &OperationRecord,
) -> Result<(), String> {
    let Some(observation) = record.absent_final_recovery_observation.as_ref() else {
        return Ok(());
    };
    validate_absent_final_recovery_observation_shape(observation)?;
    if record.phase != OperationPhase::FilesystemStaged {
        return Err(String::from(
            "absent-final recovery observation requires the filesystem-staged phase",
        ));
    }
    let Some(PreparedTargetContract::AbsentFinalNoReplace(prepared)) = record.prepared.as_ref()
    else {
        return Err(String::from(
            "absent-final recovery observation requires absent-final preparation evidence",
        ));
    };
    let Some(staged) = record.staged.as_ref() else {
        return Err(String::from(
            "absent-final recovery observation requires staged evidence",
        ));
    };
    validate_absent_final_staged_checkpoint(prepared, staged)?;
    if observation.target_parent_stable_id != prepared.target_parent.identity.stable_id {
        return Err(String::from(
            "absent-final recovery observation target-parent identity does not match preparation",
        ));
    }
    let FilesystemStagedParticipant::CopyValidated { staging, evidence } = &staged.participant;
    if observation.final_stable_id != staging.identity.stable_id
        || observation.final_len != staging.identity.len
    {
        return Err(String::from(
            "absent-final recovery observation final identity does not match staged evidence",
        ));
    }
    let (
        PreparedFileEvidence::ContentHash(staged_hash),
        PreparedFileEvidence::ContentHash(observed_hash),
    ) = (evidence, &observation.final_content)
    else {
        return Err(String::from(
            "absent-final recovery observation content is not an exact hash",
        ));
    };
    if staged_hash != observed_hash {
        return Err(String::from(
            "absent-final recovery observation content does not match staged evidence",
        ));
    }
    Ok(())
}

fn validate_absent_final_transaction_owned_proof_shape(
    proof: &AbsentFinalTransactionOwnedProof,
) -> Result<(), String> {
    if proof.target_parent_stable_id.is_empty() {
        return Err(String::from(
            "absent-final transaction-owned proof target-parent identity is empty",
        ));
    }
    if proof.final_stable_id.is_empty() {
        return Err(String::from(
            "absent-final transaction-owned proof final identity is empty",
        ));
    }
    if !matches!(proof.final_content, PreparedFileEvidence::ContentHash(_)) {
        return Err(String::from(
            "absent-final transaction-owned proof requires exact final content hash evidence",
        ));
    }
    Ok(())
}

fn validate_absent_final_transaction_owned_proof_record(
    record: &OperationRecord,
) -> Result<(), String> {
    let Some(proof) = record.absent_final_transaction_owned_proof.as_ref() else {
        return Ok(());
    };
    validate_absent_final_transaction_owned_proof_shape(proof)?;
    if record.phase != OperationPhase::FilesystemStaged {
        return Err(String::from(
            "absent-final transaction-owned proof requires the filesystem-staged phase",
        ));
    }
    let Some(recovery_observation) = record.absent_final_recovery_observation.as_ref() else {
        return Err(String::from(
            "absent-final transaction-owned proof requires a recovery observation",
        ));
    };
    validate_absent_final_recovery_observation_record(record)?;
    if proof.target_parent_stable_id != recovery_observation.target_parent_stable_id
        || proof.final_stable_id != recovery_observation.final_stable_id
        || proof.final_len != recovery_observation.final_len
        || proof.final_content != recovery_observation.final_content
    {
        return Err(String::from(
            "absent-final transaction-owned proof does not match recovery observation",
        ));
    }
    let Some(PreparedTargetContract::AbsentFinalNoReplace(prepared)) = record.prepared.as_ref()
    else {
        return Err(String::from(
            "absent-final transaction-owned proof requires absent-final preparation evidence",
        ));
    };
    let Some(staged) = record.staged.as_ref() else {
        return Err(String::from(
            "absent-final transaction-owned proof requires staged evidence",
        ));
    };
    validate_absent_final_staged_checkpoint(prepared, staged)?;
    let FilesystemStagedParticipant::CopyValidated { staging, evidence } = &staged.participant;
    if proof.target_parent_stable_id != prepared.target_parent.identity.stable_id
        || proof.final_stable_id != staging.identity.stable_id
        || proof.final_len != staging.identity.len
    {
        return Err(String::from(
            "absent-final transaction-owned proof does not match prepared or staged identity",
        ));
    }
    let (
        PreparedFileEvidence::ContentHash(staged_hash),
        PreparedFileEvidence::ContentHash(proof_hash),
    ) = (evidence, &proof.final_content)
    else {
        return Err(String::from(
            "absent-final transaction-owned proof content is not an exact hash",
        ));
    };
    if staged_hash != proof_hash {
        return Err(String::from(
            "absent-final transaction-owned proof content does not match staged evidence",
        ));
    }
    Ok(())
}

/// Schema-v2 retains a cumulative evidence prefix for every non-terminal phase.  A
/// pre-publication cancellation may stop at any prefix, while every other terminal disposition
/// requires the complete publication evidence boundary.
fn schema_v2_phase_evidence_is_valid(
    phase: OperationPhase,
    disposition: OperationDisposition,
    evidence: SchemaV2EvidencePresence,
) -> bool {
    match phase {
        OperationPhase::IntentDurable => evidence == SchemaV2EvidencePresence::NONE,
        OperationPhase::Prepared => evidence == SchemaV2EvidencePresence::PREPARED,
        OperationPhase::FilesystemStaged => matches!(
            evidence,
            SchemaV2EvidencePresence::PREPARED_STAGED
                | SchemaV2EvidencePresence::PREPARED_STAGED_WITH_ABSENT_FINAL_RECOVERY_OBSERVATION
                | SchemaV2EvidencePresence::PREPARED_STAGED_WITH_ABSENT_FINAL_RECOVERY_PROOF
        ),
        OperationPhase::FilesystemPublished
        | OperationPhase::SourceReconciled
        | OperationPhase::GlobalReconciled
        | OperationPhase::ProjectionPublished
        | OperationPhase::ReadinessScheduled => evidence == SchemaV2EvidencePresence::ALL,
        OperationPhase::Terminal if disposition == OperationDisposition::CancelledBeforePublish => {
            matches!(
                evidence,
                SchemaV2EvidencePresence::NONE
                    | SchemaV2EvidencePresence::PREPARED
                    | SchemaV2EvidencePresence::PREPARED_STAGED
            )
        }
        OperationPhase::Terminal => evidence == SchemaV2EvidencePresence::ALL,
    }
}

fn validate_schema_v2_phase_evidence_record(record: &OperationRecord) -> Result<(), String> {
    if record.schema_version != SCHEMA_V2 {
        return Ok(());
    }
    let evidence = SchemaV2EvidencePresence::from_record(record);
    if !schema_v2_phase_evidence_is_valid(record.phase, record.disposition, evidence) {
        return Err(format!(
            "schema-v2 phase/evidence combination is invalid: phase={:?}, disposition={:?}, evidence={evidence:?}",
            record.phase, record.disposition,
        ));
    }
    validate_absent_final_recovery_observation_record(record)?;
    validate_absent_final_transaction_owned_proof_record(record)
}

/// Summary of a startup scan. Scanning never mutates or deletes records.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct RecoverySummary {
    /// Number of records found in the journal directory.
    pub(crate) record_count: usize,
    /// Number of typed records that are not terminal.
    pub(crate) unresolved_count: usize,
    /// Number of malformed records retained verbatim.
    pub(crate) malformed_count: usize,
    /// Number of records using a newer schema retained verbatim.
    pub(crate) unknown_version_count: usize,
    /// Number of records larger than the bounded scan budget.
    pub(crate) oversize_count: usize,
    /// Whether a user-visible recovery/attention state is required.
    pub(crate) attention_required: bool,
}

/// Errors from ownership, durable writes, or fail-closed recovery scanning.
#[derive(Debug, thiserror::Error)]
pub(crate) enum JournalError {
    /// The profile-local journal is owned by another process.
    #[error("operation journal is owned by another process: {path}")]
    OwnedByAnotherProcess { path: PathBuf },
    /// A durable operation record could not be written or synchronized.
    #[error("operation journal write failed at {path}: {source}")]
    Write { path: PathBuf, source: io::Error },
    /// A journal record could not be decoded.
    #[error("operation journal scan failed at {path}: {source}")]
    Scan { path: PathBuf, source: io::Error },
    /// Resolving the profile-local app directory failed.
    #[error("operation journal directory unavailable: {0}")]
    AppDirectory(String),
    /// A requested update did not refer to an admitted operation.
    #[error("operation journal record not found: {0}")]
    NotFound(Uuid),
    /// A legacy record remains visible for recovery but cannot be rewritten without losing its
    /// attention-required evidence.
    #[error("operation journal record is retained as non-writable recovery evidence: {0}")]
    NotWritable(Uuid),
    /// A phase transition did not follow the bounded journal state machine.
    #[error("illegal operation journal transition for {operation_id}: {from:?} -> {to:?}")]
    IllegalTransition {
        operation_id: Uuid,
        from: OperationPhase,
        to: OperationPhase,
    },
    /// Preparation was requested without a typed descriptor.
    #[error("prepared operation {0} is missing typed evidence")]
    MissingPreparedEvidence(Uuid),
    /// Publication evidence did not prove the complete guarded boundary.
    #[error("invalid publication evidence for operation {operation_id}: {reason}")]
    InvalidPublicationEvidence { operation_id: Uuid, reason: String },
    /// Recovery observation did not match the live or durable absent-final contract.
    #[error("invalid absent-final recovery observation for operation {operation_id}: {reason}")]
    InvalidRecoveryObservation { operation_id: Uuid, reason: String },
}

/// Result boundary for the bounded pre-intent capacity gate.
///
/// A rejected request has no durable operation record and must remain distinct from a journal
/// failure encountered while persisting an already-admitted intent.
#[derive(Debug, thiserror::Error)]
pub(crate) enum BoundedAdmissionError {
    #[error(transparent)]
    Rejected(#[from] RejectedBeforeIntent),
    #[error(transparent)]
    Journal(#[from] JournalError),
}

/// Outcome after an admitted restore was freshly prepared on the journal owner thread.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum PreparedOperationOutcome {
    Prepared(Uuid),
    RetryPending { operation_id: Uuid, reason: String },
    JournalWriteFailed { operation_id: Uuid, reason: String },
}

/// Result after the prepared restore has attempted destination-local staging.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum FilesystemStageOutcome {
    FilesystemStaged(Uuid),
    FilesystemPublished(Uuid),
    PlatformQualificationRequired {
        operation_id: Uuid,
        assessment: ReplacementQualificationAssessment,
    },
    RetryPending {
        operation_id: Uuid,
        reason: String,
    },
    AuditRequired {
        operation_id: Uuid,
        reason: String,
    },
    JournalWriteFailed {
        operation_id: Uuid,
        reason: String,
    },
}

#[derive(Debug)]
enum RetainedRecord {
    Malformed {
        path: PathBuf,
        bytes: Vec<u8>,
    },
    UnknownVersion {
        path: PathBuf,
        bytes: Vec<u8>,
        version: u32,
    },
    Oversize {
        path: PathBuf,
        bytes: u64,
    },
}

/// The profile-owned journal store. Holding this value holds exclusive ownership.
pub(crate) struct OperationJournalStore {
    directory: PathBuf,
    _ownership: OwnershipLock,
    records: BTreeMap<Uuid, OperationRecord>,
    non_writable: BTreeSet<Uuid>,
    retained: Vec<RetainedRecord>,
    recovery: RecoverySummary,
    capacity_claims: BTreeMap<VolumeIdentity, u64>,
    capacity_blocked: bool,
}

impl OperationJournalStore {
    /// Open and scan a profile-local journal, acquiring exclusive ownership first.
    pub(crate) fn open(directory: PathBuf) -> Result<Self, JournalError> {
        fs::create_dir_all(&directory).map_err(|source| JournalError::Write {
            path: directory.clone(),
            source,
        })?;
        let ownership = OwnershipLock::acquire(&directory)?;
        let mut store = Self {
            directory,
            _ownership: ownership,
            records: BTreeMap::new(),
            non_writable: BTreeSet::new(),
            retained: Vec::new(),
            recovery: RecoverySummary::default(),
            capacity_claims: BTreeMap::new(),
            capacity_blocked: false,
        };
        store.scan()?;
        Ok(store)
    }

    /// Return a read-only summary collected during open. No recovery mutation is performed.
    pub(crate) fn recovery_summary(&self) -> RecoverySummary {
        self.recovery.clone()
    }

    /// Return a typed record currently retained by the store.
    pub(crate) fn record(&self, operation_id: Uuid) -> Option<&OperationRecord> {
        self.records.get(&operation_id)
    }

    fn ensure_writable(&self, operation_id: Uuid) -> Result<(), JournalError> {
        if self.non_writable.contains(&operation_id) {
            Err(JournalError::NotWritable(operation_id))
        } else {
            Ok(())
        }
    }

    /// Return all typed records in deterministic operation-id order.
    pub(crate) fn records(&self) -> impl Iterator<Item = &OperationRecord> {
        self.records.values()
    }

    pub(crate) fn capacity_claims(&self) -> &BTreeMap<VolumeIdentity, u64> {
        &self.capacity_claims
    }

    pub(crate) fn capacity_blocked(&self) -> bool {
        self.capacity_blocked
    }

    /// Durably admit one record, before any future filesystem mutation.
    #[cfg(test)]
    pub(crate) fn admit(&mut self, record: OperationRecord) -> Result<(), JournalError> {
        if self.capacity_blocked {
            return Err(JournalError::Write {
                path: self.record_path(record.operation_id),
                source: io::Error::new(
                    io::ErrorKind::InvalidData,
                    RejectedBeforeIntent::RecoveryBlocked.to_string(),
                ),
            });
        }
        if !is_supported_schema_version(record.schema_version) {
            return Err(JournalError::Write {
                path: self.record_path(record.operation_id),
                source: io::Error::new(io::ErrorKind::InvalidInput, "unsupported schema version"),
            });
        }
        let path = self.record_path(record.operation_id);
        atomic_durable_write(&path, &record)?;
        self.records.insert(record.operation_id, record);
        self.rebuild_capacity_claims();
        Ok(())
    }

    pub(crate) fn admit_capacity(&mut self, record: OperationRecord) -> Result<(), JournalError> {
        if self.capacity_blocked {
            return Err(JournalError::Write {
                path: self.record_path(record.operation_id),
                source: io::Error::new(
                    io::ErrorKind::InvalidData,
                    RejectedBeforeIntent::RecoveryBlocked.to_string(),
                ),
            });
        }
        let Some(plan) = record.capacity_plan.as_ref() else {
            return Err(JournalError::Write {
                path: self.record_path(record.operation_id),
                source: io::Error::new(io::ErrorKind::InvalidInput, "capacity plan is required"),
            });
        };
        validate_capacity_plan(plan).map_err(|error| JournalError::Write {
            path: self.record_path(record.operation_id),
            source: io::Error::new(io::ErrorKind::InvalidInput, error.to_string()),
        })?;
        let path = self.record_path(record.operation_id);
        atomic_durable_write(&path, &record)?;
        self.records.insert(record.operation_id, record);
        self.rebuild_capacity_claims();
        Ok(())
    }

    /// Durably replace one existing record as an atomic update.
    pub(crate) fn update(
        &mut self,
        operation_id: Uuid,
        phase: OperationPhase,
        disposition: OperationDisposition,
    ) -> Result<(), JournalError> {
        self.update_with_optional_replacement_qualification(operation_id, phase, disposition, None)
    }

    /// Durably replace one staged record with its latest platform qualification assessment.
    fn update_with_replacement_qualification(
        &mut self,
        operation_id: Uuid,
        assessment: ReplacementQualificationAssessment,
    ) -> Result<(), JournalError> {
        self.update_with_optional_replacement_qualification(
            operation_id,
            OperationPhase::FilesystemStaged,
            OperationDisposition::RetryPending,
            Some(assessment),
        )
    }

    fn update_with_optional_replacement_qualification(
        &mut self,
        operation_id: Uuid,
        phase: OperationPhase,
        disposition: OperationDisposition,
        replacement_qualification: Option<ReplacementQualificationAssessment>,
    ) -> Result<(), JournalError> {
        if replacement_qualification.is_some() && phase != OperationPhase::FilesystemStaged {
            return Err(JournalError::InvalidPublicationEvidence {
                operation_id,
                reason: String::from(
                    "replacement qualification evidence requires the filesystem-staged phase",
                ),
            });
        }
        let current = self
            .records
            .get(&operation_id)
            .ok_or(JournalError::NotFound(operation_id))?;
        if phase == OperationPhase::Terminal
            && disposition == OperationDisposition::CancelledBeforePublish
            && (current.phase.is_post_publication() || current.published.is_some())
        {
            return Err(JournalError::InvalidPublicationEvidence {
                operation_id,
                reason: String::from("pre-publication cancellation is invalid after publication"),
            });
        }
        if phase == OperationPhase::FilesystemPublished {
            return Err(JournalError::IllegalTransition {
                operation_id,
                from: current.phase,
                to: phase,
            });
        }
        if current.phase.is_pre_publication() && current.published.is_some() {
            return Err(JournalError::InvalidPublicationEvidence {
                operation_id,
                reason: String::from("pre-publication phase carries publication evidence"),
            });
        }
        if phase.is_pre_publication() && current.published.is_some() {
            return Err(JournalError::InvalidPublicationEvidence {
                operation_id,
                reason: String::from(
                    "publication evidence cannot survive in a pre-publication phase",
                ),
            });
        }
        if phase == OperationPhase::Terminal
            && disposition == OperationDisposition::CancelledBeforePublish
            && current.published.is_some()
        {
            return Err(JournalError::InvalidPublicationEvidence {
                operation_id,
                reason: String::from(
                    "pre-publication cancellation cannot carry publication evidence",
                ),
            });
        }
        if phase.is_post_publication() {
            validate_record_publication(current).map_err(|reason| {
                JournalError::InvalidPublicationEvidence {
                    operation_id,
                    reason,
                }
            })?;
        } else if phase == OperationPhase::Terminal
            && disposition != OperationDisposition::CancelledBeforePublish
        {
            validate_record_publication(current).map_err(|reason| {
                JournalError::InvalidPublicationEvidence {
                    operation_id,
                    reason,
                }
            })?;
        }
        self.ensure_writable(operation_id)?;
        let mut updated = current.with_update(phase, disposition);
        if let Some(assessment) = replacement_qualification {
            if current.phase == phase
                && current.disposition == disposition
                && current.replacement_qualification.as_ref() == Some(&assessment)
            {
                return Ok(());
            }
            updated.replacement_qualification = Some(assessment);
        }
        let path = self.record_path(operation_id);
        atomic_durable_write(&path, &updated)?;
        self.records.insert(operation_id, updated);
        self.rebuild_capacity_claims();
        Ok(())
    }

    fn guarded_prepare(
        &mut self,
        operation_id: Uuid,
        prepared: PreparedWaveformRestore,
    ) -> Result<(), JournalError> {
        let current = self
            .records
            .get(&operation_id)
            .ok_or(JournalError::NotFound(operation_id))?;
        match current.phase {
            OperationPhase::IntentDurable => {
                self.ensure_writable(operation_id)?;
                let updated = current
                    .with_prepared(PreparedTargetContract::ExistingExpectedIdentity(prepared));
                let path = self.record_path(operation_id);
                atomic_durable_write(&path, &updated)?;
                self.records.insert(operation_id, updated);
                self.rebuild_capacity_claims();
                Ok(())
            }
            OperationPhase::Prepared => {
                if current.prepared.is_none() {
                    return Err(JournalError::MissingPreparedEvidence(operation_id));
                }
                // The descriptor was freshly validated by the caller. Replacing it is
                // unnecessary and would make a retry non-idempotent at the byte level.
                Ok(())
            }
            phase => Err(JournalError::IllegalTransition {
                operation_id,
                from: phase,
                to: OperationPhase::Prepared,
            }),
        }
    }

    fn guarded_stage(
        &mut self,
        operation_id: Uuid,
        staged: FilesystemStagedWaveformRestore,
    ) -> Result<(), JournalError> {
        let current = self
            .records
            .get(&operation_id)
            .ok_or(JournalError::NotFound(operation_id))?;
        match current.phase {
            OperationPhase::Prepared => {
                let prepared = current
                    .prepared
                    .as_ref()
                    .ok_or(JournalError::MissingPreparedEvidence(operation_id))?;
                validate_staged_checkpoint(prepared, &staged).map_err(|reason| {
                    JournalError::Write {
                        path: self.record_path(operation_id),
                        source: io::Error::new(io::ErrorKind::InvalidData, reason),
                    }
                })?;
                self.ensure_writable(operation_id)?;
                let updated = current.with_staged(staged);
                let path = self.record_path(operation_id);
                atomic_durable_write(&path, &updated)?;
                self.records.insert(operation_id, updated);
                self.rebuild_capacity_claims();
                Ok(())
            }
            OperationPhase::FilesystemStaged => {
                let Some(existing) = current.staged.as_ref() else {
                    return Err(JournalError::MissingPreparedEvidence(operation_id));
                };
                if existing == &staged {
                    Ok(())
                } else {
                    Err(JournalError::IllegalTransition {
                        operation_id,
                        from: current.phase,
                        to: OperationPhase::FilesystemStaged,
                    })
                }
            }
            phase => Err(JournalError::IllegalTransition {
                operation_id,
                from: phase,
                to: OperationPhase::FilesystemStaged,
            }),
        }
    }

    fn record_absent_final_recovery_observation(
        &mut self,
        operation_id: Uuid,
        observation: AbsentFinalRecoveryObservation,
    ) -> Result<(), JournalError> {
        let current = self
            .records
            .get(&operation_id)
            .ok_or(JournalError::NotFound(operation_id))?;
        if current.phase != OperationPhase::FilesystemStaged {
            return Err(JournalError::InvalidRecoveryObservation {
                operation_id,
                reason: String::from(
                    "absent-final recovery observation requires the filesystem-staged phase",
                ),
            });
        }
        if let Some(existing) = current.absent_final_recovery_observation.as_ref() {
            if existing == &observation {
                return Ok(());
            }
            return Err(JournalError::InvalidRecoveryObservation {
                operation_id,
                reason: String::from("conflicting absent-final recovery observation replay"),
            });
        }
        let updated = current.with_absent_final_recovery_observation(observation);
        validate_schema_v2_phase_evidence_record(&updated).map_err(|reason| {
            JournalError::InvalidRecoveryObservation {
                operation_id,
                reason,
            }
        })?;
        self.ensure_writable(operation_id)?;
        let path = self.record_path(operation_id);
        atomic_durable_write(&path, &updated)?;
        self.records.insert(operation_id, updated);
        self.rebuild_capacity_claims();
        Ok(())
    }

    fn record_absent_final_transaction_owned_proof(
        &mut self,
        operation_id: Uuid,
        proof: AbsentFinalTransactionOwnedProof,
    ) -> Result<(), JournalError> {
        let current = self
            .records
            .get(&operation_id)
            .ok_or(JournalError::NotFound(operation_id))?;
        if current.phase != OperationPhase::FilesystemStaged {
            return Err(JournalError::InvalidRecoveryObservation {
                operation_id,
                reason: String::from(
                    "absent-final transaction-owned proof requires the filesystem-staged phase",
                ),
            });
        }
        if let Some(existing) = current.absent_final_transaction_owned_proof.as_ref() {
            if existing == &proof {
                return Ok(());
            }
            return Err(JournalError::InvalidRecoveryObservation {
                operation_id,
                reason: String::from("conflicting absent-final transaction-owned proof replay"),
            });
        }
        let updated = current.with_absent_final_transaction_owned_proof(proof);
        validate_schema_v2_phase_evidence_record(&updated).map_err(|reason| {
            JournalError::InvalidRecoveryObservation {
                operation_id,
                reason,
            }
        })?;
        self.ensure_writable(operation_id)?;
        let path = self.record_path(operation_id);
        atomic_durable_write(&path, &updated)?;
        self.records.insert(operation_id, updated);
        self.rebuild_capacity_claims();
        Ok(())
    }

    pub(crate) fn guarded_publish(
        &mut self,
        operation_id: Uuid,
        published: FilesystemPublishedWaveformRestore,
    ) -> Result<(), JournalError> {
        let current = self
            .records
            .get(&operation_id)
            .ok_or(JournalError::NotFound(operation_id))?;
        match current.phase {
            OperationPhase::FilesystemStaged => {
                let prepared = current
                    .prepared
                    .as_ref()
                    .ok_or(JournalError::MissingPreparedEvidence(operation_id))?;
                let staged = current
                    .staged
                    .as_ref()
                    .ok_or(JournalError::MissingPreparedEvidence(operation_id))?;
                validate_staged_checkpoint(prepared, staged).map_err(|reason| {
                    JournalError::InvalidPublicationEvidence {
                        operation_id,
                        reason,
                    }
                })?;
                let publication_result = match prepared {
                    PreparedTargetContract::ExistingExpectedIdentity(prepared) => {
                        validate_publication_evidence(prepared, staged, &published)
                    }
                    PreparedTargetContract::AbsentFinalNoReplace(_) => {
                        validate_absent_final_no_replace_publication(
                            current
                                .prepared
                                .as_ref()
                                .expect("prepared evidence present"),
                            staged,
                            &published,
                        )
                    }
                };
                publication_result.map_err(|reason| JournalError::InvalidPublicationEvidence {
                    operation_id,
                    reason,
                })?;
                self.ensure_writable(operation_id)?;
                let updated = current.with_published(published);
                let path = self.record_path(operation_id);
                atomic_durable_write(&path, &updated)?;
                self.records.insert(operation_id, updated);
                self.rebuild_capacity_claims();
                Ok(())
            }
            OperationPhase::FilesystemPublished => {
                let existing = current
                    .published
                    .as_ref()
                    .ok_or(JournalError::MissingPreparedEvidence(operation_id))?;
                if existing == &published {
                    Ok(())
                } else {
                    Err(JournalError::InvalidPublicationEvidence {
                        operation_id,
                        reason: String::from("conflicting publication evidence replay"),
                    })
                }
            }
            phase => Err(JournalError::IllegalTransition {
                operation_id,
                from: phase,
                to: OperationPhase::FilesystemPublished,
            }),
        }
    }

    fn rebuild_capacity_claims(&mut self) {
        self.capacity_claims.clear();
        self.capacity_blocked = self.recovery.malformed_count > 0
            || self.recovery.unknown_version_count > 0
            || self.recovery.oversize_count > 0;
        for record in self.records.values() {
            if record.phase == OperationPhase::Terminal && record.disposition.is_terminal() {
                continue;
            }
            let Some(plan) = record.capacity_plan.as_ref() else {
                self.capacity_blocked = true;
                continue;
            };
            if validate_capacity_plan(plan).is_err() {
                self.capacity_blocked = true;
                continue;
            }
            for volume in &plan.volumes {
                let current = self
                    .capacity_claims
                    .get(&volume.identity)
                    .copied()
                    .unwrap_or(0);
                match current.checked_add(volume.allocated_bytes) {
                    Some(total) => {
                        self.capacity_claims.insert(volume.identity.clone(), total);
                    }
                    None => self.capacity_blocked = true,
                }
            }
        }
    }

    fn record_path(&self, operation_id: Uuid) -> PathBuf {
        self.directory
            .join(format!("{operation_id}{RECORD_SUFFIX}"))
    }

    fn scan(&mut self) -> Result<(), JournalError> {
        let entries = fs::read_dir(&self.directory).map_err(|source| JournalError::Scan {
            path: self.directory.clone(),
            source,
        })?;
        for entry in entries {
            let entry = entry.map_err(|source| JournalError::Scan {
                path: self.directory.clone(),
                source,
            })?;
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
                continue;
            }
            self.recovery.record_count += 1;
            let mut bytes = Vec::with_capacity(MAX_RECORD_BYTES);
            File::open(&path)
                .map(|file| {
                    file.take((MAX_RECORD_BYTES as u64).saturating_add(1))
                        .read_to_end(&mut bytes)
                })
                .and_then(|result| result)
                .map_err(|source| JournalError::Scan {
                    path: path.clone(),
                    source,
                })?;
            if bytes.len() > MAX_RECORD_BYTES {
                self.recovery.oversize_count += 1;
                self.recovery.attention_required = true;
                self.retained.push(RetainedRecord::Oversize {
                    path,
                    bytes: bytes.len() as u64,
                });
                continue;
            }
            let value: Value = match serde_json::from_slice(&bytes) {
                Ok(value) => value,
                Err(_) => {
                    self.recovery.malformed_count += 1;
                    self.recovery.attention_required = true;
                    self.retained
                        .push(RetainedRecord::Malformed { path, bytes });
                    continue;
                }
            };
            let decoded = match dispatch_persisted_record(value) {
                Ok(decoded) => decoded,
                Err(()) => {
                    self.recovery.malformed_count += 1;
                    self.recovery.attention_required = true;
                    self.retained
                        .push(RetainedRecord::Malformed { path, bytes });
                    continue;
                }
            };
            let record: OperationRecord = match decoded {
                DecodedPersistedRecord::UnknownVersion(version) => {
                    self.recovery.unknown_version_count += 1;
                    self.recovery.attention_required = true;
                    self.retained.push(RetainedRecord::UnknownVersion {
                        path,
                        bytes,
                        version,
                    });
                    continue;
                }
                DecodedPersistedRecord::V1(persisted) => persisted.into(),
                DecodedPersistedRecord::V2(persisted) => persisted.into(),
            };
            if validate_schema_v2_phase_evidence_record(&record).is_err() {
                self.recovery.malformed_count += 1;
                self.recovery.attention_required = true;
                self.retained
                    .push(RetainedRecord::Malformed { path, bytes });
                continue;
            }
            let filename_id = path
                .file_stem()
                .and_then(|stem| stem.to_str())
                .and_then(|stem| Uuid::parse_str(stem).ok());
            if filename_id != Some(record.operation_id)
                || self.records.contains_key(&record.operation_id)
            {
                self.recovery.malformed_count += 1;
                self.recovery.attention_required = true;
                self.retained
                    .push(RetainedRecord::Malformed { path, bytes });
                continue;
            }
            if validate_capacity_plan_option(record.capacity_plan.as_ref()).is_err() {
                self.recovery.malformed_count += 1;
                self.recovery.attention_required = true;
                self.retained
                    .push(RetainedRecord::Malformed { path, bytes });
                continue;
            }
            if record
                .prepared
                .as_ref()
                .is_some_and(|prepared| validate_prepared_contract(prepared).is_err())
            {
                self.recovery.malformed_count += 1;
                self.recovery.attention_required = true;
                self.retained
                    .push(RetainedRecord::Malformed { path, bytes });
                continue;
            }
            if record.schema_version == SCHEMA_V1
                && record
                    .published
                    .as_ref()
                    .is_some_and(is_absent_final_no_replace_publication)
            {
                // A v1 record containing an absent-final claim is not a v2 record in disguise.
                // Retain its original bytes and refuse to expose a writable runtime projection.
                self.recovery.malformed_count += 1;
                self.recovery.attention_required = true;
                self.retained
                    .push(RetainedRecord::Malformed { path, bytes });
                continue;
            }
            if record.phase == OperationPhase::Prepared && record.prepared.is_none() {
                self.recovery.malformed_count += 1;
                self.recovery.attention_required = true;
                self.retained
                    .push(RetainedRecord::Malformed { path, bytes });
                continue;
            }
            let staged_valid = match (record.phase, record.staged.as_ref()) {
                (OperationPhase::FilesystemStaged, Some(staged)) => record
                    .prepared
                    .as_ref()
                    .is_some_and(|prepared| validate_staged_checkpoint(prepared, staged).is_ok()),
                (OperationPhase::FilesystemStaged, None)
                | (OperationPhase::IntentDurable | OperationPhase::Prepared, Some(_)) => false,
                (_, Some(staged)) => record
                    .prepared
                    .as_ref()
                    .is_some_and(|prepared| validate_staged_checkpoint(prepared, staged).is_ok()),
                (_, None) => true,
            };
            if !staged_valid {
                self.recovery.malformed_count += 1;
                self.recovery.attention_required = true;
                self.retained
                    .push(RetainedRecord::Malformed { path, bytes });
                continue;
            }
            let legacy_post_publication_without_evidence = record.schema_version == SCHEMA_V1
                && record.published.is_none()
                && (matches!(
                    record.phase,
                    OperationPhase::FilesystemPublished
                        | OperationPhase::SourceReconciled
                        | OperationPhase::GlobalReconciled
                        | OperationPhase::ProjectionPublished
                        | OperationPhase::ReadinessScheduled
                ) || (record.phase == OperationPhase::Terminal
                    && record.disposition.is_terminal()));
            if legacy_post_publication_without_evidence {
                self.recovery.malformed_count += 1;
                self.recovery.attention_required = true;
                if record.phase != OperationPhase::Terminal {
                    self.recovery.unresolved_count += 1;
                }
                self.non_writable.insert(record.operation_id);
                self.retained.push(RetainedRecord::Malformed {
                    path: path.clone(),
                    bytes: bytes.clone(),
                });
                self.records.insert(record.operation_id, record);
                continue;
            }
            let published_valid = if record.phase.is_pre_publication() {
                record.published.is_none()
            } else if record.phase.is_post_publication() {
                validate_record_publication(&record).is_ok()
            } else {
                record
                    .published
                    .as_ref()
                    .map(|_| validate_record_publication(&record).is_ok())
                    .unwrap_or(true)
            };
            if !published_valid {
                self.recovery.malformed_count += 1;
                self.recovery.attention_required = true;
                self.retained
                    .push(RetainedRecord::Malformed { path, bytes });
                continue;
            }
            if record.phase != OperationPhase::Terminal || !record.disposition.is_terminal() {
                self.recovery.unresolved_count += 1;
                self.recovery.attention_required = true;
            }
            self.records.insert(record.operation_id, record);
        }
        self.rebuild_capacity_claims();
        Ok(())
    }
}

fn validate_capacity_plan_option(
    plan: Option<&DurableCapacityPlan>,
) -> Result<(), RejectedBeforeIntent> {
    if let Some(plan) = plan {
        validate_capacity_plan(plan)
    } else {
        Ok(())
    }
}

fn validate_capacity_plan(plan: &DurableCapacityPlan) -> Result<(), RejectedBeforeIntent> {
    if plan.volumes.is_empty() {
        return Err(RejectedBeforeIntent::InvalidPlan);
    }
    let mut identities = std::collections::BTreeSet::new();
    for volume in &plan.volumes {
        let expected_allocated = super::capacity_gate::round_up_allocation(
            volume.logical_bytes,
            volume.allocation_unit,
        )?;
        if !identities.insert(volume.identity.clone())
            || volume.allocated_bytes != expected_allocated
            || volume.protected_free_bytes != super::capacity_gate::PROTECTED_FREE_SPACE_FLOOR
        {
            return Err(RejectedBeforeIntent::InvalidPlan);
        }
    }
    Ok(())
}

/// Coordinator boundary for future durable history operations.
pub(crate) struct OperationJournalCoordinator {
    store: OperationJournalStore,
}

fn clean_relative_path(path: &Path) -> Result<(), String> {
    if path.as_os_str().is_empty() || path.is_absolute() {
        return Err(String::from("path must be a non-empty relative locator"));
    }
    for component in path.components() {
        if !matches!(component, std::path::Component::Normal(_)) {
            return Err(String::from("path contains non-normal components"));
        }
    }
    Ok(())
}

fn single_clean_normal_leaf<'a>(label: &str, path: &'a Path) -> Result<&'a Path, String> {
    clean_relative_path(path)?;
    let mut components = path.components();
    let Some(std::path::Component::Normal(component)) = components.next() else {
        return Err(format!("{label} locator is not a single clean normal leaf"));
    };
    if components.next().is_some() || Path::new(component) != path {
        return Err(format!("{label} locator is not a single clean normal leaf"));
    }
    Ok(path)
}

pub(super) fn descriptor_identity(file: &File) -> Result<PreparedObjectIdentity, String> {
    let metadata = file.metadata().map_err(|error| error.to_string())?;
    let stable_id =
        wavecrate_library::filesystem_identity::stable_filesystem_identity_from_open_file(file)
            .ok_or_else(|| String::from("stable filesystem identity unavailable"))?;
    let change_marker =
        wavecrate_library::filesystem_identity::filesystem_change_marker(Path::new(""), &metadata);
    Ok(PreparedObjectIdentity {
        stable_id,
        change_marker,
        len: metadata.len(),
    })
}

pub(super) fn open_root(path: &Path) -> Result<(File, PreparedRootCapability), String> {
    let file =
        super::capacity_gate::open_no_follow_path(path).map_err(|error| error.to_string())?;
    if !file.metadata().map_err(|error| error.to_string())?.is_dir() {
        return Err(format!("root is not a directory: {}", path.display()));
    }
    let identity = descriptor_identity(&file)?;
    Ok((
        file,
        PreparedRootCapability {
            path: path.to_path_buf(),
            identity,
        },
    ))
}

pub(super) fn open_leaf_relative(
    root: &File,
    relative: &Path,
    display: &Path,
) -> Result<(File, PreparedObjectIdentity), String> {
    clean_relative_path(relative)?;
    #[cfg(unix)]
    let file = {
        use std::ffi::CString;
        use std::os::fd::{AsRawFd, FromRawFd};
        let components = relative.components().collect::<Vec<_>>();
        let mut directory = root.try_clone().map_err(|error| error.to_string())?;
        for (index, component) in components.iter().enumerate() {
            let std::path::Component::Normal(component) = component else {
                return Err(String::from("locator contains non-normal component"));
            };
            let name = CString::new(component.as_encoded_bytes())
                .map_err(|_| String::from("locator contains NUL"))?;
            let is_leaf = index + 1 == components.len();
            let flags = libc::O_RDONLY
                | libc::O_CLOEXEC
                | libc::O_NOFOLLOW
                | if is_leaf { libc::O_NONBLOCK } else { 0 }
                | if is_leaf { 0 } else { libc::O_DIRECTORY };
            let fd = unsafe { libc::openat(directory.as_raw_fd(), name.as_ptr(), flags) };
            if fd < 0 {
                return Err(std::io::Error::last_os_error().to_string());
            }
            let next = unsafe { File::from_raw_fd(fd) };
            if is_leaf {
                directory = next;
            } else {
                directory = next;
            }
        }
        directory
    };
    #[cfg(not(unix))]
    let file = File::open(display).map_err(|error| error.to_string())?;
    let metadata = file.metadata().map_err(|error| error.to_string())?;
    if !metadata.is_file() {
        return Err(format!("leaf is not a regular file: {}", display.display()));
    }
    let identity = descriptor_identity(&file)?;
    Ok((file, identity))
}

#[cfg(unix)]
fn open_staging_relative(
    root: &File,
    relative: &Path,
    display: &Path,
) -> Result<(File, PreparedObjectIdentity), String> {
    open_leaf_relative(root, relative, display)
}

#[cfg(not(unix))]
fn open_staging_relative(
    root: &File,
    relative: &Path,
    display: &Path,
) -> Result<(File, PreparedObjectIdentity), String> {
    let _ = (root, relative, display);
    Err(String::from(
        "existing staging adoption is not verified on this platform",
    ))
}

fn verify_relative_absent(root: &File, relative: &Path, display: &Path) -> Result<(), String> {
    clean_relative_path(relative)?;
    let mut components = relative.components();
    let Some(std::path::Component::Normal(component)) = components.next() else {
        return Err(String::from("locator must contain one normal component"));
    };
    if components.next().is_some() {
        return Err(String::from("locator must be a destination-local leaf"));
    }

    #[cfg(unix)]
    {
        use std::ffi::CString;
        use std::os::fd::AsRawFd;

        let name = CString::new(component.as_encoded_bytes())
            .map_err(|_| String::from("locator contains NUL"))?;
        let mut metadata = std::mem::MaybeUninit::<libc::stat>::zeroed();
        let result = unsafe {
            libc::fstatat(
                root.as_raw_fd(),
                name.as_ptr(),
                metadata.as_mut_ptr(),
                libc::AT_SYMLINK_NOFOLLOW,
            )
        };
        if result == 0 {
            return Err(format!(
                "staging locator is occupied: {}",
                display.display()
            ));
        }
        let error = io::Error::last_os_error();
        if error.kind() == io::ErrorKind::NotFound {
            return Ok(());
        }
        return Err(format!(
            "could not verify staging locator absence at {}: {error}",
            display.display()
        ));
    }

    #[cfg(not(unix))]
    {
        let _ = (root, display);
        Err(String::from(
            "staging locator absence cannot be verified on this platform",
        ))
    }
}

fn create_relative_exclusive(root: &File, relative: &Path, display: &Path) -> Result<File, String> {
    clean_relative_path(relative)?;
    let mut components = relative.components();
    let Some(std::path::Component::Normal(component)) = components.next() else {
        return Err(String::from("locator must contain one normal component"));
    };
    if components.next().is_some() {
        return Err(String::from("locator must be a destination-local leaf"));
    }

    #[cfg(unix)]
    {
        use std::ffi::CString;
        use std::os::fd::{AsRawFd, FromRawFd};

        let name = CString::new(component.as_encoded_bytes())
            .map_err(|_| String::from("locator contains NUL"))?;
        let fd = unsafe {
            libc::openat(
                root.as_raw_fd(),
                name.as_ptr(),
                libc::O_RDWR | libc::O_CREAT | libc::O_EXCL | libc::O_CLOEXEC | libc::O_NOFOLLOW,
                0o600,
            )
        };
        if fd < 0 {
            return Err(format!(
                "could not create exclusive staging locator at {}: {}",
                display.display(),
                io::Error::last_os_error()
            ));
        }
        return Ok(unsafe { File::from_raw_fd(fd) });
    }

    #[cfg(not(unix))]
    {
        let _ = (root, display);
        Err(String::from(
            "exclusive no-follow staging is not verified on this platform",
        ))
    }
}

fn validate_identity(
    label: &str,
    expected: &PreparedObjectIdentity,
    actual: &PreparedObjectIdentity,
) -> Result<(), String> {
    if expected == actual {
        Ok(())
    } else {
        Err(format!("{label} identity changed since preparation"))
    }
}

fn validate_root_identity(
    label: &str,
    expected: &PreparedObjectIdentity,
    actual: &PreparedObjectIdentity,
) -> Result<(), String> {
    if expected.stable_id == actual.stable_id {
        Ok(())
    } else {
        Err(format!("{label} identity changed since preparation"))
    }
}

fn validate_volume_identity(
    label: &str,
    expected: &VolumeIdentity,
    actual: &VolumeIdentity,
) -> Result<(), String> {
    if expected == actual {
        Ok(())
    } else {
        Err(format!("{label} volume identity changed since staging"))
    }
}

fn validate_evidence(
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

fn validate_staged_checkpoint(
    prepared: &PreparedTargetContract,
    staged: &FilesystemStagedWaveformRestore,
) -> Result<(), String> {
    match prepared {
        PreparedTargetContract::ExistingExpectedIdentity(prepared) => {
            validate_existing_staged_checkpoint(prepared, staged)
        }
        PreparedTargetContract::AbsentFinalNoReplace(prepared) => {
            validate_absent_final_staged_checkpoint(prepared, staged)
        }
    }
}

fn validate_existing_staged_checkpoint(
    prepared: &PreparedWaveformRestore,
    staged: &FilesystemStagedWaveformRestore,
) -> Result<(), String> {
    let FilesystemStagedParticipant::CopyValidated { staging, evidence } = &staged.participant;
    if staging.relative_path != prepared.staging.relative_path {
        return Err(String::from(
            "staged locator does not match prepared staging locator",
        ));
    }
    clean_relative_path(&staging.relative_path)?;
    if staging.relative_path.components().count() != 1 {
        return Err(String::from(
            "staged locator is not a destination-local leaf",
        ));
    }
    if staging.identity.stable_id.is_empty() || staging.identity.len != prepared.backup.identity.len
    {
        return Err(String::from(
            "staged identity does not match prepared backup",
        ));
    }
    validate_staged_evidence(&prepared.evidence.backup, evidence)
}

fn validate_absent_final_staged_checkpoint(
    prepared: &PreparedAbsentFinalNoReplace,
    staged: &FilesystemStagedWaveformRestore,
) -> Result<(), String> {
    validate_prepared_contract(&PreparedTargetContract::AbsentFinalNoReplace(
        prepared.clone(),
    ))?;
    let FilesystemStagedParticipant::CopyValidated { staging, evidence } = &staged.participant;
    if staging.relative_path != prepared.staging.relative_path {
        return Err(String::from(
            "absent-final staged locator does not match prepared staging leaf",
        ));
    }
    if staging.identity.stable_id.is_empty() {
        return Err(String::from("absent-final staged identity is empty"));
    }
    let (PreparedFileEvidence::ContentHash(expected), PreparedFileEvidence::ContentHash(actual)) =
        (&prepared.copy_validated_evidence, evidence)
    else {
        return Err(String::from(
            "absent-final CopyValidated evidence must be an exact content hash",
        ));
    };
    if expected != actual {
        return Err(String::from(
            "absent-final CopyValidated evidence does not match preparation",
        ));
    }
    Ok(())
}

fn validate_staged_evidence(
    backup: &PreparedFileEvidence,
    staging: &PreparedFileEvidence,
) -> Result<(), String> {
    match (backup, staging) {
        (PreparedFileEvidence::ContentHash(backup), PreparedFileEvidence::ContentHash(staging))
            if backup == staging =>
        {
            Ok(())
        }
        (
            PreparedFileEvidence::Metadata {
                len: backup_len,
                is_dir: backup_is_dir,
                ..
            },
            PreparedFileEvidence::Metadata {
                len: staging_len,
                is_dir: staging_is_dir,
                ..
            },
        ) if backup_len == staging_len && backup_is_dir == staging_is_dir && !staging_is_dir => {
            Ok(())
        }
        (PreparedFileEvidence::Unverifiable, PreparedFileEvidence::Unverifiable) => Ok(()),
        _ => Err(String::from(
            "staged content evidence does not match backup",
        )),
    }
}

struct ReacquiredPreparedRestore {
    source_root: File,
    target_root: File,
    backup_root: File,
    target: File,
    backup: File,
}

struct ReacquiredStagedRestore {
    target_parent: File,
    target: File,
    staging: File,
    target_parent_identity: PreparedObjectIdentity,
    target_identity: PreparedObjectIdentity,
    staging_identity: PreparedObjectIdentity,
    staging_content: PreparedFileEvidence,
    volume: VolumeIdentity,
}

fn reacquire_prepared_restore(
    prepared: &PreparedWaveformRestore,
) -> Result<ReacquiredPreparedRestore, String> {
    let (source_root, source_capability) = open_root(&prepared.source_root.path)?;
    validate_identity(
        "source root",
        &prepared.source_root.identity,
        &source_capability.identity,
    )?;
    let (target_root, target_capability) = open_root(&prepared.target_root.path)?;
    validate_identity(
        "target root",
        &prepared.target_root.identity,
        &target_capability.identity,
    )?;
    let (backup_root, backup_capability) = open_root(&prepared.backup_root.path)?;
    validate_identity(
        "backup root",
        &prepared.backup_root.identity,
        &backup_capability.identity,
    )?;
    let target_display = prepared
        .target_root
        .path
        .join(&prepared.target.relative_path);
    let (target, target_identity) = open_leaf_relative(
        &target_root,
        &prepared.target.relative_path,
        &target_display,
    )?;
    validate_identity("target leaf", &prepared.target.identity, &target_identity)?;
    validate_evidence(
        "target leaf",
        &prepared.evidence.target,
        &prepared_file_evidence(&target),
    )?;
    let backup_display = prepared
        .backup_root
        .path
        .join(&prepared.backup.relative_path);
    let (backup, backup_identity) = open_leaf_relative(
        &backup_root,
        &prepared.backup.relative_path,
        &backup_display,
    )?;
    validate_identity("backup leaf", &prepared.backup.identity, &backup_identity)?;
    validate_evidence(
        "backup leaf",
        &prepared.evidence.backup,
        &prepared_file_evidence(&backup),
    )?;
    Ok(ReacquiredPreparedRestore {
        source_root,
        target_root,
        backup_root,
        target,
        backup,
    })
}

fn reacquire_staged_restore(
    prepared: &PreparedWaveformRestore,
    staged: &FilesystemStagedWaveformRestore,
    capacity_plan: &DurableCapacityPlan,
) -> Result<ReacquiredStagedRestore, String> {
    let FilesystemStagedParticipant::CopyValidated {
        staging: expected_staging,
        evidence: expected_staging_content,
    } = &staged.participant;
    validate_existing_staged_checkpoint(prepared, staged)?;
    let target_leaf = single_clean_normal_leaf("target", &prepared.target.relative_path)?;
    let staging_leaf = single_clean_normal_leaf("staging", &prepared.staging.relative_path)?;
    let [volume] = capacity_plan.volumes.as_slice() else {
        return Err(String::from("capacity claim has unexpected volumes"));
    };

    let (target_root, target_root_capability) = open_root(&prepared.target_root.path)?;
    validate_root_identity(
        "target root",
        &prepared.target_root.identity,
        &target_root_capability.identity,
    )?;
    let target_parent = target_root
        .try_clone()
        .map_err(|error| format!("could not retain target parent capability: {error}"))?;
    let target_display = prepared.target_root.path.join(target_leaf);
    let (target, target_identity) =
        open_leaf_relative(&target_parent, target_leaf, &target_display)?;
    validate_identity("target leaf", &prepared.target.identity, &target_identity)?;
    validate_evidence(
        "target leaf",
        &prepared.evidence.target,
        &prepared_file_evidence(&target),
    )?;
    let target_volume = super::capacity_gate::descriptor_capacity_facts(&target)
        .map_err(|error| error.to_string())?
        .identity;
    validate_volume_identity("target", &volume.identity, &target_volume)?;

    let staging_display = prepared.target_root.path.join(staging_leaf);
    let (staging, staging_identity) =
        open_staging_relative(&target_parent, staging_leaf, &staging_display)?;
    let staging_volume = super::capacity_gate::descriptor_capacity_facts(&staging)
        .map_err(|error| error.to_string())?
        .identity;
    validate_volume_identity("staging", &volume.identity, &staging_volume)?;
    validate_identity("staging", &expected_staging.identity, &staging_identity)?;
    let staging_content = prepared_file_evidence(&staging);
    validate_evidence("staging", expected_staging_content, &staging_content)?;
    validate_staged_evidence(&prepared.evidence.backup, &staging_content)?;

    Ok(ReacquiredStagedRestore {
        target_parent,
        target,
        staging,
        target_parent_identity: target_root_capability.identity,
        target_identity,
        staging_identity,
        staging_content,
        volume: volume.identity.clone(),
    })
}

fn copy_and_validate(
    backup: &File,
    staging: &mut File,
    prepared_backup_identity: &PreparedObjectIdentity,
    prepared_evidence: &PreparedFileEvidence,
) -> Result<(PreparedObjectIdentity, PreparedFileEvidence), String> {
    staging
        .seek(SeekFrom::Start(0))
        .map_err(|error| format!("could not seek staging entry: {error}"))?;
    let mut source = backup
        .try_clone()
        .map_err(|error| format!("could not clone backup descriptor: {error}"))?;
    source
        .seek(SeekFrom::Start(0))
        .map_err(|error| format!("could not seek backup descriptor: {error}"))?;
    io::copy(&mut source, staging)
        .map_err(|error| format!("could not copy backup into staging: {error}"))?;
    staging
        .sync_all()
        .map_err(|error| format!("could not flush staging entry: {error}"))?;

    let live_backup_identity = descriptor_identity(backup)?;
    validate_identity(
        "backup leaf",
        prepared_backup_identity,
        &live_backup_identity,
    )?;
    let live_backup_evidence = prepared_file_evidence(backup);
    validate_evidence("backup leaf", prepared_evidence, &live_backup_evidence)?;
    staging
        .seek(SeekFrom::Start(0))
        .map_err(|error| format!("could not rewind staging entry: {error}"))?;
    let staged_evidence = prepared_file_evidence(staging);
    validate_staged_evidence(&live_backup_evidence, &staged_evidence)?;
    let staged_identity = descriptor_identity(staging)?;
    if staged_identity.len != backup.metadata().map_err(|error| error.to_string())?.len() {
        return Err(String::from("staging length does not match backup"));
    }
    Ok((staged_identity, staged_evidence))
}

fn infer_root(path: &Path, relative: &Path) -> Result<PathBuf, String> {
    clean_relative_path(relative)?;
    let mut root = path.to_path_buf();
    for _ in relative.components() {
        root = root
            .parent()
            .ok_or_else(|| String::from("relative locator exceeds target path"))?
            .to_path_buf();
    }
    if !root.is_absolute() || root.join(relative) != path {
        return Err(String::from(
            "target path is not source-relative to its root",
        ));
    }
    Ok(root)
}

pub(super) fn prepared_file_evidence(file: &File) -> PreparedFileEvidence {
    let Ok(metadata) = file.metadata() else {
        return PreparedFileEvidence::Unverifiable;
    };
    if metadata.is_file()
        && metadata.len() <= wavecrate::sample_sources::MAX_SOURCE_FILE_EVIDENCE_HASH_BYTES
    {
        let mut clone = match file.try_clone() {
            Ok(clone) => clone,
            Err(_) => return PreparedFileEvidence::Unverifiable,
        };
        if clone.seek(SeekFrom::Start(0)).is_err() {
            return PreparedFileEvidence::Unverifiable;
        }
        let mut bytes = Vec::with_capacity(metadata.len() as usize);
        if clone.read_to_end(&mut bytes).is_ok() {
            return PreparedFileEvidence::ContentHash(*blake3::hash(&bytes).as_bytes());
        }
    }
    PreparedFileEvidence::Metadata {
        len: metadata.len(),
        modified_ns: metadata
            .modified()
            .ok()
            .map(wavecrate_library::timestamps::system_time_to_unix_nanos),
        is_dir: metadata.is_dir(),
    }
}

fn prepare_descriptor(
    operation_id: Uuid,
    direction: super::file_io::HistoryFileIoDirection,
    actions: &[super::file_io::HistoryFileAction],
    capacity_plan: &DurableCapacityPlan,
    existing_claims: &BTreeMap<VolumeIdentity, u64>,
) -> Result<PreparedWaveformRestore, String> {
    let admission = super::capacity_gate::map_waveform_restore_shape(direction, actions)
        .map_err(|error| error.to_string())?;
    let super::file_io::HistoryFileAction::WaveformRestore { applied, .. } = &actions[0] else {
        return Err(String::from("invalid waveform restore shape"));
    };
    let Some(volume) = capacity_plan.volumes.as_slice().first() else {
        return Err(String::from("capacity claim is empty"));
    };
    if capacity_plan.volumes.len() != 1 {
        return Err(String::from("capacity claim has unexpected volumes"));
    }
    let target_root_path = infer_root(&applied.absolute_path, &applied.relative_path)?;
    let backup_name = admission
        .backup_path
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|_| admission.backup_path.parent().is_some())
        .ok_or_else(|| String::from("backup locator is not a clean leaf"))?
        .to_owned();
    let backup_root_path = admission
        .backup_path
        .parent()
        .ok_or_else(|| String::from("backup has no root"))?
        .to_path_buf();
    let (_source_root_file, source_root) =
        open_root(&target_root_path).map_err(|error| format!("source root: {error}"))?;
    let (target_root_file, target_root) =
        open_root(&target_root_path).map_err(|error| format!("target root: {error}"))?;
    let staging = PathBuf::from(format!(".wavecrate-restore-{operation_id}.stage"));
    verify_relative_absent(
        &target_root_file,
        &staging,
        &target_root_path.join(&staging),
    )?;
    let (backup_root_file, backup_root) =
        open_root(&backup_root_path).map_err(|error| format!("backup root: {error}"))?;
    let (target_file, target_identity) = open_leaf_relative(
        &target_root_file,
        &applied.relative_path,
        &applied.absolute_path,
    )?;
    let backup_relative = PathBuf::from(&backup_name);
    let (backup_file, backup_identity) =
        open_leaf_relative(&backup_root_file, &backup_relative, &admission.backup_path)?;
    // This is the single post-intent capacity revalidation. It observes live facts from the
    // already-open, capability-relative descriptors and removes this operation's own claim.
    let requirement = super::capacity_gate::CapacityRequirement {
        facts: super::capacity_gate::descriptor_capacity_facts(&target_file)
            .map_err(|error| error.to_string())?,
        logical_bytes: backup_file
            .metadata()
            .map_err(|error| error.to_string())?
            .len(),
    };
    if requirement.facts.identity != volume.identity
        || requirement.facts.allocation_unit != volume.allocation_unit
        || requirement.logical_bytes != volume.logical_bytes
        || super::capacity_gate::round_up_allocation(
            requirement.logical_bytes,
            requirement.facts.allocation_unit,
        )
        .map_err(|error| error.to_string())?
            != volume.allocated_bytes
    {
        return Err(String::from("capacity claim no longer matches live facts"));
    }
    let mut claims_without_own = existing_claims.clone();
    match claims_without_own.get_mut(&volume.identity) {
        Some(claim) if *claim >= volume.allocated_bytes => {
            *claim -= volume.allocated_bytes;
            if *claim == 0 {
                claims_without_own.remove(&volume.identity);
            }
        }
        _ => return Err(String::from("capacity claim ownership changed")),
    }
    super::capacity_gate::aggregate_capacity_plan(&[requirement], &claims_without_own)
        .map_err(|error| error.to_string())?;
    let direction = match direction {
        super::file_io::HistoryFileIoDirection::Undo => PreparedRestoreDirection::Undo,
        super::file_io::HistoryFileIoDirection::Redo => PreparedRestoreDirection::Redo,
    };
    Ok(PreparedWaveformRestore {
        direction,
        source_id: applied.source_id.clone(),
        source_root,
        target_root,
        target: PreparedLeafLocator {
            relative_path: applied.relative_path.clone(),
            identity: target_identity.clone(),
        },
        backup_root,
        backup: PreparedLeafLocator {
            relative_path: PathBuf::from(backup_name),
            identity: backup_identity,
        },
        replacement: ReplaceExpectedIdentity::Existing(target_identity),
        staging: PreparedStagingLocator {
            relative_path: staging,
            absent: true,
        },
        evidence: PreparedRestoreEvidence {
            target: prepared_file_evidence(&target_file),
            backup: prepared_file_evidence(&backup_file),
        },
    })
}

impl OperationJournalCoordinator {
    /// Open the journal in the current profile and perform a non-mutating scan.
    pub(crate) fn open_current_profile() -> Result<Self, JournalError> {
        let directory = wavecrate::app_dirs::operation_journal_dir()
            .map_err(|error| JournalError::AppDirectory(error.to_string()))?;
        Self::open(directory)
    }

    /// Open a journal at an explicit directory (used by isolated tests).
    pub(crate) fn open(directory: PathBuf) -> Result<Self, JournalError> {
        Ok(Self {
            store: OperationJournalStore::open(directory)?,
        })
    }

    /// Return the startup scan summary without mutating the journal.
    pub(crate) fn recovery_summary(&self) -> RecoverySummary {
        self.store.recovery_summary()
    }

    /// Classify one eligible schema-v2 absent-final record by reacquiring and inspecting its
    /// target-parent capability. This method is observation-only: it never updates the record,
    /// writes journal bytes, changes disposition, or alters capacity claims.
    pub(crate) fn classify_schema_v2_absent_final_recovery(
        &self,
        operation_id: Uuid,
    ) -> Result<AbsentFinalRecoveryClassification, JournalError> {
        Ok(self
            .inspect_schema_v2_absent_final_recovery(operation_id)?
            .classification)
    }

    fn inspect_schema_v2_absent_final_recovery(
        &self,
        operation_id: Uuid,
    ) -> Result<super::absent_final_recovery::AbsentFinalRecoveryInspection, JournalError> {
        let record = self
            .store
            .record(operation_id)
            .ok_or(JournalError::NotFound(operation_id))?;
        if record.schema_version != SCHEMA_V2 {
            return Err(JournalError::InvalidPublicationEvidence {
                operation_id,
                reason: String::from("absent-final recovery requires schema-v2"),
            });
        }
        if record.phase != OperationPhase::FilesystemStaged {
            return Err(JournalError::InvalidPublicationEvidence {
                operation_id,
                reason: String::from("absent-final recovery requires the filesystem-staged phase"),
            });
        }
        validate_schema_v2_phase_evidence_record(record).map_err(|reason| {
            JournalError::InvalidPublicationEvidence {
                operation_id,
                reason,
            }
        })?;
        let prepared = record
            .prepared
            .as_ref()
            .ok_or(JournalError::MissingPreparedEvidence(operation_id))?;
        let PreparedTargetContract::AbsentFinalNoReplace(prepared) = prepared else {
            return Err(JournalError::InvalidPublicationEvidence {
                operation_id,
                reason: String::from(
                    "absent-final recovery requires absent-final preparation evidence",
                ),
            });
        };
        let staged = record
            .staged
            .as_ref()
            .ok_or(JournalError::MissingPreparedEvidence(operation_id))?;
        validate_prepared_contract(&PreparedTargetContract::AbsentFinalNoReplace(
            prepared.clone(),
        ))
        .map_err(|reason| JournalError::InvalidPublicationEvidence {
            operation_id,
            reason,
        })?;
        validate_absent_final_staged_checkpoint(prepared, staged).map_err(|reason| {
            JournalError::InvalidPublicationEvidence {
                operation_id,
                reason,
            }
        })?;
        Ok(inspect_absent_final_recovery(prepared, staged))
    }

    /// Re-inspect one eligible absent-final record and durably retain only the exact
    /// `StagingMissingFinalMatches` observation. This remains a filesystem-staged journal
    /// observation; it does not publish, adopt, claim ownership, or mutate the namespace.
    pub(crate) fn record_schema_v2_absent_final_recovery_observation(
        &mut self,
        operation_id: Uuid,
    ) -> Result<AbsentFinalRecoveryClassification, JournalError> {
        let inspection = self.inspect_schema_v2_absent_final_recovery(operation_id)?;
        let existing = self
            .store
            .record(operation_id)
            .and_then(|record| record.absent_final_recovery_observation.clone());
        match (inspection.classification, inspection.observation, existing) {
            (
                AbsentFinalRecoveryClassification::StagingMissingFinalMatches,
                Some(observation),
                None,
            ) => {
                self.store
                    .record_absent_final_recovery_observation(operation_id, observation)?;
                Ok(AbsentFinalRecoveryClassification::StagingMissingFinalMatches)
            }
            (
                AbsentFinalRecoveryClassification::StagingMissingFinalMatches,
                Some(observation),
                Some(existing),
            ) if observation == existing => {
                Ok(AbsentFinalRecoveryClassification::StagingMissingFinalMatches)
            }
            (AbsentFinalRecoveryClassification::StagingMissingFinalMatches, Some(_), Some(_)) => {
                Err(JournalError::InvalidRecoveryObservation {
                    operation_id,
                    reason: String::from("conflicting live and durable absent-final evidence"),
                })
            }
            (classification, None, None) => Ok(classification),
            (classification, Some(_), None) => Err(JournalError::InvalidRecoveryObservation {
                operation_id,
                reason: format!(
                    "classifier returned {classification:?} without a matching observation"
                ),
            }),
            (classification, _, Some(_)) => Err(JournalError::InvalidRecoveryObservation {
                operation_id,
                reason: format!(
                    "durable absent-final observation is stale for live classification {classification:?}"
                ),
            }),
        }
    }

    /// Re-inspect one eligible absent-final record and durably retain a distinct adoption proof
    /// only after the live reopened final exactly matches the durable recovery observation. The
    /// proof is evidence-only: it carries no pathname, capability, publication claim, or
    /// filesystem mutation authority, and the operation remains filesystem-staged.
    pub(crate) fn record_schema_v2_absent_final_transaction_owned_proof(
        &mut self,
        operation_id: Uuid,
    ) -> Result<AbsentFinalRecoveryClassification, JournalError> {
        let inspection = self.inspect_schema_v2_absent_final_recovery(operation_id)?;
        let AbsentFinalRecoveryClassification::StagingMissingFinalMatches =
            inspection.classification
        else {
            return Err(JournalError::InvalidRecoveryObservation {
                operation_id,
                reason: format!(
                    "absent-final transaction-owned proof requires StagingMissingFinalMatches, got {:?}",
                    inspection.classification
                ),
            });
        };
        let Some(live_observation) = inspection.observation else {
            return Err(JournalError::InvalidRecoveryObservation {
                operation_id,
                reason: String::from(
                    "absent-final transaction-owned proof requires a live recovery observation",
                ),
            });
        };
        let record = self
            .store
            .record(operation_id)
            .ok_or(JournalError::NotFound(operation_id))?;
        let Some(durable_observation) = record.absent_final_recovery_observation.as_ref() else {
            return Err(JournalError::InvalidRecoveryObservation {
                operation_id,
                reason: String::from(
                    "absent-final transaction-owned proof requires an existing recovery observation",
                ),
            });
        };
        if durable_observation != &live_observation {
            return Err(JournalError::InvalidRecoveryObservation {
                operation_id,
                reason: String::from(
                    "live absent-final recovery observation conflicts with durable observation",
                ),
            });
        }
        let proof = AbsentFinalTransactionOwnedProof {
            target_parent_stable_id: live_observation.target_parent_stable_id.clone(),
            final_stable_id: live_observation.final_stable_id.clone(),
            final_len: live_observation.final_len,
            final_content: live_observation.final_content.clone(),
        };
        if let Some(existing) = record.absent_final_transaction_owned_proof.as_ref() {
            if existing == &proof {
                return Ok(AbsentFinalRecoveryClassification::StagingMissingFinalMatches);
            }
            return Err(JournalError::InvalidRecoveryObservation {
                operation_id,
                reason: String::from(
                    "existing absent-final transaction-owned proof conflicts with live evidence",
                ),
            });
        }
        self.store
            .record_absent_final_transaction_owned_proof(operation_id, proof)?;
        Ok(AbsentFinalRecoveryClassification::StagingMissingFinalMatches)
    }

    /// Admit an intent durably and return its stable operation ID.
    #[cfg(test)]
    pub(crate) fn admit(
        &mut self,
        intent: OperationIntent,
        payload: Value,
    ) -> Result<Uuid, JournalError> {
        let record = OperationRecord::new(intent, payload);
        let operation_id = record.operation_id;
        self.store.admit(record)?;
        Ok(operation_id)
    }

    /// Explicit test-only constructor for the schema-v2 absent-final contract. Production
    /// waveform-restore admission remains on `OperationRecord::new`, which is schema-v1.
    #[cfg(test)]
    pub(super) fn admit_schema_v2_absent_final_for_test(
        &mut self,
        intent: OperationIntent,
        payload: Value,
        prepared: PreparedAbsentFinalNoReplace,
        staged: FilesystemStagedWaveformRestore,
        capacity_plan: DurableCapacityPlan,
    ) -> Result<Uuid, JournalError> {
        let record = OperationRecord::new_v2_absent_final_with_capacity_plan(
            intent,
            payload,
            prepared,
            staged,
            capacity_plan,
        );
        let operation_id = record.operation_id;
        let prepared = record
            .prepared
            .as_ref()
            .ok_or(JournalError::MissingPreparedEvidence(operation_id))?;
        validate_prepared_contract(prepared).map_err(|reason| JournalError::Write {
            path: self.store.record_path(operation_id),
            source: io::Error::new(io::ErrorKind::InvalidData, reason),
        })?;
        let staged = record
            .staged
            .as_ref()
            .ok_or(JournalError::MissingPreparedEvidence(operation_id))?;
        validate_staged_checkpoint(prepared, staged).map_err(|reason| JournalError::Write {
            path: self.store.record_path(operation_id),
            source: io::Error::new(io::ErrorKind::InvalidData, reason),
        })?;
        self.store.admit_capacity(record)?;
        Ok(operation_id)
    }

    #[cfg(test)]
    pub(super) fn record_path_for_test(&self, operation_id: Uuid) -> PathBuf {
        self.store.record_path(operation_id)
    }

    #[cfg(test)]
    pub(super) fn capacity_claims_for_test(&self) -> BTreeMap<VolumeIdentity, u64> {
        self.store.capacity_claims.clone()
    }

    /// Admit exactly one bounded waveform restore after owner-thread capacity discovery.
    pub(crate) fn admit_bounded_waveform_restore(
        &mut self,
        intent: OperationIntent,
        payload: Value,
        direction: super::file_io::HistoryFileIoDirection,
        actions: &[super::file_io::HistoryFileAction],
    ) -> Result<Uuid, BoundedAdmissionError> {
        if self.store.capacity_blocked() {
            return Err(RejectedBeforeIntent::RecoveryBlocked.into());
        }
        let (_admission, capacity_plan) = super::capacity_gate::plan_waveform_restore(
            direction,
            actions,
            self.store.capacity_claims(),
        )?;
        let record = OperationRecord::new_with_capacity_plan(intent, payload, Some(capacity_plan));
        let operation_id = record.operation_id;
        self.store
            .admit_capacity(record)
            .map_err(BoundedAdmissionError::Journal)?;
        Ok(operation_id)
    }

    /// Admit and then prepare one non-extracted waveform restore on the journal owner thread.
    /// Post-intent validation failures retain the operation and its capacity claim.
    pub(crate) fn prepare_bounded_waveform_restore(
        &mut self,
        intent: OperationIntent,
        payload: Value,
        direction: super::file_io::HistoryFileIoDirection,
        actions: &[super::file_io::HistoryFileAction],
    ) -> Result<PreparedOperationOutcome, BoundedAdmissionError> {
        let operation_id =
            self.admit_bounded_waveform_restore(intent, payload, direction, actions)?;
        self.prepare_admitted_bounded_waveform_restore(operation_id, direction, actions)
    }

    fn prepare_admitted_bounded_waveform_restore(
        &mut self,
        operation_id: Uuid,
        direction: super::file_io::HistoryFileIoDirection,
        actions: &[super::file_io::HistoryFileAction],
    ) -> Result<PreparedOperationOutcome, BoundedAdmissionError> {
        let capacity_plan = self
            .store
            .record(operation_id)
            .and_then(|record| record.capacity_plan.clone())
            .ok_or_else(|| {
                BoundedAdmissionError::Journal(JournalError::MissingPreparedEvidence(operation_id))
            })?;
        let prepared = match prepare_descriptor(
            operation_id,
            direction,
            actions,
            &capacity_plan,
            self.store.capacity_claims(),
        ) {
            Ok(prepared) => prepared,
            Err(reason) => {
                return match self.store.update(
                    operation_id,
                    OperationPhase::IntentDurable,
                    OperationDisposition::RetryPending,
                ) {
                    Ok(()) => Ok(PreparedOperationOutcome::RetryPending {
                        operation_id,
                        reason,
                    }),
                    Err(error) => Ok(PreparedOperationOutcome::JournalWriteFailed {
                        operation_id,
                        reason: error.to_string(),
                    }),
                };
            }
        };
        match self.store.guarded_prepare(operation_id, prepared) {
            Ok(()) => Ok(PreparedOperationOutcome::Prepared(operation_id)),
            Err(error) => Ok(PreparedOperationOutcome::JournalWriteFailed {
                operation_id,
                reason: error.to_string(),
            }),
        }
    }

    /// Copy one prepared restore into its destination-local staging leaf and durably record the
    /// `CopyValidated` participant checkpoint. This deliberately stops before final replacement.
    pub(crate) fn stage_admitted_bounded_waveform_restore(
        &mut self,
        operation_id: Uuid,
    ) -> Result<FilesystemStageOutcome, JournalError> {
        let record = self
            .store
            .record(operation_id)
            .ok_or(JournalError::NotFound(operation_id))?;
        if !matches!(
            record.phase,
            OperationPhase::Prepared | OperationPhase::FilesystemStaged
        ) {
            return Err(JournalError::IllegalTransition {
                operation_id,
                from: record.phase,
                to: OperationPhase::FilesystemStaged,
            });
        }
        let phase = record.phase;
        let durable_staged = if phase == OperationPhase::FilesystemStaged {
            Some(
                record
                    .staged
                    .clone()
                    .ok_or(JournalError::MissingPreparedEvidence(operation_id))?,
            )
        } else {
            None
        };
        let prepared = record
            .prepared
            .clone()
            .ok_or(JournalError::MissingPreparedEvidence(operation_id))?;
        let prepared = match prepared {
            PreparedTargetContract::ExistingExpectedIdentity(prepared) => prepared,
            PreparedTargetContract::AbsentFinalNoReplace(_) => {
                return self.stage_retry_or_audit(
                    operation_id,
                    phase,
                    String::from(
                        "schema-v2 absent-final preparation requires its explicit publication seam",
                    ),
                );
            }
        };

        #[cfg(not(unix))]
        {
            return self.stage_retry_or_audit(
                operation_id,
                phase,
                String::from(
                    "descriptor-relative no-follow staging is unavailable on this platform",
                ),
            );
        }

        let reacquired = match reacquire_prepared_restore(&prepared) {
            Ok(reacquired) => reacquired,
            Err(reason) => {
                return self.stage_retry_or_audit(operation_id, phase, reason);
            }
        };
        let staging_display = prepared
            .target_root
            .path
            .join(&prepared.staging.relative_path);

        // A staged entry may have survived a crash or a failed copy. It is never replaced. A
        // complete, content-verified entry may be adopted by recording CopyValidated; anything
        // else remains untouched and retryable/auditable.
        let existing = match open_staging_relative(
            &reacquired.target_root,
            &prepared.staging.relative_path,
            &staging_display,
        ) {
            Ok((staging, identity)) => {
                let evidence = prepared_file_evidence(&staging);
                if let Some(durable_staged) = durable_staged.as_ref() {
                    let FilesystemStagedParticipant::CopyValidated {
                        staging: expected_staging,
                        evidence: expected_evidence,
                    } = &durable_staged.participant;
                    if expected_staging.relative_path != prepared.staging.relative_path
                        || expected_staging.identity != identity
                        || validate_evidence("staging", expected_evidence, &evidence).is_err()
                    {
                        return self.stage_retry_or_audit(
                            operation_id,
                            phase,
                            format!(
                                "durable staging checkpoint does not match live entry at {staging_display:?}"
                            ),
                        );
                    }
                    return Ok(FilesystemStageOutcome::FilesystemStaged(operation_id));
                }
                if !matches!(
                    &prepared.evidence.backup,
                    PreparedFileEvidence::ContentHash(_)
                ) {
                    return self.stage_retry_or_audit(
                        operation_id,
                        phase,
                        format!(
                            "occupied staging cannot be adopted without exact backup content evidence at {staging_display:?}"
                        ),
                    );
                }
                if let Err(error) = staging.sync_all() {
                    return self.stage_retry_or_audit(
                        operation_id,
                        phase,
                        format!("could not synchronize adopted staging entry: {error}"),
                    );
                }
                let backup_len = match reacquired.backup.metadata() {
                    Ok(metadata) => metadata.len(),
                    Err(error) => {
                        return self.stage_retry_or_audit(
                            operation_id,
                            phase,
                            format!("could not inspect backup length: {error}"),
                        );
                    }
                };
                if identity.len != backup_len
                    || validate_staged_evidence(&prepared.evidence.backup, &evidence).is_err()
                {
                    return self.stage_retry_or_audit(
                        operation_id,
                        phase,
                        format!("staging entry is occupied and does not match {staging_display:?}"),
                    );
                }
                Some((staging, identity, evidence))
            }
            Err(_) => {
                if durable_staged.is_some() {
                    return self.stage_retry_or_audit(
                        operation_id,
                        phase,
                        format!("durable staging entry is missing or cannot be opened at {staging_display:?}"),
                    );
                }
                if let Err(reason) = verify_relative_absent(
                    &reacquired.target_root,
                    &prepared.staging.relative_path,
                    &staging_display,
                ) {
                    return self.stage_retry_or_audit(operation_id, phase, reason);
                }
                None
            }
        };

        let (staging_identity, staging_evidence) =
            if let Some((_staging, identity, evidence)) = existing {
                (identity, evidence)
            } else {
                let mut staging = match create_relative_exclusive(
                    &reacquired.target_root,
                    &prepared.staging.relative_path,
                    &staging_display,
                ) {
                    Ok(staging) => staging,
                    Err(reason) => return self.stage_retry_or_audit(operation_id, phase, reason),
                };
                match copy_and_validate(
                    &reacquired.backup,
                    &mut staging,
                    &prepared.backup.identity,
                    &prepared.evidence.backup,
                ) {
                    Ok(result) => result,
                    Err(reason) => return self.stage_retry_or_audit(operation_id, phase, reason),
                }
            };

        if let Err(error) = reacquired.target_root.sync_all() {
            return self.stage_retry_or_audit(
                operation_id,
                phase,
                format!("could not synchronize staging directory: {error}"),
            );
        }

        let staged = FilesystemStagedWaveformRestore {
            participant: FilesystemStagedParticipant::CopyValidated {
                staging: PreparedLeafLocator {
                    relative_path: prepared.staging.relative_path.clone(),
                    identity: staging_identity,
                },
                evidence: staging_evidence,
            },
        };
        match self.store.guarded_stage(operation_id, staged) {
            Ok(()) => Ok(FilesystemStageOutcome::FilesystemStaged(operation_id)),
            Err(error) => Ok(FilesystemStageOutcome::JournalWriteFailed {
                operation_id,
                reason: error.to_string(),
            }),
        }
    }

    fn stage_retry_or_audit(
        &mut self,
        operation_id: Uuid,
        phase: OperationPhase,
        reason: String,
    ) -> Result<FilesystemStageOutcome, JournalError> {
        let disposition = if phase == OperationPhase::FilesystemStaged {
            OperationDisposition::AuditRequired
        } else {
            OperationDisposition::RetryPending
        };
        match self.store.update(operation_id, phase, disposition) {
            Ok(()) => {
                if disposition == OperationDisposition::AuditRequired {
                    Ok(FilesystemStageOutcome::AuditRequired {
                        operation_id,
                        reason,
                    })
                } else {
                    Ok(FilesystemStageOutcome::RetryPending {
                        operation_id,
                        reason,
                    })
                }
            }
            Err(error) => Ok(FilesystemStageOutcome::JournalWriteFailed {
                operation_id,
                reason: error.to_string(),
            }),
        }
    }

    /// Attempt publication of one already-staged restore through the sealed adapter seam.
    ///
    /// The production adapter is intentionally unsupported in this slice, so this method cannot
    /// mutate the target namespace.  A test-only adapter exercises the same guarded evidence
    /// path without standing in for a platform qualification.
    pub(crate) fn attempt_publish_staged_waveform_restore(
        &mut self,
        operation_id: Uuid,
    ) -> Result<FilesystemStageOutcome, JournalError> {
        self.attempt_publish_staged_waveform_restore_with_adapter(
            operation_id,
            &ProductionExpectedIdentityReplacementAdapter,
        )
    }

    fn attempt_publish_staged_waveform_restore_with_adapter<A>(
        &mut self,
        operation_id: Uuid,
        adapter: &A,
    ) -> Result<FilesystemStageOutcome, JournalError>
    where
        A: ExpectedIdentityReplacementAdapter,
    {
        let record = self
            .store
            .record(operation_id)
            .ok_or(JournalError::NotFound(operation_id))?;
        if record.phase != OperationPhase::FilesystemStaged {
            return Err(JournalError::IllegalTransition {
                operation_id,
                from: record.phase,
                to: OperationPhase::FilesystemPublished,
            });
        }
        let prepared = record
            .prepared
            .clone()
            .ok_or(JournalError::MissingPreparedEvidence(operation_id))?;
        let staged = record
            .staged
            .clone()
            .ok_or(JournalError::MissingPreparedEvidence(operation_id))?;
        let capacity_plan = record
            .capacity_plan
            .clone()
            .ok_or(JournalError::MissingPreparedEvidence(operation_id))?;

        let prepared = match prepared {
            PreparedTargetContract::ExistingExpectedIdentity(prepared) => prepared,
            PreparedTargetContract::AbsentFinalNoReplace(_) => {
                return self.update_attempt_disposition(
                    operation_id,
                    OperationDisposition::AuditRequired,
                    String::from(
                        "schema-v2 absent-final publication requires its explicit qualified seam",
                    ),
                );
            }
        };

        let reacquired = match reacquire_staged_restore(&prepared, &staged, &capacity_plan) {
            Ok(reacquired) => reacquired,
            Err(reason) => return self.attempt_retry_or_audit(operation_id, reason),
        };
        let request = ExpectedIdentityReplacementRequest {
            target_parent: &reacquired.target_parent,
            target: &reacquired.target,
            staging: &reacquired.staging,
            target_leaf: &prepared.target.relative_path,
            staging_leaf: &prepared.staging.relative_path,
            target_parent_identity: &reacquired.target_parent_identity,
            expected_target: &reacquired.target_identity,
            staging_identity: &reacquired.staging_identity,
            staging_content: &reacquired.staging_content,
            volume: &reacquired.volume,
        };
        match adapter.attempt(request) {
            ExpectedIdentityReplacementOutcome::PlatformQualificationRequired { assessment } => {
                self.update_platform_qualification(operation_id, assessment)
            }
            ExpectedIdentityReplacementOutcome::Drift { reason }
            | ExpectedIdentityReplacementOutcome::Ambiguous { reason } => self
                .update_attempt_disposition(
                    operation_id,
                    OperationDisposition::AuditRequired,
                    reason,
                ),
            ExpectedIdentityReplacementOutcome::QualifiedSuccess(qualified) => {
                let published = super::publication::from_qualified_adapter_result(qualified);
                if let Err(reason) = validate_publication_evidence(&prepared, &staged, &published) {
                    return self.update_attempt_disposition(
                        operation_id,
                        OperationDisposition::AuditRequired,
                        reason,
                    );
                }
                match self.store.guarded_publish(operation_id, published) {
                    Ok(()) => Ok(FilesystemStageOutcome::FilesystemPublished(operation_id)),
                    Err(error) => Ok(FilesystemStageOutcome::JournalWriteFailed {
                        operation_id,
                        reason: error.to_string(),
                    }),
                }
            }
        }
    }

    fn update_platform_qualification(
        &mut self,
        operation_id: Uuid,
        assessment: ReplacementQualificationAssessment,
    ) -> Result<FilesystemStageOutcome, JournalError> {
        match self
            .store
            .update_with_replacement_qualification(operation_id, assessment.clone())
        {
            Ok(()) => Ok(FilesystemStageOutcome::PlatformQualificationRequired {
                operation_id,
                assessment,
            }),
            Err(error) => Ok(FilesystemStageOutcome::JournalWriteFailed {
                operation_id,
                reason: error.to_string(),
            }),
        }
    }

    fn update_attempt_disposition(
        &mut self,
        operation_id: Uuid,
        disposition: OperationDisposition,
        reason: String,
    ) -> Result<FilesystemStageOutcome, JournalError> {
        match self
            .store
            .update(operation_id, OperationPhase::FilesystemStaged, disposition)
        {
            Ok(()) => {
                if disposition == OperationDisposition::RetryPending {
                    Ok(FilesystemStageOutcome::RetryPending {
                        operation_id,
                        reason,
                    })
                } else {
                    Ok(FilesystemStageOutcome::AuditRequired {
                        operation_id,
                        reason,
                    })
                }
            }
            Err(error) => Ok(FilesystemStageOutcome::JournalWriteFailed {
                operation_id,
                reason: error.to_string(),
            }),
        }
    }

    fn attempt_retry_or_audit(
        &mut self,
        operation_id: Uuid,
        reason: String,
    ) -> Result<FilesystemStageOutcome, JournalError> {
        self.update_attempt_disposition(operation_id, OperationDisposition::AuditRequired, reason)
    }

    /// Advance a staged waveform restore only with typed, validated publication evidence.
    /// This records evidence; it deliberately does not perform the publication primitive.
    pub(crate) fn guarded_publish(
        &mut self,
        operation_id: Uuid,
        published: FilesystemPublishedWaveformRestore,
    ) -> Result<(), JournalError> {
        self.store.guarded_publish(operation_id, published)
    }

    /// Advance phase/disposition through one atomic durable record replacement.
    pub(crate) fn update(
        &mut self,
        operation_id: Uuid,
        phase: OperationPhase,
        disposition: OperationDisposition,
    ) -> Result<(), JournalError> {
        if phase == OperationPhase::Prepared {
            return Err(JournalError::IllegalTransition {
                operation_id,
                from: self
                    .store
                    .record(operation_id)
                    .ok_or(JournalError::NotFound(operation_id))?
                    .phase,
                to: OperationPhase::Prepared,
            });
        }
        self.store.update(operation_id, phase, disposition)
    }

    /// Guarded, idempotent IntentDurable -> Prepared transition for validated evidence.
    pub(crate) fn guarded_prepare(
        &mut self,
        operation_id: Uuid,
        prepared: PreparedWaveformRestore,
    ) -> Result<(), JournalError> {
        self.store.guarded_prepare(operation_id, prepared)
    }

    /// Return a typed operation record, if present.
    pub(crate) fn record(&self, operation_id: Uuid) -> Option<&OperationRecord> {
        self.store.record(operation_id)
    }
}

struct OwnershipLock {
    #[cfg(any(unix, windows))]
    file: Option<File>,
}

impl OwnershipLock {
    fn acquire(directory: &Path) -> Result<Self, JournalError> {
        let path = directory.join(LOCK_FILE_NAME);
        #[cfg(unix)]
        {
            use std::os::fd::AsRawFd;
            use std::os::unix::fs::OpenOptionsExt;

            let mut options = OpenOptions::new();
            options
                .read(true)
                .write(true)
                .create(true)
                .custom_flags(libc::O_NOFOLLOW);
            let mut file = options.open(&path).map_err(|source| JournalError::Write {
                path: path.clone(),
                source,
            })?;
            let metadata = file.metadata().map_err(|source| JournalError::Write {
                path: path.clone(),
                source,
            })?;
            if !metadata.is_file() {
                return Err(JournalError::Write {
                    path,
                    source: io::Error::new(
                        io::ErrorKind::InvalidData,
                        "operation-journal owner is not a regular file",
                    ),
                });
            }
            // An advisory descriptor lock is released by the kernel if this
            // process crashes, while the path remains available for inspection.
            let result = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
            if result != 0 {
                let source = io::Error::last_os_error();
                if source.kind() == io::ErrorKind::WouldBlock {
                    return Err(JournalError::OwnedByAnotherProcess { path });
                }
                return Err(JournalError::Write { path, source });
            }
            file.set_len(0)
                .and_then(|_| file.write_all(format!("pid={}\n", std::process::id()).as_bytes()))
                .and_then(|_| file.sync_all())
                .map_err(|source| JournalError::Write {
                    path: path.clone(),
                    source,
                })?;
            return Ok(Self { file: Some(file) });
        }

        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            use std::os::windows::io::AsRawHandle;
            use windows::Win32::Storage::FileSystem::{
                FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_OPEN_REPARSE_POINT,
                LOCKFILE_EXCLUSIVE_LOCK, LOCKFILE_FAIL_IMMEDIATELY, LockFileEx,
            };
            use windows::Win32::System::IO::OVERLAPPED;

            let mut options = OpenOptions::new();
            options
                .read(true)
                .write(true)
                .create(true)
                .share_mode(0x00000001 | 0x00000002)
                .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT.0);
            let mut file = options.open(&path).map_err(|source| JournalError::Write {
                path: path.clone(),
                source,
            })?;
            use std::os::windows::fs::MetadataExt;
            let metadata = file.metadata().map_err(|source| JournalError::Write {
                path: path.clone(),
                source,
            })?;
            if !metadata.is_file()
                || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0
            {
                return Err(JournalError::Write {
                    path,
                    source: io::Error::new(
                        io::ErrorKind::InvalidData,
                        "operation-journal owner is not a regular non-reparse file",
                    ),
                });
            }
            let mut overlapped = OVERLAPPED::default();
            let result = unsafe {
                LockFileEx(
                    windows::Win32::Foundation::HANDLE(file.as_raw_handle()),
                    LOCKFILE_EXCLUSIVE_LOCK | LOCKFILE_FAIL_IMMEDIATELY,
                    None,
                    u32::MAX,
                    u32::MAX,
                    &mut overlapped,
                )
            };
            if result.is_err() {
                return Err(JournalError::OwnedByAnotherProcess { path });
            }
            file.set_len(0)
                .and_then(|_| file.write_all(format!("pid={}\n", std::process::id()).as_bytes()))
                .and_then(|_| file.sync_all())
                .map_err(|source| JournalError::Write {
                    path: path.clone(),
                    source,
                })?;
            return Ok(Self { file: Some(file) });
        }

        #[cfg(all(not(unix), not(windows)))]
        Err(JournalError::Write {
            path: path.to_path_buf(),
            source: io::Error::new(
                io::ErrorKind::Unsupported,
                "no verified profile ownership primitive on this platform",
            ),
        })
    }
}

impl Drop for OwnershipLock {
    fn drop(&mut self) {
        #[cfg(any(unix, windows))]
        self.file.take();
    }
}

fn atomic_durable_write(path: &Path, record: &OperationRecord) -> Result<(), JournalError> {
    validate_schema_v2_phase_evidence_record(record).map_err(|reason| JournalError::Write {
        path: path.to_path_buf(),
        source: io::Error::new(io::ErrorKind::InvalidInput, reason),
    })?;
    let directory = path.parent().ok_or_else(|| JournalError::Write {
        path: path.to_path_buf(),
        source: io::Error::new(io::ErrorKind::InvalidInput, "record path has no parent"),
    })?;
    let temp_path = directory.join(format!(
        ".{}.tmp",
        path.file_name().unwrap_or_default().to_string_lossy()
    ));
    let bytes = encode_record(record).map_err(|source| JournalError::Write {
        path: path.to_path_buf(),
        source,
    })?;
    if bytes.len() > MAX_RECORD_BYTES {
        return Err(JournalError::Write {
            path: path.to_path_buf(),
            source: io::Error::new(
                io::ErrorKind::InvalidData,
                format!("journal record exceeds {MAX_RECORD_BYTES} bytes"),
            ),
        });
    }
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp_path)
        .map_err(|source| JournalError::Write {
            path: temp_path.clone(),
            source,
        })?;
    if let Err(source) = file.write_all(&bytes).and_then(|_| file.sync_all()) {
        let _ = fs::remove_file(&temp_path);
        return Err(JournalError::Write {
            path: temp_path,
            source,
        });
    }
    drop(file);
    if let Err(source) = replace_file(&temp_path, path) {
        let _ = fs::remove_file(&temp_path);
        return Err(JournalError::Write {
            path: path.to_path_buf(),
            source,
        });
    }
    if let Err(source) = sync_directory(directory) {
        return Err(JournalError::Write {
            path: directory.to_path_buf(),
            source,
        });
    }
    Ok(())
}

fn replace_file(temp_path: &Path, path: &Path) -> io::Result<()> {
    #[cfg(target_os = "windows")]
    {
        use windows::Win32::Storage::FileSystem::{
            MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
        };
        use windows::core::PCWSTR;
        fn wide(path: &Path) -> Vec<u16> {
            use std::os::windows::ffi::OsStrExt;
            path.as_os_str()
                .encode_wide()
                .chain(std::iter::once(0))
                .collect()
        }
        let from = wide(temp_path);
        let to = wide(path);
        unsafe {
            MoveFileExW(
                PCWSTR(from.as_ptr()),
                PCWSTR(to.as_ptr()),
                MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
            )
        }
        .map_err(io::Error::other)
    }
    #[cfg(not(target_os = "windows"))]
    {
        fs::rename(temp_path, path)
    }
}

fn sync_directory(directory: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        File::open(directory)?.sync_all()
    }
    #[cfg(target_os = "windows")]
    {
        // `replace_file` uses MOVEFILE_WRITE_THROUGH. Windows does not expose a
        // portable directory-handle `fsync`; the write-through rename is the
        // strongest namespace durability primitive this store claims here.
        let _ = directory;
        Ok(())
    }
    #[cfg(all(not(unix), not(target_os = "windows")))]
    {
        let _ = directory;
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "directory synchronization is not verified on this platform",
        ))
    }
}

fn unix_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|duration| i64::try_from(duration.as_millis()).ok())
        .unwrap_or(i64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static TEST_LOCK: Mutex<()> = Mutex::new(());

    fn fixture_directory() -> tempfile::TempDir {
        #[cfg(target_os = "macos")]
        {
            return tempfile::tempdir_in("/private/tmp").expect("fixture directory");
        }
        #[cfg(not(target_os = "macos"))]
        tempfile::tempdir().expect("fixture directory")
    }

    fn intent() -> OperationIntent {
        OperationIntent {
            actor: OperationActor::User,
            kind: OperationKind::FileHistory,
            label: String::from("test"),
        }
    }

    fn schema_v1_bytes(record: &OperationRecord) -> Vec<u8> {
        encode_schema_v1(record).expect("encode schema-v1 fixture")
    }

    fn schema_v1_value(record: &OperationRecord) -> Value {
        serde_json::to_value(persisted_v1_from_record(record).expect("schema-v1 value"))
            .expect("encode schema-v1 value")
    }

    fn qualification_assessment() -> ReplacementQualificationAssessment {
        ReplacementQualificationAssessment {
            platform_family: ReplacementPlatformFamily::Macos,
            observed_filesystem: ObservedFilesystemClassification::SameVolume,
            volume: VolumeIdentity { device: 1 },
            candidate: ReplacementCandidatePrimitive::NoPublicCandidate,
            candidate_assessment: ReplacementCandidateAssessment::NoQualifiedCandidate,
            missing_invariant: ReplacementMissingInvariant::AtomicExpectedTargetIdentityComparison,
            decision: ReplacementQualificationDecision::PlatformQualificationRequired,
            retry_condition:
                ReplacementQualificationRetryCondition::PlatformBuildOrQualificationPolicyChange,
        }
    }

    fn valid_capacity_plan() -> DurableCapacityPlan {
        DurableCapacityPlan {
            volumes: vec![super::super::capacity_gate::DurableVolumeCapacity {
                identity: VolumeIdentity { device: 77 },
                allocation_unit: 4096,
                allocation_class:
                    super::super::capacity_gate::CapacityAllocationClass::DestinationStaging,
                logical_bytes: 4096,
                allocated_bytes: 4096,
                protected_free_bytes: super::super::capacity_gate::PROTECTED_FREE_SPACE_FLOOR,
            }],
        }
    }

    fn absent_final_v2_fixture() -> (
        PreparedAbsentFinalNoReplace,
        FilesystemStagedWaveformRestore,
        DurableCapacityPlan,
    ) {
        let target_parent_identity = PreparedObjectIdentity {
            stable_id: String::from("v2-target-parent"),
            change_marker: None,
            len: 0,
        };
        let target_parent = PreparedRootCapability {
            path: PathBuf::from("/v2-fixture"),
            identity: target_parent_identity,
        };
        let staging_identity = PreparedObjectIdentity {
            stable_id: String::from("v2-staging"),
            change_marker: None,
            len: 4,
        };
        let evidence = PreparedFileEvidence::ContentHash([7; 32]);
        let prepared = PreparedAbsentFinalNoReplace {
            direction: PreparedRestoreDirection::Undo,
            source_id: String::from("v2-fixture-source"),
            source_root: target_parent.clone(),
            target_parent,
            final_leaf: PathBuf::from("final.wav"),
            staging: PreparedStagingLocator {
                relative_path: PathBuf::from("staging.wav"),
                absent: true,
            },
            final_observation: AbsentFinalObservation::ObservedAbsent,
            copy_validated_evidence: evidence.clone(),
        };
        let staged = FilesystemStagedWaveformRestore {
            participant: FilesystemStagedParticipant::CopyValidated {
                staging: PreparedLeafLocator {
                    relative_path: PathBuf::from("staging.wav"),
                    identity: staging_identity,
                },
                evidence,
            },
        };
        (prepared, staged, valid_capacity_plan())
    }

    fn admit_absent_final_v2_fixture(
        journal: &mut OperationJournalCoordinator,
    ) -> (
        Uuid,
        PreparedAbsentFinalNoReplace,
        FilesystemStagedWaveformRestore,
    ) {
        let (prepared, staged, capacity_plan) = absent_final_v2_fixture();
        let operation_id = journal
            .admit_schema_v2_absent_final_for_test(
                intent(),
                serde_json::json!({"schema": 2}),
                prepared.clone(),
                staged.clone(),
                capacity_plan,
            )
            .expect("admit schema-v2 absent-final fixture");
        (operation_id, prepared, staged)
    }

    fn invalid_v2_admission_record() -> OperationRecord {
        let (prepared, staged, capacity_plan) = absent_final_v2_fixture();
        let mut record = OperationRecord::new_v2_absent_final_with_capacity_plan(
            intent(),
            serde_json::json!({"schema": 2}),
            prepared,
            staged,
            capacity_plan,
        );
        record.phase = OperationPhase::IntentDurable;
        record
    }

    fn assert_invalid_input_write(error: JournalError, path: &Path) {
        match error {
            JournalError::Write {
                path: error_path,
                source,
            } => {
                assert_eq!(error_path, path);
                assert_eq!(source.kind(), io::ErrorKind::InvalidInput);
            }
            other => panic!("expected invalid-input journal write error, got {other:?}"),
        }
    }

    fn v2_absent_record_on_disk() -> (tempfile::TempDir, Uuid, PathBuf, Value) {
        let directory = tempfile::tempdir().unwrap();
        let mut journal = OperationJournalCoordinator::open(directory.path().to_path_buf())
            .expect("open v2 fixture journal");
        let (operation_id, _, _) = admit_absent_final_v2_fixture(&mut journal);
        let path = journal.store.record_path(operation_id);
        let value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        drop(journal);
        (directory, operation_id, path, value)
    }

    fn v2_phase_evidence_record_on_disk(
        phase: OperationPhase,
        disposition: OperationDisposition,
        evidence: SchemaV2EvidencePresence,
    ) -> (tempfile::TempDir, Uuid, PathBuf, Vec<u8>) {
        let (directory, operation_id, path, mut value) = v2_absent_record_on_disk();
        let prepared_value = value["prepared"].clone();
        let staged_value = value["staged"].clone();
        let (prepared, staged, _) = absent_final_v2_fixture();
        let recovery_observation = AbsentFinalRecoveryObservation {
            target_parent_stable_id: prepared.target_parent.identity.stable_id.clone(),
            final_stable_id: match &staged.participant {
                FilesystemStagedParticipant::CopyValidated { staging, .. } => {
                    staging.identity.stable_id.clone()
                }
            },
            final_len: match &staged.participant {
                FilesystemStagedParticipant::CopyValidated { staging, .. } => staging.identity.len,
            },
            final_content: match &staged.participant {
                FilesystemStagedParticipant::CopyValidated { evidence, .. } => evidence.clone(),
            },
        };
        let recovery_observation_value = serde_json::to_value(recovery_observation).unwrap();
        let transaction_owned_proof = AbsentFinalTransactionOwnedProof {
            target_parent_stable_id: prepared.target_parent.identity.stable_id.clone(),
            final_stable_id: match &staged.participant {
                FilesystemStagedParticipant::CopyValidated { staging, .. } => {
                    staging.identity.stable_id.clone()
                }
            },
            final_len: match &staged.participant {
                FilesystemStagedParticipant::CopyValidated { staging, .. } => staging.identity.len,
            },
            final_content: match &staged.participant {
                FilesystemStagedParticipant::CopyValidated { evidence, .. } => evidence.clone(),
            },
        };
        let transaction_owned_proof_value = serde_json::to_value(transaction_owned_proof).unwrap();
        let publication = super::super::publication::test_absent_final_publication_evidence(
            &prepared.target_parent.identity,
            &staged,
        );
        let published_value = serde_json::to_value(publication).unwrap();
        value["phase"] = serde_json::to_value(phase).unwrap();
        value["disposition"] = serde_json::to_value(disposition).unwrap();
        value["prepared"] = if evidence.prepared {
            prepared_value
        } else {
            Value::Null
        };
        value["staged"] = if evidence.staged {
            staged_value
        } else {
            Value::Null
        };
        value["absent_final_recovery_observation"] = if evidence.absent_final_recovery_observation {
            recovery_observation_value
        } else {
            Value::Null
        };
        value["absent_final_transaction_owned_proof"] =
            if evidence.absent_final_transaction_owned_proof {
                transaction_owned_proof_value
            } else {
                Value::Null
            };
        value["published"] = if evidence.published {
            published_value
        } else {
            Value::Null
        };
        let bytes = serde_json::to_vec(&value).unwrap();
        fs::write(&path, &bytes).unwrap();
        (directory, operation_id, path, bytes)
    }

    fn assert_v2_malformed_record_retained<F>(mutate: F)
    where
        F: FnOnce(&mut Value),
    {
        let (directory, operation_id, path, mut value) = v2_absent_record_on_disk();
        mutate(&mut value);
        let bytes = serde_json::to_vec(&value).unwrap();
        fs::write(&path, &bytes).unwrap();
        assert_unknown_nested_record_is_retained_unchanged(
            directory.path(),
            &path,
            operation_id,
            &bytes,
        );
    }

    fn add_valid_recovery_observation(value: &mut Value) {
        let (prepared, staged, _) = absent_final_v2_fixture();
        let observation = AbsentFinalRecoveryObservation {
            target_parent_stable_id: prepared.target_parent.identity.stable_id,
            final_stable_id: match &staged.participant {
                FilesystemStagedParticipant::CopyValidated { staging, .. } => {
                    staging.identity.stable_id.clone()
                }
            },
            final_len: match &staged.participant {
                FilesystemStagedParticipant::CopyValidated { staging, .. } => staging.identity.len,
            },
            final_content: match &staged.participant {
                FilesystemStagedParticipant::CopyValidated { evidence, .. } => evidence.clone(),
            },
        };
        value["absent_final_recovery_observation"] =
            serde_json::to_value(observation).expect("encode recovery observation");
    }

    fn add_valid_transaction_owned_proof(value: &mut Value) {
        let (prepared, staged, _) = absent_final_v2_fixture();
        let proof = AbsentFinalTransactionOwnedProof {
            target_parent_stable_id: prepared.target_parent.identity.stable_id,
            final_stable_id: match &staged.participant {
                FilesystemStagedParticipant::CopyValidated { staging, .. } => {
                    staging.identity.stable_id.clone()
                }
            },
            final_len: match &staged.participant {
                FilesystemStagedParticipant::CopyValidated { staging, .. } => staging.identity.len,
            },
            final_content: match &staged.participant {
                FilesystemStagedParticipant::CopyValidated { evidence, .. } => evidence.clone(),
            },
        };
        value["absent_final_transaction_owned_proof"] =
            serde_json::to_value(proof).expect("encode transaction-owned proof");
    }

    fn assert_v2_recovery_observation_retained_as_malformed<F>(mutate: F)
    where
        F: FnOnce(&mut Value),
    {
        assert_v2_malformed_record_retained(|value| {
            add_valid_recovery_observation(value);
            mutate(value);
        });
    }

    fn assert_v2_transaction_owned_proof_retained_as_malformed<F>(mutate: F)
    where
        F: FnOnce(&mut Value),
    {
        assert_v2_malformed_record_retained(|value| {
            add_valid_recovery_observation(value);
            add_valid_transaction_owned_proof(value);
            mutate(value);
        });
    }

    fn publication_with_reopened_identity_mismatch(
        publication: &FilesystemPublishedWaveformRestore,
    ) -> FilesystemPublishedWaveformRestore {
        let mut value = serde_json::to_value(publication).unwrap();
        let len = value["reopened_final"]["identity"]["len"]
            .as_u64()
            .expect("reopened identity length");
        value["reopened_final"]["identity"]["len"] = Value::from(len + 1);
        serde_json::from_value(value).expect("decode drifted publication fixture")
    }

    fn assert_unknown_nested_record_is_retained_unchanged(
        directory: &Path,
        path: &Path,
        operation_id: Uuid,
        bytes: &[u8],
    ) {
        for _ in 0..2 {
            let mut journal = OperationJournalCoordinator::open(directory.to_path_buf()).unwrap();
            let summary = journal.recovery_summary();
            assert_eq!(summary.malformed_count, 1);
            assert_eq!(summary.unknown_version_count, 0);
            assert!(summary.attention_required);
            assert!(journal.store.capacity_blocked());
            assert!(journal.record(operation_id).is_none());
            assert!(matches!(
                journal.admit(intent(), Value::Null),
                Err(JournalError::Write { .. })
            ));
            for (phase, disposition) in [
                (
                    OperationPhase::IntentDurable,
                    OperationDisposition::RetryPending,
                ),
                (OperationPhase::Prepared, OperationDisposition::None),
                (
                    OperationPhase::FilesystemStaged,
                    OperationDisposition::AuditRequired,
                ),
                (
                    OperationPhase::Terminal,
                    OperationDisposition::CancelledBeforePublish,
                ),
            ] {
                assert!(matches!(
                    journal.update(operation_id, phase, disposition),
                    Err(JournalError::NotFound(id)) if id == operation_id
                ));
                assert_eq!(fs::read(path).unwrap(), bytes);
            }
            assert_eq!(fs::read(path).unwrap(), bytes);
        }
    }

    fn assert_v2_phase_evidence_record_is_retained_unchanged(
        phase: OperationPhase,
        disposition: OperationDisposition,
        evidence: SchemaV2EvidencePresence,
    ) {
        let (directory, operation_id, path, bytes) =
            v2_phase_evidence_record_on_disk(phase, disposition, evidence);
        assert!(!schema_v2_phase_evidence_is_valid(
            phase,
            disposition,
            evidence
        ));
        assert_unknown_nested_record_is_retained_unchanged(
            directory.path(),
            &path,
            operation_id,
            &bytes,
        );
    }

    #[test]
    fn profile_local_directory_isolated_by_app_root() {
        let _lock = TEST_LOCK.lock().unwrap();
        let first = tempfile::tempdir().unwrap();
        let second = tempfile::tempdir().unwrap();
        let mut one = OperationJournalCoordinator::open(first.path().join("journal")).unwrap();
        let two = OperationJournalCoordinator::open(second.path().join("journal")).unwrap();
        let id = one.admit(intent(), Value::Null).unwrap();
        assert!(one.record(id).is_some());
        assert!(two.record(id).is_none());
    }

    #[test]
    fn durable_record_reopens_with_same_typed_state() {
        let _lock = TEST_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let id;
        {
            let mut journal = OperationJournalCoordinator::open(dir.path().to_path_buf()).unwrap();
            id = journal
                .admit(intent(), serde_json::json!({"bounded": true}))
                .unwrap();
            journal
                .update(
                    id,
                    OperationPhase::IntentDurable,
                    OperationDisposition::RetryPending,
                )
                .unwrap();
        }
        let journal = OperationJournalCoordinator::open(dir.path().to_path_buf()).unwrap();
        let record = journal.record(id).unwrap();
        assert_eq!(record.phase, OperationPhase::IntentDurable);
        assert_eq!(record.disposition, OperationDisposition::RetryPending);
        assert!(journal.store.capacity_blocked());
    }

    #[test]
    fn capacity_plan_claim_is_durable_and_reconstructed_after_restart() {
        let _lock = TEST_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let identity = VolumeIdentity { device: 77 };
        let plan = DurableCapacityPlan {
            volumes: vec![super::super::capacity_gate::DurableVolumeCapacity {
                identity: identity.clone(),
                allocation_unit: 4096,
                allocation_class:
                    super::super::capacity_gate::CapacityAllocationClass::DestinationStaging,
                logical_bytes: 4096,
                allocated_bytes: 4096,
                protected_free_bytes: super::super::capacity_gate::PROTECTED_FREE_SPACE_FLOOR,
            }],
        };
        {
            let mut store = OperationJournalStore::open(dir.path().to_path_buf()).unwrap();
            let record =
                OperationRecord::new_with_capacity_plan(intent(), Value::Null, Some(plan.clone()));
            store.admit_capacity(record).unwrap();
            assert_eq!(store.capacity_claims().get(&identity), Some(&4096));
            assert!(!store.capacity_blocked());
        }
        let store = OperationJournalStore::open(dir.path().to_path_buf()).unwrap();
        assert_eq!(store.capacity_claims().get(&identity), Some(&4096));
        assert!(!store.capacity_blocked());
    }

    #[test]
    fn invalid_capacity_plans_are_retained_and_block_admission_after_restart() {
        let _lock = TEST_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let identity = VolumeIdentity { device: 77 };
        for (_, plan) in [
            (
                "empty",
                DurableCapacityPlan {
                    volumes: Vec::new(),
                },
            ),
            (
                "undercharged",
                DurableCapacityPlan {
                    volumes: vec![super::super::capacity_gate::DurableVolumeCapacity {
                        identity: identity.clone(),
                        allocation_unit: 4096,
                        allocation_class:
                            super::super::capacity_gate::CapacityAllocationClass::DestinationStaging,
                        logical_bytes: 4097,
                        allocated_bytes: 4096,
                        protected_free_bytes:
                            super::super::capacity_gate::PROTECTED_FREE_SPACE_FLOOR,
                    }],
                },
            ),
        ] {
            let record = OperationRecord::new_with_capacity_plan(intent(), Value::Null, Some(plan));
            let path = dir.path().join(format!("{}.json", record.operation_id));
            fs::write(path, schema_v1_bytes(&record)).unwrap();
        }
        let store = OperationJournalStore::open(dir.path().to_path_buf()).unwrap();
        assert_eq!(store.recovery_summary().malformed_count, 2);
        assert!(store.capacity_blocked());
        assert_eq!(
            fs::read_dir(dir.path())
                .unwrap()
                .filter_map(Result::ok)
                .filter(
                    |entry| entry.path().extension().and_then(|ext| ext.to_str()) == Some("json")
                )
                .count(),
            2
        );
    }

    #[test]
    fn coordinator_returns_typed_insufficient_capacity_without_a_record() {
        let _lock = TEST_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let files = fixture_directory();
        let backup = files.path().join("before.wav");
        let target = files.path().join("target.wav");
        fs::write(&backup, vec![7_u8; 4097]).unwrap();
        fs::write(&target, vec![0_u8; 4097]).unwrap();
        let action = crate::native_app::waveform_edits::waveform_restore_action_for_capacity_tests(
            backup, target, false,
        );
        let mut coordinator = OperationJournalCoordinator::open(dir.path().to_path_buf()).unwrap();
        let (_, mut plan) =
            crate::native_app::transaction_history::capacity_gate::plan_waveform_restore(
                crate::native_app::transaction_history::file_io::HistoryFileIoDirection::Undo,
                std::slice::from_ref(&action),
                &BTreeMap::new(),
            )
            .unwrap();
        let volume = &mut plan.volumes[0];
        let logical = (1_u64 << 62) - ((1_u64 << 62) % volume.allocation_unit);
        volume.logical_bytes = logical;
        volume.allocated_bytes = logical;
        let record = OperationRecord::new_with_capacity_plan(intent(), Value::Null, Some(plan));
        coordinator.store.admit_capacity(record).unwrap();
        let json_before = fs::read_dir(dir.path())
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| entry.path().extension().and_then(|ext| ext.to_str()) == Some("json"))
            .count();
        let rejected = coordinator
            .admit_bounded_waveform_restore(
                intent(),
                Value::Null,
                crate::native_app::transaction_history::file_io::HistoryFileIoDirection::Undo,
                std::slice::from_ref(&action),
            )
            .unwrap_err();
        assert!(matches!(
            rejected,
            BoundedAdmissionError::Rejected(RejectedBeforeIntent::InsufficientSpace(..))
        ));
        let json_after = fs::read_dir(dir.path())
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| entry.path().extension().and_then(|ext| ext.to_str()) == Some("json"))
            .count();
        assert_eq!(json_after, json_before);
    }

    #[test]
    fn waveform_restore_prepares_typed_descriptor_without_filesystem_mutation() {
        let _lock = TEST_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let files = fixture_directory();
        let backup = files.path().join("before.wav");
        let target = files.path().join("target.wav");
        fs::write(&backup, vec![7_u8; 4097]).unwrap();
        fs::write(&target, vec![0_u8; 4097]).unwrap();
        let before_backup = fs::read(&backup).unwrap();
        let before_target = fs::read(&target).unwrap();
        let before_names = fs::read_dir(files.path())
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect::<Vec<_>>();
        let action = crate::native_app::waveform_edits::waveform_restore_action_for_capacity_tests(
            backup.clone(),
            target.clone(),
            false,
        );
        let mut coordinator = OperationJournalCoordinator::open(dir.path().to_path_buf()).unwrap();
        let outcome = coordinator
            .prepare_bounded_waveform_restore(
                intent(),
                Value::Null,
                super::super::file_io::HistoryFileIoDirection::Undo,
                std::slice::from_ref(&action),
            )
            .unwrap();
        let id = match outcome {
            PreparedOperationOutcome::Prepared(id) => id,
            other => panic!("expected prepared, got {other:?}"),
        };
        let record = coordinator.record(id).unwrap();
        assert_eq!(record.phase, OperationPhase::Prepared);
        assert_eq!(record.disposition, OperationDisposition::None);
        assert!(record.prepared.is_some());
        assert_eq!(fs::read(&backup).unwrap(), before_backup);
        assert_eq!(fs::read(&target).unwrap(), before_target);
        assert_eq!(
            fs::read_dir(files.path())
                .unwrap()
                .map(|entry| entry.unwrap().file_name())
                .collect::<Vec<_>>(),
            before_names
        );
        drop(coordinator);
        let reopened = OperationJournalCoordinator::open(dir.path().to_path_buf()).unwrap();
        let reopened_record = reopened.record(id).unwrap();
        assert_eq!(reopened_record.phase, OperationPhase::Prepared);
        assert_eq!(reopened_record.disposition, OperationDisposition::None);
        let prepared = reopened_record
            .prepared
            .as_ref()
            .unwrap()
            .as_existing()
            .unwrap();
        assert_eq!(prepared.source_id, "test");
        assert_eq!(prepared.target.relative_path, PathBuf::from("target.wav"));
        assert_eq!(prepared.backup.relative_path, PathBuf::from("before.wav"));
        assert_eq!(
            prepared.staging.relative_path.as_os_str().to_string_lossy(),
            format!(".wavecrate-restore-{id}.stage")
        );
        assert_eq!(reopened.store.capacity_claims().len(), 1);
    }

    fn prepared_restore_fixture() -> (
        tempfile::TempDir,
        tempfile::TempDir,
        OperationJournalCoordinator,
        Uuid,
        PathBuf,
        PathBuf,
        PathBuf,
    ) {
        let journal_dir = tempfile::tempdir().unwrap();
        let files = fixture_directory();
        let backup = files.path().join("before.wav");
        let target = files.path().join("target.wav");
        fs::write(&backup, vec![7_u8; 4097]).unwrap();
        fs::write(&target, vec![0_u8; 4097]).unwrap();
        let action = crate::native_app::waveform_edits::waveform_restore_action_for_capacity_tests(
            backup.clone(),
            target.clone(),
            false,
        );
        let mut journal = OperationJournalCoordinator::open(journal_dir.path().to_path_buf())
            .expect("open fixture journal");
        let id = match journal
            .prepare_bounded_waveform_restore(
                intent(),
                Value::Null,
                super::super::file_io::HistoryFileIoDirection::Undo,
                std::slice::from_ref(&action),
            )
            .expect("prepare restore")
        {
            PreparedOperationOutcome::Prepared(id) => id,
            other => panic!("expected prepared restore, got {other:?}"),
        };
        let staging = files.path().join(format!(".wavecrate-restore-{id}.stage"));
        (journal_dir, files, journal, id, backup, target, staging)
    }

    #[test]
    fn prepared_waveform_restore_stages_copy_and_reopens_validated_checkpoint() {
        let (journal_dir, _files, mut journal, id, backup, _target, staging) =
            prepared_restore_fixture();
        let outcome = journal
            .stage_admitted_bounded_waveform_restore(id)
            .expect("stage restore");
        assert_eq!(outcome, FilesystemStageOutcome::FilesystemStaged(id));
        assert_eq!(fs::read(&staging).unwrap(), fs::read(&backup).unwrap());
        let record = journal.record(id).unwrap();
        assert_eq!(record.phase, OperationPhase::FilesystemStaged);
        assert_eq!(record.disposition, OperationDisposition::None);
        assert!(matches!(
            record.staged.as_ref().map(|staged| &staged.participant),
            Some(FilesystemStagedParticipant::CopyValidated { .. })
        ));
        drop(journal);
        let reopened = OperationJournalCoordinator::open(journal_dir.path().to_path_buf())
            .expect("reopen staged journal");
        let reopened_record = reopened.record(id).expect("staged record after restart");
        assert_eq!(reopened_record.phase, OperationPhase::FilesystemStaged);
        assert_eq!(reopened_record.disposition, OperationDisposition::None);
        assert!(matches!(
            reopened_record
                .staged
                .as_ref()
                .map(|staged| &staged.participant),
            Some(FilesystemStagedParticipant::CopyValidated { .. })
        ));
        assert_eq!(fs::read(&staging).unwrap(), fs::read(&backup).unwrap());
    }

    #[test]
    fn production_publication_adapter_is_unsupported_without_namespace_mutation() {
        let (journal_dir, _files, mut journal, id, backup, target, staging) =
            prepared_restore_fixture();
        journal
            .stage_admitted_bounded_waveform_restore(id)
            .expect("stage restore");
        let target_before = fs::read(&target).unwrap();
        let staging_before = fs::read(&staging).unwrap();
        let names_before = fs::read_dir(target.parent().unwrap())
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect::<Vec<_>>();
        let claims_before = journal.store.capacity_claims().clone();

        let outcome = journal
            .attempt_publish_staged_waveform_restore(id)
            .expect("publication attempt");
        let assessment = match &outcome {
            FilesystemStageOutcome::PlatformQualificationRequired {
                operation_id,
                assessment,
            } if *operation_id == id => assessment.clone(),
            other => panic!("expected platform qualification, got {other:?}"),
        };
        assert_eq!(
            assessment.missing_invariant,
            super::super::expected_identity_replacement::ReplacementMissingInvariant::AtomicExpectedTargetIdentityComparison
        );
        assert_eq!(
            assessment.retry_condition,
            super::super::expected_identity_replacement::ReplacementQualificationRetryCondition::PlatformBuildOrQualificationPolicyChange
        );
        let record = journal.record(id).unwrap();
        assert_eq!(record.phase, OperationPhase::FilesystemStaged);
        assert_eq!(record.disposition, OperationDisposition::RetryPending);
        assert_eq!(record.replacement_qualification.as_ref(), Some(&assessment));
        assert!(matches!(
            record.staged.as_ref().map(|staged| &staged.participant),
            Some(FilesystemStagedParticipant::CopyValidated { .. })
        ));
        assert_eq!(
            record
                .prepared
                .as_ref()
                .unwrap()
                .as_existing()
                .unwrap()
                .target
                .relative_path,
            PathBuf::from("target.wav")
        );
        assert_eq!(journal.store.capacity_claims(), &claims_before);
        assert_eq!(fs::read(&backup).unwrap(), vec![7_u8; 4097]);
        assert_eq!(fs::read(&target).unwrap(), target_before);
        assert_eq!(fs::read(&staging).unwrap(), staging_before);
        assert_eq!(
            fs::read_dir(target.parent().unwrap())
                .unwrap()
                .map(|entry| entry.unwrap().file_name())
                .collect::<Vec<_>>(),
            names_before
        );

        drop(journal);
        let reopened = OperationJournalCoordinator::open(journal_dir.path().to_path_buf()).unwrap();
        let reopened_record = reopened.record(id).unwrap();
        assert_eq!(reopened_record.phase, OperationPhase::FilesystemStaged);
        assert_eq!(
            reopened_record.disposition,
            OperationDisposition::RetryPending
        );
        assert_eq!(
            reopened_record.replacement_qualification.as_ref(),
            Some(&assessment)
        );
        assert!(matches!(
            reopened_record
                .staged
                .as_ref()
                .map(|staged| &staged.participant),
            Some(FilesystemStagedParticipant::CopyValidated { .. })
        ));
        assert_eq!(
            reopened_record
                .prepared
                .as_ref()
                .unwrap()
                .as_existing()
                .unwrap()
                .target
                .relative_path,
            PathBuf::from("target.wav")
        );
        assert_eq!(reopened.store.capacity_claims(), &claims_before);
    }

    #[test]
    fn repeated_platform_qualification_assessment_is_idempotent() {
        let (_journal_dir, _files, mut journal, id, _backup, _target, _staging) =
            prepared_restore_fixture();
        journal
            .stage_admitted_bounded_waveform_restore(id)
            .expect("stage restore");
        let first = journal
            .attempt_publish_staged_waveform_restore(id)
            .expect("first qualification assessment");
        let first_record = journal.record(id).unwrap().clone();
        let second = journal
            .attempt_publish_staged_waveform_restore(id)
            .expect("repeated qualification assessment");
        assert_eq!(second, first);
        assert_eq!(journal.record(id), Some(&first_record));
    }

    #[test]
    fn nested_target_locator_is_rejected_before_qualified_adapter_and_preserves_recoverable_stage()
    {
        let (journal_dir, _files, mut journal, id, _backup, target, staging) =
            prepared_restore_fixture();
        journal
            .stage_admitted_bounded_waveform_restore(id)
            .expect("stage restore");
        let target_before = fs::read(&target).unwrap();
        let staging_before = fs::read(&staging).unwrap();
        let record = {
            let record = journal.store.records.get_mut(&id).unwrap();
            record
                .prepared
                .as_mut()
                .unwrap()
                .as_existing_mut()
                .unwrap()
                .target
                .relative_path = PathBuf::from("nested/target.wav");
            record.clone()
        };
        let record_path = journal.store.record_path(id);
        atomic_durable_write(&record_path, &record).unwrap();

        let adapter = super::super::expected_identity_replacement::
            TestQualifiedExpectedIdentityReplacementAdapter { drift: None };
        assert!(matches!(
            journal
                .attempt_publish_staged_waveform_restore_with_adapter(id, &adapter)
                .expect("nested target attempt"),
            FilesystemStageOutcome::AuditRequired { operation_id, .. } if operation_id == id
        ));

        let record = journal.record(id).unwrap();
        assert_eq!(record.phase, OperationPhase::FilesystemStaged);
        assert_eq!(record.disposition, OperationDisposition::AuditRequired);
        assert!(record.staged.is_some());
        assert_eq!(fs::read(&target).unwrap(), target_before);
        assert_eq!(fs::read(&staging).unwrap(), staging_before);

        drop(journal);
        let reopened = OperationJournalCoordinator::open(journal_dir.path().to_path_buf()).unwrap();
        let reopened_record = reopened.record(id).unwrap();
        assert_eq!(reopened_record.phase, OperationPhase::FilesystemStaged);
        assert_eq!(
            reopened_record.disposition,
            OperationDisposition::AuditRequired
        );
        assert!(reopened_record.staged.is_some());
        assert_eq!(
            reopened_record
                .prepared
                .as_ref()
                .unwrap()
                .as_existing()
                .unwrap()
                .target
                .relative_path,
            PathBuf::from("nested/target.wav")
        );
        assert_eq!(fs::read(&target).unwrap(), target_before);
        assert_eq!(fs::read(&staging).unwrap(), staging_before);
    }

    #[test]
    fn live_volume_identity_guard_rejects_mismatch() {
        let expected = VolumeIdentity { device: 1 };
        let actual = VolumeIdentity { device: 2 };
        assert!(validate_volume_identity("target", &expected, &actual).is_err());
        assert!(validate_volume_identity("target", &expected, &expected).is_ok());
    }

    #[test]
    fn qualified_test_adapter_is_the_only_path_to_guarded_publication() {
        let (journal_dir, _files, mut journal, id, _backup, target, staging) =
            prepared_restore_fixture();
        journal
            .stage_admitted_bounded_waveform_restore(id)
            .expect("stage restore");
        let claims_before = journal.store.capacity_claims().clone();
        let qualification = journal
            .attempt_publish_staged_waveform_restore(id)
            .expect("unsupported production assessment");
        assert!(matches!(
            qualification,
            FilesystemStageOutcome::PlatformQualificationRequired { operation_id, .. }
                if operation_id == id
        ));
        assert!(
            journal
                .record(id)
                .unwrap()
                .replacement_qualification
                .is_some()
        );
        let adapter = super::super::expected_identity_replacement::
            TestQualifiedExpectedIdentityReplacementAdapter { drift: None };

        assert_eq!(
            journal
                .attempt_publish_staged_waveform_restore_with_adapter(id, &adapter)
                .expect("qualified publication attempt"),
            FilesystemStageOutcome::FilesystemPublished(id)
        );
        let record = journal.record(id).unwrap();
        assert_eq!(record.phase, OperationPhase::FilesystemPublished);
        assert_eq!(record.disposition, OperationDisposition::None);
        assert!(record.replacement_qualification.is_none());
        assert!(record.published.is_some());
        assert_eq!(journal.store.capacity_claims(), &claims_before);
        assert_eq!(fs::read(&target).unwrap(), vec![0_u8; 4097]);
        assert_eq!(fs::read(&staging).unwrap(), vec![7_u8; 4097]);

        drop(journal);
        let reopened = OperationJournalCoordinator::open(journal_dir.path().to_path_buf()).unwrap();
        let reopened_record = reopened.record(id).unwrap();
        assert_eq!(reopened_record.phase, OperationPhase::FilesystemPublished);
        assert!(reopened_record.published.is_some());
        assert_eq!(reopened.store.capacity_claims(), &claims_before);
    }

    #[test]
    fn target_drift_blocks_adapter_and_preserves_staged_restore() {
        let (_journal_dir, _files, mut journal, id, _backup, target, staging) =
            prepared_restore_fixture();
        journal
            .stage_admitted_bounded_waveform_restore(id)
            .expect("stage restore");
        let staging_before = fs::read(&staging).unwrap();
        fs::write(&target, vec![9_u8; 4097]).unwrap();
        let adapter = super::super::expected_identity_replacement::
            TestQualifiedExpectedIdentityReplacementAdapter { drift: None };

        assert!(matches!(
            journal
                .attempt_publish_staged_waveform_restore_with_adapter(id, &adapter)
                .expect("drift attempt"),
            FilesystemStageOutcome::AuditRequired { operation_id, .. } if operation_id == id
        ));
        let record = journal.record(id).unwrap();
        assert_eq!(record.phase, OperationPhase::FilesystemStaged);
        assert_eq!(record.disposition, OperationDisposition::AuditRequired);
        assert_eq!(fs::read(&target).unwrap(), vec![9_u8; 4097]);
        assert_eq!(fs::read(&staging).unwrap(), staging_before);
    }

    #[test]
    fn guarded_publication_requires_copy_validated_staging() {
        let (_journal_dir, _files, mut journal, id, _backup, _target, _staging) =
            prepared_restore_fixture();
        let prepared = journal
            .record(id)
            .unwrap()
            .prepared
            .as_ref()
            .unwrap()
            .as_existing()
            .unwrap()
            .clone();
        let error = journal
            .guarded_publish(
                id,
                super::super::publication::test_publication_evidence(
                    &prepared,
                    &FilesystemStagedWaveformRestore {
                        participant: FilesystemStagedParticipant::CopyValidated {
                            staging: prepared.target.clone(),
                            evidence: prepared.evidence.backup.clone(),
                        },
                    },
                    None,
                ),
            )
            .unwrap_err();
        assert!(matches!(error, JournalError::IllegalTransition { .. }));
        assert_eq!(journal.record(id).unwrap().phase, OperationPhase::Prepared);
    }

    #[test]
    fn guarded_publication_rejects_drifted_evidence_without_mutating_the_record() {
        let (_journal_dir, _files, mut journal, id, _backup, _target, _staging) =
            prepared_restore_fixture();
        journal.stage_admitted_bounded_waveform_restore(id).unwrap();
        let record = journal.record(id).unwrap().clone();
        let prepared = record.prepared.as_ref().unwrap().as_existing().unwrap();
        let staged = record.staged.as_ref().unwrap();
        for drift in [
            super::super::publication::TestPublicationDrift::UnqualifiedReplacement,
            super::super::publication::TestPublicationDrift::ExpectedIdentity,
            super::super::publication::TestPublicationDrift::DisplacedIdentity,
            super::super::publication::TestPublicationDrift::ReopenedIdentity,
            super::super::publication::TestPublicationDrift::ReopenedContent,
            super::super::publication::TestPublicationDrift::Visibility,
            super::super::publication::TestPublicationDrift::Atomicity,
            super::super::publication::TestPublicationDrift::Synchronization,
        ] {
            let error = journal
                .guarded_publish(
                    id,
                    super::super::publication::test_publication_evidence(
                        prepared,
                        staged,
                        Some(drift),
                    ),
                )
                .unwrap_err();
            assert!(matches!(
                error,
                JournalError::InvalidPublicationEvidence { .. }
            ));
            assert_eq!(
                journal.record(id).unwrap().phase,
                OperationPhase::FilesystemStaged
            );
        }
    }

    #[test]
    fn guarded_publication_rejects_metadata_only_and_unverifiable_content() {
        for drift in [
            super::super::publication::TestPublicationDrift::MetadataOnly,
            super::super::publication::TestPublicationDrift::Unverifiable,
        ] {
            let (_journal_dir, _files, mut journal, id, _backup, _target, _staging) =
                prepared_restore_fixture();
            journal.stage_admitted_bounded_waveform_restore(id).unwrap();
            let record = journal.store.records.get_mut(&id).unwrap();
            let staged_len = match &record.staged.as_ref().unwrap().participant {
                FilesystemStagedParticipant::CopyValidated { staging, .. } => staging.identity.len,
            };
            let evidence = match drift {
                super::super::publication::TestPublicationDrift::MetadataOnly => {
                    PreparedFileEvidence::Metadata {
                        len: staged_len,
                        modified_ns: None,
                        is_dir: false,
                    }
                }
                super::super::publication::TestPublicationDrift::Unverifiable => {
                    PreparedFileEvidence::Unverifiable
                }
                _ => unreachable!(),
            };
            record
                .prepared
                .as_mut()
                .unwrap()
                .as_existing_mut()
                .unwrap()
                .evidence
                .backup = evidence.clone();
            let FilesystemStagedParticipant::CopyValidated {
                evidence: staged_evidence,
                ..
            } = &mut record.staged.as_mut().unwrap().participant;
            *staged_evidence = evidence;
            let record = journal.record(id).unwrap().clone();
            let publication = super::super::publication::test_publication_evidence(
                record.prepared.as_ref().unwrap().as_existing().unwrap(),
                record.staged.as_ref().unwrap(),
                Some(drift),
            );

            let error = journal.guarded_publish(id, publication).unwrap_err();
            assert!(matches!(
                error,
                JournalError::InvalidPublicationEvidence { .. }
            ));
            assert_eq!(
                journal.record(id).unwrap().phase,
                OperationPhase::FilesystemStaged
            );
        }
    }

    #[test]
    fn guarded_publication_is_durable_idempotent_and_rejects_conflicting_replay() {
        let (journal_dir, _files, mut journal, id, _backup, _target, _staging) =
            prepared_restore_fixture();
        journal.stage_admitted_bounded_waveform_restore(id).unwrap();
        let record = journal.record(id).unwrap().clone();
        let publication = super::super::publication::test_publication_evidence(
            record.prepared.as_ref().unwrap().as_existing().unwrap(),
            record.staged.as_ref().unwrap(),
            None,
        );
        journal.guarded_publish(id, publication.clone()).unwrap();
        journal.guarded_publish(id, publication.clone()).unwrap();
        assert_eq!(
            journal.record(id).unwrap().phase,
            OperationPhase::FilesystemPublished
        );
        assert_eq!(
            journal.record(id).unwrap().published.as_ref(),
            Some(&publication)
        );
        drop(journal);

        let mut reopened =
            OperationJournalCoordinator::open(journal_dir.path().to_path_buf()).unwrap();
        assert_eq!(
            reopened.record(id).unwrap().published.as_ref(),
            Some(&publication)
        );
        let record = reopened.record(id).unwrap().clone();
        let conflicting = super::super::publication::test_publication_evidence(
            record.prepared.as_ref().unwrap().as_existing().unwrap(),
            record.staged.as_ref().unwrap(),
            Some(super::super::publication::TestPublicationDrift::ReopenedIdentity),
        );
        assert!(matches!(
            reopened.guarded_publish(id, conflicting),
            Err(JournalError::InvalidPublicationEvidence { .. })
        ));
        assert_eq!(
            reopened.record(id).unwrap().phase,
            OperationPhase::FilesystemPublished
        );
    }

    #[test]
    fn generic_update_cannot_bypass_the_publication_guard() {
        let (_journal_dir, _files, mut journal, id, _backup, _target, _staging) =
            prepared_restore_fixture();
        journal.stage_admitted_bounded_waveform_restore(id).unwrap();
        assert!(matches!(
            journal.update(
                id,
                OperationPhase::SourceReconciled,
                OperationDisposition::None
            ),
            Err(JournalError::InvalidPublicationEvidence { .. })
        ));
        assert_eq!(
            journal.record(id).unwrap().phase,
            OperationPhase::FilesystemStaged
        );
        let record = journal.record(id).unwrap().clone();
        let publication = super::super::publication::test_publication_evidence(
            record.prepared.as_ref().unwrap().as_existing().unwrap(),
            record.staged.as_ref().unwrap(),
            None,
        );
        journal.guarded_publish(id, publication).unwrap();
        assert!(matches!(
            journal.update(
                id,
                OperationPhase::FilesystemPublished,
                OperationDisposition::None
            ),
            Err(JournalError::IllegalTransition { .. })
        ));
    }

    #[test]
    fn publication_evidence_survives_source_reconciliation_and_restart() {
        let (journal_dir, _files, mut journal, id, _backup, _target, _staging) =
            prepared_restore_fixture();
        journal.stage_admitted_bounded_waveform_restore(id).unwrap();
        let record = journal.record(id).unwrap().clone();
        let publication = super::super::publication::test_publication_evidence(
            record.prepared.as_ref().unwrap().as_existing().unwrap(),
            record.staged.as_ref().unwrap(),
            None,
        );
        journal.guarded_publish(id, publication.clone()).unwrap();
        journal
            .update(
                id,
                OperationPhase::SourceReconciled,
                OperationDisposition::None,
            )
            .unwrap();
        drop(journal);

        let reopened = OperationJournalCoordinator::open(journal_dir.path().to_path_buf())
            .expect("reopen source-reconciled journal");
        let record = reopened.record(id).expect("published record after restart");
        assert_eq!(record.phase, OperationPhase::SourceReconciled);
        assert_eq!(record.published.as_ref(), Some(&publication));
        assert_eq!(reopened.recovery_summary().malformed_count, 0);
    }

    #[test]
    fn prepublication_record_with_publication_evidence_is_retained_verbatim() {
        let (journal_dir, _files, mut journal, id, _backup, _target, _staging) =
            prepared_restore_fixture();
        journal.stage_admitted_bounded_waveform_restore(id).unwrap();
        let record = journal.record(id).unwrap().clone();
        let publication = super::super::publication::test_publication_evidence(
            record.prepared.as_ref().unwrap().as_existing().unwrap(),
            record.staged.as_ref().unwrap(),
            None,
        );
        journal.guarded_publish(id, publication).unwrap();
        let mut invalid = journal.record(id).unwrap().clone();
        invalid.phase = OperationPhase::FilesystemStaged;
        let path = journal_dir.path().join(format!("{id}.json"));
        let bytes = schema_v1_bytes(&invalid);
        fs::write(&path, &bytes).unwrap();
        drop(journal);

        let reopened = OperationJournalCoordinator::open(journal_dir.path().to_path_buf())
            .expect("reopen invalid pre-publication journal");
        assert_eq!(reopened.recovery_summary().malformed_count, 1);
        assert!(reopened.record(id).is_none());
        assert_eq!(fs::read(path).unwrap(), bytes);
    }

    #[test]
    fn durable_staging_missing_after_restart_requires_audit() {
        let (journal_dir, _files, mut journal, id, _backup, _target, staging) =
            prepared_restore_fixture();
        assert_eq!(
            journal.stage_admitted_bounded_waveform_restore(id).unwrap(),
            FilesystemStageOutcome::FilesystemStaged(id)
        );
        drop(journal);
        fs::remove_file(&staging).unwrap();

        let mut reopened = OperationJournalCoordinator::open(journal_dir.path().to_path_buf())
            .expect("reopen staged journal");
        let outcome = reopened
            .stage_admitted_bounded_waveform_restore(id)
            .expect("audit missing staging");
        assert!(matches!(
            outcome,
            FilesystemStageOutcome::AuditRequired { operation_id, .. } if operation_id == id
        ));
        let record = reopened.record(id).unwrap();
        assert_eq!(record.phase, OperationPhase::FilesystemStaged);
        assert_eq!(record.disposition, OperationDisposition::AuditRequired);
    }

    #[cfg(unix)]
    #[test]
    fn durable_staging_same_content_replacement_after_restart_requires_audit() {
        let (journal_dir, files, mut journal, id, _backup, _target, staging) =
            prepared_restore_fixture();
        assert_eq!(
            journal.stage_admitted_bounded_waveform_restore(id).unwrap(),
            FilesystemStageOutcome::FilesystemStaged(id)
        );
        let original = File::open(&staging).unwrap();
        let original_identity = descriptor_identity(&original).unwrap();
        let original_bytes = fs::read(&staging).unwrap();
        drop(journal);
        fs::remove_file(&staging).unwrap();
        fs::write(&staging, original_bytes).unwrap();
        let replacement = File::open(&staging).unwrap();
        assert_ne!(
            original_identity,
            descriptor_identity(&replacement).unwrap()
        );
        drop(replacement);
        drop(original);

        let mut reopened = OperationJournalCoordinator::open(journal_dir.path().to_path_buf())
            .expect("reopen staged journal");
        let outcome = reopened
            .stage_admitted_bounded_waveform_restore(id)
            .expect("audit replaced staging");
        assert!(matches!(
            outcome,
            FilesystemStageOutcome::AuditRequired { operation_id, .. } if operation_id == id
        ));
        assert_eq!(
            reopened.record(id).unwrap().disposition,
            OperationDisposition::AuditRequired
        );
        assert_eq!(
            fs::read(&staging).unwrap(),
            fs::read(files.path().join("before.wav")).unwrap()
        );
    }

    #[cfg(not(unix))]
    #[test]
    fn staging_adoption_is_fail_closed_without_pathname_fallback() {
        let directory = tempfile::tempdir().unwrap();
        let existing = directory.path().join("existing.stage");
        fs::write(&existing, b"staging").unwrap();
        let root = File::open(&existing).unwrap();
        let error =
            open_staging_relative(&root, Path::new("existing.stage"), &existing).unwrap_err();
        assert!(error.contains("not verified"));
    }

    #[test]
    fn prepared_waveform_restore_rejects_identity_drift_before_staging() {
        let (_journal_dir, _files, mut journal, id, backup, _target, staging) =
            prepared_restore_fixture();
        fs::remove_file(&backup).unwrap();
        fs::write(&backup, vec![8_u8; 4097]).unwrap();

        let outcome = journal
            .stage_admitted_bounded_waveform_restore(id)
            .expect("stage identity drift");
        assert!(matches!(
            outcome,
            FilesystemStageOutcome::RetryPending { operation_id, .. } if operation_id == id
        ));
        assert!(!staging.exists());
        let record = journal.record(id).unwrap();
        assert_eq!(record.phase, OperationPhase::Prepared);
        assert_eq!(record.disposition, OperationDisposition::RetryPending);
    }

    #[cfg(unix)]
    #[test]
    fn copy_and_validate_rejects_large_metadata_only_backup_identity_drift() {
        let directory = fixture_directory();
        let backup_path = directory.path().join("large-before.wav");
        let staging_path = directory.path().join("large-stage");
        let mut backup = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .open(&backup_path)
            .unwrap();
        backup
            .set_len(wavecrate::sample_sources::MAX_SOURCE_FILE_EVIDENCE_HASH_BYTES + 1)
            .unwrap();
        backup.sync_all().unwrap();
        let prepared_identity = descriptor_identity(&backup).unwrap();
        let prepared_evidence = prepared_file_evidence(&backup);
        assert!(matches!(
            prepared_evidence,
            PreparedFileEvidence::Metadata { .. }
        ));

        backup.seek(SeekFrom::Start(0)).unwrap();
        backup.write_all(&[8]).unwrap();
        backup.sync_all().unwrap();
        let mut staging = File::create(staging_path).unwrap();
        let error = copy_and_validate(
            &backup,
            &mut staging,
            &prepared_identity,
            &prepared_evidence,
        )
        .unwrap_err();

        assert!(error.contains("backup leaf identity changed since preparation"));
    }

    #[cfg(unix)]
    #[test]
    fn open_leaf_relative_rejects_fifo_without_blocking() {
        use std::os::unix::ffi::OsStrExt;

        let directory = fixture_directory();
        let fifo = directory.path().join("collision");
        let fifo_name = std::ffi::CString::new(fifo.as_os_str().as_bytes()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(fifo_name.as_ptr(), 0o600) }, 0);
        let root = File::open(directory.path()).unwrap();
        let relative = PathBuf::from("collision");
        let display = fifo.clone();
        let (sender, receiver) = std::sync::mpsc::channel();
        let handle = std::thread::spawn(move || {
            sender
                .send(open_leaf_relative(&root, &relative, &display))
                .unwrap();
        });

        let first_result = receiver.recv_timeout(std::time::Duration::from_millis(250));
        let completed_without_writer = first_result.is_ok();
        let result = match first_result {
            Ok(result) => result,
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                let writer = OpenOptions::new().write(true).open(&fifo).unwrap();
                drop(writer);
                receiver
                    .recv_timeout(std::time::Duration::from_secs(1))
                    .expect("FIFO leaf open did not complete after FIFO writer unblock")
            }
            Err(error) => panic!("FIFO leaf open result channel failed: {error}"),
        };
        handle.join().unwrap();

        assert!(
            completed_without_writer,
            "FIFO leaf open blocked before regular-file rejection"
        );
        assert!(result.is_err());
    }

    #[test]
    fn occupied_staging_entry_is_preserved_during_stage_retry() {
        let (_journal_dir, _files, mut journal, id, _backup, _target, staging) =
            prepared_restore_fixture();
        let occupied = b"unrelated staging payload";
        fs::write(&staging, occupied).unwrap();

        let outcome = journal
            .stage_admitted_bounded_waveform_restore(id)
            .expect("stage collision");
        assert!(matches!(
            outcome,
            FilesystemStageOutcome::RetryPending { operation_id, .. } if operation_id == id
        ));
        assert_eq!(fs::read(&staging).unwrap(), occupied);
        let record = journal.record(id).unwrap();
        assert_eq!(record.phase, OperationPhase::Prepared);
        assert_eq!(record.disposition, OperationDisposition::RetryPending);
        assert!(record.staged.is_none());
    }

    #[test]
    fn large_metadata_only_backup_does_not_adopt_same_length_corrupted_staging() {
        let (_journal_dir, _files, mut journal, id, backup, _target, staging) =
            prepared_restore_fixture();
        let metadata_only_len = wavecrate::sample_sources::MAX_SOURCE_FILE_EVIDENCE_HASH_BYTES + 1;
        let backup_file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&backup)
            .unwrap();
        backup_file.set_len(metadata_only_len).unwrap();
        backup_file.sync_all().unwrap();
        let prepared_backup_identity = descriptor_identity(&backup_file).unwrap();
        let prepared_backup_evidence = prepared_file_evidence(&backup_file);
        assert!(matches!(
            &prepared_backup_evidence,
            PreparedFileEvidence::Metadata { .. }
        ));
        let prepared = journal
            .store
            .records
            .get_mut(&id)
            .unwrap()
            .prepared
            .as_mut()
            .unwrap()
            .as_existing_mut()
            .unwrap();
        prepared.backup.identity = prepared_backup_identity;
        prepared.evidence.backup = prepared_backup_evidence;
        drop(backup_file);

        let mut staging_file = OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(&staging)
            .unwrap();
        staging_file.set_len(metadata_only_len).unwrap();
        staging_file.seek(SeekFrom::Start(0)).unwrap();
        staging_file.write_all(&[8]).unwrap();
        staging_file.sync_all().unwrap();
        let staging_len_before = staging_file.metadata().unwrap().len();
        drop(staging_file);

        let outcome = journal
            .stage_admitted_bounded_waveform_restore(id)
            .expect("stage metadata-only collision");
        assert!(matches!(
            outcome,
            FilesystemStageOutcome::RetryPending { operation_id, .. } if operation_id == id
        ));
        let mut staging_file = File::open(&staging).unwrap();
        let mut staging_first_byte = [0_u8; 1];
        staging_file.read_exact(&mut staging_first_byte).unwrap();
        assert_eq!(staging_file.metadata().unwrap().len(), staging_len_before);
        assert_eq!(staging_first_byte, [8]);
        let record = journal.record(id).unwrap();
        assert_eq!(record.phase, OperationPhase::Prepared);
        assert_eq!(record.disposition, OperationDisposition::RetryPending);
        assert!(record.staged.is_none());
    }

    #[test]
    fn occupied_staging_entry_retries_without_mutation_and_reopens_unresolved() {
        let _lock = TEST_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let files = fixture_directory();
        let backup = files.path().join("before.wav");
        let target = files.path().join("target.wav");
        let staging_bytes = b"occupied staging entry";
        fs::write(&backup, vec![7_u8; 4097]).unwrap();
        fs::write(&target, vec![0_u8; 4097]).unwrap();
        let action = crate::native_app::waveform_edits::waveform_restore_action_for_capacity_tests(
            backup,
            target.clone(),
            false,
        );
        let mut coordinator = OperationJournalCoordinator::open(dir.path().to_path_buf()).unwrap();
        let id = coordinator
            .admit_bounded_waveform_restore(
                intent(),
                Value::Null,
                super::super::file_io::HistoryFileIoDirection::Undo,
                std::slice::from_ref(&action),
            )
            .unwrap();
        let staging = files.path().join(format!(".wavecrate-restore-{id}.stage"));
        fs::write(&staging, staging_bytes).unwrap();
        let claims_before = coordinator.store.capacity_claims().clone();
        let outcome = coordinator
            .prepare_admitted_bounded_waveform_restore(
                id,
                super::super::file_io::HistoryFileIoDirection::Undo,
                std::slice::from_ref(&action),
            )
            .unwrap();
        assert!(matches!(
            outcome,
            PreparedOperationOutcome::RetryPending { operation_id, .. } if operation_id == id
        ));
        let record = coordinator.record(id).unwrap();
        assert_eq!(record.phase, OperationPhase::IntentDurable);
        assert_eq!(record.disposition, OperationDisposition::RetryPending);
        assert!(record.prepared.is_none());
        assert_eq!(coordinator.store.capacity_claims(), &claims_before);
        assert_eq!(fs::read(&staging).unwrap(), staging_bytes);
        assert_eq!(fs::read(&target).unwrap(), vec![0_u8; 4097]);
        drop(coordinator);

        let reopened = OperationJournalCoordinator::open(dir.path().to_path_buf()).unwrap();
        let reopened_record = reopened.record(id).unwrap();
        assert_eq!(reopened_record.phase, OperationPhase::IntentDurable);
        assert_eq!(
            reopened_record.disposition,
            OperationDisposition::RetryPending
        );
        assert!(reopened_record.prepared.is_none());
        assert_eq!(reopened.store.capacity_claims(), &claims_before);
        assert_eq!(fs::read(&staging).unwrap(), staging_bytes);
    }

    #[test]
    fn schema_v1_fixture_without_optional_evidence_reopens_byte_for_byte() {
        let _lock = TEST_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let record = OperationRecord::new(intent(), serde_json::json!({"legacy": true}));
        let operation_id = record.operation_id;
        let path = dir.path().join(format!("{operation_id}.json"));
        let bytes = schema_v1_bytes(&record);
        fs::write(&path, &bytes).unwrap();

        let first = OperationJournalCoordinator::open(dir.path().to_path_buf()).unwrap();
        assert_eq!(first.record(operation_id), Some(&record));
        assert_eq!(first.recovery_summary().malformed_count, 0);
        assert_eq!(fs::read(&path).unwrap(), bytes);
        drop(first);

        let second = OperationJournalCoordinator::open(dir.path().to_path_buf()).unwrap();
        assert_eq!(second.record(operation_id), Some(&record));
        assert_eq!(fs::read(&path).unwrap(), bytes);
    }

    #[test]
    fn schema_v1_fixture_with_all_optional_evidence_reopens_byte_for_byte() {
        let _lock = TEST_LOCK.lock().unwrap();
        let (journal_dir, _files, mut journal, operation_id, _backup, _target, _staging) =
            prepared_restore_fixture();
        journal
            .stage_admitted_bounded_waveform_restore(operation_id)
            .unwrap();
        let before_publication = journal.record(operation_id).unwrap().clone();
        let publication = super::super::publication::test_publication_evidence(
            before_publication
                .prepared
                .as_ref()
                .unwrap()
                .as_existing()
                .unwrap(),
            before_publication.staged.as_ref().unwrap(),
            None,
        );
        journal.guarded_publish(operation_id, publication).unwrap();

        let expected = {
            let record = journal.store.records.get_mut(&operation_id).unwrap();
            record.replacement_qualification = Some(qualification_assessment());
            record.clone()
        };
        assert!(expected.capacity_plan.is_some());
        assert!(expected.prepared.is_some());
        assert!(expected.staged.is_some());
        assert!(expected.published.is_some());
        assert!(expected.replacement_qualification.is_some());
        let path = journal.store.record_path(operation_id);
        atomic_durable_write(&path, &expected).unwrap();
        let bytes = fs::read(&path).unwrap();
        drop(journal);

        let reopened = OperationJournalCoordinator::open(journal_dir.path().to_path_buf()).unwrap();
        assert_eq!(reopened.record(operation_id), Some(&expected));
        assert_eq!(fs::read(&path).unwrap(), bytes);
        drop(reopened);

        let reopened_again =
            OperationJournalCoordinator::open(journal_dir.path().to_path_buf()).unwrap();
        assert_eq!(reopened_again.record(operation_id), Some(&expected));
        assert_eq!(fs::read(&path).unwrap(), bytes);
    }

    #[test]
    fn unknown_v1_top_level_field_is_retained_and_cannot_be_rewritten() {
        let _lock = TEST_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let record = OperationRecord::new(intent(), Value::Null);
        let operation_id = record.operation_id;
        let path = dir.path().join(format!("{operation_id}.json"));
        let mut value = schema_v1_value(&record);
        value.as_object_mut().unwrap().insert(
            String::from("future_evidence"),
            serde_json::json!({"schema": 2, "retained": true}),
        );
        let bytes = serde_json::to_vec(&value).unwrap();
        fs::write(&path, &bytes).unwrap();

        let mut journal = OperationJournalCoordinator::open(dir.path().to_path_buf()).unwrap();
        let summary = journal.recovery_summary();
        assert_eq!(summary.malformed_count, 1);
        assert_eq!(summary.unknown_version_count, 0);
        assert!(summary.attention_required);
        assert!(journal.store.capacity_blocked());
        assert!(journal.record(operation_id).is_none());
        assert!(matches!(
            journal.update(
                operation_id,
                OperationPhase::IntentDurable,
                OperationDisposition::RetryPending,
            ),
            Err(JournalError::NotFound(id)) if id == operation_id
        ));
        drop(journal);

        let reopened = OperationJournalCoordinator::open(dir.path().to_path_buf()).unwrap();
        assert_eq!(reopened.recovery_summary().malformed_count, 1);
        assert!(reopened.recovery_summary().attention_required);
        assert_eq!(fs::read(&path).unwrap(), bytes);
    }

    #[test]
    fn unknown_v1_intent_field_is_retained_fail_closed() {
        let _lock = TEST_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let record = OperationRecord::new(intent(), Value::Null);
        let operation_id = record.operation_id;
        let path = dir.path().join(format!("{operation_id}.json"));
        let mut value = schema_v1_value(&record);
        value["intent"]
            .as_object_mut()
            .unwrap()
            .insert(String::from("future_intent_evidence"), Value::Bool(true));
        let bytes = serde_json::to_vec(&value).unwrap();
        fs::write(&path, &bytes).unwrap();

        assert_unknown_nested_record_is_retained_unchanged(dir.path(), &path, operation_id, &bytes);
    }

    #[test]
    fn unknown_v1_optional_prepared_evidence_field_is_retained_fail_closed() {
        let _lock = TEST_LOCK.lock().unwrap();
        let (journal_dir, _files, journal, operation_id, _backup, _target, _staging) =
            prepared_restore_fixture();
        let path = journal.store.record_path(operation_id);
        let mut value: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        value["prepared"]
            .as_object_mut()
            .unwrap()
            .insert(String::from("future_prepared_evidence"), Value::Bool(true));
        let bytes = serde_json::to_vec(&value).unwrap();
        fs::write(&path, &bytes).unwrap();
        drop(journal);

        assert_unknown_nested_record_is_retained_unchanged(
            journal_dir.path(),
            &path,
            operation_id,
            &bytes,
        );
    }

    #[test]
    fn unknown_v1_prepared_file_evidence_field_is_retained_fail_closed() {
        let _lock = TEST_LOCK.lock().unwrap();
        let (journal_dir, _files, journal, operation_id, _backup, _target, _staging) =
            prepared_restore_fixture();
        let path = journal.store.record_path(operation_id);
        let mut value: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        value["prepared"]["evidence"]
            .as_object_mut()
            .unwrap()
            .insert(String::from("future_file_evidence"), Value::Bool(true));
        let bytes = serde_json::to_vec(&value).unwrap();
        fs::write(&path, &bytes).unwrap();
        drop(journal);

        assert_unknown_nested_record_is_retained_unchanged(
            journal_dir.path(),
            &path,
            operation_id,
            &bytes,
        );
    }

    #[test]
    fn unknown_v1_capacity_volume_field_is_retained_fail_closed() {
        let _lock = TEST_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let record = OperationRecord::new_with_capacity_plan(
            intent(),
            Value::Null,
            Some(valid_capacity_plan()),
        );
        let operation_id = record.operation_id;
        let path = dir.path().join(format!("{operation_id}.json"));
        let mut value = schema_v1_value(&record);
        value["capacity_plan"]["volumes"][0]
            .as_object_mut()
            .unwrap()
            .insert(String::from("future_capacity_evidence"), Value::Bool(true));
        let bytes = serde_json::to_vec(&value).unwrap();
        fs::write(&path, &bytes).unwrap();

        assert_unknown_nested_record_is_retained_unchanged(dir.path(), &path, operation_id, &bytes);
    }

    #[test]
    fn future_schema_version_is_retained_blocking_and_idempotent() {
        let _lock = TEST_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let record = OperationRecord::new(intent(), Value::Null);
        let operation_id = record.operation_id;
        let path = dir.path().join(format!("{operation_id}.json"));
        let mut value = schema_v1_value(&record);
        value
            .as_object_mut()
            .unwrap()
            .insert(String::from("schema_version"), Value::from(3_u32));
        let bytes = serde_json::to_vec(&value).unwrap();
        fs::write(&path, &bytes).unwrap();

        let mut first = OperationJournalCoordinator::open(dir.path().to_path_buf()).unwrap();
        let first_summary = first.recovery_summary();
        assert_eq!(first_summary.malformed_count, 0);
        assert_eq!(first_summary.unknown_version_count, 1);
        assert!(first_summary.attention_required);
        assert!(first.store.capacity_blocked());
        assert!(first.record(operation_id).is_none());
        assert_eq!(fs::read(&path).unwrap(), bytes);
        assert!(matches!(
            first
                .store
                .admit(OperationRecord::new(intent(), Value::Null)),
            Err(JournalError::Write { .. })
        ));
        drop(first);

        let second = OperationJournalCoordinator::open(dir.path().to_path_buf()).unwrap();
        assert_eq!(second.recovery_summary(), first_summary);
        assert!(second.record(operation_id).is_none());
        assert_eq!(fs::read(&path).unwrap(), bytes);
    }

    #[test]
    fn malformed_v1_field_type_is_retained_and_not_admitted() {
        let _lock = TEST_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let record = OperationRecord::new(intent(), Value::Null);
        let operation_id = record.operation_id;
        let path = dir.path().join(format!("{operation_id}.json"));
        let mut value = schema_v1_value(&record);
        value.as_object_mut().unwrap().insert(
            String::from("phase"),
            serde_json::json!({"not": "an operation phase"}),
        );
        let bytes = serde_json::to_vec(&value).unwrap();
        fs::write(&path, &bytes).unwrap();

        let journal = OperationJournalCoordinator::open(dir.path().to_path_buf()).unwrap();
        let summary = journal.recovery_summary();
        assert_eq!(summary.malformed_count, 1);
        assert_eq!(summary.unknown_version_count, 0);
        assert!(summary.attention_required);
        assert!(journal.store.capacity_blocked());
        assert!(journal.record(operation_id).is_none());
        assert_eq!(fs::read(&path).unwrap(), bytes);
    }

    #[test]
    fn schema_v1_record_without_replacement_qualification_reopens_unchanged() {
        let _lock = TEST_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let record = OperationRecord::new(intent(), serde_json::json!({"legacy": true}));
        let operation_id = record.operation_id;
        let path = dir.path().join(format!("{operation_id}.json"));
        let mut value = schema_v1_value(&record);
        value
            .as_object_mut()
            .unwrap()
            .remove("replacement_qualification")
            .expect("current record has optional qualification field");
        let bytes = serde_json::to_vec(&value).unwrap();
        fs::write(&path, &bytes).unwrap();

        let journal = OperationJournalCoordinator::open(dir.path().to_path_buf()).unwrap();
        let reopened = journal.record(operation_id).expect("legacy record reopens");
        assert_eq!(reopened.operation_id, operation_id);
        assert_eq!(reopened.payload, serde_json::json!({"legacy": true}));
        assert_eq!(reopened.phase, OperationPhase::IntentDurable);
        assert_eq!(reopened.disposition, OperationDisposition::None);
        assert!(reopened.replacement_qualification.is_none());
        assert_eq!(journal.recovery_summary().malformed_count, 0);
        assert_eq!(fs::read(path).unwrap(), bytes);
    }

    fn assert_malformed_qualification_recovery(
        journal: &OperationJournalCoordinator,
        operation_id: Uuid,
        path: &Path,
        bytes: &[u8],
    ) {
        let summary = journal.recovery_summary();
        assert_eq!(summary.malformed_count, 1);
        assert!(summary.attention_required);
        assert!(journal.record(operation_id).is_none());
        assert!(journal.store.capacity_blocked());
        assert_eq!(fs::read(path).unwrap(), bytes);
    }

    #[test]
    fn unknown_replacement_qualification_field_is_retained_fail_closed() {
        let _lock = TEST_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let record = OperationRecord::new(intent(), Value::Null);
        let operation_id = record.operation_id;
        let path = dir.path().join(format!("{operation_id}.json"));
        let mut value = schema_v1_value(&record);
        value.as_object_mut().unwrap().insert(
            String::from("replacement_qualification"),
            serde_json::json!({
                "platform_family": "linux",
                "observed_filesystem": "same_volume",
                "volume": {"device": 1},
                "candidate": "no_public_candidate",
                "candidate_assessment": "no_qualified_candidate",
                "missing_invariant": "atomic_expected_target_identity_comparison",
                "decision": "platform_qualification_required",
                "retry_condition": "platform_build_or_qualification_policy_change",
                "future_evidence": true
            }),
        );
        let bytes = serde_json::to_vec(&value).unwrap();
        fs::write(&path, &bytes).unwrap();

        {
            let journal = OperationJournalCoordinator::open(dir.path().to_path_buf()).unwrap();
            assert_malformed_qualification_recovery(&journal, operation_id, &path, &bytes);
        }
        let reopened = OperationJournalCoordinator::open(dir.path().to_path_buf()).unwrap();
        assert_malformed_qualification_recovery(&reopened, operation_id, &path, &bytes);
        drop(reopened);
        assert_eq!(fs::read(path).unwrap(), bytes);
    }

    #[test]
    fn unknown_replacement_qualification_volume_field_is_retained_fail_closed() {
        let _lock = TEST_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let record = OperationRecord::new(intent(), Value::Null);
        let operation_id = record.operation_id;
        let path = dir.path().join(format!("{operation_id}.json"));
        let mut value = schema_v1_value(&record);
        value.as_object_mut().unwrap().insert(
            String::from("replacement_qualification"),
            serde_json::json!({
                "platform_family": "linux",
                "observed_filesystem": "same_volume",
                "volume": {"device": 1, "future_volume_evidence": true},
                "candidate": "no_public_candidate",
                "candidate_assessment": "no_qualified_candidate",
                "missing_invariant": "atomic_expected_target_identity_comparison",
                "decision": "platform_qualification_required",
                "retry_condition": "platform_build_or_qualification_policy_change"
            }),
        );
        let bytes = serde_json::to_vec(&value).unwrap();
        fs::write(&path, &bytes).unwrap();

        {
            let journal = OperationJournalCoordinator::open(dir.path().to_path_buf()).unwrap();
            assert_malformed_qualification_recovery(&journal, operation_id, &path, &bytes);
        }
        let reopened = OperationJournalCoordinator::open(dir.path().to_path_buf()).unwrap();
        assert_malformed_qualification_recovery(&reopened, operation_id, &path, &bytes);
        drop(reopened);
        assert_eq!(fs::read(path).unwrap(), bytes);
    }

    #[test]
    fn malformed_and_unknown_records_are_retained_and_reported() {
        let _lock = TEST_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("malformed.json"), b"not json").unwrap();
        fs::write(
            dir.path().join("unknown.json"),
            br#"{"schema_version":99,"operation_id":"00000000-0000-0000-0000-000000000000"}"#,
        )
        .unwrap();
        let journal = OperationJournalCoordinator::open(dir.path().to_path_buf()).unwrap();
        let summary = journal.recovery_summary();
        assert_eq!(summary.malformed_count, 1);
        assert_eq!(summary.unknown_version_count, 1);
        assert!(summary.attention_required);
        assert_eq!(
            fs::read(dir.path().join("malformed.json")).unwrap(),
            b"not json"
        );
    }

    #[test]
    fn prepared_record_without_evidence_is_retained_verbatim() {
        let _lock = TEST_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let mut record = OperationRecord::new(intent(), Value::Null);
        record.phase = OperationPhase::Prepared;
        let path = dir.path().join(format!("{}.json", record.operation_id));
        let bytes = schema_v1_bytes(&record);
        fs::write(&path, &bytes).unwrap();
        let journal = OperationJournalCoordinator::open(dir.path().to_path_buf()).unwrap();
        assert_eq!(journal.recovery_summary().malformed_count, 1);
        assert!(journal.recovery_summary().attention_required);
        assert!(journal.record(record.operation_id).is_none());
        assert_eq!(fs::read(path).unwrap(), bytes);
    }

    #[test]
    fn published_record_without_evidence_is_retained_and_attention_required() {
        let _lock = TEST_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let mut record = OperationRecord::new(intent(), Value::Null);
        record.phase = OperationPhase::FilesystemPublished;
        let path = dir.path().join(format!("{}.json", record.operation_id));
        let bytes = schema_v1_bytes(&record);
        fs::write(&path, &bytes).unwrap();
        let journal = OperationJournalCoordinator::open(dir.path().to_path_buf()).unwrap();
        let summary = journal.recovery_summary();
        assert_eq!(summary.malformed_count, 1);
        assert_eq!(summary.unresolved_count, 1);
        assert!(summary.attention_required);
        let retained = journal.record(record.operation_id).unwrap();
        assert_eq!(retained.phase, OperationPhase::FilesystemPublished);
        assert!(retained.published.is_none());
        assert!(journal.store.capacity_blocked());
        assert_eq!(fs::read(path).unwrap(), bytes);
    }

    #[test]
    fn legacy_source_reconciled_record_without_publication_evidence_is_accessible() {
        let _lock = TEST_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let mut record = OperationRecord::new(intent(), Value::Null);
        record.phase = OperationPhase::SourceReconciled;
        let path = dir.path().join(format!("{}.json", record.operation_id));
        let mut value = schema_v1_value(&record);
        value
            .as_object_mut()
            .unwrap()
            .remove("published")
            .expect("published field in current serialization");
        let bytes = serde_json::to_vec(&value).unwrap();
        fs::write(&path, &bytes).unwrap();

        let mut journal = OperationJournalCoordinator::open(dir.path().to_path_buf()).unwrap();
        let summary = journal.recovery_summary();
        let retained = journal.record(record.operation_id).unwrap();
        assert_eq!(retained.phase, OperationPhase::SourceReconciled);
        assert!(retained.published.is_none());
        assert_eq!(summary.malformed_count, 1);
        assert_eq!(summary.unresolved_count, 1);
        assert!(summary.attention_required);
        assert!(journal.store.capacity_blocked());
        assert!(matches!(
            journal.update(
                record.operation_id,
                OperationPhase::Terminal,
                OperationDisposition::CancelledBeforePublish,
            ),
            Err(JournalError::InvalidPublicationEvidence { .. })
        ));
        assert_eq!(fs::read(path).unwrap(), bytes);
    }

    #[test]
    fn legacy_filesystem_published_record_without_publication_evidence_is_accessible() {
        let _lock = TEST_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let mut record = OperationRecord::new(intent(), Value::Null);
        record.phase = OperationPhase::FilesystemPublished;
        let path = dir.path().join(format!("{}.json", record.operation_id));
        let mut value = schema_v1_value(&record);
        value
            .as_object_mut()
            .unwrap()
            .remove("published")
            .expect("published field in current serialization");
        let bytes = serde_json::to_vec(&value).unwrap();
        fs::write(&path, &bytes).unwrap();

        let mut journal = OperationJournalCoordinator::open(dir.path().to_path_buf()).unwrap();
        let summary = journal.recovery_summary();
        let retained = journal.record(record.operation_id).unwrap();
        assert_eq!(retained.phase, OperationPhase::FilesystemPublished);
        assert!(retained.published.is_none());
        assert_eq!(summary.malformed_count, 1);
        assert_eq!(summary.unresolved_count, 1);
        assert!(summary.attention_required);
        assert!(journal.store.capacity_blocked());
        assert!(matches!(
            journal.update(
                record.operation_id,
                OperationPhase::Terminal,
                OperationDisposition::CancelledBeforePublish,
            ),
            Err(JournalError::InvalidPublicationEvidence { .. })
        ));
        assert_eq!(fs::read(path).unwrap(), bytes);
    }

    #[test]
    fn oversize_record_is_retained_without_reading_payload() {
        let _lock = TEST_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("oversize.json");
        let bytes = vec![b'x'; MAX_RECORD_BYTES + 1];
        fs::write(&path, &bytes).unwrap();
        let journal = OperationJournalCoordinator::open(dir.path().to_path_buf()).unwrap();
        assert_eq!(journal.recovery_summary().oversize_count, 1);
        assert!(journal.recovery_summary().attention_required);
        assert_eq!(
            fs::metadata(path).unwrap().len(),
            (MAX_RECORD_BYTES + 1) as u64
        );
    }

    #[test]
    fn ownership_is_exclusive_until_store_is_dropped() {
        let _lock = TEST_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let journal = OperationJournalCoordinator::open(dir.path().to_path_buf()).unwrap();
        assert!(matches!(
            OperationJournalCoordinator::open(dir.path().to_path_buf()),
            Err(JournalError::OwnedByAnotherProcess { .. })
        ));
        drop(journal);
        assert!(OperationJournalCoordinator::open(dir.path().to_path_buf()).is_ok());
    }

    #[cfg(unix)]
    #[test]
    fn stale_lock_artifact_does_not_block_reopen() {
        let _lock = TEST_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join(LOCK_FILE_NAME), b"pid=unrelated\n").unwrap();
        let journal = OperationJournalCoordinator::open(dir.path().to_path_buf());
        assert!(journal.is_ok());
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_lock_path_fails_closed_without_touching_target() {
        let _lock = TEST_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("target");
        fs::write(&target, b"do not touch").unwrap();
        std::os::unix::fs::symlink(&target, dir.path().join(LOCK_FILE_NAME)).unwrap();
        let result = OperationJournalCoordinator::open(dir.path().to_path_buf());
        assert!(result.is_err());
        assert_eq!(fs::read(target).unwrap(), b"do not touch");
    }

    #[cfg(windows)]
    #[test]
    fn reparse_lock_path_fails_closed_without_touching_target() {
        let _lock = TEST_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("target");
        fs::write(&target, b"do not touch").unwrap();
        std::os::windows::fs::symlink_file(&target, dir.path().join(LOCK_FILE_NAME)).unwrap();
        let result = OperationJournalCoordinator::open(dir.path().to_path_buf());
        assert!(result.is_err());
        assert_eq!(fs::read(target).unwrap(), b"do not touch");
    }

    #[test]
    fn recovery_scan_is_idempotent_and_does_not_mutate_records() {
        let _lock = TEST_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let mut journal = OperationJournalCoordinator::open(dir.path().to_path_buf()).unwrap();
        let id = journal.admit(intent(), Value::Null).unwrap();
        drop(journal);
        let before = fs::read_dir(dir.path())
            .unwrap()
            .map(|entry| {
                let path = entry.unwrap().path();
                let is_lock =
                    path.file_name().and_then(|name| name.to_str()) == Some(LOCK_FILE_NAME);
                (
                    path.file_name().unwrap().to_owned(),
                    if is_lock {
                        Vec::new()
                    } else {
                        fs::read(path).unwrap_or_default()
                    },
                )
            })
            .collect::<Vec<_>>();
        let journal = OperationJournalCoordinator::open(dir.path().to_path_buf()).unwrap();
        assert_eq!(journal.recovery_summary().unresolved_count, 1);
        drop(journal);
        let after = fs::read_dir(dir.path())
            .unwrap()
            .map(|entry| {
                let path = entry.unwrap().path();
                let is_lock =
                    path.file_name().and_then(|name| name.to_str()) == Some(LOCK_FILE_NAME);
                (
                    path.file_name().unwrap().to_owned(),
                    if is_lock {
                        Vec::new()
                    } else {
                        fs::read(path).unwrap_or_default()
                    },
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(before, after);
        assert!(
            before
                .iter()
                .any(|(name, _)| name.to_string_lossy() == format!("{id}.json"))
        );
    }

    #[test]
    fn schema_v1_prepared_record_adapts_to_existing_identity_without_rewrite() {
        let _lock = TEST_LOCK.lock().unwrap();
        let (journal_dir, _files, journal, operation_id, _backup, _target, _staging) =
            prepared_restore_fixture();
        let record = journal.record(operation_id).unwrap().clone();
        assert_eq!(record.schema_version, SCHEMA_V1);
        assert!(matches!(
            record.prepared,
            Some(PreparedTargetContract::ExistingExpectedIdentity(_))
        ));
        let path = journal.store.record_path(operation_id);
        let bytes = fs::read(&path).unwrap();
        drop(journal);

        let reopened = OperationJournalCoordinator::open(journal_dir.path().to_path_buf()).unwrap();
        assert_eq!(reopened.record(operation_id), Some(&record));
        assert!(matches!(
            reopened.record(operation_id).unwrap().prepared,
            Some(PreparedTargetContract::ExistingExpectedIdentity(_))
        ));
        assert_eq!(fs::read(path).unwrap(), bytes);
    }

    #[test]
    fn schema_v1_absent_final_evidence_is_retained_raw_and_non_writable() {
        let _lock = TEST_LOCK.lock().unwrap();
        let (journal_dir, _files, mut journal, operation_id, _backup, _target, _staging) =
            prepared_restore_fixture();
        journal
            .stage_admitted_bounded_waveform_restore(operation_id)
            .unwrap();
        let record = journal.record(operation_id).unwrap().clone();
        let prepared = record.prepared.as_ref().unwrap().as_existing().unwrap();
        let staged = record.staged.as_ref().unwrap();
        let absent_publication = super::super::publication::test_absent_final_publication_evidence(
            &prepared.target_root.identity,
            staged,
        );
        let path = journal.store.record_path(operation_id);
        let mut value = schema_v1_value(&record);
        value["phase"] = serde_json::to_value(OperationPhase::FilesystemPublished).unwrap();
        value["published"] = serde_json::to_value(absent_publication).unwrap();
        let bytes = serde_json::to_vec(&value).unwrap();
        fs::write(&path, &bytes).unwrap();
        drop(journal);

        let mut reopened =
            OperationJournalCoordinator::open(journal_dir.path().to_path_buf()).unwrap();
        let summary = reopened.recovery_summary();
        assert_eq!(summary.malformed_count, 1);
        assert!(summary.attention_required);
        assert!(reopened.store.capacity_blocked());
        assert!(reopened.record(operation_id).is_none());
        assert!(matches!(
            reopened.update(
                operation_id,
                OperationPhase::IntentDurable,
                OperationDisposition::RetryPending,
            ),
            Err(JournalError::NotFound(id)) if id == operation_id
        ));
        assert_eq!(fs::read(&path).unwrap(), bytes);
    }

    #[test]
    fn schema_v2_absent_final_round_trips_staging_publication_and_remains_writable() {
        let _lock = TEST_LOCK.lock().unwrap();
        let directory = tempfile::tempdir().unwrap();
        let mut journal =
            OperationJournalCoordinator::open(directory.path().to_path_buf()).unwrap();
        let (operation_id, prepared, staged) = admit_absent_final_v2_fixture(&mut journal);
        let staged_record = journal.record(operation_id).unwrap().clone();
        assert_eq!(staged_record.schema_version, SCHEMA_V2);
        assert_eq!(staged_record.phase, OperationPhase::FilesystemStaged);
        assert!(matches!(
            staged_record.prepared,
            Some(PreparedTargetContract::AbsentFinalNoReplace(_))
        ));
        let path = journal.store.record_path(operation_id);
        let staged_bytes = fs::read(&path).unwrap();
        assert_eq!(
            journal
                .store
                .capacity_claims()
                .get(&VolumeIdentity { device: 77 }),
            Some(&4096)
        );
        drop(journal);

        let mut reopened =
            OperationJournalCoordinator::open(directory.path().to_path_buf()).unwrap();
        assert_eq!(reopened.record(operation_id), Some(&staged_record));
        assert_eq!(reopened.recovery_summary().unresolved_count, 1);
        assert!(reopened.recovery_summary().attention_required);
        assert!(!reopened.store.capacity_blocked());
        assert_eq!(
            reopened
                .store
                .capacity_claims()
                .get(&VolumeIdentity { device: 77 }),
            Some(&4096)
        );
        assert_eq!(fs::read(&path).unwrap(), staged_bytes);

        let publication = super::super::publication::test_absent_final_publication_evidence(
            &prepared.target_parent.identity,
            &staged,
        );
        reopened
            .guarded_publish(operation_id, publication.clone())
            .unwrap();
        assert_eq!(
            reopened.record(operation_id).unwrap().phase,
            OperationPhase::FilesystemPublished
        );
        assert_eq!(
            reopened.record(operation_id).unwrap().published.as_ref(),
            Some(&publication)
        );
        let published_bytes = fs::read(&path).unwrap();
        drop(reopened);

        let mut writable =
            OperationJournalCoordinator::open(directory.path().to_path_buf()).unwrap();
        assert_eq!(
            writable.record(operation_id).unwrap().published.as_ref(),
            Some(&publication)
        );
        assert_eq!(fs::read(&path).unwrap(), published_bytes);
        writable
            .update(
                operation_id,
                OperationPhase::SourceReconciled,
                OperationDisposition::None,
            )
            .unwrap();
        assert_eq!(
            writable.record(operation_id).unwrap().phase,
            OperationPhase::SourceReconciled
        );
    }

    #[test]
    fn schema_v2_generic_update_cannot_bypass_absent_final_publication_guard() {
        let _lock = TEST_LOCK.lock().unwrap();
        let directory = tempfile::tempdir().unwrap();
        let mut journal =
            OperationJournalCoordinator::open(directory.path().to_path_buf()).unwrap();
        let (operation_id, prepared, staged) = admit_absent_final_v2_fixture(&mut journal);
        assert!(matches!(
            journal.update(
                operation_id,
                OperationPhase::SourceReconciled,
                OperationDisposition::None,
            ),
            Err(JournalError::InvalidPublicationEvidence { .. })
        ));
        assert_eq!(
            journal.record(operation_id).unwrap().phase,
            OperationPhase::FilesystemStaged
        );
        let publication = super::super::publication::test_absent_final_publication_evidence(
            &prepared.target_parent.identity,
            &staged,
        );
        journal.guarded_publish(operation_id, publication).unwrap();
        assert!(matches!(
            journal.update(
                operation_id,
                OperationPhase::FilesystemPublished,
                OperationDisposition::None,
            ),
            Err(JournalError::IllegalTransition { .. })
        ));
    }

    #[test]
    fn schema_v2_absent_final_rejects_expected_identity_publication_in_both_directions() {
        let _lock = TEST_LOCK.lock().unwrap();

        let (_journal_dir, _files, mut expected_journal, expected_id, _backup, _target, _staging) =
            prepared_restore_fixture();
        expected_journal
            .stage_admitted_bounded_waveform_restore(expected_id)
            .unwrap();
        let expected_record = expected_journal.record(expected_id).unwrap().clone();
        let absent_publication = super::super::publication::test_absent_final_publication_evidence(
            &expected_record
                .prepared
                .as_ref()
                .unwrap()
                .as_existing()
                .unwrap()
                .target_root
                .identity,
            expected_record.staged.as_ref().unwrap(),
        );
        assert!(matches!(
            expected_journal.guarded_publish(expected_id, absent_publication),
            Err(JournalError::InvalidPublicationEvidence { .. })
        ));
        assert_eq!(
            expected_journal.record(expected_id).unwrap().phase,
            OperationPhase::FilesystemStaged
        );
        drop(expected_journal);

        let (
            _expected_dir,
            _expected_files,
            mut publication_source,
            publication_id,
            _backup,
            _target,
            _staging,
        ) = prepared_restore_fixture();
        publication_source
            .stage_admitted_bounded_waveform_restore(publication_id)
            .unwrap();
        let publication_source_record = publication_source.record(publication_id).unwrap().clone();
        let expected_publication = super::super::publication::test_publication_evidence(
            publication_source_record
                .prepared
                .as_ref()
                .unwrap()
                .as_existing()
                .unwrap(),
            publication_source_record.staged.as_ref().unwrap(),
            None,
        );
        drop(publication_source);

        let directory = tempfile::tempdir().unwrap();
        let mut absent_journal =
            OperationJournalCoordinator::open(directory.path().to_path_buf()).unwrap();
        let (operation_id, _, _) = admit_absent_final_v2_fixture(&mut absent_journal);
        assert!(matches!(
            absent_journal.guarded_publish(operation_id, expected_publication),
            Err(JournalError::InvalidPublicationEvidence { .. })
        ));
        assert_eq!(
            absent_journal.record(operation_id).unwrap().phase,
            OperationPhase::FilesystemStaged
        );
    }

    #[test]
    fn schema_v2_absent_final_rejects_mismatched_reopened_evidence_without_mutation() {
        let _lock = TEST_LOCK.lock().unwrap();
        let directory = tempfile::tempdir().unwrap();
        let mut journal =
            OperationJournalCoordinator::open(directory.path().to_path_buf()).unwrap();
        let (operation_id, prepared, staged) = admit_absent_final_v2_fixture(&mut journal);
        let publication = super::super::publication::test_absent_final_publication_evidence(
            &prepared.target_parent.identity,
            &staged,
        );
        let mismatched = publication_with_reopened_identity_mismatch(&publication);
        let path = journal.store.record_path(operation_id);
        let bytes_before = fs::read(&path).unwrap();
        assert!(matches!(
            journal.guarded_publish(operation_id, mismatched),
            Err(JournalError::InvalidPublicationEvidence { .. })
        ));
        assert_eq!(
            journal.record(operation_id).unwrap().phase,
            OperationPhase::FilesystemStaged
        );
        assert!(journal.record(operation_id).unwrap().published.is_none());
        assert_eq!(fs::read(path).unwrap(), bytes_before);
    }

    #[test]
    fn schema_v2_absent_final_publication_replay_is_idempotent_and_conflicts_fail_closed() {
        let _lock = TEST_LOCK.lock().unwrap();
        let directory = tempfile::tempdir().unwrap();
        let mut journal =
            OperationJournalCoordinator::open(directory.path().to_path_buf()).unwrap();
        let (operation_id, prepared, staged) = admit_absent_final_v2_fixture(&mut journal);
        let publication = super::super::publication::test_absent_final_publication_evidence(
            &prepared.target_parent.identity,
            &staged,
        );
        journal
            .guarded_publish(operation_id, publication.clone())
            .unwrap();
        let path = journal.store.record_path(operation_id);
        let bytes_after_publish = fs::read(&path).unwrap();
        journal
            .guarded_publish(operation_id, publication.clone())
            .unwrap();
        assert_eq!(fs::read(&path).unwrap(), bytes_after_publish);
        drop(journal);

        let mut reopened =
            OperationJournalCoordinator::open(directory.path().to_path_buf()).unwrap();
        reopened
            .guarded_publish(operation_id, publication.clone())
            .unwrap();
        assert_eq!(fs::read(&path).unwrap(), bytes_after_publish);
        let conflicting = publication_with_reopened_identity_mismatch(&publication);
        assert!(matches!(
            reopened.guarded_publish(operation_id, conflicting),
            Err(JournalError::InvalidPublicationEvidence { .. })
        ));
        assert_eq!(fs::read(&path).unwrap(), bytes_after_publish);
        assert_eq!(
            reopened.record(operation_id).unwrap().published.as_ref(),
            Some(&publication)
        );
    }

    #[test]
    fn schema_v2_unknown_and_malformed_nested_evidence_are_retained_fail_closed() {
        let _lock = TEST_LOCK.lock().unwrap();
        assert_v2_malformed_record_retained(|value| {
            value.as_object_mut().unwrap().insert(
                String::from("future_v2_evidence"),
                serde_json::json!({"retained": true}),
            );
        });
        assert_v2_malformed_record_retained(|value| {
            value["prepared"]["AbsentFinalNoReplace"]
                .as_object_mut()
                .unwrap()
                .insert(String::from("future_nested_evidence"), Value::Bool(true));
        });
        assert_v2_malformed_record_retained(|value| {
            value["prepared"]["AbsentFinalNoReplace"]["copy_validated_evidence"] =
                Value::String(String::from("MalformedEvidence"));
        });
    }

    #[test]
    fn schema_v2_absent_final_recovery_observation_round_trips_and_old_v2_defaults_absent() {
        let _lock = TEST_LOCK.lock().unwrap();
        let (directory, operation_id, path, mut value) = v2_absent_record_on_disk();
        add_valid_recovery_observation(&mut value);
        add_valid_transaction_owned_proof(&mut value);
        let bytes = serde_json::to_vec(&value).unwrap();
        fs::write(&path, &bytes).unwrap();

        let journal = OperationJournalCoordinator::open(directory.path().to_path_buf()).unwrap();
        let observation = journal
            .record(operation_id)
            .unwrap()
            .absent_final_recovery_observation
            .clone();
        assert!(observation.is_some());
        assert!(
            journal
                .record(operation_id)
                .unwrap()
                .absent_final_transaction_owned_proof
                .is_some()
        );
        assert_eq!(journal.recovery_summary().malformed_count, 0);
        assert_eq!(fs::read(&path).unwrap(), bytes);
        drop(journal);

        value
            .as_object_mut()
            .unwrap()
            .remove("absent_final_transaction_owned_proof");
        let old_v2_bytes = serde_json::to_vec(&value).unwrap();
        fs::write(&path, &old_v2_bytes).unwrap();
        let old_v2 = OperationJournalCoordinator::open(directory.path().to_path_buf()).unwrap();
        assert_eq!(
            old_v2
                .record(operation_id)
                .unwrap()
                .absent_final_recovery_observation,
            observation
        );
        assert_eq!(
            old_v2
                .record(operation_id)
                .unwrap()
                .absent_final_transaction_owned_proof,
            None
        );
        assert_eq!(old_v2.recovery_summary().malformed_count, 0);
        assert_eq!(fs::read(&path).unwrap(), old_v2_bytes);
    }

    #[test]
    fn schema_v1_recovery_observation_is_unknown_and_not_encoded() {
        let _lock = TEST_LOCK.lock().unwrap();
        let directory = tempfile::tempdir().unwrap();
        let record = OperationRecord::new(intent(), Value::Null);
        let operation_id = record.operation_id;
        let path = directory.path().join(format!("{operation_id}.json"));
        let mut value = schema_v1_value(&record);
        add_valid_recovery_observation(&mut value);
        let bytes = serde_json::to_vec(&value).unwrap();
        fs::write(&path, &bytes).unwrap();

        let journal = OperationJournalCoordinator::open(directory.path().to_path_buf()).unwrap();
        assert!(journal.record(operation_id).is_none());
        assert_eq!(journal.recovery_summary().malformed_count, 1);
        assert_eq!(fs::read(&path).unwrap(), bytes);

        let mut runtime = record;
        runtime.absent_final_recovery_observation = Some(AbsentFinalRecoveryObservation {
            target_parent_stable_id: String::from("parent"),
            final_stable_id: String::from("final"),
            final_len: 4,
            final_content: PreparedFileEvidence::ContentHash([7; 32]),
        });
        assert!(encode_schema_v1(&runtime).is_err());
    }

    #[test]
    fn schema_v1_transaction_owned_proof_is_unknown_and_not_encoded() {
        let _lock = TEST_LOCK.lock().unwrap();
        let directory = tempfile::tempdir().unwrap();
        let record = OperationRecord::new(intent(), Value::Null);
        let operation_id = record.operation_id;
        let path = directory.path().join(format!("{operation_id}.json"));
        let mut value = schema_v1_value(&record);
        add_valid_transaction_owned_proof(&mut value);
        let bytes = serde_json::to_vec(&value).unwrap();
        fs::write(&path, &bytes).unwrap();

        let journal = OperationJournalCoordinator::open(directory.path().to_path_buf()).unwrap();
        assert!(journal.record(operation_id).is_none());
        assert_eq!(journal.recovery_summary().malformed_count, 1);
        assert_eq!(fs::read(&path).unwrap(), bytes);

        let mut runtime = record;
        runtime.absent_final_transaction_owned_proof = Some(AbsentFinalTransactionOwnedProof {
            target_parent_stable_id: String::from("parent"),
            final_stable_id: String::from("final"),
            final_len: 4,
            final_content: PreparedFileEvidence::ContentHash([7; 32]),
        });
        assert!(encode_schema_v1(&runtime).is_err());
    }

    #[test]
    fn schema_v2_invalid_recovery_observation_evidence_is_retained_unchanged() {
        let _lock = TEST_LOCK.lock().unwrap();
        assert_v2_recovery_observation_retained_as_malformed(|value| {
            value["absent_final_recovery_observation"]
                .as_object_mut()
                .unwrap()
                .insert(String::from("future_nested_evidence"), Value::Bool(true));
        });
        assert_v2_recovery_observation_retained_as_malformed(|value| {
            value["absent_final_recovery_observation"]["final_content"] = serde_json::json!({
                "Metadata": {"len": 4, "modified_ns": null, "is_dir": false}
            });
        });
        assert_v2_recovery_observation_retained_as_malformed(|value| {
            value["phase"] = serde_json::to_value(OperationPhase::Prepared).unwrap();
        });
        assert_v2_recovery_observation_retained_as_malformed(|value| {
            value["staged"] = Value::Null;
        });
        assert_v2_recovery_observation_retained_as_malformed(|value| {
            let prepared = value["prepared"]["AbsentFinalNoReplace"].clone();
            value["prepared"] = serde_json::json!({"ExistingExpectedIdentity": prepared});
        });
        assert_v2_recovery_observation_retained_as_malformed(|value| {
            value["absent_final_recovery_observation"]["final_len"] = Value::from(99_u64);
        });
        assert_v2_recovery_observation_retained_as_malformed(|value| {
            value["absent_final_recovery_observation"]["final_stable_id"] =
                Value::from("different-final");
        });
    }

    #[test]
    fn schema_v2_invalid_transaction_owned_proof_evidence_is_retained_unchanged() {
        let _lock = TEST_LOCK.lock().unwrap();
        assert_v2_transaction_owned_proof_retained_as_malformed(|value| {
            value["absent_final_transaction_owned_proof"]
                .as_object_mut()
                .unwrap()
                .insert(String::from("future_nested_evidence"), Value::Bool(true));
        });
        assert_v2_transaction_owned_proof_retained_as_malformed(|value| {
            value["absent_final_transaction_owned_proof"]["final_content"] = serde_json::json!({
                "Metadata": {"len": 4, "modified_ns": null, "is_dir": false}
            });
        });
        assert_v2_transaction_owned_proof_retained_as_malformed(|value| {
            value["absent_final_transaction_owned_proof"]["final_len"] = Value::from(99_u64);
        });
        assert_v2_transaction_owned_proof_retained_as_malformed(|value| {
            value["absent_final_transaction_owned_proof"]["final_stable_id"] =
                Value::from("different-final");
        });
        assert_v2_transaction_owned_proof_retained_as_malformed(|value| {
            value
                .as_object_mut()
                .unwrap()
                .remove("absent_final_recovery_observation");
        });
        assert_v2_transaction_owned_proof_retained_as_malformed(|value| {
            value["phase"] = serde_json::to_value(OperationPhase::Prepared).unwrap();
        });
    }

    #[test]
    fn schema_v2_phase_evidence_matrix_rejects_inconsistent_records() {
        let _lock = TEST_LOCK.lock().unwrap();
        let invalid_cases = [
            (
                "intent with prepared evidence",
                OperationPhase::IntentDurable,
                OperationDisposition::None,
                SchemaV2EvidencePresence::PREPARED,
            ),
            (
                "intent with absent-final recovery observation",
                OperationPhase::IntentDurable,
                OperationDisposition::None,
                SchemaV2EvidencePresence::PREPARED_STAGED_WITH_ABSENT_FINAL_RECOVERY_OBSERVATION,
            ),
            (
                "intent with staged evidence",
                OperationPhase::IntentDurable,
                OperationDisposition::None,
                SchemaV2EvidencePresence {
                    prepared: false,
                    staged: true,
                    absent_final_recovery_observation: false,
                    absent_final_transaction_owned_proof: false,
                    published: false,
                },
            ),
            (
                "intent with published evidence",
                OperationPhase::IntentDurable,
                OperationDisposition::None,
                SchemaV2EvidencePresence {
                    prepared: false,
                    staged: false,
                    absent_final_recovery_observation: false,
                    absent_final_transaction_owned_proof: false,
                    published: true,
                },
            ),
            (
                "filesystem staged with transaction-owned proof but no observation",
                OperationPhase::FilesystemStaged,
                OperationDisposition::None,
                SchemaV2EvidencePresence {
                    prepared: true,
                    staged: true,
                    absent_final_recovery_observation: false,
                    absent_final_transaction_owned_proof: true,
                    published: false,
                },
            ),
            (
                "prepared without prepared evidence",
                OperationPhase::Prepared,
                OperationDisposition::None,
                SchemaV2EvidencePresence::NONE,
            ),
            (
                "prepared with staged evidence",
                OperationPhase::Prepared,
                OperationDisposition::None,
                SchemaV2EvidencePresence::PREPARED_STAGED,
            ),
            (
                "prepared with absent-final recovery observation",
                OperationPhase::Prepared,
                OperationDisposition::None,
                SchemaV2EvidencePresence::PREPARED_STAGED_WITH_ABSENT_FINAL_RECOVERY_OBSERVATION,
            ),
            (
                "prepared with published evidence",
                OperationPhase::Prepared,
                OperationDisposition::None,
                SchemaV2EvidencePresence {
                    prepared: true,
                    staged: false,
                    absent_final_recovery_observation: false,
                    absent_final_transaction_owned_proof: false,
                    published: true,
                },
            ),
            (
                "filesystem staged without prepared evidence",
                OperationPhase::FilesystemStaged,
                OperationDisposition::None,
                SchemaV2EvidencePresence {
                    prepared: false,
                    staged: true,
                    absent_final_recovery_observation: false,
                    absent_final_transaction_owned_proof: false,
                    published: false,
                },
            ),
            (
                "filesystem staged without staged evidence",
                OperationPhase::FilesystemStaged,
                OperationDisposition::None,
                SchemaV2EvidencePresence::PREPARED,
            ),
            (
                "filesystem staged with published evidence",
                OperationPhase::FilesystemStaged,
                OperationDisposition::None,
                SchemaV2EvidencePresence::ALL,
            ),
            (
                "filesystem published without prepared evidence",
                OperationPhase::FilesystemPublished,
                OperationDisposition::None,
                SchemaV2EvidencePresence {
                    prepared: false,
                    staged: true,
                    absent_final_recovery_observation: false,
                    absent_final_transaction_owned_proof: false,
                    published: true,
                },
            ),
            (
                "filesystem published without staged evidence",
                OperationPhase::FilesystemPublished,
                OperationDisposition::None,
                SchemaV2EvidencePresence {
                    prepared: true,
                    staged: false,
                    absent_final_recovery_observation: false,
                    absent_final_transaction_owned_proof: false,
                    published: true,
                },
            ),
            (
                "filesystem published without published evidence",
                OperationPhase::FilesystemPublished,
                OperationDisposition::None,
                SchemaV2EvidencePresence::PREPARED_STAGED,
            ),
            (
                "pre-publication cancellation with published evidence",
                OperationPhase::Terminal,
                OperationDisposition::CancelledBeforePublish,
                SchemaV2EvidencePresence::ALL,
            ),
            (
                "successful terminal record without publication evidence",
                OperationPhase::Terminal,
                OperationDisposition::Succeeded,
                SchemaV2EvidencePresence::PREPARED_STAGED,
            ),
            (
                "post-publication cancellation without publication evidence",
                OperationPhase::Terminal,
                OperationDisposition::CancelledAfterPublish,
                SchemaV2EvidencePresence::PREPARED_STAGED,
            ),
        ];
        for (label, phase, disposition, evidence) in invalid_cases {
            assert_v2_phase_evidence_record_is_retained_unchanged(phase, disposition, evidence);
            assert!(
                !schema_v2_phase_evidence_is_valid(phase, disposition, evidence),
                "{label}"
            );
        }

        let valid_cases = [
            (
                OperationPhase::IntentDurable,
                OperationDisposition::None,
                SchemaV2EvidencePresence::NONE,
            ),
            (
                OperationPhase::Prepared,
                OperationDisposition::None,
                SchemaV2EvidencePresence::PREPARED,
            ),
            (
                OperationPhase::FilesystemStaged,
                OperationDisposition::None,
                SchemaV2EvidencePresence::PREPARED_STAGED,
            ),
            (
                OperationPhase::FilesystemStaged,
                OperationDisposition::None,
                SchemaV2EvidencePresence::PREPARED_STAGED_WITH_ABSENT_FINAL_RECOVERY_OBSERVATION,
            ),
            (
                OperationPhase::FilesystemStaged,
                OperationDisposition::None,
                SchemaV2EvidencePresence::PREPARED_STAGED_WITH_ABSENT_FINAL_RECOVERY_PROOF,
            ),
            (
                OperationPhase::Terminal,
                OperationDisposition::CancelledBeforePublish,
                SchemaV2EvidencePresence::NONE,
            ),
            (
                OperationPhase::Terminal,
                OperationDisposition::CancelledBeforePublish,
                SchemaV2EvidencePresence::PREPARED,
            ),
            (
                OperationPhase::Terminal,
                OperationDisposition::CancelledBeforePublish,
                SchemaV2EvidencePresence::PREPARED_STAGED,
            ),
            (
                OperationPhase::Terminal,
                OperationDisposition::Succeeded,
                SchemaV2EvidencePresence::ALL,
            ),
        ];
        for (phase, disposition, evidence) in valid_cases {
            assert!(
                schema_v2_phase_evidence_is_valid(phase, disposition, evidence),
                "valid phase/evidence combination rejected: {phase:?} {disposition:?} {evidence:?}"
            );
        }
        for phase in [
            OperationPhase::FilesystemPublished,
            OperationPhase::SourceReconciled,
            OperationPhase::GlobalReconciled,
            OperationPhase::ProjectionPublished,
            OperationPhase::ReadinessScheduled,
        ] {
            assert!(schema_v2_phase_evidence_is_valid(
                phase,
                OperationDisposition::None,
                SchemaV2EvidencePresence::ALL,
            ));
            assert!(!schema_v2_phase_evidence_is_valid(
                phase,
                OperationDisposition::None,
                SchemaV2EvidencePresence::PREPARED_STAGED,
            ));
        }
    }

    #[test]
    fn schema_v2_invalid_direct_admission_leaves_journal_untouched() {
        let _lock = TEST_LOCK.lock().unwrap();
        for use_capacity_admission in [false, true] {
            let directory = tempfile::tempdir().unwrap();
            let mut store = OperationJournalStore::open(directory.path().to_path_buf()).unwrap();
            let record = invalid_v2_admission_record();
            let operation_id = record.operation_id;
            let path = store.record_path(operation_id);
            let temp_path = directory.path().join(format!(".{operation_id}.json.tmp"));
            let recovery_before = store.recovery_summary();

            let result = if use_capacity_admission {
                store.admit_capacity(record)
            } else {
                store.admit(record)
            };
            assert_invalid_input_write(result.unwrap_err(), &path);
            assert!(!path.exists());
            assert!(!temp_path.exists());
            assert!(store.record(operation_id).is_none());
            assert!(store.capacity_claims().is_empty());
            assert!(!store.capacity_blocked());
            assert_eq!(store.recovery_summary(), recovery_before);
            drop(store);

            let reopened = OperationJournalStore::open(directory.path().to_path_buf()).unwrap();
            assert_eq!(reopened.recovery_summary(), recovery_before);
            assert!(reopened.record(operation_id).is_none());
            assert!(reopened.capacity_claims().is_empty());
            assert!(!reopened.capacity_blocked());
            assert!(!path.exists());
            assert!(!temp_path.exists());
        }
    }

    #[test]
    fn schema_v2_invalid_backward_updates_leave_record_unchanged() {
        let _lock = TEST_LOCK.lock().unwrap();
        let directory = tempfile::tempdir().unwrap();
        let mut journal =
            OperationJournalCoordinator::open(directory.path().to_path_buf()).unwrap();
        let (operation_id, _, _) = admit_absent_final_v2_fixture(&mut journal);
        let path = journal.store.record_path(operation_id);
        let temp_path = directory.path().join(format!(".{operation_id}.json.tmp"));
        let record_before = journal.record(operation_id).unwrap().clone();
        let bytes_before = fs::read(&path).unwrap();
        let claims_before = journal.store.capacity_claims().clone();
        let recovery_before = journal.recovery_summary();

        for (phase, disposition) in [
            (
                OperationPhase::IntentDurable,
                OperationDisposition::RetryPending,
            ),
            (OperationPhase::Prepared, OperationDisposition::None),
        ] {
            let error = journal
                .store
                .update(operation_id, phase, disposition)
                .unwrap_err();
            assert_invalid_input_write(error, &path);
            assert_eq!(journal.record(operation_id), Some(&record_before));
            assert_eq!(fs::read(&path).unwrap(), bytes_before);
            assert!(!temp_path.exists());
            assert_eq!(journal.store.capacity_claims(), &claims_before);
            assert_eq!(journal.recovery_summary(), recovery_before);
        }

        drop(journal);
        let reopened = OperationJournalCoordinator::open(directory.path().to_path_buf()).unwrap();
        assert_eq!(reopened.record(operation_id), Some(&record_before));
        assert_eq!(reopened.recovery_summary().record_count, 1);
        assert_eq!(reopened.recovery_summary().unresolved_count, 1);
        assert_eq!(reopened.recovery_summary().malformed_count, 0);
        assert!(reopened.recovery_summary().attention_required);
        assert_eq!(reopened.store.capacity_claims(), &claims_before);
        assert_eq!(fs::read(&path).unwrap(), bytes_before);
        assert!(!temp_path.exists());
    }

    #[test]
    fn schema_v2_cancelled_before_publish_prefixes_cross_durable_boundary() {
        let _lock = TEST_LOCK.lock().unwrap();
        for evidence in [
            SchemaV2EvidencePresence::NONE,
            SchemaV2EvidencePresence::PREPARED,
            SchemaV2EvidencePresence::PREPARED_STAGED,
        ] {
            let directory = tempfile::tempdir().unwrap();
            let mut journal =
                OperationJournalCoordinator::open(directory.path().to_path_buf()).unwrap();
            let (operation_id, _, _) = admit_absent_final_v2_fixture(&mut journal);
            let mut record = journal.record(operation_id).unwrap().clone();
            if !evidence.prepared {
                record.prepared = None;
            }
            if !evidence.staged {
                record.staged = None;
            }
            record.published = None;
            record.phase = OperationPhase::Terminal;
            record.disposition = OperationDisposition::CancelledBeforePublish;
            let path = journal.store.record_path(operation_id);
            atomic_durable_write(&path, &record).unwrap();
            let bytes = fs::read(&path).unwrap();
            let temp_path = directory.path().join(format!(".{operation_id}.json.tmp"));
            drop(journal);

            let reopened =
                OperationJournalCoordinator::open(directory.path().to_path_buf()).unwrap();
            assert_eq!(reopened.record(operation_id), Some(&record));
            assert_eq!(reopened.recovery_summary().malformed_count, 0);
            assert!(!reopened.recovery_summary().attention_required);
            assert!(reopened.store.capacity_claims().is_empty());
            assert_eq!(fs::read(&path).unwrap(), bytes);
            assert!(!temp_path.exists());
        }
    }

    #[test]
    fn schema_v2_future_version_is_retained_blocking_and_byte_stable() {
        let _lock = TEST_LOCK.lock().unwrap();
        let (directory, operation_id, path, mut value) = v2_absent_record_on_disk();
        value["schema_version"] = Value::from(3_u32);
        let bytes = serde_json::to_vec(&value).unwrap();
        fs::write(&path, &bytes).unwrap();

        for _ in 0..2 {
            let mut journal =
                OperationJournalCoordinator::open(directory.path().to_path_buf()).unwrap();
            let summary = journal.recovery_summary();
            assert_eq!(summary.malformed_count, 0);
            assert_eq!(summary.unknown_version_count, 1);
            assert!(summary.attention_required);
            assert!(journal.store.capacity_blocked());
            assert!(journal.record(operation_id).is_none());
            assert!(matches!(
                journal.update(
                    operation_id,
                    OperationPhase::IntentDurable,
                    OperationDisposition::RetryPending,
                ),
                Err(JournalError::NotFound(id)) if id == operation_id
            ));
            assert!(matches!(
                journal.admit(intent(), Value::Null),
                Err(JournalError::Write { .. })
            ));
            assert_eq!(fs::read(&path).unwrap(), bytes);
        }
    }

    #[test]
    fn schema_v2_prepublication_publication_evidence_is_retained_verbatim() {
        let _lock = TEST_LOCK.lock().unwrap();
        let directory = tempfile::tempdir().unwrap();
        let mut journal =
            OperationJournalCoordinator::open(directory.path().to_path_buf()).unwrap();
        let (operation_id, prepared, staged) = admit_absent_final_v2_fixture(&mut journal);
        let publication = super::super::publication::test_absent_final_publication_evidence(
            &prepared.target_parent.identity,
            &staged,
        );
        let mut invalid = journal.record(operation_id).unwrap().clone();
        invalid.published = Some(publication);
        let path = journal.store.record_path(operation_id);
        let invalid_bytes = encode_record(&invalid).unwrap();
        fs::write(&path, invalid_bytes).unwrap();
        let bytes = fs::read(&path).unwrap();
        drop(journal);

        let reopened = OperationJournalCoordinator::open(directory.path().to_path_buf()).unwrap();
        assert_eq!(reopened.recovery_summary().malformed_count, 1);
        assert!(reopened.recovery_summary().attention_required);
        assert!(reopened.store.capacity_blocked());
        assert!(reopened.record(operation_id).is_none());
        assert_eq!(fs::read(path).unwrap(), bytes);
    }

    #[test]
    fn schema_v2_postpublication_missing_evidence_is_retained_raw_and_not_admitted() {
        let _lock = TEST_LOCK.lock().unwrap();
        let directory = tempfile::tempdir().unwrap();
        let mut journal =
            OperationJournalCoordinator::open(directory.path().to_path_buf()).unwrap();
        let (operation_id, _, _) = admit_absent_final_v2_fixture(&mut journal);
        let mut invalid = journal.record(operation_id).unwrap().clone();
        invalid.phase = OperationPhase::FilesystemPublished;
        invalid.published = None;
        let path = journal.store.record_path(operation_id);
        let invalid_bytes = encode_record(&invalid).unwrap();
        fs::write(&path, invalid_bytes).unwrap();
        let bytes = fs::read(&path).unwrap();
        drop(journal);

        let mut reopened =
            OperationJournalCoordinator::open(directory.path().to_path_buf()).unwrap();
        let summary = reopened.recovery_summary();
        assert_eq!(summary.malformed_count, 1);
        assert_eq!(summary.unresolved_count, 0);
        assert!(summary.attention_required);
        assert!(reopened.store.capacity_blocked());
        assert!(reopened.record(operation_id).is_none());
        assert!(matches!(
            reopened.update(
                operation_id,
                OperationPhase::IntentDurable,
                OperationDisposition::RetryPending,
            ),
            Err(JournalError::NotFound(id)) if id == operation_id
        ));
        assert_eq!(fs::read(path).unwrap(), bytes);
    }

    #[test]
    fn schema_v2_absent_final_production_workflow_stays_staged_and_auditable() {
        let _lock = TEST_LOCK.lock().unwrap();
        let directory = tempfile::tempdir().unwrap();
        let mut journal =
            OperationJournalCoordinator::open(directory.path().to_path_buf()).unwrap();
        let (operation_id, _, staged) = admit_absent_final_v2_fixture(&mut journal);

        assert!(matches!(
            journal
                .stage_admitted_bounded_waveform_restore(operation_id)
                .unwrap(),
            FilesystemStageOutcome::AuditRequired { operation_id: id, .. } if id == operation_id
        ));
        let record = journal.record(operation_id).unwrap();
        assert_eq!(record.phase, OperationPhase::FilesystemStaged);
        assert_eq!(record.disposition, OperationDisposition::AuditRequired);
        assert_eq!(record.staged.as_ref(), Some(&staged));
        assert!(record.published.is_none());

        assert!(matches!(
            journal.attempt_publish_staged_waveform_restore(operation_id),
            Ok(FilesystemStageOutcome::AuditRequired { operation_id: id, .. }) if id == operation_id
        ));
        let record = journal.record(operation_id).unwrap();
        assert_eq!(record.phase, OperationPhase::FilesystemStaged);
        assert_eq!(record.disposition, OperationDisposition::AuditRequired);
        assert_eq!(record.staged.as_ref(), Some(&staged));
        assert!(record.published.is_none());
    }

    #[test]
    fn absent_final_recovery_rejects_schema_v2_existing_target_contract() {
        let _lock = TEST_LOCK.lock().unwrap();
        let directory = tempfile::tempdir().unwrap();
        let mut journal =
            OperationJournalCoordinator::open(directory.path().to_path_buf()).unwrap();
        let (prepared, staged, capacity_plan) = absent_final_v2_fixture();
        let FilesystemStagedParticipant::CopyValidated { staging, evidence } =
            staged.participant.clone();
        let identity = staging.identity.clone();
        let existing = PreparedWaveformRestore {
            direction: prepared.direction,
            source_id: prepared.source_id.clone(),
            source_root: prepared.source_root.clone(),
            target_root: prepared.target_parent.clone(),
            target: PreparedLeafLocator {
                relative_path: prepared.final_leaf.clone(),
                identity: identity.clone(),
            },
            backup_root: prepared.target_parent.clone(),
            backup: PreparedLeafLocator {
                relative_path: prepared.staging.relative_path.clone(),
                identity: identity.clone(),
            },
            replacement: ReplaceExpectedIdentity::Existing(identity),
            staging: prepared.staging.clone(),
            evidence: PreparedRestoreEvidence {
                target: PreparedFileEvidence::Missing,
                backup: evidence,
            },
        };
        let mut record = OperationRecord::new_v2_absent_final_with_capacity_plan(
            intent(),
            serde_json::json!({"schema": 2}),
            prepared,
            staged,
            capacity_plan,
        );
        let operation_id = record.operation_id;
        record.prepared = Some(PreparedTargetContract::ExistingExpectedIdentity(existing));
        journal.store.admit_capacity(record).unwrap();

        assert!(matches!(
            journal.classify_schema_v2_absent_final_recovery(operation_id),
            Err(JournalError::InvalidPublicationEvidence { .. })
        ));
    }
}
