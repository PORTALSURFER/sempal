use super::*;
use crate::egui_app::controller::library::wav_io::file_metadata;
use crate::egui_app::controller::jobs::{
    ClipboardPasteOutcome, ClipboardPasteResult, FileOpMessage, FileOpResult, SourcePasteAdded,
};
use crate::sample_sources::db::file_ops_journal;
use crate::sample_sources::{SourceDatabase, is_supported_audio};
use std::path::{Path, PathBuf};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc::Sender,
};

impl EguiController {
    /// Paste file paths from the system clipboard into the active source.
    pub fn paste_files_from_clipboard(&mut self) -> bool {
        let paths = match read_clipboard_paths() {
            Ok(Some(paths)) => paths,
            Ok(None) => return false,
            Err(err) => {
                self.set_status(err, StatusTone::Error);
                return true;
            }
        };
        if self.runtime.jobs.file_ops_in_progress() {
            self.set_status("Another file operation is already running", StatusTone::Warning);
            return true;
        }
        let Some(source) = self.current_source() else {
            self.set_status("Select a source first", StatusTone::Info);
            return true;
        };
        let job = ClipboardPasteJob {
            kind: ClipboardPasteJobKind::Source {
                source_id: source.id,
                source_root: source.root,
                target_folder: PathBuf::new(),
            },
            paths,
            action_label: "paste",
            action_progress: "Pasting",
            action_past_tense: "Pasted",
            target_label: "source".to_string(),
        };
        self.begin_clipboard_paste_job(job, "Pasting files");
        true
    }

    /// Import external audio files into the active source folder.
    pub(crate) fn import_external_files_to_source_folder(
        &mut self,
        target_folder: PathBuf,
        paths: Vec<PathBuf>,
    ) {
        if paths.is_empty() {
            return;
        }
        let Some(source) = self.current_source() else {
            self.set_status("Select a source first", StatusTone::Info);
            return;
        };
        if let Err(err) = validate_relative_folder_path(&target_folder) {
            self.set_status(err, StatusTone::Error);
            return;
        }
        if self.runtime.jobs.file_ops_in_progress() {
            self.set_status("Another file operation is already running", StatusTone::Warning);
            return;
        }
        let target_label = if target_folder.as_os_str().is_empty() {
            "source root".to_string()
        } else {
            format!("folder {}", target_folder.display())
        };
        let job = ClipboardPasteJob {
            kind: ClipboardPasteJobKind::Source {
                source_id: source.id,
                source_root: source.root,
                target_folder,
            },
            paths,
            action_label: "import",
            action_progress: "Importing",
            action_past_tense: "Imported",
            target_label,
        };
        self.begin_clipboard_paste_job(job, "Importing files");
    }
}

#[cfg(test)]
impl EguiController {
    /// Import external audio files synchronously for tests without spawning background jobs.
    pub(crate) fn import_external_files_to_source_folder_for_tests(
        &mut self,
        target_folder: PathBuf,
        paths: Vec<PathBuf>,
    ) -> Result<ClipboardPasteResult, String> {
        if paths.is_empty() {
            return Err("No files to import".into());
        }
        let Some(source) = self.current_source() else {
            return Err("Select a source first".into());
        };
        validate_relative_folder_path(&target_folder)?;
        if self.runtime.jobs.file_ops_in_progress() {
            return Err("Another file operation is already running".into());
        }
        let target_label = if target_folder.as_os_str().is_empty() {
            "source root".to_string()
        } else {
            format!("folder {}", target_folder.display())
        };
        let job = ClipboardPasteJob {
            kind: ClipboardPasteJobKind::Source {
                source_id: source.id,
                source_root: source.root,
                target_folder,
            },
            paths,
            action_label: "import",
            action_progress: "Importing",
            action_past_tense: "Imported",
            target_label,
        };
        Ok(run_clipboard_paste_job(
            job,
            Arc::new(AtomicBool::new(false)),
            None,
        ))
    }
}

struct ClipboardPasteJob {
    kind: ClipboardPasteJobKind,
    paths: Vec<PathBuf>,
    action_label: &'static str,
    action_progress: &'static str,
    action_past_tense: &'static str,
    target_label: String,
}

enum ClipboardPasteJobKind {
    Source {
        source_id: SourceId,
        source_root: PathBuf,
        target_folder: PathBuf,
    },
}

fn read_clipboard_paths() -> Result<Option<Vec<PathBuf>>, String> {
    let paths = crate::external_clipboard::read_file_paths()?;
    if paths.is_empty() {
        Ok(None)
    } else {
        Ok(Some(paths))
    }
}

