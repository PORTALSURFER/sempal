use super::super::DragDropController;
use crate::egui_app::controller::library::collection_items_helpers::file_metadata;
use crate::egui_app::controller::jobs::{FileOpMessage, SourceMoveRequest, SourceMoveResult, SourceMoveSuccess};
use crate::egui_app::state::DragSample;
use crate::egui_app::ui::style::StatusTone;
use crate::sample_sources::{Rating, SourceDatabase, SourceId, WavEntry};
use crate::sample_sources::db::file_ops_journal;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc::Sender,
};
use tracing::info;

impl DragDropController<'_> {
    pub(crate) fn handle_sample_drop_to_source(
        &mut self,
        source_id: SourceId,
        relative_path: PathBuf,
        target_source_id: SourceId,
    ) -> bool {
        let sample = DragSample {
            source_id,
            relative_path,
        };
        self.handle_samples_drop_to_source(std::slice::from_ref(&sample), target_source_id);
        true
    }

    pub(crate) fn handle_samples_drop_to_source(
        &mut self,
        samples: &[DragSample],
        target_source_id: SourceId,
    ) {
        if samples.is_empty() {
            return;
        }
        if self.runtime.jobs.file_ops_in_progress() {
            self.set_status("Another file operation is already running", StatusTone::Warning);
            return;
        }
        if samples.iter().all(|sample| sample.source_id == target_source_id) {
            self.set_status("Samples are already in that source", StatusTone::Info);
            return;
        }
        let Some(target_source) = self
            .library
            .sources
            .iter()
            .find(|source| source.id == target_source_id)
            .cloned()
        else {
            self.set_status("Target source not available for move", StatusTone::Error);
            return;
        };
        let mut requests = Vec::new();
        let mut errors = Vec::new();
        for sample in samples {
            let Some(source) = self
                .library
                .sources
                .iter()
                .find(|source| source.id == sample.source_id)
                .cloned()
            else {
                errors.push(format!(
                    "Source not available for move: {}",
                    sample.relative_path.display()
                ));
                continue;
            };
            requests.push(SourceMoveRequest {
                source_id: source.id,
                source_root: source.root,
                relative_path: sample.relative_path.clone(),
            });
        }
        if requests.is_empty() {
            if let Some(err) = errors.pop() {
                self.set_status(err, StatusTone::Error);
            }
            return;
        }
        self.set_status("Moving samples...", StatusTone::Busy);
        self.show_status_progress(
            crate::egui_app::state::ProgressTaskKind::FileOps,
            "Moving samples",
            requests.len(),
            true,
        );
        let target_root = target_source.root.clone();
        let cancel = Arc::new(AtomicBool::new(false));
        #[cfg(test)]
        {
            let result = run_source_move_task(
                target_source_id,
                target_root,
                requests,
                errors,
                cancel,
                None,
            );
            self.apply_source_move_result(result);
            if self.ui.progress.task == Some(crate::egui_app::state::ProgressTaskKind::FileOps) {
                self.clear_progress();
            }
        }
        #[cfg(not(test))]
        {
            let (tx, rx) = std::sync::mpsc::channel();
            self.runtime.jobs.start_file_ops(rx, cancel.clone());
            std::thread::spawn(move || {
                let result = run_source_move_task(
                    target_source_id,
                    target_root,
                    requests,
                    errors,
                    cancel,
                    Some(&tx),
                );
                let _ = tx.send(FileOpMessage::Finished(FileOpResult::SourceMove(result)));
            });
        }
    }

    /// Apply a completed background source move job.
    pub(crate) fn apply_source_move_result(&mut self, result: SourceMoveResult) {
        let Some(target_source) = self
            .library
            .sources
            .iter()
            .find(|source| source.id == result.target_source_id)
            .cloned()
        else {
            self.set_status("Target source not available for move", StatusTone::Error);
            return;
        };
        let mut moved_sources = HashSet::new();
        let mut collections_changed = false;
        for entry in &result.moved {
            let Some(source) = self
                .library
                .sources
                .iter()
                .find(|source| source.id == entry.source_id)
                .cloned()
            else {
                continue;
            };
            self.prune_cached_sample(&source, &entry.relative_path);
            self.insert_cached_entry(
                &target_source,
                WavEntry {
                    relative_path: entry.target_relative.clone(),
                    file_size: entry.file_size,
                    modified_ns: entry.modified_ns,
                    content_hash: None,
                    tag: entry.tag,
                    looped: entry.looped,
                    missing: false,
                    last_played_at: entry.last_played_at,
                },
            );
            moved_sources.insert(source.id.clone());
            moved_sources.insert(target_source.id.clone());
            if self.update_collections_for_source_move(
                &source.id,
                &target_source.id,
                &entry.relative_path,
                &entry.target_relative,
            ) {
                collections_changed = true;
            }
        }
        if collections_changed {
            let _ = self.persist_config("Failed to save collections after move");
        }
        for source_id in moved_sources {
            let Some(source) = self
                .library
                .sources
                .iter()
                .find(|source| source.id == source_id)
                .cloned()
            else {
                continue;
            };
            self.invalidate_wav_entries_for_source_preserve_folders(&source);
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
            eprintln!("Source move error: {err}");
        }
        info!(
            "Source move completed: {} moved, {} errors",
            moved,
            result.errors.len()
        );
    }

    pub(super) fn register_moved_sample_for_source(
        &mut self,
        source: &crate::sample_sources::SampleSource,
        relative_path: &Path,
        file_size: u64,
        modified_ns: i64,
        tag: Rating,
        looped: bool,
        last_played_at: Option<i64>,
    ) -> Result<(), String> {
        let db = self
            .database_for(source)
            .map_err(|err| format!("Database unavailable: {err}"))?;
        db.upsert_file(relative_path, file_size, modified_ns)
            .map_err(|err| format!("Failed to register file: {err}"))?;
        db.set_tag(relative_path, tag)
            .map_err(|err| format!("Failed to set tag: {err}"))?;
        db.set_looped(relative_path, looped)
            .map_err(|err| format!("Failed to set loop marker: {err}"))?;
        if let Some(last_played_at) = last_played_at {
            db.set_last_played_at(relative_path, last_played_at)
                .map_err(|err| format!("Failed to copy playback age: {err}"))?;
        }
        Ok(())
    }

    pub(super) fn remove_source_db_entry(
        &mut self,
        source: &crate::sample_sources::SampleSource,
        relative_path: &Path,
    ) -> Result<(), String> {
        let db = self
            .database_for(source)
            .map_err(|err| format!("Database unavailable: {err}"))?;
        db.remove_file(relative_path)
            .map_err(|err| format!("Failed to drop database row: {err}"))
    }

    fn remove_target_db_entry(
        &mut self,
        source: &crate::sample_sources::SampleSource,
        relative_path: &Path,
    ) -> Result<(), String> {
        let db = self
            .database_for(source)
            .map_err(|err| format!("Database unavailable: {err}"))?;
        db.remove_file(relative_path)
            .map_err(|err| format!("Failed to drop database row: {err}"))
    }

    pub(super) fn update_collections_for_source_move(
        &mut self,
        from: &SourceId,
        to: &SourceId,
        old_relative: &Path,
        new_relative: &Path,
    ) -> bool {
        let new_path = new_relative.to_path_buf();
        let mut changed = false;
        for collection in &mut self.library.collections {
            if collection.contains(to, &new_path) {
                let before = collection.members.len();
                collection.members.retain(|member| {
                    if member.clip_root.is_some() {
                        return true;
                    }
                    &member.source_id != from || member.relative_path.as_path() != old_relative
                });
                if before != collection.members.len() {
                    changed = true;
                }
                continue;
            }
            for member in &mut collection.members {
                if member.clip_root.is_some() {
                    continue;
                }
                if &member.source_id == from && member.relative_path == old_relative {
                    member.source_id = to.clone();
                    member.relative_path = new_path.clone();
                    changed = true;
                }
            }
        }
        changed
    }
}

