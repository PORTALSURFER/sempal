use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::params;
use uuid::Uuid;

use super::util::{map_sql_error, normalize_relative_path, parse_relative_path_from_db};
use super::{Rating, SourceDatabase, SourceDbError};

/// File operation kinds tracked in the per-source journal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FileOpKind {
    /// Moving a file between sources.
    Move,
    /// Copying a file into a source or collection.
    Copy,
}

impl FileOpKind {
    fn as_str(self) -> &'static str {
        match self {
            FileOpKind::Move => "move",
            FileOpKind::Copy => "copy",
        }
    }

    fn from_str(value: &str) -> Option<Self> {
        match value {
            "move" => Some(FileOpKind::Move),
            "copy" => Some(FileOpKind::Copy),
            _ => None,
        }
    }
}

/// Persistent stages for file operations that need crash recovery.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FileOpStage {
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
    fn as_str(self) -> &'static str {
        match self {
            FileOpStage::Intent => "intent",
            FileOpStage::Staged => "staged",
            FileOpStage::TargetDb => "target_db",
            FileOpStage::SourceDb => "source_db",
        }
    }

    fn from_str(value: &str) -> Option<Self> {
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
pub(crate) struct FileOpJournalEntry {
    pub(crate) id: String,
    pub(crate) kind: FileOpKind,
    pub(crate) stage: FileOpStage,
    pub(crate) source_root: Option<PathBuf>,
    pub(crate) source_relative: Option<PathBuf>,
    pub(crate) target_relative: PathBuf,
    pub(crate) staged_relative: Option<PathBuf>,
    pub(crate) file_size: Option<u64>,
    pub(crate) modified_ns: Option<i64>,
    pub(crate) tag: Option<Rating>,
    pub(crate) looped: Option<bool>,
    pub(crate) last_played_at: Option<i64>,
    pub(crate) created_at: i64,
}

impl FileOpJournalEntry {
    /// Build a new journal entry for a move operation.
    pub(crate) fn new_move(
        id: String,
        source_root: PathBuf,
        source_relative: PathBuf,
        target_relative: PathBuf,
        staged_relative: PathBuf,
        tag: Rating,
        looped: bool,
        last_played_at: Option<i64>,
    ) -> Result<Self, SourceDbError> {
        Ok(Self {
            id,
            kind: FileOpKind::Move,
            stage: FileOpStage::Intent,
            source_root: Some(source_root),
            source_relative: Some(source_relative),
            target_relative,
            staged_relative: Some(staged_relative),
            file_size: None,
            modified_ns: None,
            tag: Some(tag),
            looped: Some(looped),
            last_played_at,
            created_at: now_epoch_seconds()?,
        })
    }

    /// Build a new journal entry for a copy operation.
    pub(crate) fn new_copy(
        id: String,
        target_relative: PathBuf,
        staged_relative: PathBuf,
    ) -> Result<Self, SourceDbError> {
        Ok(Self {
            id,
            kind: FileOpKind::Copy,
            stage: FileOpStage::Intent,
            source_root: None,
            source_relative: None,
            target_relative,
            staged_relative: Some(staged_relative),
            file_size: None,
            modified_ns: None,
            tag: None,
            looped: None,
            last_played_at: None,
            created_at: now_epoch_seconds()?,
        })
    }
}

/// Summary of reconciliation work performed for pending file ops.
#[derive(Debug, Default)]
pub(crate) struct FileOpReconcileSummary {
    pub(crate) total: usize,
    pub(crate) completed: usize,
    pub(crate) errors: Vec<String>,
}

/// Generate a unique identifier for a pending file operation.
pub(crate) fn new_op_id() -> String {
    Uuid::new_v4().to_string()
}

/// Create a deterministic staging path that lives beside the final destination.
pub(crate) fn staged_relative_for_target(
    target_relative: &Path,
    op_id: &str,
) -> Result<PathBuf, SourceDbError> {
    let file_name = target_relative
        .file_name()
        .ok_or_else(|| SourceDbError::InvalidRelativePath(target_relative.to_path_buf()))?;
    let staged_name = format!("{}.sempal_pending_{}", file_name.to_string_lossy(), op_id);
    Ok(target_relative.with_file_name(staged_name))
}

/// Insert a new journal entry before mutating the filesystem.
pub(crate) fn insert_entry(
    db: &SourceDatabase,
    entry: &FileOpJournalEntry,
) -> Result<(), SourceDbError> {
    let target_relative = normalize_relative_path(&entry.target_relative)?;
    let staged_relative = match entry.staged_relative.as_ref() {
        Some(path) => Some(normalize_relative_path(path)?),
        None => None,
    };
    let source_relative = match entry.source_relative.as_ref() {
        Some(path) => Some(normalize_relative_path(path)?),
        None => None,
    };
    db.connection
        .execute(
            "INSERT INTO file_ops_journal (
                id, op_type, stage, source_root, source_relative, target_relative,
                staged_relative, file_size, modified_ns, tag, looped, last_played_at, created_at
             )
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
            params![
                entry.id,
                entry.kind.as_str(),
                entry.stage.as_str(),
                entry.source_root.as_ref().map(|path| path.to_string_lossy().to_string()),
                source_relative,
                target_relative,
                staged_relative,
                entry.file_size.map(|size| size as i64),
                entry.modified_ns,
                entry.tag.map(|tag| tag.as_i64()),
                entry.looped.map(|looped| if looped { 1i64 } else { 0i64 }),
                entry.last_played_at,
                entry.created_at,
            ],
        )
        .map_err(map_sql_error)?;
    Ok(())
}