impl EguiController {
    fn begin_clipboard_paste_job(&mut self, job: ClipboardPasteJob, title: &str) {
        if job.paths.is_empty() {
            self.set_status(
                format!("No files to {}", job.action_label),
                StatusTone::Warning,
            );
            return;
        }
        let total = job.paths.len();
        self.set_status(format!("{title}..."), StatusTone::Busy);
        self.show_status_progress(
            crate::egui_app::state::ProgressTaskKind::FileOps,
            title,
            total,
            true,
        );
        let (tx, rx) = std::sync::mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        self.runtime.jobs.start_file_ops(rx, cancel.clone());
        std::thread::spawn(move || {
            let result = run_clipboard_paste_job(job, cancel, Some(&tx));
            let _ = tx.send(FileOpMessage::Finished(FileOpResult::ClipboardPaste(result)));
        });
    }
}

fn unique_destination_name(root: &Path, path: &Path) -> Result<PathBuf, String> {
    let file_name = path
        .file_name()
        .ok_or_else(|| "File has no name".to_string())?;
    let candidate = PathBuf::from(file_name);
    if !root.join(&candidate).exists() {
        return Ok(candidate);
    }
    let stem = path
        .file_stem()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| "sample".to_string());
    let extension = path
        .extension()
        .map(|ext| ext.to_string_lossy().to_string());
    for index in 1..=999 {
        let suffix = format!("{stem}_copy{index:03}");
        let file_name = if let Some(ext) = &extension {
            format!("{suffix}.{ext}")
        } else {
            suffix
        };
        let candidate = PathBuf::from(file_name);
        if !root.join(&candidate).exists() {
            return Ok(candidate);
        }
    }
    Err("Unable to find a unique destination name".into())
}

fn validate_relative_folder_path(path: &Path) -> Result<(), String> {
    if path.is_absolute() {
        return Err("Target folder must be a relative path".into());
    }
    if path
        .components()
        .any(|component| matches!(component, std::path::Component::ParentDir))
    {
        return Err("Target folder cannot contain '..'".into());
    }
    Ok(())
}

