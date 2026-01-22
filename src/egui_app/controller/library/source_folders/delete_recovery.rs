//! Crash recovery support for staged folder deletes.

use crate::egui_app::controller::EguiController;
use crate::egui_app::controller::jobs::JobMessage;
use crate::egui_app::controller::library::source_cache_invalidator;
use crate::egui_app::state::{
    FolderDeleteRecoveryAction as UiDeleteRecoveryAction,
    FolderDeleteRecoveryEntry as UiDeleteRecoveryEntry,
    FolderDeleteRecoveryStatus as UiDeleteRecoveryStatus,
};
use crate::egui_app::view_model;
use crate::sample_sources::{SampleSource, SourceId};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// Folder name used to stage pending deletes inside a source root.
pub(crate) const DELETE_STAGING_DIR: &str = ".sempal_delete_staging";
const DELETE_JOURNAL_FILE: &str = "delete_journal.json";
const RESTORE_SUFFIX: &str = ".restored";

/// Journal stage for a staged folder delete.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum DeleteJournalStage {
    /// Intent recorded before the filesystem rename completes.
    Intent,
    /// Folder has been moved into staging.
    Staged,
    /// Database has been updated; staged folder can be deleted.
    DbCommitted,
}

/// Persistent journal entry for a staged folder delete.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct DeleteJournalEntry {
    id: String,
    original_relative: String,
    staged_relative: String,
    stage: DeleteJournalStage,
    created_at: i64,
}

/// Journal container stored on disk.
#[derive(Debug, Default, Serialize, Deserialize)]
struct DeleteJournal {
    entries: Vec<DeleteJournalEntry>,
}

/// Metadata for a folder staged for deletion.
#[derive(Debug, Clone)]
pub(crate) struct DeleteStagingInfo {
    /// Unique journal identifier for this staged delete.
    pub(crate) id: String,
    /// Relative path of the original folder within the source.
    pub(crate) original_relative: PathBuf,
    /// Relative path inside the staging root.
    pub(crate) staged_relative: PathBuf,
    /// Absolute staged path on disk.
    pub(crate) staged_absolute: PathBuf,
}

/// Recovery action taken for a staged delete.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DeleteRecoveryAction {
    /// Move the staged folder back into the source tree.
    Restore,
    /// Permanently delete the staged folder.
    Finalize,
}

/// Outcome for a recovery attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DeleteRecoveryStatus {
    /// Recovery action succeeded.
    Completed,
    /// Recovery action failed and needs attention.
    Failed,
}

/// Per-folder recovery result for UI reporting.
#[derive(Debug, Clone)]
pub(crate) struct DeleteRecoveryEntry {
    /// Source identifier for the staged folder.
    pub(crate) source_id: SourceId,
    /// Source root for display and follow-up refreshes.
    pub(crate) source_root: PathBuf,
    /// Original relative path within the source.
    pub(crate) original_relative: PathBuf,
    /// Action taken during recovery.
    pub(crate) action: DeleteRecoveryAction,
    /// Outcome of the action.
    pub(crate) status: DeleteRecoveryStatus,
    /// Optional extra detail for the UI.
    pub(crate) detail: Option<String>,
}

/// Summary of staged delete recovery across all sources.
#[derive(Debug, Default)]
pub(crate) struct DeleteRecoveryReport {
    /// Per-folder recovery outcomes.
    pub(crate) entries: Vec<DeleteRecoveryEntry>,
    /// Non-fatal errors encountered during recovery.
    pub(crate) errors: Vec<String>,
}

/// Stage a folder for deletion and record it in the journal.
pub(crate) fn stage_folder_for_delete(
    absolute: &Path,
    staging_root: &Path,
    relative: &Path,
) -> Result<DeleteStagingInfo, String> {
    let staged_relative = unique_staging_relative(staging_root, relative);
    let staged_absolute = staging_root.join(&staged_relative);
    ensure_staging_parent(&staged_absolute, staging_root)?;
    let id = new_delete_op_id();
    let entry = DeleteJournalEntry {
        id: id.clone(),
        original_relative: relative.to_string_lossy().to_string(),
        staged_relative: staged_relative.to_string_lossy().to_string(),
        stage: DeleteJournalStage::Intent,
        created_at: now_epoch_seconds()?,
    };
    insert_entry(staging_root, entry)?;
    if let Err(err) = fs::rename(absolute, &staged_absolute) {
        let _ = remove_entry(staging_root, &id);
        return Err(format!("Failed to stage folder delete: {err}"));
    }
    if let Err(err) = update_entry_stage(staging_root, &id, DeleteJournalStage::Staged) {
        let _ = fs::rename(&staged_absolute, absolute);
        let _ = remove_entry(staging_root, &id);
        return Err(format!("Failed to record delete staging: {err}"));
    }
    Ok(DeleteStagingInfo {
        id,
        original_relative: relative.to_path_buf(),
        staged_relative,
        staged_absolute,
    })
}

