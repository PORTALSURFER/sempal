use super::super::{DragDropController, file_metadata};
use crate::egui_app::controller::jobs::{
    FileOpMessage, FileOpResult, FolderEntryMove, FolderMoveRequest, FolderMoveResult,
    FolderSampleMoveRequest, FolderSampleMoveResult,
};
use crate::egui_app::state::{DragSample, ProgressTaskKind};
use crate::egui_app::ui::style::StatusTone;
use crate::sample_sources::{SourceDatabase, SourceId, WavEntry};
use std::path::{Path, PathBuf};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc::Sender,
};
use tracing::{info, warn};

impl DragDropController<'_> {
    /// Enqueue a background move for a single sample dropped into a folder.
    pub(crate) fn handle_sample_drop_to_folder(
        &mut self,
        source_id: SourceId,
        relative_path: PathBuf,
        target_folder: &Path,
    ) {
        let sample = DragSample {
            source_id,
            relative_path,
        };
        self.handle_samples_drop_to_folder(std::slice::from_ref(&sample), target_folder);
    }

    /// Enqueue background moves for multiple samples dropped into a folder.
    pub(crate) fn handle_samples_drop_to_folder(
        &mut self,
        samples: &[DragSample],
        target_folder: &Path,
    ) {
        if samples.is_empty() {
            return;
        }
        info!(
            "Folder drop requested: sample_count={} target={}",
            samples.len(),
            target_folder.display()
        );
        if self.runtime.jobs.file_ops_in_progress() {
            self.set_status("Another file operation is already running", StatusTone::Warning);
            return;
        }
        let source_id = samples[0].source_id.clone();
        if samples.iter().any(|sample| sample.source_id != source_id) {
            self.set_status(
                "Samples must come from the same source to move into a folder",
                StatusTone::Warning,
            );
            return;
        }
        let Some(source) = self
            .library
            .sources
            .iter()
            .find(|s| s.id == source_id)
            .cloned()
        else {
            warn!("Folder move: missing source {:?}", source_id);
            self.set_status("Source not available for move", StatusTone::Error);
            return;
        };
        if self
            .selection_state
            .ctx
            .selected_source
            .as_ref()
            .is_some_and(|selected| selected != &source.id)
        {
            warn!(
                "Folder move blocked: selected source {:?} differs from sample source {:?}",
                self.selection_state.ctx.selected_source, source.id
            );
            self.set_status(
                "Switch to the sample's source before moving into its folders",
                StatusTone::Warning,
            );
            return;
        }
        let mut requests = Vec::new();
        let mut errors = Vec::new();
        let mut skipped = 0usize;
        for sample in samples {
            let file_name = match sample.relative_path.file_name() {
                Some(name) => name.to_owned(),
                None => {
                    errors.push(format!(
                        "Sample name unavailable for move: {}",
                        sample.relative_path.display()
                    ));
                    continue;
                }
            };
            let new_relative = if target_folder.as_os_str().is_empty() {
                PathBuf::from(file_name)
            } else {
                target_folder.join(file_name)
            };
            if new_relative == sample.relative_path {
                skipped += 1;
                continue;
            }
            requests.push(FolderSampleMoveRequest {
                relative_path: sample.relative_path.clone(),
                target_relative: new_relative,
            });
        }
        if requests.is_empty() {
            if let Some(err) = errors.first() {
                self.set_status(err.clone(), StatusTone::Error);
            } else if skipped > 0 {
                self.set_status("Samples are already in that folder", StatusTone::Info);
            }
            return;
        }
        let label = if requests.len() == 1 {
            "Moving sample"
        } else {
            "Moving samples"
        };
        self.set_status(format!("{label}..."), StatusTone::Busy);
        self.show_status_progress(
            ProgressTaskKind::FileOps,
            label.to_string(),
            requests.len(),
            true,
        );
        let cancel = Arc::new(AtomicBool::new(false));
        #[cfg(test)]
        {
            let result = run_folder_sample_move_task(
                source.id.clone(),
                source.root.clone(),
                requests,
                errors,
                cancel,
                None,
            );
            let message = FileOpMessage::Finished(FileOpResult::FolderSampleMove(result));
            if let FileOpMessage::Finished(FileOpResult::FolderSampleMove(result)) = message {
                self.apply_folder_sample_move_result(result);
            }
            if self.ui.progress.task == Some(ProgressTaskKind::FileOps) {
                self.clear_progress();
            }
        }
        #[cfg(not(test))]
        {
            let (tx, rx) = std::sync::mpsc::channel();
            self.runtime.jobs.start_file_ops(rx, cancel.clone());
            std::thread::spawn(move || {
                let result = run_folder_sample_move_task(
                    source.id.clone(),
                    source.root.clone(),
                    requests,
                    errors,
                    cancel,
                    Some(&tx),
                );
                let _ = tx.send(FileOpMessage::Finished(FileOpResult::FolderSampleMove(result)));
            });
        }
    }

    /// Apply a completed background in-source sample move job.
    pub(crate) fn apply_folder_sample_move_result(&mut self, result: FolderSampleMoveResult) {
        let Some(source) = self
            .library
            .sources
            .iter()
            .find(|source| source.id == result.source_id)
            .cloned()
        else {
            self.set_status("Source not available for move", StatusTone::Error);
            return;
        };
        let mut updates = Vec::new();
        let mut collections_changed = false;
        for entry in &result.moved {
            let old_entry = WavEntry {
                relative_path: entry.old_relative.clone(),
                file_size: entry.file_size,
                modified_ns: entry.modified_ns,
                content_hash: None,
                tag: entry.tag,
                looped: entry.looped,
                missing: false,
                last_played_at: entry.last_played_at,
            };
            let new_entry = WavEntry {
                relative_path: entry.new_relative.clone(),
                file_size: entry.file_size,
                modified_ns: entry.modified_ns,
                content_hash: None,
                tag: entry.tag,
                looped: entry.looped,
                missing: false,
                last_played_at: entry.last_played_at,
            };
            updates.push((old_entry, new_entry));
            if self.update_collections_for_rename(
                &source.id,
                &entry.old_relative,
                &entry.new_relative,
            ) {
                collections_changed = true;
            }
        }
        if !updates.is_empty() {
            self.apply_folder_entry_updates(&source, &updates);
        }
        if collections_changed {
            let _ = self.persist_config("Failed to save collections after move");
        }
        let moved = result.moved.len();
        if moved == 0 && result.errors.is_empty() {
            if result.cancelled {
                self.set_status("Move cancelled", StatusTone::Warning);
            } else {
                self.set_status("No samples moved", StatusTone::Warning);
            }
            return;
        }
        let tone = if result.errors.is_empty() && !result.cancelled {
            StatusTone::Info
        } else {
            StatusTone::Warning
        };
        let mut message = format!("Moved {moved} sample(s)");
        if !result.errors.is_empty() {
            message.push_str(&format!(" with {} error(s)", result.errors.len()));
        }
        if result.cancelled {
            message.push_str(" (cancelled)");
        }
        self.set_status(message, tone);
        for err in &result.errors {
            eprintln!("Folder move error: {err}");
        }
        info!(
            "Folder move completed: {} moved, {} errors",
            moved,
            result.errors.len()
        );
    }

    /// Enqueue a background move for a folder dropped onto another folder.
    pub(crate) fn handle_folder_drop_to_folder(
        &mut self,
        source_id: SourceId,
        folder: PathBuf,
        target_folder: &Path,
    ) {
        info!(
            "Folder drag requested: source_id={:?} folder={} target={}",
            source_id,
            folder.display(),
            target_folder.display()
        );
        if self.runtime.jobs.file_ops_in_progress() {
            self.set_status("Another file operation is already running", StatusTone::Warning);
            return;
        }
        let Some(source) = self
            .library
            .sources
            .iter()
            .find(|s| s.id == source_id)
            .cloned()
        else {
            warn!("Folder drag: missing source {:?}", source_id);
            self.set_status("Source not available for move", StatusTone::Error);
            return;
        };
        if folder.as_os_str().is_empty() {
            self.set_status("Root folder cannot be moved", StatusTone::Warning);
            return;
        }
        if target_folder == folder {
            self.set_status("Folder is already there", StatusTone::Info);
            return;
        }
        if target_folder.starts_with(&folder) {
            self.set_status("Cannot move a folder into itself", StatusTone::Warning);
            return;
        }
        if self
            .selection_state
            .ctx
            .selected_source
            .as_ref()
            .is_some_and(|selected| selected != &source.id)
        {
            warn!(
                "Folder drag blocked: selected source {:?} differs from folder source {:?}",
                self.selection_state.ctx.selected_source, source.id
            );
            self.set_status(
                "Switch to the folder's source before moving it",
                StatusTone::Warning,
            );
            return;
        }
        let label = "Moving folder";
        self.set_status(format!("{label}..."), StatusTone::Busy);
        self.show_status_progress(ProgressTaskKind::FileOps, label.to_string(), 1, true);
        let cancel = Arc::new(AtomicBool::new(false));
        let request = FolderMoveRequest {
            source_id: source.id.clone(),
            source_root: source.root.clone(),
            folder,
            target_folder: target_folder.to_path_buf(),
        };
        #[cfg(test)]
        {
            let result = run_folder_move_task(request, cancel, None);
            let message = FileOpMessage::Finished(FileOpResult::FolderMove(result));
            if let FileOpMessage::Finished(FileOpResult::FolderMove(result)) = message {
                self.apply_folder_move_result(result);
            }
            if self.ui.progress.task == Some(ProgressTaskKind::FileOps) {
                self.clear_progress();
            }
        }
        #[cfg(not(test))]
        {
            let (tx, rx) = std::sync::mpsc::channel();
            self.runtime.jobs.start_file_ops(rx, cancel.clone());
            std::thread::spawn(move || {
                let result = run_folder_move_task(request, cancel, Some(&tx));
                let _ = tx.send(FileOpMessage::Finished(FileOpResult::FolderMove(result)));
            });
        }
    }

    /// Apply a completed background folder move job.
    pub(crate) fn apply_folder_move_result(&mut self, result: FolderMoveResult) {
        let Some(source) = self
            .library
            .sources
            .iter()
            .find(|source| source.id == result.source_id)
            .cloned()
        else {
            self.set_status("Source not available for move", StatusTone::Error);
            return;
        };
        if !result.folder_moved {
            if result.errors.is_empty() {
                if result.cancelled {
                    self.set_status("Move cancelled", StatusTone::Warning);
                } else {
                    self.set_status("No folders moved", StatusTone::Warning);
                }
            } else if result.cancelled {
                self.set_status("Move cancelled", StatusTone::Warning);
            } else {
                self.set_status(result.errors[0].clone(), StatusTone::Error);
            }
            for err in &result.errors {
                eprintln!("Folder move error: {err}");
            }
            return;
        }
        let mut updates = Vec::new();
        let mut collections_changed = false;
        for entry in &result.moved {
            let old_entry = WavEntry {
                relative_path: entry.old_relative.clone(),
                file_size: entry.file_size,
                modified_ns: entry.modified_ns,
                content_hash: None,
                tag: entry.tag,
                looped: entry.looped,
                missing: false,
                last_played_at: entry.last_played_at,
            };
            let new_entry = WavEntry {
                relative_path: entry.new_relative.clone(),
                file_size: entry.file_size,
                modified_ns: entry.modified_ns,
                content_hash: None,
                tag: entry.tag,
                looped: entry.looped,
                missing: false,
                last_played_at: entry.last_played_at,
            };
            updates.push((old_entry, new_entry));
            if self.update_collections_for_rename(
                &source.id,
                &entry.old_relative,
                &entry.new_relative,
            ) {
                collections_changed = true;
            }
        }
        if !updates.is_empty() {
            self.apply_folder_entry_updates(&source, &updates);
        }
        if collections_changed {
            let _ = self.persist_config("Failed to save collection after folder move");
        }
        self.remap_folder_state(&result.old_folder, &result.new_folder);
        self.remap_manual_folders(&result.old_folder, &result.new_folder);
        self.focus_drop_target_folder(&result.new_folder);
        let tone = if result.errors.is_empty() && !result.cancelled {
            StatusTone::Info
        } else {
            StatusTone::Warning
        };
        let mut message = format!("Moved folder to {}", result.new_folder.display());
        if !result.errors.is_empty() {
            message.push_str(&format!(" with {} error(s)", result.errors.len()));
        }
        if result.cancelled {
            message.push_str(" (cancelled)");
        }
        self.set_status(message, tone);
        for err in &result.errors {
            eprintln!("Folder move error: {err}");
        }
    }
}