/// Update a journal entry stage and optional metadata after filesystem work.
pub(crate) fn update_stage(
    db: &SourceDatabase,
    id: &str,
    stage: FileOpStage,
    file_size: Option<u64>,
    modified_ns: Option<i64>,
) -> Result<(), SourceDbError> {
    db.connection
        .execute(
            "UPDATE file_ops_journal
             SET stage = ?1,
                 file_size = COALESCE(?2, file_size),
                 modified_ns = COALESCE(?3, modified_ns)
             WHERE id = ?4",
            params![
                stage.as_str(),
                file_size.map(|size| size as i64),
                modified_ns,
                id,
            ],
        )
        .map_err(map_sql_error)?;
    Ok(())
}

/// Remove a resolved journal entry after reconciliation.
pub(crate) fn remove_entry(db: &SourceDatabase, id: &str) -> Result<(), SourceDbError> {
    db.connection
        .execute("DELETE FROM file_ops_journal WHERE id = ?1", params![id])
        .map_err(map_sql_error)?;
    Ok(())
}

/// Load all pending journal entries for reconciliation.
pub(crate) fn list_entries(db: &SourceDatabase) -> Result<Vec<FileOpJournalEntry>, SourceDbError> {
    let mut stmt = db
        .connection
        .prepare(
            "SELECT id, op_type, stage, source_root, source_relative, target_relative,
                    staged_relative, file_size, modified_ns, tag, looped, last_played_at, created_at
             FROM file_ops_journal",
        )
        .map_err(map_sql_error)?;
    let rows = stmt
        .query_map([], |row| {
            let kind = FileOpKind::from_str(row.get::<_, String>(1)?.as_str())
                .ok_or_else(|| rusqlite::Error::InvalidQuery)?;
            let stage = FileOpStage::from_str(row.get::<_, String>(2)?.as_str())
                .ok_or_else(|| rusqlite::Error::InvalidQuery)?;
            let source_root: Option<String> = row.get(3)?;
            let source_relative: Option<String> = row.get(4)?;
            let target_relative: String = row.get(5)?;
            let staged_relative: Option<String> = row.get(6)?;
            let source_relative = match source_relative {
                Some(path) => match parse_relative_path_from_db(&path) {
                    Ok(path) => Some(path),
                    Err(err) => {
                        tracing::warn!("Skipping journal entry with invalid source path: {err}");
                        return Ok(None);
                    }
                },
                None => None,
            };
            let target_relative = match parse_relative_path_from_db(&target_relative) {
                Ok(path) => path,
                Err(err) => {
                    tracing::warn!("Skipping journal entry with invalid target path: {err}");
                    return Ok(None);
                }
            };
            let staged_relative = match staged_relative {
                Some(path) => match parse_relative_path_from_db(&path) {
                    Ok(path) => Some(path),
                    Err(err) => {
                        tracing::warn!("Skipping journal entry with invalid staged path: {err}");
                        return Ok(None);
                    }
                },
                None => None,
            };
            let tag = row
                .get::<_, Option<i64>>(9)?
                .map(Rating::from_i64);
            let looped = row.get::<_, Option<i64>>(10)?.map(|flag| flag != 0);
            Ok(Some(FileOpJournalEntry {
                id: row.get(0)?,
                kind,
                stage,
                source_root: source_root.map(PathBuf::from),
                source_relative,
                target_relative,
                staged_relative,
                file_size: row.get::<_, Option<i64>>(7)?.map(|size| size as u64),
                modified_ns: row.get(8)?,
                tag,
                looped,
                last_played_at: row.get(11)?,
                created_at: row.get(12)?,
            }))
        })
        .map_err(map_sql_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(map_sql_error)?;
    Ok(rows.into_iter().flatten().collect())
}

/// Reconcile all pending file ops against the filesystem and database.
pub(crate) fn reconcile_pending_ops(
    db: &SourceDatabase,
) -> Result<FileOpReconcileSummary, String> {
    let entries = list_entries(db).map_err(|err| err.to_string())?;
    let mut summary = FileOpReconcileSummary {
        total: entries.len(),
        completed: 0,
        errors: Vec::new(),
    };
    for entry in entries {
        match reconcile_entry(db, &entry) {
            Ok(()) => {
                if let Err(err) = remove_entry(db, &entry.id) {
                    summary
                        .errors
                        .push(format!("Failed to remove journal entry {}: {err}", entry.id));
                } else {
                    summary.completed += 1;
                }
            }
            Err(err) => summary.errors.push(err),
        }
    }
    Ok(summary)
}

fn reconcile_entry(db: &SourceDatabase, entry: &FileOpJournalEntry) -> Result<(), String> {
    let target_root = db.root();
    let target_absolute = target_root.join(&entry.target_relative);
    let staged_absolute = entry
        .staged_relative
        .as_ref()
        .map(|path| target_root.join(path));
    let staged_exists = staged_absolute
        .as_ref()
        .is_some_and(|path| path.is_file());
    let target_exists = target_absolute.is_file();
    if staged_exists {
        if !target_exists {
            if let Some(parent) = target_absolute.parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|err| format!("Failed to create target dir: {err}"))?;
            }
            std::fs::rename(
                staged_absolute.as_ref().expect("checked staged path"),
                &target_absolute,
            )
            .map_err(|err| format!("Failed to finalize staged file: {err}"))?;
        } else if let Some(staged) = staged_absolute.as_ref() {
            std::fs::remove_file(staged)
                .map_err(|err| format!("Failed to remove staged file: {err}"))?;
        }
    }
    if target_absolute.is_file() {
        let (file_size, modified_ns) = file_metadata(&target_absolute)?;
        let mut batch = db.write_batch().map_err(|err| err.to_string())?;
        batch
            .upsert_file(&entry.target_relative, file_size, modified_ns)
            .map_err(|err| err.to_string())?;
        if let Some(tag) = entry.tag {
            batch
                .set_tag(&entry.target_relative, tag)
                .map_err(|err| err.to_string())?;
        }
        if let Some(looped) = entry.looped {
            batch
                .set_looped(&entry.target_relative, looped)
                .map_err(|err| err.to_string())?;
        }
        if let Some(last_played_at) = entry.last_played_at {
            batch
                .set_last_played_at(&entry.target_relative, last_played_at)
                .map_err(|err| err.to_string())?;
        }
        batch.commit().map_err(|err| err.to_string())?;
    } else {
        db.remove_file(&entry.target_relative)
            .map_err(|err| format!("Failed to drop target DB row: {err}"))?;
    }
    if entry.kind == FileOpKind::Move {
        reconcile_source_entry(db, entry, target_absolute.is_file())?;
    }
    Ok(())
}