fn unique_destination_path(root: &Path, relative: &Path) -> Result<PathBuf, String> {
    if !root.join(relative).exists() {
        return Ok(relative.to_path_buf());
    }
    let parent = relative.parent().unwrap_or_else(|| Path::new(""));
    let file_name = relative
        .file_name()
        .ok_or_else(|| "Sample has no file name".to_string())?;
    let stem = Path::new(file_name)
        .file_stem()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| "sample".to_string());
    let extension = Path::new(file_name)
        .extension()
        .map(|ext| ext.to_string_lossy().to_string());
    for index in 1..=999 {
        let suffix = format!("{stem}_move{index:03}");
        let file_name = if let Some(ext) = &extension {
            format!("{suffix}.{ext}")
        } else {
            suffix
        };
        let candidate = parent.join(file_name);
        if !root.join(&candidate).exists() {
            return Ok(candidate);
        }
    }
    Err("Failed to find destination file name".into())
}

pub(super) fn move_sample_file(source: &Path, destination: &Path) -> Result<(), String> {
    match std::fs::rename(source, destination) {
        Ok(()) => Ok(()),
        Err(rename_err) => {
            if let Err(copy_err) = std::fs::copy(source, destination) {
                return Err(format!(
                    "Failed to move file: {rename_err}; copy failed: {copy_err}"
                ));
            }
            if let Err(remove_err) = std::fs::remove_file(source) {
                let _ = std::fs::remove_file(destination);
                return Err(format!("Failed to remove original file: {remove_err}"));
            }
            Ok(())
        }
    }
}

