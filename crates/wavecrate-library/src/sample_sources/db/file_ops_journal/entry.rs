use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use uuid::Uuid;

use super::super::{Rating, SourceDbError};

/// File operation kinds tracked in the per-source journal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileOpKind {
    /// Moving a file between sources.
    Move,
    /// Copying a file into a source.
    Copy,
}

impl FileOpKind {
    pub(super) fn as_str(self) -> &'static str {
        match self {
            FileOpKind::Move => "move",
            FileOpKind::Copy => "copy",
        }
    }

    pub(super) fn from_str(value: &str) -> Option<Self> {
        match value {
            "move" => Some(FileOpKind::Move),
            "copy" => Some(FileOpKind::Copy),
            _ => None,
        }
    }
}

/// Persistent journal stages for file operations that need crash recovery.
///
/// Recovery treats the enum as an append-only lifecycle:
/// `Intent -> Staged -> TargetDb -> SourceDb`.
///
/// The stage is descriptive rather than authoritative; reconcile still inspects
/// the actual filesystem and database state so startup recovery remains
/// idempotent after partial writes or repeated runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileOpStage {
    /// Intent recorded before any filesystem mutations.
    Intent,
    /// File moved/copied into staging location.
    Staged,
    /// Target database updated.
    TargetDb,
    /// Source database updated (move only).
    SourceDb,
}

impl FileOpStage {
    pub(super) fn as_str(self) -> &'static str {
        match self {
            FileOpStage::Intent => "intent",
            FileOpStage::Staged => "staged",
            FileOpStage::TargetDb => "target_db",
            FileOpStage::SourceDb => "source_db",
        }
    }

    pub(super) fn from_str(value: &str) -> Option<Self> {
        match value {
            "intent" => Some(FileOpStage::Intent),
            "staged" => Some(FileOpStage::Staged),
            "target_db" => Some(FileOpStage::TargetDb),
            "source_db" => Some(FileOpStage::SourceDb),
            _ => None,
        }
    }
}

/// Durable journal entry used to reconcile file and database state after crashes.
#[derive(Debug, Clone)]
pub struct FileOpJournalEntry {
    /// Stable journal identifier.
    pub id: String,
    /// Operation kind being tracked.
    pub kind: FileOpKind,
    /// Most recently persisted recovery stage.
    pub stage: FileOpStage,
    /// Source root for move operations.
    pub source_root: Option<PathBuf>,
    /// Source-relative path for move operations.
    pub source_relative: Option<PathBuf>,
    /// Final target-relative destination path.
    pub target_relative: PathBuf,
    /// Temporary staged path used before finalization.
    pub staged_relative: Option<PathBuf>,
    /// Recorded file size once filesystem work has completed.
    pub file_size: Option<u64>,
    /// Recorded modified timestamp once filesystem work has completed.
    pub modified_ns: Option<i64>,
    /// Stable identity of the staged/target file object.
    pub file_identity: Option<String>,
    /// Stable identity of the target source root.
    pub target_root_identity: Option<String>,
    /// Stable identity of the move source root.
    pub source_root_identity: Option<String>,
    /// Stored keep/trash rating that should survive reconciliation.
    pub tag: Option<Rating>,
    /// Stored loop marker that should survive reconciliation.
    pub looped: Option<bool>,
    /// Stored lock marker that should survive reconciliation.
    pub locked: Option<bool>,
    /// Stored playback timestamp that should survive reconciliation.
    pub last_played_at: Option<i64>,
    /// Stored curation timestamp that should survive reconciliation.
    pub last_curated_at: Option<i64>,
    /// Journal creation time in epoch seconds.
    pub created_at: i64,
}