fn reconcile_source_entry(
    target_db: &SourceDatabase,
    entry: &FileOpJournalEntry,
    target_exists: bool,
) -> Result<(), String> {
    let Some(source_root) = entry.source_root.as_ref() else {
        return Ok(());
    };
    let Some(source_relative) = entry.source_relative.as_ref() else {
        return Ok(());
    };
    if !source_root.is_dir() {
        return Ok(());
    }
    let source_absolute = source_root.join(source_relative);
    if source_absolute.is_file() && !target_exists {
        return Ok(());
    }
    let source_db = SourceDatabase::open(source_root)
        .map_err(|err| format!("Failed to open source DB for recovery: {err}"))?;
    if !source_absolute.is_file() {
        source_db
            .remove_file(source_relative)
            .map_err(|err| format!("Failed to drop source DB row: {err}"))?;
    } else if target_exists {
        tracing::warn!(
            "Move recovery left duplicate file at {} -> {}",
            source_absolute.display(),
            target_db.root().display()
        );
    }
    Ok(())
}

fn file_metadata(path: &Path) -> Result<(u64, i64), String> {
    let metadata = std::fs::metadata(path)
        .map_err(|err| format!("Failed to read {}: {err}", path.display()))?;
    let modified_ns = metadata
        .modified()
        .map_err(|err| format!("Missing modified time for {}: {err}", path.display()))?
        .duration_since(UNIX_EPOCH)
        .map_err(|_| "File modified time is before epoch".to_string())?
        .as_nanos() as i64;
    Ok((metadata.len(), modified_ns))
}

