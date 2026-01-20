use super::*;
use crate::egui_app::controller::library::collection_export;
use crate::egui_app::controller::library::collection_items_helpers::file_metadata;
use crate::egui_app::controller::jobs::{
    ClipboardPasteOutcome, ClipboardPasteResult, FileOpMessage, FileOpResult, SourcePasteAdded,
};
use crate::sample_sources::{SourceDatabase, is_supported_audio};
use std::path::{Path, PathBuf};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc::Sender,
};

impl EguiController {
    /// Paste file paths from the system clipboard into the active source or collection.
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
        let job = if let Some(collection_id) = self.current_collection_id() {
            let Some(collection) = self
                .library
                .collections
                .iter()
                .find(|collection| &collection.id == &collection_id)
            else {
                self.set_status("Collection not found", StatusTone::Error);
                return true;
            };
            ClipboardPasteJob {
                kind: ClipboardPasteJobKind::Collection {
                    collection_id,
                    export_root: collection_export::resolved_export_dir(
                        collection,
                        self.settings.collection_export_root.as_deref(),
                    ),
                },
                paths,
                action_label: "paste",
                action_progress: "Pasting",
                action_past_tense: "Pasted",
                target_label: "collection".to_string(),
            }
        } else {
            let Some(source) = self.current_source() else {
                self.set_status("Select a source first", StatusTone::Info);
                return true;
            };
            ClipboardPasteJob {
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
            }
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
    Collection {
        collection_id: CollectionId,
        export_root: Option<PathBuf>,
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

fn resolve_collection_clip_root(
    collection_id: &CollectionId,
    export_root: Option<PathBuf>,
) -> Result<PathBuf, String> {
    if let Some(path) = export_root {
        if path.exists() && !path.is_dir() {
            return Err(format!(
                "Collection export path is not a directory: {}",
                path.display()
            ));
        }
        std::fs::create_dir_all(&path).map_err(|err| {
            format!(
                "Failed to create collection export path {}: {err}",
                path.display()
            )
        })?;
        return Ok(path);
    }
    let fallback = crate::app_dirs::app_root_dir()
        .map_err(|err| err.to_string())?
        .join("collection_clips")
        .join(collection_id.as_str());
    std::fs::create_dir_all(&fallback)
        .map_err(|err| format!("Failed to create collection clip folder: {err}"))?;
    Ok(fallback)
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
                        let absolute = source_root.join(&relative);
                        if let Err(err) = std::fs::copy(&path, &absolute) {
                            errors.push(format!(
                                "Failed to {} {}: {err}",
                                job.action_label,
                                path.display()
                            ));
                            completed += 1;
                            report_progress(sender, completed, detail);
                            continue;
                        }
                        let result = (|| -> Result<(u64, i64), String> {
                            let (file_size, modified_ns) = file_metadata(&absolute)?;
                            let db = db.as_ref().ok_or_else(|| "Source DB unavailable".to_string())?;
                            db.upsert_file(&relative, file_size, modified_ns)
                                .map_err(|err| format!("Failed to register file: {err}"))?;
                            Ok((file_size, modified_ns))
                        })();
                        match result {
                            Ok((file_size, modified_ns)) => {
                                added.push(SourcePasteAdded {
                                    relative_path: relative,
                                    file_size,
                                    modified_ns,
                                });
                            }
                            Err(err) => {
                                if let Err(remove_err) = std::fs::remove_file(&absolute) {
                                    errors.push(format!(
                                        "{err}; failed to remove file {}: {remove_err}",
                                        absolute.display()
                                    ));
                                } else {
                                    errors.push(err);
                                }
                            }
                        }
                        completed += 1;
                        report_progress(sender, completed, detail);
                    }
                }
            }
            ClipboardPasteOutcome::Source { source_id, added }
        }
        ClipboardPasteJobKind::Collection {
            collection_id,
            export_root,
        } => {
            let mut added = Vec::new();
            let clip_root = match resolve_collection_clip_root(&collection_id, export_root) {
                Ok(path) => path,
                Err(err) => {
                    errors.push(err);
                    return ClipboardPasteResult {
                        outcome: ClipboardPasteOutcome::Collection {
                            collection_id,
                            clip_root: PathBuf::new(),
                            added,
                        },
                        skipped,
                        errors,
                        cancelled,
                        target_label: job.target_label,
                        action_past_tense: job.action_past_tense,
                    };
                }
            };
            let db = match SourceDatabase::open(&clip_root) {
                Ok(db) => Some(db),
                Err(err) => {
                    errors.push(format!("Failed to open collection DB: {err}"));
                    None
                }
            };
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
                let relative = match unique_destination_name(&clip_root, &path) {
                    Ok(name) => name,
                    Err(err) => {
                        errors.push(err);
                        completed += 1;
                        report_progress(sender, completed, detail);
                        continue;
                    }
                };
                let absolute = clip_root.join(&relative);
                if let Err(err) = std::fs::copy(&path, &absolute) {
                    errors.push(format!(
                        "Failed to {} {}: {err}",
                        job.action_label,
                        path.display()
                    ));
                    completed += 1;
                    report_progress(sender, completed, detail);
                    continue;
                }
                let result = (|| -> Result<(), String> {
                    let (file_size, modified_ns) = file_metadata(&absolute)?;
                    let db = db.as_ref().ok_or_else(|| "Collection DB unavailable".to_string())?;
                    db.upsert_file(&relative, file_size, modified_ns)
                        .map_err(|err| format!("Failed to register collection file: {err}"))?;
                    Ok(())
                })();
                if let Err(err) = result {
                    if let Err(remove_err) = std::fs::remove_file(&absolute) {
                        errors.push(format!(
                            "{err}; failed to remove file {}: {remove_err}",
                            absolute.display()
                        ));
                    } else {
                        errors.push(err);
                    }
                } else {
                    added.push(relative);
                }
                completed += 1;
                report_progress(sender, completed, detail);
            }
            ClipboardPasteOutcome::Collection {
                collection_id,
                clip_root,
                added,
            }
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