fn run_folder_sample_move_task(
    source_id: SourceId,
    source_root: PathBuf,
    requests: Vec<FolderSampleMoveRequest>,
    mut errors: Vec<String>,
    cancel: Arc<AtomicBool>,
    sender: Option<&Sender<FileOpMessage>>,
) -> FolderSampleMoveResult {
    let mut moved = Vec::new();
    let mut completed = 0usize;
    let mut cancelled = false;
    if !source_root.is_dir() {
        errors.push(format!(
            "Source folder missing: {}",
            source_root.display()
        ));
        return FolderSampleMoveResult {
            source_id,
            moved,
            errors,
            cancelled,
        };
    }
    let db = match SourceDatabase::open(&source_root) {
        Ok(db) => db,
        Err(err) => {
            errors.push(format!("Failed to open source DB: {err}"));
            return FolderSampleMoveResult {
                source_id,
                moved,
                errors,
                cancelled,
            };
        }
    };
    for request in requests {
        if cancel.load(Ordering::Relaxed) {
            cancelled = true;
            break;
        }
        let detail = Some(format!("Moving {}", request.relative_path.display()));
        let absolute = source_root.join(&request.relative_path);
        if !absolute.is_file() {
            errors.push(format!("File missing: {}", request.relative_path.display()));
            completed += 1;
            report_progress(sender, completed, detail);
            continue;
        }
        if let Some(parent) = request.target_relative.parent() {
            let target_dir = source_root.join(parent);
            if !target_dir.is_dir() {
                errors.push(format!(
                    "Folder not found: {}",
                    parent.display()
                ));
                completed += 1;
                report_progress(sender, completed, detail);
                continue;
            }
        }
        let target_absolute = source_root.join(&request.target_relative);
        if target_absolute.exists() {
            errors.push(format!(
                "A file already exists at {}",
                request.target_relative.display()
            ));
            completed += 1;
            report_progress(sender, completed, detail);
            continue;
        }
        let tag = match db.tag_for_path(&request.relative_path) {
            Ok(Some(tag)) => tag,
            Ok(None) => {
                errors.push("Sample not found in database".to_string());
                completed += 1;
                report_progress(sender, completed, detail);
                continue;
            }
            Err(err) => {
                errors.push(format!("Failed to read database: {err}"));
                completed += 1;
                report_progress(sender, completed, detail);
                continue;
            }
        };
        let looped = match db.looped_for_path(&request.relative_path) {
            Ok(Some(looped)) => looped,
            Ok(None) => {
                errors.push("Sample not found in database".to_string());
                completed += 1;
                report_progress(sender, completed, detail);
                continue;
            }
            Err(err) => {
                errors.push(format!("Failed to read database: {err}"));
                completed += 1;
                report_progress(sender, completed, detail);
                continue;
            }
        };
        let last_played_at = match db.last_played_at_for_path(&request.relative_path) {
            Ok(value) => value,
            Err(err) => {
                errors.push(format!("Failed to read database: {err}"));
                completed += 1;
                report_progress(sender, completed, detail);
                continue;
            }
        };
        if let Err(err) = std::fs::rename(&absolute, &target_absolute) {
            errors.push(format!("Failed to move file: {err}"));
            completed += 1;
            report_progress(sender, completed, detail);
            continue;
        }
        let (file_size, modified_ns) = match file_metadata(&target_absolute) {
            Ok(meta) => meta,
            Err(err) => {
                let _ = std::fs::rename(&target_absolute, &absolute);
                errors.push(err);
                completed += 1;
                report_progress(sender, completed, detail);
                continue;
            }
        };
        let mut batch = match db.write_batch() {
            Ok(batch) => batch,
            Err(err) => {
                let _ = std::fs::rename(&target_absolute, &absolute);
                errors.push(format!("Failed to start database update: {err}"));
                completed += 1;
                report_progress(sender, completed, detail);
                continue;
            }
        };
        if let Err(err) = batch.remove_file(&request.relative_path) {
            let _ = std::fs::rename(&target_absolute, &absolute);
            errors.push(format!("Failed to drop old entry: {err}"));
            completed += 1;
            report_progress(sender, completed, detail);
            continue;
        }
        if let Err(err) = batch.upsert_file(&request.target_relative, file_size, modified_ns) {
            let _ = std::fs::rename(&target_absolute, &absolute);
            errors.push(format!("Failed to register moved file: {err}"));
            completed += 1;
            report_progress(sender, completed, detail);
            continue;
        }
        if let Err(err) = batch.set_tag(&request.target_relative, tag) {
            let _ = std::fs::rename(&target_absolute, &absolute);
            errors.push(format!("Failed to copy tag: {err}"));
            completed += 1;
            report_progress(sender, completed, detail);
            continue;
        }
        if let Err(err) = batch.set_looped(&request.target_relative, looped) {
            let _ = std::fs::rename(&target_absolute, &absolute);
            errors.push(format!("Failed to copy loop marker: {err}"));
            completed += 1;
            report_progress(sender, completed, detail);
            continue;
        }
        if let Some(last_played_at) = last_played_at {
            if let Err(err) = batch.set_last_played_at(&request.target_relative, last_played_at) {
                let _ = std::fs::rename(&target_absolute, &absolute);
                errors.push(format!("Failed to copy playback age: {err}"));
                completed += 1;
                report_progress(sender, completed, detail);
                continue;
            }
        }
        if let Err(err) = batch.commit() {
            let _ = std::fs::rename(&target_absolute, &absolute);
            errors.push(format!("Failed to save move: {err}"));
            completed += 1;
            report_progress(sender, completed, detail);
            continue;
        }
        moved.push(FolderEntryMove {
            old_relative: request.relative_path,
            new_relative: request.target_relative,
            file_size,
            modified_ns,
            tag,
            looped,
            last_played_at,
        });
        completed += 1;
        report_progress(sender, completed, detail);
    }
    FolderSampleMoveResult {
        source_id,
        moved,
        errors,
        cancelled,
    }
}

