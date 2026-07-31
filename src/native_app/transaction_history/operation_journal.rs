//! Profile-local durable intent journal for future transaction recovery.
//!
//! This module records intent and coordinator state only. It deliberately does not
//! contain, replay, or interpret filesystem actions. Startup recovery is a bounded
//! scan that retains every record it cannot decode and reports that attention is
//! required.

#![allow(dead_code)]

use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use super::capacity_gate::{DurableCapacityPlan, RejectedBeforeIntent, VolumeIdentity};

const CURRENT_SCHEMA_VERSION: u32 = 1;
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

/// One complete, bounded durable operation record.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
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
    #[serde(default)]
    pub(crate) capacity_plan: Option<DurableCapacityPlan>,
    /// Creation timestamp in Unix milliseconds.
    pub(crate) created_unix_ms: i64,
    /// Last update timestamp in Unix milliseconds.
    pub(crate) updated_unix_ms: i64,
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
            schema_version: CURRENT_SCHEMA_VERSION,
            operation_id: Uuid::new_v4(),
            intent,
            phase: OperationPhase::IntentDurable,
            disposition: OperationDisposition::None,
            payload,
            capacity_plan,
            created_unix_ms: now,
            updated_unix_ms: now,
        }
    }

    fn with_update(&self, phase: OperationPhase, disposition: OperationDisposition) -> Self {
        let mut updated = self.clone();
        updated.phase = phase;
        updated.disposition = disposition;
        updated.updated_unix_ms = unix_millis();
        updated
    }
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
        if record.schema_version != CURRENT_SCHEMA_VERSION {
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
        let current = self
            .records
            .get(&operation_id)
            .ok_or(JournalError::NotFound(operation_id))?;
        let updated = current.with_update(phase, disposition);
        let path = self.record_path(operation_id);
        atomic_durable_write(&path, &updated)?;
        self.records.insert(operation_id, updated);
        self.rebuild_capacity_claims();
        Ok(())
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
            let version = value
                .get("schema_version")
                .and_then(Value::as_u64)
                .and_then(|value| u32::try_from(value).ok());
            let Some(version) = version else {
                self.recovery.malformed_count += 1;
                self.recovery.attention_required = true;
                self.retained
                    .push(RetainedRecord::Malformed { path, bytes });
                continue;
            };
            if version != CURRENT_SCHEMA_VERSION {
                self.recovery.unknown_version_count += 1;
                self.recovery.attention_required = true;
                self.retained.push(RetainedRecord::UnknownVersion {
                    path,
                    bytes,
                    version,
                });
                continue;
            }
            match serde_json::from_value::<OperationRecord>(value) {
                Ok(record) => {
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
                    if record.phase != OperationPhase::Terminal || !record.disposition.is_terminal()
                    {
                        self.recovery.unresolved_count += 1;
                        self.recovery.attention_required = true;
                    }
                    self.records.insert(record.operation_id, record);
                }
                Err(_) => {
                    self.recovery.malformed_count += 1;
                    self.recovery.attention_required = true;
                    self.retained
                        .push(RetainedRecord::Malformed { path, bytes });
                }
            }
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

    /// Advance phase/disposition through one atomic durable record replacement.
    pub(crate) fn update(
        &mut self,
        operation_id: Uuid,
        phase: OperationPhase,
        disposition: OperationDisposition,
    ) -> Result<(), JournalError> {
        self.store.update(operation_id, phase, disposition)
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
            path,
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
    let directory = path.parent().ok_or_else(|| JournalError::Write {
        path: path.to_path_buf(),
        source: io::Error::new(io::ErrorKind::InvalidInput, "record path has no parent"),
    })?;
    let temp_path = directory.join(format!(
        ".{}.tmp",
        path.file_name().unwrap_or_default().to_string_lossy()
    ));
    let bytes = serde_json::to_vec_pretty(record).map_err(|source| JournalError::Write {
        path: path.to_path_buf(),
        source: io::Error::other(source),
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

    fn intent() -> OperationIntent {
        OperationIntent {
            actor: OperationActor::User,
            kind: OperationKind::FileHistory,
            label: String::from("test"),
        }
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
                    OperationPhase::Prepared,
                    OperationDisposition::RetryPending,
                )
                .unwrap();
        }
        let journal = OperationJournalCoordinator::open(dir.path().to_path_buf()).unwrap();
        let record = journal.record(id).unwrap();
        assert_eq!(record.phase, OperationPhase::Prepared);
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
            fs::write(path, serde_json::to_vec(&record).unwrap()).unwrap();
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
        let files = tempfile::tempdir().unwrap();
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
}