fn run_source_move_task(
    target_source_id: SourceId,
    target_root: PathBuf,
    requests: Vec<SourceMoveRequest>,
    mut errors: Vec<String>,
    cancel: Arc<AtomicBool>,
    sender: Option<&Sender<FileOpMessage>>,
) -> SourceMoveResult {
    let mut moved = Vec::new();
    let mut completed = 0usize;
    let mut cancelled = false;
    if !target_root.is_dir() {
        errors.push(format!(
            "Target source folder missing: {}",
            target_root.display()
        ));
        return SourceMoveResult {
            target_source_id,
            moved,
            errors,
            cancelled,
        };
    }
    let target_db = match SourceDatabase::open(&target_root) {
        Ok(db) => db,
        Err(err) => {
            errors.push(format!("Failed to open target DB: {err}"));
            return SourceMoveResult {
                target_source_id,
                moved,
                errors,
                cancelled,
            };
        }
    };
    let mut source_dbs: HashMap<PathBuf, SourceDatabase> = HashMap::new();

    for request in requests {
        if cancel.load(Ordering::Relaxed) {
            cancelled = true;
            break;
        }
        let detail = Some(format!("Moving {}", request.relative_path.display()));
        let absolute = request.source_root.join(&request.relative_path);
        if !absolute.is_file() {
            errors.push(format!("File missing: {}", request.relative_path.display()));
            completed += 1;
            report_progress(sender, completed, detail);
            continue;
        }
        let target_relative = match unique_destination_path(&target_root, &request.relative_path) {
            Ok(path) => path,
            Err(err) => {
                errors.push(err);
                completed += 1;
                report_progress(sender, completed, detail);
                continue;
            }
        };
        if let Some(parent) = target_relative.parent() {
            let target_dir = target_root.join(parent);
            if let Err(err) = std::fs::create_dir_all(&target_dir) {
                errors.push(format!(
                    "Failed to create target folder {}: {err}",
                    target_dir.display()
                ));
                completed += 1;
                report_progress(sender, completed, detail);
                continue;
            }
        }
        let source_db = match source_dbs.entry(request.source_root.clone()) {
            std::collections::hash_map::Entry::Occupied(entry) => entry.into_mut(),
            std::collections::hash_map::Entry::Vacant(entry) => match SourceDatabase::open(&request.source_root) {
                Ok(db) => entry.insert(db),
                Err(err) => {
                    errors.push(format!("Failed to open source DB: {err}"));
                    completed += 1;
                    report_progress(sender, completed, detail);
                    continue;
                }
            },
        };
        let tag = match source_db.tag_for_path(&request.relative_path) {
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
        let looped = match source_db.looped_for_path(&request.relative_path) {
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
        let last_played_at = match source_db.last_played_at_for_path(&request.relative_path) {
            Ok(value) => value,
            Err(err) => {
                errors.push(format!("Failed to read database: {err}"));
                completed += 1;
                report_progress(sender, completed, detail);
                continue;
            }
        };
        let op_id = file_ops_journal::new_op_id();
        let staged_relative = match file_ops_journal::staged_relative_for_target(&target_relative, &op_id) {
            Ok(path) => path,
            Err(err) => {
                errors.push(format!("Failed to build staging path: {err}"));
                completed += 1;
                report_progress(sender, completed, detail);
                continue;
            }
        };
        let journal_entry = match file_ops_journal::FileOpJournalEntry::new_move(
            op_id.clone(),
            request.source_root.clone(),
            request.relative_path.clone(),
            target_relative.clone(),
            staged_relative.clone(),
            tag,
            looped,
            last_played_at,
        ) {
            Ok(entry) => entry,
            Err(err) => {
                errors.push(format!("Failed to stage move journal: {err}"));
                completed += 1;
                report_progress(sender, completed, detail);
                continue;
            }
        };
        if let Err(err) = file_ops_journal::insert_entry(&target_db, &journal_entry) {
            errors.push(format!("Failed to record move journal: {err}"));
            completed += 1;
            report_progress(sender, completed, detail);
            continue;
        }
        let staged_absolute = target_root.join(&staged_relative);
        if let Err(err) = move_sample_file(&absolute, &staged_absolute) {
            remove_move_journal_entry(&mut errors, &target_db, &op_id);
            errors.push(err);
            completed += 1;
            report_progress(sender, completed, detail);
            continue;
        }
        let (file_size, modified_ns) = match file_metadata(&staged_absolute) {
            Ok(meta) => meta,
            Err(err) => {
                rollback_move_to_source(&mut errors, &staged_absolute, &absolute);
                remove_move_journal_entry(&mut errors, &target_db, &op_id);
                errors.push(err);
                completed += 1;
                report_progress(sender, completed, detail);
                continue;
            }
        };
        if let Err(err) = file_ops_journal::update_stage(
            &target_db,
            &op_id,
            file_ops_journal::FileOpStage::Staged,
            Some(file_size),
            Some(modified_ns),
        ) {
            rollback_move_to_source(&mut errors, &staged_absolute, &absolute);
            remove_move_journal_entry(&mut errors, &target_db, &op_id);
            errors.push(format!("Failed to update move journal: {err}"));
            completed += 1;
            report_progress(sender, completed, detail);
            continue;
        }
        let mut batch = match target_db.write_batch() {
            Ok(batch) => batch,
            Err(err) => {
                rollback_move_to_source(&mut errors, &staged_absolute, &absolute);
                remove_move_journal_entry(&mut errors, &target_db, &op_id);
                errors.push(format!("Failed to open target DB batch: {err}"));
                completed += 1;
                report_progress(sender, completed, detail);
                continue;
            }
        };
        if let Err(err) = batch.upsert_file(&target_relative, file_size, modified_ns) {
            rollback_move_to_source(&mut errors, &staged_absolute, &absolute);
            remove_move_journal_entry(&mut errors, &target_db, &op_id);
            errors.push(format!("Failed to register file: {err}"));
            completed += 1;
            report_progress(sender, completed, detail);
            continue;
        }
        if let Err(err) = batch.set_tag(&target_relative, tag) {
            rollback_move_to_source(&mut errors, &staged_absolute, &absolute);
            remove_move_journal_entry(&mut errors, &target_db, &op_id);
            errors.push(format!("Failed to set tag: {err}"));
            completed += 1;
            report_progress(sender, completed, detail);
            continue;
        }
        if let Err(err) = batch.set_looped(&target_relative, looped) {
            rollback_move_to_source(&mut errors, &staged_absolute, &absolute);
            remove_move_journal_entry(&mut errors, &target_db, &op_id);
            errors.push(format!("Failed to set loop marker: {err}"));
            completed += 1;
            report_progress(sender, completed, detail);
            continue;
        }
        if let Some(last_played_at) = last_played_at {
            if let Err(err) = batch.set_last_played_at(&target_relative, last_played_at) {
                rollback_move_to_source(&mut errors, &staged_absolute, &absolute);
                remove_move_journal_entry(&mut errors, &target_db, &op_id);
                errors.push(format!("Failed to copy playback age: {err}"));
                completed += 1;
                report_progress(sender, completed, detail);
                continue;
            }
        }
        if let Err(err) = batch.commit() {
            rollback_move_to_source(&mut errors, &staged_absolute, &absolute);
            remove_move_journal_entry(&mut errors, &target_db, &op_id);
            errors.push(format!("Failed to commit target DB update: {err}"));
            completed += 1;
            report_progress(sender, completed, detail);
            continue;
        }
        if let Err(err) = file_ops_journal::update_stage(
            &target_db,
            &op_id,
            file_ops_journal::FileOpStage::TargetDb,
            None,
            None,
        ) {
            errors.push(format!("Failed to update move journal: {err}"));
        }
        if let Err(err) = source_db.remove_file(&request.relative_path) {
            errors.push(format!("Failed to drop database row: {err}"));
            completed += 1;
            report_progress(sender, completed, detail);
            continue;
        }
        if let Err(err) = file_ops_journal::update_stage(
            &target_db,
            &op_id,
            file_ops_journal::FileOpStage::SourceDb,
            None,
            None,
        ) {
            errors.push(format!("Failed to update move journal: {err}"));
        }
        let target_absolute = target_root.join(&target_relative);
        if let Err(err) = std::fs::rename(&staged_absolute, &target_absolute) {
            errors.push(format!("Failed to finalize move: {err}"));
            completed += 1;
            report_progress(sender, completed, detail);
            continue;
        }
        remove_move_journal_entry(&mut errors, &target_db, &op_id);
        moved.push(SourceMoveSuccess {
            source_id: request.source_id,
            target_source_id: target_source_id.clone(),
            relative_path: request.relative_path,
            target_relative,
            file_size,
            modified_ns,
            tag,
            looped,
            last_played_at,
        });
        completed += 1;
        report_progress(sender, completed, detail);
    }
    SourceMoveResult {
        target_source_id,
        moved,
        errors,
        cancelled,
    }
}

fn remove_move_journal_entry(
    errors: &mut Vec<String>,
    db: &SourceDatabase,
    op_id: &str,
) {
    if let Err(err) = file_ops_journal::remove_entry(db, op_id) {
        errors.push(format!("Failed to clear move journal: {err}"));
    }
}

fn rollback_move_to_source(errors: &mut Vec<String>, from: &Path, to: &Path) {
    if let Err(err) = move_sample_file(from, to) {
        errors.push(format!("Failed to restore moved file: {err}"));
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
    use crate::egui_app::controller::EguiController;
    use crate::egui_app::controller::test_support::{sample_entry, write_test_wav};
    use crate::sample_sources::{Rating, SampleSource};
    use crate::waveform::WaveformRenderer;
    use tempfile::tempdir;

    #[test]
    fn moving_multiple_samples_to_source_clears_browser_rows() {
        let temp = tempdir().unwrap();
        let source_root = temp.path().join("source_a");
        let target_root = temp.path().join("source_b");
        std::fs::create_dir_all(&source_root).unwrap();
        std::fs::create_dir_all(&target_root).unwrap();
        let source = SampleSource::new(source_root);
        let target = SampleSource::new(target_root);
        let renderer = WaveformRenderer::new(10, 10);
        let mut controller = EguiController::new(renderer, None);
        controller.library.sources.push(source.clone());
        controller.library.sources.push(target.clone());
        controller.selection_state.ctx.selected_source = Some(source.id.clone());
        controller.cache_db(&source).unwrap();
        controller.cache_db(&target).unwrap();
        write_test_wav(&source.root.join("one.wav"), &[0.0, 0.1, -0.1]);
        write_test_wav(&source.root.join("two.wav"), &[0.0, 0.1, -0.1]);
        controller
            .ensure_sample_db_entry(&source, Path::new("one.wav"))
            .unwrap();
        controller
            .ensure_sample_db_entry(&source, Path::new("two.wav"))
            .unwrap();
        controller.set_wav_entries_for_tests(vec![
            sample_entry("one.wav", Rating::NEUTRAL),
            sample_entry("two.wav", Rating::NEUTRAL),
        ]);
        controller.rebuild_wav_lookup();
        controller.rebuild_browser_lists();

        let samples = vec![
            DragSample {
                source_id: source.id.clone(),
                relative_path: PathBuf::from("one.wav"),
            },
            DragSample {
                source_id: source.id.clone(),
                relative_path: PathBuf::from("two.wav"),
            },
        ];
        controller
            .drag_drop()
            .handle_samples_drop_to_source(&samples, target.id.clone());

        assert!(
            controller
                .wav_index_for_path(Path::new("one.wav"))
                .is_none()
        );
        assert!(
            controller
                .wav_index_for_path(Path::new("two.wav"))
                .is_none()
        );
    }
}