/// Mark a staged delete as safe to finalize after DB updates.
pub(crate) fn mark_delete_db_committed(staging_root: &Path, id: &str) -> Result<(), String> {
    update_entry_stage(staging_root, id, DeleteJournalStage::DbCommitted)
}

/// Remove a journal entry after a staged delete is resolved.
pub(crate) fn remove_delete_entry(staging_root: &Path, id: &str) -> Result<(), String> {
    remove_entry(staging_root, id)
}

/// Roll back a staged folder delete, restoring the folder and journal state.
pub(crate) fn rollback_staged_folder(
    info: &DeleteStagingInfo,
    absolute: &Path,
    staging_root: &Path,
    err: &str,
) -> Result<(), String> {
    if let Err(restore_err) = fs::rename(&info.staged_absolute, absolute) {
        return Err(format!(
            "{err} (also failed to restore folder: {restore_err})"
        ));
    }
    let _ = remove_entry(staging_root, &info.id);
    cleanup_staging_root(staging_root);
    Err(err.to_string())
}

/// Recover staged deletes for the provided sources.
pub(crate) fn recover_staged_deletes(sources: &[SampleSource]) -> DeleteRecoveryReport {
    let mut report = DeleteRecoveryReport::default();
    for source in sources {
        if !source.root.is_dir() {
            continue;
        }
        let staging_root = source.root.join(DELETE_STAGING_DIR);
        if !staging_root.is_dir() {
            continue;
        }
        let journal = match load_journal(&staging_root) {
            Ok(journal) => journal,
            Err(err) => {
                report
                    .errors
                    .push(format!("Failed to read delete journal for {}: {err}", source.root.display()));
                DeleteJournal::default()
            }
        };
        let journaled_roots = journaled_staged_roots(&journal);
        recover_journaled_entries(source, &staging_root, &journal, &mut report);
        recover_unjournaled_entries(
            source,
            &staging_root,
            &journaled_roots,
            &mut report,
        );
        cleanup_staging_root(&staging_root);
    }
    report
}

impl EguiController {
    /// Start background recovery for staged folder deletes after the UI is ready.
    pub(crate) fn start_folder_delete_recovery_if_needed(&mut self) {
        if self.runtime.delete_recovery_started {
            return;
        }
        if self.library.sources.is_empty() {
            return;
        }
        self.runtime.delete_recovery_started = true;
        self.ui.sources.folders.delete_recovery.in_progress = true;
        self.ui.sources.folders.delete_recovery.entries.clear();
        let sources = self.library.sources.clone();
        let tx = self.runtime.jobs.message_sender();
        std::thread::spawn(move || {
            let report = recover_staged_deletes(&sources);
            let _ = tx.send(JobMessage::FolderDeleteRecoveryFinished(report));
        });
    }

