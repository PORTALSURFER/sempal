//! Apply background file operation results to controller state.

use super::*;
use crate::egui_app::controller::jobs::{
    ClipboardPasteOutcome, ClipboardPasteResult, FileOpMessage, FileOpResult, UndoFileOpResult,
    UndoFileOutcome,
};
use crate::egui_app::controller::undo_jobs;
use crate::egui_app::controller::undo::{DeferredUndo, UndoDirection};
use crate::sample_sources::collections::CollectionMember;
use std::sync::{
    Arc,
    atomic::AtomicBool,
};

impl EguiController {
    /// Apply a completed background file operation to controller state.
    pub(crate) fn apply_file_op_result(&mut self, result: FileOpResult) {
        match result {
            FileOpResult::ClipboardPaste(result) => self.apply_clipboard_paste_result(result),
            FileOpResult::SourceMove(result) => {
                self.drag_drop().apply_source_move_result(result);
            }
            FileOpResult::FolderSampleMove(result) => {
                self.drag_drop().apply_folder_sample_move_result(result);
            }
            FileOpResult::FolderMove(result) => {
                self.drag_drop().apply_folder_move_result(result);
            }
            FileOpResult::UndoFile(result) => self.apply_undo_file_result(result),
        }
    }

    fn apply_clipboard_paste_result(&mut self, result: ClipboardPasteResult) {
        match &result.outcome {
            ClipboardPasteOutcome::Source { source_id, added } => {
                if let Some(source) = self
                    .library
                    .sources
                    .iter()
                    .find(|source| &source.id == source_id)
                    .cloned()
                {
                    if let Some(last) = added.last() {
                        self.runtime
                            .jobs
                            .set_pending_select_path(Some(last.relative_path.clone()));
                    }
                    if !added.is_empty() {
                        self.invalidate_wav_entries_for_source(&source);
                    }
                    for sample in added {
                        self.enqueue_similarity_for_new_sample(
                            &source,
                            &sample.relative_path,
                            sample.file_size,
                            sample.modified_ns,
                        );
                    }
                } else {
                    self.set_status("Source not available for paste", StatusTone::Error);
                    return;
                }
            }
            ClipboardPasteOutcome::Collection {
                collection_id,
                clip_root,
                added,
            } => {
                if let Err(err) = self.apply_collection_clip_add(
                    collection_id,
                    clip_root,
                    added,
                ) {
                    self.set_status(err, StatusTone::Error);
                    return;
                }
            }
        }
        self.report_clipboard_paste_summary(&result);
    }

    fn apply_collection_clip_add(
        &mut self,
        collection_id: &CollectionId,
        clip_root: &std::path::Path,
        added: &[std::path::PathBuf],
    ) -> Result<(), String> {
        if !self.settings.feature_flags.collections_enabled {
            return Err("Collections are disabled".into());
        }
        let Some(collection) = self
            .library
            .collections
            .iter_mut()
            .find(|collection| &collection.id == collection_id)
        else {
            return Err("Collection not found".into());
        };
        let clip_source_id =
            SourceId::from_string(format!("collection-{}", collection_id.as_str()));
        let mut added_members = Vec::new();
        for relative in added {
            let member = CollectionMember {
                source_id: clip_source_id.clone(),
                relative_path: relative.clone(),
                clip_root: Some(clip_root.to_path_buf()),
            };
            if !collection.contains(&member.source_id, &member.relative_path) {
                collection.members.push(member.clone());
                added_members.push(member);
            }
        }
        if added_members.is_empty() {
            return Ok(());
        }
        self.persist_config("Failed to save collection")?;
        self.refresh_collections_ui();
        for member in &added_members {
            if let Err(err) = self.export_member_if_needed(collection_id, member) {
                self.set_status(err, StatusTone::Warning);
            }
        }
        Ok(())
    }

    fn report_clipboard_paste_summary(&mut self, result: &ClipboardPasteResult) {
        let added = match &result.outcome {
            ClipboardPasteOutcome::Source { added, .. } => added.len(),
            ClipboardPasteOutcome::Collection { added, .. } => added.len(),
        };
        if added == 0 && result.errors.is_empty() && result.skipped == 0 {
            if result.cancelled {
                self.set_status("File operation cancelled", StatusTone::Warning);
            } else {
                self.set_status("No files processed", StatusTone::Warning);
            }
            return;
        }
        let tone = if result.errors.is_empty() && !result.cancelled {
            StatusTone::Info
        } else {
            StatusTone::Warning
        };
        let mut message = format!(
            "{} {} file(s) into {}",
            result.action_past_tense, added, result.target_label
        );
        if result.skipped > 0 {
            message.push_str(&format!(" (skipped {})", result.skipped));
        }
        if !result.errors.is_empty() {
            message.push_str(&format!(" with {} error(s)", result.errors.len()));
        }
        if result.cancelled {
            message.push_str(" (cancelled)");
        }
        self.set_status(message, tone);
        for err in &result.errors {
            eprintln!("File operation error: {err}");
        }
    }