fn run_clipboard_paste_job(
    job: ClipboardPasteJob,
    cancel: Arc<AtomicBool>,
    sender: Option<&Sender<FileOpMessage>>,
) -> ClipboardPasteResult {
    let mut skipped = 0usize;
    let mut errors = Vec::new();
    let mut completed = 0usize;
    let mut cancelled = false;
    let outcome = match job.kind {
        ClipboardPasteJobKind::Source {
            source_id,
            source_root,
            target_folder,
        } => {
            let mut added = Vec::new();
            if let Err(err) = validate_relative_folder_path(&target_folder) {
                errors.push(err);
            } else if !source_root.is_dir() {
                errors.push("Source folder is not available".to_string());
            } else {
                let target_root = source_root.join(&target_folder);
                if !target_root.exists() {
                    if let Err(err) = std::fs::create_dir_all(&target_root) {
                        errors.push(format!(
                            "Failed to create folder {}: {err}",
                            target_root.display()
                        ));
                    }
                } else if !target_root.is_dir() {
                    errors.push(format!(
                        "Target folder is not a directory: {}",
                        target_root.display()
                    ));
                }
                let db = match SourceDatabase::open(&source_root) {
                    Ok(db) => Some(db),
                    Err(err) => {
                        errors.push(format!("Failed to open source DB: {err}"));
                        None
                    }
                };
                if errors.is_empty() {
                    for path in job.paths {
                        if cancel.load(Ordering::Relaxed) {
                            cancelled = true;
                            break;
                        }
                        let detail = Some(format!("{} {}", job.action_progress, path.display()));
                        if !path.is_file() || !is_supported_audio(&path) {
                            skipped += 1;
                            completed += 1;
                            report_progress(sender, completed, detail);
                            continue;
                        }
                        let relative_name = match unique_destination_name(&target_root, &path) {
                            Ok(name) => name,
                            Err(err) => {
                                errors.push(err);
                                completed += 1;
                                report_progress(sender, completed, detail);
                                continue;
                            }
                        };
                        let relative = if target_folder.as_os_str().is_empty() {
                            relative_name
                        } else {
                            target_folder.join(relative_name)
                        };
                        let db = match db.as_ref() {
                            Some(db) => db,
                            None => {
                                errors.push("Source DB unavailable".to_string());
                                completed += 1;
                                report_progress(sender, completed, detail);
                                continue;
                            }
                        };
                        let op_id = file_ops_journal::new_op_id();
                        let staged_relative = match file_ops_journal::staged_relative_for_target(&relative, &op_id) {
                            Ok(path) => path,
                            Err(err) => {
                                errors.push(format!("Failed to build staging path: {err}"));
                                completed += 1;
                                report_progress(sender, completed, detail);
                                continue;
                            }
                        };
                        let journal_entry = match file_ops_journal::FileOpJournalEntry::new_copy(
                            op_id.clone(),
                            relative.clone(),
                            staged_relative.clone(),
                        ) {
                            Ok(entry) => entry,
                            Err(err) => {
                                errors.push(format!("Failed to stage copy journal: {err}"));
                                completed += 1;
                                report_progress(sender, completed, detail);
                                continue;
                            }
                        };
                        if let Err(err) = file_ops_journal::insert_entry(db, &journal_entry) {
                            errors.push(format!("Failed to record copy journal: {err}"));
                            completed += 1;
                            report_progress(sender, completed, detail);
                            continue;
                        }
                        let staged_absolute = source_root.join(&staged_relative);
                        if let Err(err) = std::fs::copy(&path, &staged_absolute) {
                            remove_copy_journal_entry(&mut errors, db, &op_id);
                            errors.push(format!(
                                "Failed to {} {}: {err}",
                                job.action_label,
                                path.display()
                            ));
                            completed += 1;
                            report_progress(sender, completed, detail);
                            continue;
                        }
                        let (file_size, modified_ns) = match file_metadata(&staged_absolute) {
                            Ok(meta) => meta,
                            Err(err) => {
                                remove_staged_file(&mut errors, &staged_absolute);
                                remove_copy_journal_entry(&mut errors, db, &op_id);
                                errors.push(err);
                                completed += 1;
                                report_progress(sender, completed, detail);
                                continue;
                            }
                        };
                        if let Err(err) = file_ops_journal::update_stage(
                            db,
                            &op_id,
                            file_ops_journal::FileOpStage::Staged,
                            Some(file_size),
                            Some(modified_ns),
                        ) {
                            remove_staged_file(&mut errors, &staged_absolute);
                            remove_copy_journal_entry(&mut errors, db, &op_id);
                            errors.push(format!("Failed to update copy journal: {err}"));
                            completed += 1;
                            report_progress(sender, completed, detail);
                            continue;
                        }
                        let mut batch = match db.write_batch() {
                            Ok(batch) => batch,
                            Err(err) => {
                                remove_staged_file(&mut errors, &staged_absolute);
                                remove_copy_journal_entry(&mut errors, db, &op_id);
                                errors.push(format!("Failed to open source DB batch: {err}"));
                                completed += 1;
                                report_progress(sender, completed, detail);
                                continue;
                            }
                        };
                        if let Err(err) = batch.upsert_file(&relative, file_size, modified_ns) {
                            remove_staged_file(&mut errors, &staged_absolute);
                            remove_copy_journal_entry(&mut errors, db, &op_id);
                            errors.push(format!("Failed to register file: {err}"));
                            completed += 1;
                            report_progress(sender, completed, detail);
                            continue;
                        }
                        if let Err(err) = batch.commit() {
                            remove_staged_file(&mut errors, &staged_absolute);
                            remove_copy_journal_entry(&mut errors, db, &op_id);
                            errors.push(format!("Failed to commit source DB update: {err}"));
                            completed += 1;
                            report_progress(sender, completed, detail);
                            continue;
                        }
                        if let Err(err) = file_ops_journal::update_stage(
                            db,
                            &op_id,
                            file_ops_journal::FileOpStage::TargetDb,
                            None,
                            None,
                        ) {
                            errors.push(format!("Failed to update copy journal: {err}"));
                        }
                        let absolute = source_root.join(&relative);
                        if let Err(err) = std::fs::rename(&staged_absolute, &absolute) {
                            errors.push(format!("Failed to finalize copy: {err}"));
                            completed += 1;
                            report_progress(sender, completed, detail);
                            continue;
                        }
                        remove_copy_journal_entry(&mut errors, db, &op_id);
                        added.push(SourcePasteAdded {
                            relative_path: relative,
                            file_size,
                            modified_ns,
                        });
                        completed += 1;
                        report_progress(sender, completed, detail);
                    }
                }
            }
            ClipboardPasteOutcome::Source { source_id, added }
        }
    };
    ClipboardPasteResult {
        outcome,
        skipped,
        errors,
        cancelled,
        target_label: job.target_label,
        action_past_tense: job.action_past_tense,
    }
}

fn report_progress(
    sender: Option<&Sender<FileOpMessage>>,
    completed: usize,
    detail: Option<String>,
) {
    if let Some(tx) = sender {
        let _ = tx.send(FileOpMessage::Progress { completed, detail });
    }
}

fn remove_copy_journal_entry(errors: &mut Vec<String>, db: &SourceDatabase, op_id: &str) {
    if let Err(err) = file_ops_journal::remove_entry(db, op_id) {
        errors.push(format!("Failed to clear copy journal: {err}"));
    }
}

fn remove_staged_file(errors: &mut Vec<String>, path: &Path) {
    if let Err(err) = std::fs::remove_file(path) {
        errors.push(format!(
            "Failed to remove staged file {}: {err}",
            path.display()
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_relative_folder_path_blocks_parent_dirs() {
        assert!(validate_relative_folder_path(Path::new("..")).is_err());
        assert!(validate_relative_folder_path(Path::new("foo/../bar")).is_err());
    }

    #[test]
    fn validate_relative_folder_path_allows_relative() {
        assert!(validate_relative_folder_path(Path::new("")).is_ok());
        assert!(validate_relative_folder_path(Path::new("samples/drums")).is_ok());
    }
}