fn run_folder_move_task(
    request: FolderMoveRequest,
    cancel: Arc<AtomicBool>,
    sender: Option<&Sender<FileOpMessage>>,
) -> FolderMoveResult {
    let mut errors = Vec::new();
    let mut moved = Vec::new();
    let mut folder_moved = false;
    let mut cancelled = false;
    if cancel.load(Ordering::Relaxed) {
        cancelled = true;
        return FolderMoveResult {
            source_id: request.source_id,
            old_folder: request.folder,
            new_folder: request.target_folder,
            folder_moved,
            moved,
            errors,
            cancelled,
        };
    }
    if request.folder.as_os_str().is_empty() {
        errors.push("Root folder cannot be moved".to_string());
        return FolderMoveResult {
            source_id: request.source_id,
            old_folder: request.folder,
            new_folder: request.target_folder,
            folder_moved,
            moved,
            errors,
            cancelled,
        };
    }
    if request.target_folder == request.folder {
        errors.push("Folder is already there".to_string());
        return FolderMoveResult {
            source_id: request.source_id,
            old_folder: request.folder,
            new_folder: request.target_folder,
            folder_moved,
            moved,
            errors,
            cancelled,
        };
    }
    if request.target_folder.starts_with(&request.folder) {
        errors.push("Cannot move a folder into itself".to_string());
        return FolderMoveResult {
            source_id: request.source_id,
            old_folder: request.folder,
            new_folder: request.target_folder,
            folder_moved,
            moved,
            errors,
            cancelled,
        };
    }
    let folder_name = match request.folder.file_name() {
        Some(name) => name.to_owned(),
        None => {
            errors.push("Folder name unavailable for move".to_string());
            return FolderMoveResult {
                source_id: request.source_id,
                old_folder: request.folder,
                new_folder: request.target_folder,
                folder_moved,
                moved,
                errors,
                cancelled,
            };
        }
    };
    let new_relative = if request.target_folder.as_os_str().is_empty() {
        PathBuf::from(folder_name)
    } else {
        request.target_folder.join(folder_name)
    };
    let absolute_old = request.source_root.join(&request.folder);
    if !absolute_old.is_dir() {
        errors.push(format!("Folder not found: {}", request.folder.display()));
        return FolderMoveResult {
            source_id: request.source_id,
            old_folder: request.folder,
            new_folder: new_relative,
            folder_moved,
            moved,
            errors,
            cancelled,
        };
    }
    if !request.target_folder.as_os_str().is_empty() {
        let destination_dir = request.source_root.join(&request.target_folder);
        if !destination_dir.is_dir() {
            errors.push(format!(
                "Folder not found: {}",
                request.target_folder.display()
            ));
            return FolderMoveResult {
                source_id: request.source_id,
                old_folder: request.folder,
                new_folder: new_relative,
                folder_moved,
                moved,
                errors,
                cancelled,
            };
        }
    }
    let absolute_new = request.source_root.join(&new_relative);
    if absolute_new.exists() {
        errors.push(format!("Folder already exists: {}", new_relative.display()));
        return FolderMoveResult {
            source_id: request.source_id,
            old_folder: request.folder,
            new_folder: new_relative,
            folder_moved,
            moved,
            errors,
            cancelled,
        };
    }
    let db = match SourceDatabase::open(&request.source_root) {
        Ok(db) => db,
        Err(err) => {
            errors.push(format!("Failed to open source DB: {err}"));
            return FolderMoveResult {
                source_id: request.source_id,
                old_folder: request.folder,
                new_folder: new_relative,
                folder_moved,
                moved,
                errors,
                cancelled,
            };
        }
    };
    let entries = match db.list_files() {
        Ok(entries) => entries
            .into_iter()
            .filter(|entry| entry.relative_path.starts_with(&request.folder))
            .collect::<Vec<_>>(),
        Err(err) => {
            errors.push(format!("Failed to list folder entries: {err}"));
            return FolderMoveResult {
                source_id: request.source_id,
                old_folder: request.folder,
                new_folder: new_relative,
                folder_moved,
                moved,
                errors,
                cancelled,
            };
        }
    };
    if let Err(err) = std::fs::rename(&absolute_old, &absolute_new) {
        errors.push(format!("Failed to move folder: {err}"));
        return FolderMoveResult {
            source_id: request.source_id,
            old_folder: request.folder,
            new_folder: new_relative,
            folder_moved,
            moved,
            errors,
            cancelled,
        };
    }
    folder_moved = true;
    if !entries.is_empty() {
        let mut updates = Vec::with_capacity(entries.len());
        let mut batch = match db.write_batch() {
            Ok(batch) => batch,
            Err(err) => {
                let _ = std::fs::rename(&absolute_new, &absolute_old);
                errors.push(format!("Failed to start database update: {err}"));
                return FolderMoveResult {
                    source_id: request.source_id,
                    old_folder: request.folder,
                    new_folder: new_relative,
                    folder_moved: false,
                    moved,
                    errors,
                    cancelled,
                };
            }
        };
        for entry in &entries {
            let suffix = entry
                .relative_path
                .strip_prefix(&request.folder)
                .unwrap_or_else(|_| Path::new(""));
            let updated_path = new_relative.join(suffix);
            if let Err(err) = batch.remove_file(&entry.relative_path) {
                let _ = std::fs::rename(&absolute_new, &absolute_old);
                errors.push(format!("Failed to drop old entry: {err}"));
                return FolderMoveResult {
                    source_id: request.source_id,
                    old_folder: request.folder,
                    new_folder: new_relative,
                    folder_moved: false,
                    moved,
                    errors,
                    cancelled,
                };
            }
            if let Err(err) = batch.upsert_file(&updated_path, entry.file_size, entry.modified_ns) {
                let _ = std::fs::rename(&absolute_new, &absolute_old);
                errors.push(format!("Failed to register moved file: {err}"));
                return FolderMoveResult {
                    source_id: request.source_id,
                    old_folder: request.folder,
                    new_folder: new_relative,
                    folder_moved: false,
                    moved,
                    errors,
                    cancelled,
                };
            }
            if let Err(err) = batch.set_tag(&updated_path, entry.tag) {
                let _ = std::fs::rename(&absolute_new, &absolute_old);
                errors.push(format!("Failed to copy tag: {err}"));
                return FolderMoveResult {
                    source_id: request.source_id,
                    old_folder: request.folder,
                    new_folder: new_relative,
                    folder_moved: false,
                    moved,
                    errors,
                    cancelled,
                };
            }
            if let Some(last_played_at) = entry.last_played_at {
                if let Err(err) = batch.set_last_played_at(&updated_path, last_played_at) {
                    let _ = std::fs::rename(&absolute_new, &absolute_old);
                    errors.push(format!("Failed to copy playback age: {err}"));
                    return FolderMoveResult {
                        source_id: request.source_id,
                        old_folder: request.folder,
                        new_folder: new_relative,
                        folder_moved: false,
                        moved,
                        errors,
                        cancelled,
                    };
                }
            }
            updates.push(FolderEntryMove {
                old_relative: entry.relative_path.clone(),
                new_relative: updated_path,
                file_size: entry.file_size,
                modified_ns: entry.modified_ns,
                tag: entry.tag,
                looped: entry.looped,
                last_played_at: entry.last_played_at,
            });
        }
        if let Err(err) = batch.commit() {
            let _ = std::fs::rename(&absolute_new, &absolute_old);
            errors.push(format!("Failed to save folder move: {err}"));
            return FolderMoveResult {
                source_id: request.source_id,
                old_folder: request.folder,
                new_folder: new_relative,
                folder_moved: false,
                moved,
                errors,
                cancelled,
            };
        }
        moved = updates;
    }
    report_progress(sender, 1, Some(format!("Moved {}", request.folder.display())));
    FolderMoveResult {
        source_id: request.source_id,
        old_folder: request.folder,
        new_folder: new_relative,
        folder_moved,
        moved,
        errors,
        cancelled,
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
    use crate::egui_app::controller::test_support::write_test_wav;
    use crate::sample_sources::{Rating, SampleSource};
    use tempfile::tempdir;

    #[test]
    fn folder_sample_move_updates_db_entry() {
        let temp = tempdir().unwrap();
        let source_root = temp.path().join("source");
        let target_dir = source_root.join("folder");
        std::fs::create_dir_all(&target_dir).unwrap();
        let source = SampleSource::new(source_root.clone());
        let wav_path = source_root.join("one.wav");
        write_test_wav(&wav_path, &[0.0, 0.1, -0.1]);
        let (file_size, modified_ns) = file_metadata(&wav_path).unwrap();
        let db = SourceDatabase::open(&source_root).unwrap();
        let mut batch = db.write_batch().unwrap();
        batch
            .upsert_file(Path::new("one.wav"), file_size, modified_ns)
            .unwrap();
        batch.set_tag(Path::new("one.wav"), Rating::KEEP_1).unwrap();
        batch.set_looped(Path::new("one.wav"), true).unwrap();
        batch
            .set_last_played_at(Path::new("one.wav"), 42)
            .unwrap();
        batch.commit().unwrap();

        let request = FolderSampleMoveRequest {
            relative_path: PathBuf::from("one.wav"),
            target_relative: PathBuf::from("folder/one.wav"),
        };
        let result = run_folder_sample_move_task(
            source.id.clone(),
            source_root.clone(),
            vec![request],
            Vec::new(),
            Arc::new(AtomicBool::new(false)),
            None,
        );

        assert!(result.errors.is_empty());
        assert_eq!(result.moved.len(), 1);
        assert!(source_root.join("folder/one.wav").is_file());

        let db = SourceDatabase::open(&source_root).unwrap();
        assert!(db.tag_for_path(Path::new("one.wav")).unwrap().is_none());
        assert_eq!(
            db.tag_for_path(Path::new("folder/one.wav"))
                .unwrap(),
            Some(Rating::KEEP_1)
        );
        assert_eq!(
            db.looped_for_path(Path::new("folder/one.wav"))
                .unwrap(),
            Some(true)
        );
        assert_eq!(
            db.last_played_at_for_path(Path::new("folder/one.wav"))
                .unwrap(),
            Some(42)
        );
    }

    #[test]
    fn folder_move_updates_db_entries() {
        let temp = tempdir().unwrap();
        let source_root = temp.path().join("source");
        let old_dir = source_root.join("old");
        let target_dir = source_root.join("dest");
        std::fs::create_dir_all(&old_dir).unwrap();
        std::fs::create_dir_all(&target_dir).unwrap();
        let source = SampleSource::new(source_root.clone());
        let wav_path = old_dir.join("one.wav");
        write_test_wav(&wav_path, &[0.0, 0.1, -0.1]);
        let (file_size, modified_ns) = file_metadata(&wav_path).unwrap();
        let db = SourceDatabase::open(&source_root).unwrap();
        let mut batch = db.write_batch().unwrap();
        batch
            .upsert_file(Path::new("old/one.wav"), file_size, modified_ns)
            .unwrap();
        batch.set_tag(Path::new("old/one.wav"), Rating::KEEP_1).unwrap();
        batch.commit().unwrap();

        let request = FolderMoveRequest {
            source_id: source.id.clone(),
            source_root: source_root.clone(),
            folder: PathBuf::from("old"),
            target_folder: PathBuf::from("dest"),
        };
        let result = run_folder_move_task(
            request,
            Arc::new(AtomicBool::new(false)),
            None,
        );

        assert!(result.errors.is_empty());
        assert_eq!(result.moved.len(), 1);
        assert!(source_root.join("dest/old/one.wav").is_file());

        let db = SourceDatabase::open(&source_root).unwrap();
        assert!(db.tag_for_path(Path::new("old/one.wav")).unwrap().is_none());
        assert_eq!(
            db.tag_for_path(Path::new("dest/old/one.wav"))
                .unwrap(),
            Some(Rating::KEEP_1)
        );
    }
}