    /// Apply staged delete recovery results to UI state and cached data.
    pub(crate) fn apply_folder_delete_recovery_report(&mut self, report: DeleteRecoveryReport) {
        self.ui.sources.folders.delete_recovery.in_progress = false;
        let mut ui_entries = Vec::new();
        let mut restored = 0usize;
        let mut finalized = 0usize;
        let mut failed = 0usize;
        let mut affected_sources = std::collections::HashSet::new();
        for entry in report.entries {
            let source_label = self
                .library
                .sources
                .iter()
                .find(|source| source.id == entry.source_id)
                .map(|source| view_model::source_row(source, false).name)
                .unwrap_or_else(|| entry.source_root.to_string_lossy().to_string());
            let (action, status) = match (entry.action, entry.status) {
                (DeleteRecoveryAction::Restore, DeleteRecoveryStatus::Completed) => {
                    restored += 1;
                    affected_sources.insert(entry.source_id.clone());
                    (UiDeleteRecoveryAction::Restore, UiDeleteRecoveryStatus::Completed)
                }
                (DeleteRecoveryAction::Finalize, DeleteRecoveryStatus::Completed) => {
                    finalized += 1;
                    affected_sources.insert(entry.source_id.clone());
                    (UiDeleteRecoveryAction::Finalize, UiDeleteRecoveryStatus::Completed)
                }
                (DeleteRecoveryAction::Restore, DeleteRecoveryStatus::Failed) => {
                    failed += 1;
                    (UiDeleteRecoveryAction::Restore, UiDeleteRecoveryStatus::Failed)
                }
                (DeleteRecoveryAction::Finalize, DeleteRecoveryStatus::Failed) => {
                    failed += 1;
                    (UiDeleteRecoveryAction::Finalize, UiDeleteRecoveryStatus::Failed)
                }
            };
            ui_entries.push(UiDeleteRecoveryEntry {
                source_label,
                relative_path: entry.original_relative,
                action,
                status,
                detail: entry.detail,
            });
        }
        self.ui.sources.folders.delete_recovery.entries = ui_entries;
        if restored + finalized + failed > 0 {
            let mut message = format!(
                "Recovered {} staged delete(s): {} restored, {} finalized",
                restored + finalized + failed,
                restored,
                finalized
            );
            if failed > 0 || !report.errors.is_empty() {
                let error_count = failed + report.errors.len();
                message.push_str(&format!(" ({} error(s))", error_count));
            }
            let tone = if failed > 0 || !report.errors.is_empty() {
                crate::egui_app::ui::style::StatusTone::Warning
            } else {
                crate::egui_app::ui::style::StatusTone::Info
            };
            self.set_status(message, tone);
        } else if !report.errors.is_empty() {
            self.set_status(
                format!(
                    "Delete recovery encountered {} error(s)",
                    report.errors.len()
                ),
                crate::egui_app::ui::style::StatusTone::Warning,
            );
        }
        for err in report.errors {
            eprintln!("Delete recovery error: {err}");
        }
        if !affected_sources.is_empty() {
            let mut invalidator = source_cache_invalidator::SourceCacheInvalidator::new_from_state(
                &mut self.cache,
                &mut self.ui_cache,
                &mut self.library.missing,
            );
            for source_id in &affected_sources {
                invalidator.invalidate_all(source_id);
            }
            if let Some(source) = self.current_source()
                && affected_sources.contains(&source.id)
            {
                if let Some(loaded) = self.sample_view.wav.loaded_wav.as_ref() {
                    let absolute = source.root.join(loaded);
                    if !absolute.is_file() {
                        self.clear_waveform_view();
                    }
                }
                self.refresh_folder_browser();
                self.queue_wav_load();
            }
        }
    }

    /// Clear the staged delete recovery log.
    pub(crate) fn clear_folder_delete_recovery_log(&mut self) {
        self.ui.sources.folders.delete_recovery.entries.clear();
    }
}

fn recover_journaled_entries(
    source: &SampleSource,
    staging_root: &Path,
    journal: &DeleteJournal,
    report: &mut DeleteRecoveryReport,
) {
    for entry in journal.entries.clone() {
        let original_relative = PathBuf::from(entry.original_relative.clone());
        let staged_relative = PathBuf::from(entry.staged_relative.clone());
        let staged = staging_root.join(&staged_relative);
        let original = source.root.join(&original_relative);
        let (action, status, detail, remove_from_journal) = match entry.stage {
            DeleteJournalStage::DbCommitted => match finalize_staged_folder(&staged) {
                Ok(detail) => (DeleteRecoveryAction::Finalize, DeleteRecoveryStatus::Completed, detail, true),
                Err(err) => (DeleteRecoveryAction::Finalize, DeleteRecoveryStatus::Failed, Some(err), false),
            },
            DeleteJournalStage::Intent | DeleteJournalStage::Staged => {
                if !staged.exists() && original.exists() {
                    (DeleteRecoveryAction::Restore, DeleteRecoveryStatus::Completed, Some("Already restored".into()), true)
                } else {
                    match restore_staged_folder(&staged, &original) {
                        Ok(detail) => (DeleteRecoveryAction::Restore, DeleteRecoveryStatus::Completed, detail, true),
                        Err(err) => (DeleteRecoveryAction::Restore, DeleteRecoveryStatus::Failed, Some(err), false),
                    }
                }
            }
        };
        report.entries.push(DeleteRecoveryEntry {
            source_id: source.id.clone(),
            source_root: source.root.clone(),
            original_relative,
            action,
            status,
            detail: detail.clone(),
        });
        if remove_from_journal {
            if let Err(err) = remove_entry(staging_root, &entry.id) {
                report
                    .errors
                    .push(format!("Failed to update delete journal: {err}"));
            }
        }
    }
}