/// Initialization payload for creating move journal entries without wide signatures.
#[derive(Debug, Clone)]
pub struct MoveJournalEntryInit {
    /// Absolute source root that currently owns the file.
    pub source_root: PathBuf,
    /// Source-relative input path.
    pub source_relative: PathBuf,
    /// Final target-relative destination path.
    pub target_relative: PathBuf,
    /// Temporary staged path used before finalization.
    pub staged_relative: PathBuf,
    /// Stored keep/trash rating that should survive reconciliation.
    pub tag: Rating,
    /// Stored loop marker that should survive reconciliation.
    pub looped: bool,
    /// Stored lock marker that should survive reconciliation.
    pub locked: bool,
    /// Stored playback timestamp that should survive reconciliation.
    pub last_played_at: Option<i64>,
    /// Stored curation timestamp that should survive reconciliation.
    pub last_curated_at: Option<i64>,
}

/// Initialization payload for creating copy journal entries without wide signatures.
#[derive(Debug, Clone)]
pub struct CopyJournalEntryInit {
    /// Final target-relative destination path.
    pub target_relative: PathBuf,
    /// Temporary staged path used before finalization.
    pub staged_relative: PathBuf,
    /// Stored keep/trash rating that should survive reconciliation.
    pub tag: Rating,
    /// Stored loop marker that should survive reconciliation.
    pub looped: bool,
    /// Stored lock marker that should survive reconciliation.
    pub locked: bool,
    /// Stored playback timestamp that should survive reconciliation.
    pub last_played_at: Option<i64>,
    /// Stored curation timestamp that should survive reconciliation.
    pub last_curated_at: Option<i64>,
}

impl FileOpJournalEntry {
    /// Build a new journal entry for a move operation.
    pub fn new_move(id: String, init: MoveJournalEntryInit) -> Result<Self, SourceDbError> {
        Ok(Self {
            id,
            kind: FileOpKind::Move,
            stage: FileOpStage::Intent,
            source_root: Some(init.source_root),
            source_relative: Some(init.source_relative),
            target_relative: init.target_relative,
            staged_relative: Some(init.staged_relative),
            file_size: None,
            modified_ns: None,
            file_identity: None,
            target_root_identity: None,
            source_root_identity: None,
            tag: Some(init.tag),
            looped: Some(init.looped),
            locked: Some(init.locked),
            last_played_at: init.last_played_at,
            last_curated_at: init.last_curated_at,
            created_at: now_epoch_seconds()?,
        })
    }

    /// Build a new journal entry for a copy operation.
    pub fn new_copy(id: String, init: CopyJournalEntryInit) -> Result<Self, SourceDbError> {
        Ok(Self {
            id,
            kind: FileOpKind::Copy,
            stage: FileOpStage::Intent,
            source_root: None,
            source_relative: None,
            target_relative: init.target_relative,
            staged_relative: Some(init.staged_relative),
            file_size: None,
            modified_ns: None,
            file_identity: None,
            target_root_identity: None,
            source_root_identity: None,
            tag: Some(init.tag),
            looped: Some(init.looped),
            locked: Some(init.locked),
            last_played_at: init.last_played_at,
            last_curated_at: init.last_curated_at,
            created_at: now_epoch_seconds()?,
        })
    }
}

/// Generate a unique identifier for a pending file operation.
pub fn new_op_id() -> String {
    Uuid::new_v4().to_string()
}

/// Create a deterministic staging path that lives beside the final destination.
pub fn staged_relative_for_target(
    target_relative: &Path,
    op_id: &str,
) -> Result<PathBuf, SourceDbError> {
    let file_name = target_relative
        .file_name()
        .ok_or_else(|| SourceDbError::InvalidRelativePath(target_relative.to_path_buf()))?;
    let staged_name = format!(
        "{}.wavecrate_pending_{}",
        file_name.to_string_lossy(),
        op_id
    );
    Ok(target_relative.with_file_name(staged_name))
}

fn now_epoch_seconds() -> Result<i64, SourceDbError> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| SourceDbError::Unexpected)?;
    Ok(now.as_secs() as i64)
}