    fn apply_undo_file_result(&mut self, result: UndoFileOpResult) {
        let Some(pending) = self.history.pending_undo.take() else {
            self.set_status("Undo completion arrived without a pending entry", StatusTone::Error);
            return;
        };
        let action_label = match pending.direction {
            UndoDirection::Undo => "Undo",
            UndoDirection::Redo => "Redo",
        };
        if result.cancelled {
            self.restore_deferred_entry(pending);
            self.set_status(format!("{action_label} cancelled"), StatusTone::Warning);
            return;
        }
        match result.result {
            Ok(outcome) => {
                self.apply_undo_file_outcome(&outcome);
                self.commit_deferred_entry(pending);
            }
            Err(err) => {
                self.restore_deferred_entry(pending);
                self.set_status(format!("{action_label} failed: {err}"), StatusTone::Error);
            }
        }
    }

    /// Start a deferred undo/redo job and track its completion.
    pub(crate) fn begin_deferred_undo_job(&mut self, pending: DeferredUndo<EguiController>) {
        let label = pending.entry.label.clone();
        let direction = pending.direction;
        let job = pending.job.clone();
        let title = match direction {
            UndoDirection::Undo => format!("Undoing {label}"),
            UndoDirection::Redo => format!("Redoing {label}"),
        };
        self.history.pending_undo = Some(pending);
        self.set_status(format!("{title}..."), StatusTone::Busy);
        self.show_status_progress(
            crate::egui_app::state::ProgressTaskKind::FileOps,
            title,
            1,
            true,
        );
        let (tx, rx) = std::sync::mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        self.runtime.jobs.start_file_ops(rx, cancel.clone());
        std::thread::spawn(move || {
            let result = undo_jobs::run_undo_file_job(job, cancel, Some(&tx));
            let _ = tx.send(FileOpMessage::Finished(FileOpResult::UndoFile(result)));
        });
    }

    fn apply_undo_file_outcome(&mut self, outcome: &UndoFileOutcome) {
        match outcome {
            UndoFileOutcome::Overwrite {
                source_id,
                relative_path,
                file_size,
                modified_ns,
                tag,
                looped,
                last_played_at,
            } => {
                let Some(source) = self
                    .library
                    .sources
                    .iter()
                    .find(|source| &source.id == source_id)
                    .cloned()
                else {
                    self.set_status("Source not available for undo", StatusTone::Error);
                    return;
                };
                let entry = WavEntry {
                    relative_path: relative_path.clone(),
                    file_size: *file_size,
                    modified_ns: *modified_ns,
                    content_hash: None,
                    tag: *tag,
                    looped: *looped,
                    missing: false,
                    last_played_at: *last_played_at,
                };
                self.update_cached_entry(&source, relative_path, entry);
                self.refresh_waveform_for_sample(&source, relative_path);
                self.reexport_collections_for_sample(&source.id, relative_path);
            }
            UndoFileOutcome::Removed {
                source_id,
                relative_path,
            } => {
                let Some(source) = self
                    .library
                    .sources
                    .iter()
                    .find(|source| &source.id == source_id)
                    .cloned()
                else {
                    self.set_status("Source not available for undo", StatusTone::Error);
                    return;
                };
                self.prune_cached_sample(&source, relative_path);
            }
            UndoFileOutcome::Restored {
                source_id,
                relative_path,
                file_size,
                modified_ns,
                tag,
                looped,
                last_played_at,
            } => {
                let Some(source) = self
                    .library
                    .sources
                    .iter()
                    .find(|source| &source.id == source_id)
                    .cloned()
                else {
                    self.set_status("Source not available for undo", StatusTone::Error);
                    return;
                };
                self.insert_cached_entry(
                    &source,
                    WavEntry {
                        relative_path: relative_path.clone(),
                        file_size: *file_size,
                        modified_ns: *modified_ns,
                        content_hash: None,
                        tag: *tag,
                        looped: *looped,
                        missing: false,
                        last_played_at: *last_played_at,
                    },
                );
                self.refresh_waveform_for_sample(&source, relative_path);
                self.reexport_collections_for_sample(&source.id, relative_path);
            }
        }
    }

    fn commit_deferred_entry(&mut self, pending: DeferredUndo<EguiController>) {
        match pending.direction {
            UndoDirection::Undo => {
                let label = pending.entry.label.clone();
                self.history.undo_stack.push_redo_entry(pending.entry);
                self.set_status(format!("Undid {label}"), StatusTone::Info);
            }
            UndoDirection::Redo => {
                let label = pending.entry.label.clone();
                self.history.undo_stack.restore_undo_entry(pending.entry);
                self.set_status(format!("Redid {label}"), StatusTone::Info);
            }
        }
    }

    fn restore_deferred_entry(&mut self, pending: DeferredUndo<EguiController>) {
        match pending.direction {
            UndoDirection::Undo => {
                self.history.undo_stack.restore_undo_entry(pending.entry);
            }
            UndoDirection::Redo => {
                self.history.undo_stack.restore_redo_entry(pending.entry);
            }
        }
    }
}