fn recover_unjournaled_entries(
    source: &SampleSource,
    staging_root: &Path,
    journaled_roots: &[PathBuf],
    report: &mut DeleteRecoveryReport,
) {
    let unjournaled = find_unjournaled_staged_roots(staging_root, &source.root, journaled_roots);
    for relative in unjournaled {
        let staged = staging_root.join(&relative);
        let original = source.root.join(&relative);
        match restore_staged_folder(&staged, &original) {
            Ok(detail) => report.entries.push(DeleteRecoveryEntry {
                source_id: source.id.clone(),
                source_root: source.root.clone(),
                original_relative: relative,
                action: DeleteRecoveryAction::Restore,
                status: DeleteRecoveryStatus::Completed,
                detail,
            }),
            Err(err) => report.entries.push(DeleteRecoveryEntry {
                source_id: source.id.clone(),
                source_root: source.root.clone(),
                original_relative: relative,
                action: DeleteRecoveryAction::Restore,
                status: DeleteRecoveryStatus::Failed,
                detail: Some(err),
            }),
        }
    }
}

fn finalize_staged_folder(staged: &Path) -> Result<Option<String>, String> {
    if !staged.exists() {
        return Ok(Some("Already finalized".into()));
    }
    fs::remove_dir_all(staged).map_err(|err| format!("Failed to delete staged folder: {err}"))?;
    Ok(None)
}

fn restore_staged_folder(staged: &Path, original: &Path) -> Result<Option<String>, String> {
    if !staged.exists() {
        return Err("Staged folder missing".into());
    }
    let (target, detail) = unique_restore_path(original);
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)
            .map_err(|err| format!("Failed to restore folder: {err}"))?;
    }
    fs::rename(staged, &target).map_err(|err| format!("Failed to restore folder: {err}"))?;
    Ok(detail)
}

fn unique_restore_path(original: &Path) -> (PathBuf, Option<String>) {
    if !original.exists() {
        return (original.to_path_buf(), None);
    }
    let parent = original.parent().unwrap_or_else(|| Path::new(""));
    let name = original
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("folder");
    for idx in 1..=1000 {
        let candidate = parent.join(format!("{name}{RESTORE_SUFFIX}-{idx}"));
        if !candidate.exists() {
            return (
                candidate,
                Some(format!("Restored as {}", candidate.display())),
            );
        }
    }
    let fallback = parent.join(format!(
        "{name}{RESTORE_SUFFIX}-{}",
        uuid::Uuid::new_v4()
    ));
    (
        fallback.clone(),
        Some(format!("Restored as {}", fallback.display())),
    )
}

fn journaled_staged_roots(journal: &DeleteJournal) -> Vec<PathBuf> {
    journal
        .entries
        .iter()
        .map(|entry| PathBuf::from(&entry.staged_relative))
        .collect()
}

fn find_unjournaled_staged_roots(
    staging_root: &Path,
    source_root: &Path,
    journaled_roots: &[PathBuf],
) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    let mut stack = vec![PathBuf::new()];
    while let Some(relative) = stack.pop() {
        let current = staging_root.join(&relative);
        let entries = match fs::read_dir(&current) {
            Ok(entries) => entries,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            let name = entry.file_name();
            if name.to_string_lossy() == DELETE_JOURNAL_FILE {
                continue;
            }
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let child_relative = relative.join(&name);
            if path_is_under_roots(&child_relative, journaled_roots) {
                continue;
            }
            let target = source_root.join(&child_relative);
            if target.exists() {
                stack.push(child_relative);
            } else {
                roots.push(child_relative);
            }
        }
    }
    roots
}

fn path_is_under_roots(candidate: &Path, roots: &[PathBuf]) -> bool {
    roots.iter().any(|root| candidate.starts_with(root))
}

fn unique_staging_relative(staging_root: &Path, relative: &Path) -> PathBuf {
    let mut candidate = relative.to_path_buf();
    if !staging_root.join(&candidate).exists() {
        return candidate;
    }
    let parent = relative.parent().unwrap_or_else(|| Path::new(""));
    let name = relative
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("folder");
    for idx in 1..=1000 {
        let mut alt = PathBuf::from(parent);
        alt.push(format!("{name}.staged-{idx}"));
        candidate = alt;
        if !staging_root.join(&candidate).exists() {
            return candidate;
        }
    }
    candidate
}