fn now_epoch_seconds() -> Result<i64, SourceDbError> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| SourceDbError::Unexpected)?;
    Ok(now.as_secs() as i64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn write_wav(path: &Path) {
        std::fs::write(path, [0u8; 16]).unwrap();
    }

    #[test]
    fn reconcile_move_from_staged_file() {
        let temp = tempdir().unwrap();
        let source_root = temp.path().join("source");
        let target_root = temp.path().join("target");
        std::fs::create_dir_all(&source_root).unwrap();
        std::fs::create_dir_all(&target_root).unwrap();
        let source_db = SourceDatabase::open(&source_root).unwrap();
        let target_db = SourceDatabase::open(&target_root).unwrap();

        let source_relative = PathBuf::from("one.wav");
        let source_absolute = source_root.join(&source_relative);
        write_wav(&source_absolute);
        source_db.upsert_file(&source_relative, 16, 1).unwrap();
        source_db.set_tag(&source_relative, Rating::KEEP_1).unwrap();
        source_db.set_looped(&source_relative, true).unwrap();
        source_db.set_last_played_at(&source_relative, 123).unwrap();

        let target_relative = PathBuf::from("moved.wav");
        let staged_relative = staged_relative_for_target(&target_relative, "test").unwrap();
        let entry = FileOpJournalEntry::new_move(
            "move-test".to_string(),
            source_root.clone(),
            source_relative.clone(),
            target_relative.clone(),
            staged_relative.clone(),
            Rating::KEEP_1,
            true,
            Some(123),
        )
        .unwrap();
        insert_entry(&target_db, &entry).unwrap();

        let staged_absolute = target_root.join(&staged_relative);
        std::fs::rename(&source_absolute, &staged_absolute).unwrap();
        update_stage(&target_db, &entry.id, FileOpStage::Staged, Some(16), Some(1)).unwrap();

        let summary = reconcile_pending_ops(&target_db).unwrap();
        assert_eq!(summary.completed, 1);

        assert!(!staged_absolute.exists());
        assert!(target_root.join(&target_relative).exists());
        assert!(source_db.tag_for_path(&source_relative).unwrap().is_none());
        assert_eq!(
            target_db.tag_for_path(&target_relative).unwrap(),
            Some(Rating::KEEP_1)
        );
        assert_eq!(
            target_db.looped_for_path(&target_relative).unwrap(),
            Some(true)
        );
        assert_eq!(
            target_db.last_played_at_for_path(&target_relative).unwrap(),
            Some(123)
        );
        assert!(list_entries(&target_db).unwrap().is_empty());
    }

    #[test]
    fn reconcile_same_source_move_from_staged_file() {
        let temp = tempdir().unwrap();
        let source_root = temp.path().join("source");
        std::fs::create_dir_all(&source_root).unwrap();
        let db = SourceDatabase::open(&source_root).unwrap();

        let source_relative = PathBuf::from("one.wav");
        let source_absolute = source_root.join(&source_relative);
        write_wav(&source_absolute);
        db.upsert_file(&source_relative, 16, 1).unwrap();
        db.set_tag(&source_relative, Rating::KEEP_1).unwrap();
        db.set_looped(&source_relative, true).unwrap();
        db.set_last_played_at(&source_relative, 123).unwrap();

        let target_relative = PathBuf::from("moved.wav");
        let staged_relative = staged_relative_for_target(&target_relative, "test").unwrap();
        let entry = FileOpJournalEntry::new_move(
            "move-test".to_string(),
            source_root.clone(),
            source_relative.clone(),
            target_relative.clone(),
            staged_relative.clone(),
            Rating::KEEP_1,
            true,
            Some(123),
        )
        .unwrap();
        insert_entry(&db, &entry).unwrap();

        let staged_absolute = source_root.join(&staged_relative);
        std::fs::rename(&source_absolute, &staged_absolute).unwrap();
        update_stage(&db, &entry.id, FileOpStage::Staged, Some(16), Some(1)).unwrap();

        let summary = reconcile_pending_ops(&db).unwrap();
        assert_eq!(summary.completed, 1);

        assert!(!staged_absolute.exists());
        assert!(source_root.join(&target_relative).exists());
        assert!(db.tag_for_path(&source_relative).unwrap().is_none());
        assert_eq!(
            db.tag_for_path(&target_relative).unwrap(),
            Some(Rating::KEEP_1)
        );
        assert_eq!(
            db.looped_for_path(&target_relative).unwrap(),
            Some(true)
        );
        assert_eq!(
            db.last_played_at_for_path(&target_relative).unwrap(),
            Some(123)
        );
        assert!(list_entries(&db).unwrap().is_empty());
    }

    #[test]
    fn reconcile_copy_from_staged_file() {
        let temp = tempdir().unwrap();
        let target_root = temp.path().join("target");
        std::fs::create_dir_all(&target_root).unwrap();
        let target_db = SourceDatabase::open(&target_root).unwrap();

        let source_path = temp.path().join("external.wav");
        write_wav(&source_path);
        let target_relative = PathBuf::from("copied.wav");
        let staged_relative = staged_relative_for_target(&target_relative, "copy").unwrap();
        let entry = FileOpJournalEntry::new_copy(
            "copy-test".to_string(),
            target_relative.clone(),
            staged_relative.clone(),
        )
        .unwrap();
        insert_entry(&target_db, &entry).unwrap();

        let staged_absolute = target_root.join(&staged_relative);
        std::fs::copy(&source_path, &staged_absolute).unwrap();
        update_stage(&target_db, &entry.id, FileOpStage::Staged, Some(16), Some(1)).unwrap();

        let summary = reconcile_pending_ops(&target_db).unwrap();
        assert_eq!(summary.completed, 1);
        assert!(target_root.join(&target_relative).exists());
        assert!(target_db.tag_for_path(&target_relative).unwrap().is_some());
        assert!(list_entries(&target_db).unwrap().is_empty());
    }
}
