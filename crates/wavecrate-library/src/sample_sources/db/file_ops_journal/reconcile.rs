use std::path::Path;

use super::super::SourceDatabase;
use super::FileOpJournalEntry;
use super::entry::{FileOpKind, FileOpStage};
use super::recovery_io::{
    OpenedFile, RecoveryRoot, RecoverySourceDatabases, SourceDatabaseRecoveryAccess,
};
use super::store::FileOpJournalStore;

struct FileOpRecoveryCoordinator<'a, D> {
    target_db: &'a SourceDatabase,
    journal: FileOpJournalStore<'a>,
    source_databases: D,
}

/// Summary of reconciliation work performed for pending file ops.
#[derive(Debug, Default)]
pub struct FileOpReconcileSummary {
    /// Total number of journal rows considered, including malformed rows.
    pub total: usize,
    /// Number of rows successfully reconciled and removed.
    pub completed: usize,
    /// Human-readable reconciliation errors.
    pub errors: Vec<String>,
}

/// Reconcile all pending file ops against the filesystem and database.
pub fn reconcile_pending_ops(db: &SourceDatabase) -> Result<FileOpReconcileSummary, String> {
    FileOpRecoveryCoordinator {
        target_db: db,
        journal: FileOpJournalStore::new(db),
        source_databases: SourceDatabaseRecoveryAccess,
    }
    .reconcile()
}

impl<D: RecoverySourceDatabases> FileOpRecoveryCoordinator<'_, D> {
    fn reconcile(&self) -> Result<FileOpReconcileSummary, String> {
        let listed = self.journal.list().map_err(|err| err.to_string())?;
        let mut summary = FileOpReconcileSummary {
            total: listed.entries.len() + listed.malformed.len(),
            completed: 0,
            errors: Vec::new(),
        };
        for malformed in listed.malformed {
            let message = malformed.describe();
            summary.errors.push(format!(
                "{message}; retained journal row for diagnosis and retry"
            ));
        }
        for entry in listed.entries {
            match reconcile_entry(self.target_db, &entry, &self.source_databases) {
                Ok(()) => {
                    if let Err(err) = self.journal.remove(&entry.id) {
                        summary.errors.push(format!(
                            "Failed to remove journal entry {}: {err}",
                            entry.id
                        ));
                    } else {
                        summary.completed += 1;
                    }
                }
                Err(err) => summary.errors.push(err),
            }
        }
        Ok(summary)
    }
}

fn reconcile_entry(
    db: &SourceDatabase,
    entry: &FileOpJournalEntry,
    source_databases: &impl RecoverySourceDatabases,
) -> Result<(), String> {
    let target_root = RecoveryRoot::open(db.root(), entry.target_root_identity.as_deref())?;
    let staged = match entry.staged_relative.as_deref() {
        Some(path) => target_root.open_file(path)?,
        None => None,
    };
    let target = target_root.open_file(&entry.target_relative)?;
    validate_staged_file_identity(entry, staged.as_ref())?;
    validate_existing_target_identity(entry, target.as_ref(), staged.is_some())?;

    // A move whose staged file still exists must validate the source root before
    // finalization, so an unavailable/replaced source leaves both staged data and
    // the journal available for a later retry.
    let source_root = if entry.kind == FileOpKind::Move && staged.is_some() {
        Some(preflight_source_root(entry)?)
    } else {
        None
    };

    reconcile_staged_file(
        &target_root,
        entry.staged_relative.as_deref(),
        &entry.target_relative,
        staged.as_ref(),
        target.is_some(),
    )?;
    let target = target_root.open_file(&entry.target_relative)?;
    validate_staged_file_identity(entry, staged.as_ref())?;
    validate_existing_target_identity(entry, target.as_ref(), staged.is_some())?;
    let target_exists = reconcile_target_entry(db, entry, target.as_ref())?;
    if let (Some(staged_relative), Some(staged)) =
        (entry.staged_relative.as_deref(), staged.as_ref())
    {
        target_root.remove_file_if_identity(staged_relative, &staged.identity)?;
    }
    if entry.kind == FileOpKind::Move {
        reconcile_source_entry(db, entry, target_exists, source_root, source_databases)?;
    }
    Ok(())
}

fn preflight_source_root(entry: &FileOpJournalEntry) -> Result<RecoveryRoot, String> {
    let source_root = entry.source_root.as_deref().ok_or_else(|| {
        format!(
            "Deferred move recovery for {}: source root is missing",
            entry.id
        )
    })?;
    let root = RecoveryRoot::open(source_root, entry.source_root_identity.as_deref())?;
    if let Some(source_relative) = entry.source_relative.as_deref()
        && let Some(source_file) = root.open_file(source_relative)?
    {
        validate_opened_file(
            entry,
            &source_file,
            "source",
            "source file was replaced before recovery replay",
        )?;
    }
    Ok(root)
}

/// Finalize one staged file into the target path or clean the stale staged copy.
fn reconcile_staged_file(
    target_root: &RecoveryRoot,
    staged_relative: Option<&Path>,
    target_relative: &Path,
    staged: Option<&OpenedFile>,
    target_exists: bool,
) -> Result<(), String> {
    let Some(staged_relative) = staged_relative else {
        return Ok(());
    };
    let Some(staged) = staged else {
        return Ok(());
    };
    if !target_exists {
        target_root.ensure_parent(target_relative)?;
        target_root.hard_link_no_replace(staged_relative, &staged.identity, target_relative)?;
    }
    Ok(())
}

