use super::*;
use super::trash_move::{TrashMoveFinished, TrashMoveMessage};
#[cfg(test)]
use super::trash_move::run_trash_move_task_with_progress;
#[cfg(not(test))]
use super::trash_move::run_trash_move_task;
use crate::sample_sources::config::normalize_path;
use rfd::{FileDialog, MessageButtons, MessageDialog, MessageDialogResult, MessageLevel};
use std::fs;
use std::path::PathBuf;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
#[cfg(not(test))]
use std::sync::mpsc::channel;

impl EguiController {
    /// Open a folder picker and persist the chosen trash folder.
    pub fn pick_trash_folder(&mut self) {
        let Some(path) = FileDialog::new().pick_folder() else {
            return;
        };
        let normalized = normalize_path(path.as_path());
        match self.apply_trash_folder(Some(normalized.clone())) {
            Ok(()) => self.set_status(
                format!("Trash folder set to {}", normalized.display()),
                StatusTone::Info,
            ),
            Err(err) => self.set_status(err, StatusTone::Error),
        }
    }

    /// Open the configured trash folder in the OS file explorer.
    pub fn open_trash_folder(&mut self) {
        let Ok(path) = self.ensure_trash_folder_ready() else {
            return;
        };
        if let Err(err) = open::that(&path) {
            self.set_status(
                format!("Could not open trash folder {}: {err}", path.display()),
                StatusTone::Error,
            );
        }
    }

    /// Move all samples tagged as Trash into the configured trash folder after confirmation.
    pub fn move_all_trashed_to_folder(&mut self) {
        if self.jobs.trash_move_rx.is_some() {
            self.set_status("Trash move already in progress", StatusTone::Warning);
            return;
        }
        if !self.confirm_warning(
            "Move trashed samples?",
            "All samples tagged as Trash will be moved to the configured trash folder and removed from sources/collections. Continue?",
        ) {
            return;
        }
        let Ok(trash_root) = self.ensure_trash_folder_ready() else {
            return;
        };
        self.set_status("Moving trashed samples...", StatusTone::Busy);
        self.show_status_progress("Moving trashed samples", 0, true);
        self.update_progress_detail("Scanning for trashed samples...");

        let cancel = Arc::new(AtomicBool::new(false));

        let sources = self.sources.clone();
        let collections = self.collections.clone();

        #[cfg(test)]
        {
            let cancel_after = self.progress_cancel_after;
            let finished = run_trash_move_task_with_progress(
                sources,
                collections,
                trash_root,
                cancel.clone(),
                |message| match message {
                    TrashMoveMessage::SetTotal(total) => {
                        self.ui.progress.total = total;
                    }
                    TrashMoveMessage::Progress { completed, detail } => {
                        self.ui.progress.completed = completed;
                        self.ui.progress.detail = detail;
                        if let Some(cancel_after) = cancel_after
                            && completed >= cancel_after
                        {
                            cancel.store(true, Ordering::Relaxed);
                        }
                    }
                    TrashMoveMessage::Finished(_) => {}
                },
            );
            self.apply_trash_move_finished(finished);
        }

        #[cfg(not(test))]
        {
            let (tx, rx) = channel();
            self.jobs.trash_move_cancel = Some(cancel.clone());
            self.jobs.trash_move_rx = Some(rx);
            std::thread::spawn(move || {
                let _ = run_trash_move_task(sources, collections, trash_root, cancel, Some(&tx));
            });
        }
    }

    /// Permanently delete the contents of the configured trash folder after confirmation.
    pub fn take_out_trash(&mut self) {
        if !self.confirm_warning(
            "Take out trash?",
            "Everything inside the trash folder will be permanently deleted. Continue?",
        ) {
            return;
        }
        let Ok(trash_root) = self.ensure_trash_folder_ready() else {
            return;
        };
        self.set_status("Deleting trash...", StatusTone::Busy);
        let mut files_removed = 0usize;
        let mut errors = Vec::new();
        let mut stack = vec![trash_root.clone()];
        let mut dirs = Vec::new();
        while let Some(dir) = stack.pop() {
            match fs::read_dir(&dir) {
                Ok(entries) => {
                    dirs.push(dir.clone());
                    for entry in entries {
                        match entry {
                            Ok(entry) => {
                                let path = entry.path();
                                if path.is_dir() {
                                    stack.push(path);
                                } else if path.is_file() {
                                    match fs::remove_file(&path) {
                                        Ok(_) => files_removed += 1,
                                        Err(err) => errors.push(format!(
                                            "Failed to delete {}: {err}",
                                            path.display()
                                        )),
                                    }
                                }
                            }
                            Err(err) => errors.push(format!("Failed to read entry: {err}")),
                        }
                    }
                }
                Err(err) => errors.push(format!(
                    "Failed to read trash folder {}: {err}",
                    dir.display()
                )),
            }
        }
        for dir in dirs.into_iter().rev() {
            if dir == trash_root {
                continue;
            }
            if let Err(err) = fs::remove_dir(&dir)
                && dir.exists()
            {
                errors.push(format!("Failed to remove folder {}: {err}", dir.display()));
            }
        }
        if errors.is_empty() {
            self.set_status(
                format!("Deleted {files_removed} file(s) from trash"),
                StatusTone::Info,
            );
        } else {
            let summary = format!(
                "Deleted {files_removed} file(s) from trash with {} error(s)",
                errors.len()
            );
            self.set_status(summary, StatusTone::Warning);
            for err in errors {
                eprintln!("Trash delete error: {err}");
            }
        }
    }