fn ensure_staging_parent(staged: &Path, staging_root: &Path) -> Result<(), String> {
    if let Some(parent) = staged.parent() {
        fs::create_dir_all(parent)
            .map_err(|err| format!("Failed to prepare folder delete staging: {err}"))?;
        mark_staging_root_hidden(staging_root);
    }
    Ok(())
}

fn mark_staging_root_hidden(staging_root: &Path) {
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::ffi::OsStrExt;
        use windows::{
            Win32::Storage::FileSystem::{FILE_ATTRIBUTE_HIDDEN, SetFileAttributesW},
            core::PCWSTR,
        };

        let mut wide: Vec<u16> = staging_root.as_os_str().encode_wide().collect();
        wide.push(0);
        let _ = unsafe { SetFileAttributesW(PCWSTR(wide.as_ptr()), FILE_ATTRIBUTE_HIDDEN) };
    }
}

/// Remove the staging root if it is now empty.
pub(crate) fn cleanup_staging_root(staging_root: &Path) {
    if let Ok(mut entries) = fs::read_dir(staging_root) {
        if entries.next().is_none() {
            let _ = fs::remove_dir(staging_root);
        }
    }
}

fn new_delete_op_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

fn now_epoch_seconds() -> Result<i64, String> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|err| format!("System time error: {err}"))?;
    Ok(now.as_secs() as i64)
}

fn journal_path(staging_root: &Path) -> PathBuf {
    staging_root.join(DELETE_JOURNAL_FILE)
}

fn load_journal(staging_root: &Path) -> Result<DeleteJournal, String> {
    let path = journal_path(staging_root);
    if !path.exists() {
        return Ok(DeleteJournal::default());
    }
    let bytes = fs::read(&path).map_err(|err| format!("Failed to read delete journal: {err}"))?;
    serde_json::from_slice(&bytes).map_err(|err| format!("Failed to parse delete journal: {err}"))
}

fn insert_entry(staging_root: &Path, entry: DeleteJournalEntry) -> Result<(), String> {
    let mut journal = load_journal(staging_root)?;
    if journal.entries.iter().any(|existing| existing.id == entry.id) {
        return Err("Delete journal entry already exists".into());
    }
    journal.entries.push(entry);
    save_journal(staging_root, &journal)
}

fn update_entry_stage(
    staging_root: &Path,
    id: &str,
    stage: DeleteJournalStage,
) -> Result<(), String> {
    let mut journal = load_journal(staging_root)?;
    let entry = journal
        .entries
        .iter_mut()
        .find(|entry| entry.id == id)
        .ok_or_else(|| "Delete journal entry missing".to_string())?;
    entry.stage = stage;
    save_journal(staging_root, &journal)
}

fn remove_entry(staging_root: &Path, id: &str) -> Result<(), String> {
    let mut journal = load_journal(staging_root)?;
    let before = journal.entries.len();
    journal.entries.retain(|entry| entry.id != id);
    if journal.entries.len() == before {
        return Err("Delete journal entry missing".into());
    }
    save_journal(staging_root, &journal)
}

fn save_journal(staging_root: &Path, journal: &DeleteJournal) -> Result<(), String> {
    fs::create_dir_all(staging_root)
        .map_err(|err| format!("Failed to prepare delete journal: {err}"))?;
    let path = journal_path(staging_root);
    if journal.entries.is_empty() {
        if path.exists() {
            fs::remove_file(&path)
                .map_err(|err| format!("Failed to clear delete journal: {err}"))?;
        }
        cleanup_staging_root(staging_root);
        return Ok(());
    }
    let bytes = serde_json::to_vec_pretty(journal)
        .map_err(|err| format!("Failed to serialize delete journal: {err}"))?;
    let tmp_path = path.with_extension("tmp");
    fs::write(&tmp_path, bytes)
        .map_err(|err| format!("Failed to write delete journal: {err}"))?;
    if path.exists() {
        let _ = fs::remove_file(&path);
    }
    fs::rename(&tmp_path, &path)
        .map_err(|err| format!("Failed to save delete journal: {err}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn unique_restore_path_avoids_collisions() {
        let dir = tempdir().unwrap();
        let original = dir.path().join("folder");
        fs::create_dir_all(&original).unwrap();
        let (target, detail) = unique_restore_path(&original);
        assert_ne!(target, original);
        assert!(detail.is_some());
    }
}