fn validate_staged_file_identity(
    entry: &FileOpJournalEntry,
    staged: Option<&OpenedFile>,
) -> Result<(), String> {
    let Some(staged) = staged else {
        return Ok(());
    };
    validate_opened_file(
        entry,
        staged,
        "staged",
        "staged file no longer matches the recorded journal metadata",
    )
}

fn validate_existing_target_identity(
    entry: &FileOpJournalEntry,
    target: Option<&OpenedFile>,
    staged_exists: bool,
) -> Result<(), String> {
    let Some(target) = target else {
        return Ok(());
    };
    validate_opened_file(
        entry,
        target,
        "target",
        &format!(
            "target path was reused before recovery replay{}",
            if staged_exists {
                "; leaving staged copy intact"
            } else {
                "; no staged copy remains to reconcile safely"
            }
        ),
    )
}

fn validate_opened_file(
    entry: &FileOpJournalEntry,
    actual: &OpenedFile,
    location: &str,
    mismatch_reason: &str,
) -> Result<(), String> {
    let Some(expected_identity) = entry.file_identity.as_deref() else {
        return Err(format!(
            "Deferred file-op recovery for {}: {location} file exists but journaled identity is incomplete; leaving staged copy intact",
            entry.id
        ));
    };
    let facts_match = entry
        .file_size
        .is_none_or(|expected| expected == actual.file_size)
        && entry
            .modified_ns
            .is_none_or(|expected| expected == actual.modified_ns);
    if expected_identity == actual.identity && facts_match {
        return Ok(());
    }
    Err(format!(
        "Deferred file-op recovery for {}: {location} file does not match journaled identity (expected {expected_identity}, found {}); {mismatch_reason}",
        entry.id, actual.identity
    ))
}

/// Reconcile one target DB row and return whether the target file exists afterwards.
fn reconcile_target_entry(
    db: &SourceDatabase,
    entry: &FileOpJournalEntry,
    target: Option<&OpenedFile>,
) -> Result<bool, String> {
    if let Some(target) = target {
        let mut batch = db.write_batch().map_err(|err| err.to_string())?;
        match entry.kind {
            FileOpKind::Copy => batch
                .upsert_file_without_hash(
                    &entry.target_relative,
                    target.file_size,
                    target.modified_ns,
                )
                .map_err(|err| err.to_string())?,
            FileOpKind::Move => batch
                .upsert_file(&entry.target_relative, target.file_size, target.modified_ns)
                .map_err(|err| err.to_string())?,
        }
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
        if let Some(locked) = entry.locked {
            batch
                .set_locked(&entry.target_relative, locked)
                .map_err(|err| err.to_string())?;
        }
        if let Some(last_played_at) = entry.last_played_at {
            batch
                .set_last_played_at(&entry.target_relative, last_played_at)
                .map_err(|err| err.to_string())?;
        } else {
            batch
                .clear_last_played_at(&entry.target_relative)
                .map_err(|err| err.to_string())?;
        }
        if let Some(last_curated_at) = entry.last_curated_at {
            batch
                .set_last_curated_at(&entry.target_relative, last_curated_at)
                .map_err(|err| err.to_string())?;
        } else {
            batch
                .clear_last_curated_at(&entry.target_relative)
                .map_err(|err| err.to_string())?;
        }
        batch.commit().map_err(|err| err.to_string())?;
        Ok(true)
    } else {
        db.remove_file(&entry.target_relative)
            .map_err(|err| format!("Failed to drop target DB row: {err}"))?;
        Ok(false)
    }
}

fn reconcile_source_entry(
    target_db: &SourceDatabase,
    entry: &FileOpJournalEntry,
    target_exists: bool,
    source_root: Option<RecoveryRoot>,
    source_databases: &impl RecoverySourceDatabases,
) -> Result<(), String> {
    let Some(source_root_path) = entry.source_root.as_deref() else {
        return Ok(());
    };
    let Some(source_relative) = entry.source_relative.as_deref() else {
        return Ok(());
    };
    let source_root = match source_root {
        Some(root) => Some(root),
        None => RecoveryRoot::open_if_available(
            source_root_path,
            entry.source_root_identity.as_deref(),
        )?,
    };
    let Some(source_root) = source_root else {
        if should_defer_source_cleanup(entry, target_exists) {
            return Err(format!(
                "Deferred move recovery for {} until source root is available: {}",
                entry.id,
                source_root_path.display()
            ));
        }
        return Ok(());
    };
    let source_file = source_root.open_file(source_relative)?;
    if entry.file_identity.is_some()
        && let Some(source_file) = source_file.as_ref()
    {
        validate_opened_file(
            entry,
            source_file,
            "source",
            "source file was replaced before recovery replay",
        )?;
    }
    if source_file.is_some() && !target_exists {
        return Ok(());
    }
    let source_db = source_databases.open(&source_root)?;
    if source_file.is_none() {
        source_root.revalidate_named_root()?;
        source_db
            .remove_file(source_relative)
            .map_err(|err| format!("Failed to drop source DB row: {err}"))?;
    } else if target_exists {
        tracing::warn!(
            "Move recovery left duplicate file at {} -> {}",
            source_root_path.join(source_relative).display(),
            target_db.root().display()
        );
    }
    Ok(())
}

fn should_defer_source_cleanup(entry: &FileOpJournalEntry, target_exists: bool) -> bool {
    target_exists && entry.stage != FileOpStage::SourceDb
}