    pub(super) fn poll_trash_move(&mut self) {
        let Some(rx) = self.jobs.trash_move_rx.as_ref() else {
            return;
        };
        if let Some(cancel) = self.jobs.trash_move_cancel.as_ref()
            && self.ui.progress.cancel_requested
        {
            cancel.store(true, Ordering::Relaxed);
        }
        let mut finished = None;
        while let Ok(message) = rx.try_recv() {
            match message {
                TrashMoveMessage::SetTotal(total) => {
                    self.ui.progress.total = total;
                }
                TrashMoveMessage::Progress { completed, detail } => {
                    self.ui.progress.completed = completed;
                    self.ui.progress.detail = detail;
                }
                TrashMoveMessage::Finished(result) => {
                    finished = Some(result);
                    break;
                }
            }
        }
        if let Some(result) = finished {
            self.jobs.trash_move_rx = None;
            self.jobs.trash_move_cancel = None;
            self.apply_trash_move_finished(result);
        }
    }

    fn apply_trash_move_finished(&mut self, result: TrashMoveFinished) {
        if result.collections_changed {
            self.collections = result.collections;
            self.refresh_collections_ui();
            let _ = self.persist_config("Failed to save collections after trash move");
        }

        let mut invalidator = source_cache_invalidator::SourceCacheInvalidator::new(
            &mut self.db_cache,
            &mut self.wav_cache.entries,
            &mut self.wav_cache.lookup,
            &mut self.label_cache,
            &mut self.missing.wavs,
            &mut self.folder_browsers,
        );
        for source_id in &result.affected_sources {
            invalidator.invalidate_all(source_id);
        }

        if let Some(source) = self.current_source()
            && result.affected_sources.iter().any(|id| id == &source.id)
        {
            if let Some(loaded) = self.wav_selection.loaded_wav.as_ref() {
                let absolute = source.root.join(loaded);
                if !absolute.is_file() {
                    self.clear_waveform_view();
                }
            }
            self.queue_wav_load();
        }

        if result.cancelled {
            self.set_status(
                format!(
                    "Canceled trash move after {}/{} sample(s)",
                    result.moved, result.total
                ),
                StatusTone::Warning,
            );
        } else if result.total == 0 {
            self.set_status("No trashed samples to move", StatusTone::Info);
        } else if result.errors.is_empty() {
            self.set_status(
                format!("Moved {} trashed sample(s)", result.moved),
                StatusTone::Info,
            );
        } else {
            self.set_status(
                format!(
                    "Moved {} sample(s) with {} error(s)",
                    result.moved,
                    result.errors.len()
                ),
                StatusTone::Warning,
            );
        }

        for err in result.errors {
            eprintln!("Trash move error: {err}");
        }
        self.clear_progress();
    }

    fn apply_trash_folder(&mut self, folder: Option<PathBuf>) -> Result<(), String> {
        let normalized = folder.map(|path| normalize_path(path.as_path()));
        if let Some(path) = normalized.as_ref() {
            if path.exists() && !path.is_dir() {
                return Err(format!("Trash path is not a directory: {}", path.display()));
            }
            fs::create_dir_all(path).map_err(|err| {
                format!("Unable to create trash folder {}: {err}", path.display())
            })?;
        }
        self.trash_folder = normalized.clone();
        self.ui.trash_folder = normalized;
        self.persist_config("Failed to save trash folder")
    }

    fn ensure_trash_folder_ready(&mut self) -> Result<PathBuf, ()> {
        let Some(path) = self.trash_folder.clone() else {
            self.set_status("Set a trash folder first", StatusTone::Warning);
            return Err(());
        };
        if path.exists() && !path.is_dir() {
            self.set_status(
                format!("Trash path is not a directory: {}", path.display()),
                StatusTone::Error,
            );
            return Err(());
        }
        if !path.exists()
            && let Err(err) = fs::create_dir_all(&path)
        {
            self.set_status(
                format!("Unable to create trash folder {}: {err}", path.display()),
                StatusTone::Error,
            );
            return Err(());
        }
        Ok(path)
    }

    fn confirm_warning(&self, title: &str, description: &str) -> bool {
        if cfg!(test) {
            return true;
        }
        matches!(
            MessageDialog::new()
                .set_level(MessageLevel::Warning)
                .set_title(title)
                .set_description(description)
                .set_buttons(MessageButtons::YesNo)
                .show(),
            MessageDialogResult::Yes
        )
    }
}
