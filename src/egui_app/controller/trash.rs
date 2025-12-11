use super::*;
use crate::sample_sources::config::normalize_path;
use rfd::{FileDialog, MessageButtons, MessageDialog, MessageDialogResult, MessageLevel};
use std::fs;
use std::path::{Path, PathBuf};

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
        if !self.confirm_warning(
            "Move trashed samples?",
            "All samples tagged as Trash will be moved to the configured trash folder and removed from sources/collections. Continue?",
        ) {
            return;
        }
        let Ok(trash_root) = self.ensure_trash_folder_ready() else {
            return;
        };
        let (batches, mut errors) = self.collect_trashed_batches();
        let total: usize = batches.iter().map(|batch| batch.entries.len()).sum();
        if total == 0 {
            self.set_status("No trashed samples to move", StatusTone::Info);
            return;
        }
        self.set_status("Moving trashed samples...", StatusTone::Busy);
        self.show_progress("Moving trashed samples", total, true);
        let outcome = self.move_batches_to_trash(batches, &trash_root);
        errors.extend(outcome.errors);
        self.finish_trash_move(
            total,
            outcome.moved,
            outcome.removed_from_collections,
            errors,
            outcome.cancelled,
        );
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

    fn move_single_to_trash(
        &mut self,
        source: &SampleSource,
        entry: &WavEntry,
        trash_root: &Path,
    ) -> Result<bool, String> {
        let absolute = source.root.join(&entry.relative_path);
        if !absolute.is_file() {
            return Err(format!("File not found for trash: {}", absolute.display()));
        }
        let destination = unique_destination(trash_root, &entry.relative_path)?;
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent).map_err(|err| {
                format!("Unable to prepare trash folder {}: {err}", parent.display())
            })?;
        }
        if let Err(err) = fs::rename(&absolute, &destination) {
            fs::copy(&absolute, &destination).map_err(|copy_err| {
                format!(
                    "Failed to move {} to trash: rename error {err}; copy error {copy_err}",
                    absolute.display()
                )
            })?;
            fs::remove_file(&absolute).map_err(|remove_err| {
                format!(
                    "Failed to remove original {} after copy: {remove_err}",
                    absolute.display()
                )
            })?;
        }
        let db = self
            .database_for(source)
            .map_err(|err| format!("Database unavailable: {err}"))?;
        db.remove_file(&entry.relative_path)
            .map_err(|err| format!("Failed to drop database row: {err}"))?;
        self.prune_cached_sample(source, &entry.relative_path);
        let collections_changed =
            self.remove_sample_from_collections(&source.id, &entry.relative_path);
        Ok(collections_changed)
    }

    fn collect_trashed_batches(&mut self) -> (Vec<TrashMoveBatch>, Vec<String>) {
        let mut batches = Vec::new();
        let mut errors = Vec::new();
        for source in self.sources.clone() {
            let db = match self.database_for(&source) {
                Ok(db) => db,
                Err(err) => {
                    errors.push(format!("{}: {err}", source.root.display()));
                    continue;
                }
            };
            let entries = match db.list_files() {
                Ok(entries) => entries,
                Err(err) => {
                    errors.push(format!("{}: {err}", source.root.display()));
                    continue;
                }
            };
            let trashed: Vec<WavEntry> = entries
                .into_iter()
                .filter(|entry| entry.tag == SampleTag::Trash)
                .collect();
            if !trashed.is_empty() {
                batches.push(TrashMoveBatch {
                    source: source.clone(),
                    entries: trashed,
                });
            }
        }
        (batches, errors)
    }

    fn move_batches_to_trash(
        &mut self,
        batches: Vec<TrashMoveBatch>,
        trash_root: &Path,
    ) -> TrashMoveOutcome {
        let mut outcome = TrashMoveOutcome::default();
        for batch in batches {
            if self.transfer_batch_entries(batch, trash_root, &mut outcome) {
                break;
            }
        }
        if self.ui.progress.cancel_requested {
            outcome.cancelled = true;
        }
        outcome
    }

    fn transfer_batch_entries(
        &mut self,
        batch: TrashMoveBatch,
        trash_root: &Path,
        outcome: &mut TrashMoveOutcome,
    ) -> bool {
        for entry in batch.entries {
            if self.ui.progress.cancel_requested {
                outcome.cancelled = true;
                return true;
            }
            self.update_progress_detail(format!("Moving {}", entry.relative_path.display()));
            match self.move_single_to_trash(&batch.source, &entry, trash_root) {
                Ok(changed_collections) => {
                    outcome.moved += 1;
                    outcome.removed_from_collections |= changed_collections;
                }
                Err(err) => outcome.errors.push(err),
            }
            self.advance_progress();
            #[cfg(test)]
            if let Some(threshold) = self.progress_cancel_after
                && outcome.moved >= threshold
            {
                self.request_progress_cancel();
            }
        }
        false
    }

    fn finish_trash_move(
        &mut self,
        total: usize,
        moved: usize,
        removed_from_collections: bool,
        mut errors: Vec<String>,
        cancelled: bool,
    ) {
        if removed_from_collections {
            let _ = self.persist_config("Failed to save collections after trash move");
        }
        if cancelled {
            self.set_status(
                format!("Canceled trash move after {moved}/{total} sample(s)"),
                StatusTone::Warning,
            );
        } else if errors.is_empty() {
            self.set_status(format!("Moved {moved} trashed sample(s)"), StatusTone::Info);
        } else {
            let summary = format!("Moved {moved} sample(s) with {} error(s)", errors.len());
            self.set_status(summary, StatusTone::Warning);
        }
        for err in errors.drain(..) {
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

struct TrashMoveBatch {
    source: SampleSource,
    entries: Vec<WavEntry>,
}

#[derive(Default)]
struct TrashMoveOutcome {
    moved: usize,
    removed_from_collections: bool,
    cancelled: bool,
    errors: Vec<String>,
}

fn unique_destination(root: &Path, relative: &Path) -> Result<PathBuf, String> {
    let mut candidate = root.join(relative);
    if !candidate.exists() {
        return Ok(candidate);
    }
    let parent = candidate
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| root.to_path_buf());
    let stem = relative
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("file");
    let ext = relative.extension().and_then(|e| e.to_str()).unwrap_or("");
    for idx in 1..=1000 {
        let mut name = format!("{stem}_{idx}");
        if !ext.is_empty() {
            name.push('.');
            name.push_str(ext);
        }
        candidate = parent.join(name);
        if !candidate.exists() {
            return Ok(candidate);
        }
    }
    Err("Could not create unique trash destination".into())
}
